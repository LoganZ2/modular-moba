//! Per-tick decision making: hero orders, minions, towers and jungle monsters.
//!
//! Every unit ends this phase with `engage` (who to attack), `chase` (whether
//! to follow that target) and `move_goal` (where to walk). Attacks and
//! movement are then executed generically by `combat` and `movement`.

use super::abilities::{self, PendingResult};
use super::*;
use crate::units::TOWER_RANGE;

const MINION_ACQUIRE: f32 = 130.0;
const MINION_CHASE: f32 = 220.0;
const CALL_FOR_HELP_TIME: f32 = 2.0;
const MONSTER_LEASH: f32 = 260.0;

pub(crate) fn think(w: &mut World) {
    for i in 0..w.entities.len() {
        if !w.entities[i].alive {
            continue;
        }
        match w.entities[i].kind {
            UnitKind::Hero(_) => hero_think(w, i),
            UnitKind::Minion(_) => minion_think(w, i),
            UnitKind::Structure(_) => tower_think(w, i),
            UnitKind::Monster(_) => monster_think(w, i),
        }
        resolve_engage(w, i);
    }
}

/// Is `t` a living, targetable enemy of `e`?
fn hostile_target(w: &World, i: usize, t: EntityId) -> Option<usize> {
    let ti = w.idx(t)?;
    let (e, te) = (&w.entities[i], &w.entities[ti]);
    if !te.targetable() || te.team == e.team {
        return None;
    }
    if e.team != Team::Neutral && !w.visible_to(e.team, ti) {
        return None;
    }
    Some(ti)
}

/// Turn `engage` + `chase` into a movement goal (or drop the target).
fn resolve_engage(w: &mut World, i: usize) {
    let Some(t) = w.entities[i].engage else {
        return;
    };
    let Some(ti) = hostile_target(w, i, t) else {
        w.entities[i].engage = None;
        return;
    };
    let in_range = w.entities[i].in_attack_range(&w.entities[ti]);
    let tpos = w.entities[ti].pos;
    let e = &mut w.entities[i];
    if in_range {
        e.move_goal = None;
    } else if e.chase {
        e.move_goal = Some(tpos);
    } else {
        e.engage = None;
    }
}

/// Nearest enemy matching `filter` within `radius` (edge distance).
fn nearest_enemy(w: &World, i: usize, radius: f32, filter: impl Fn(&Entity) -> bool) -> Option<EntityId> {
    let e = &w.entities[i];
    let mut best: Option<(f32, EntityId)> = None;
    for (ti, t) in w.entities.iter().enumerate() {
        if !t.targetable() || t.team == e.team || !filter(t) {
            continue;
        }
        let d = e.edge_distance(t);
        if d > radius {
            continue;
        }
        if e.team != Team::Neutral && !w.visible_to(e.team, ti) {
            continue;
        }
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, t.id));
        }
    }
    best.map(|(_, id)| id)
}

/// An enemy hero near `pos` that recently attacked one of `team`'s heroes
/// who is also near `pos`.
fn call_for_help(w: &World, team: Team, pos: Vec2, radius: f32) -> Option<EntityId> {
    for (ti, t) in w.entities.iter().enumerate() {
        if !t.alive || t.team == team || !t.is_hero() || t.pos.distance(pos) > radius || !t.targetable() {
            continue;
        }
        let Some((victim, at)) = t.hero().and_then(|h| h.aggression) else {
            continue;
        };
        if w.time - at > CALL_FOR_HELP_TIME {
            continue;
        }
        let victim_near =
            w.get(victim).map(|v| v.team == team && v.pos.distance(pos) <= radius + 60.0).unwrap_or(false);
        if victim_near && w.visible_to(team, ti) {
            return Some(t.id);
        }
    }
    None
}

// ------------------------------------------------------------------ heroes

fn hero_think(w: &mut World, i: usize) {
    {
        let e = &mut w.entities[i];
        if e.dash.is_some() {
            return;
        }
        let h = e.hero().unwrap();
        if h.cast.is_some() || h.recall.is_some() || e.stunned() {
            e.move_goal = None;
            e.engage = None;
            return;
        }
        if let Some(src) = e.taunted_by() {
            e.engage = Some(src);
            e.chase = true;
            e.move_goal = None;
            return;
        }
    }
    let order = w.entities[i].order;
    match order {
        Order::Idle => {
            let e = &w.entities[i];
            // Keep hitting the current target while it stays in range,
            // otherwise auto-acquire the closest enemy in range.
            let keep = e.engage.and_then(|t| hostile_target(w, i, t)).filter(|&ti| e.in_attack_range(&w.entities[ti]));
            let engage = match keep {
                Some(ti) => Some(w.entities[ti].id),
                None => nearest_enemy(w, i, e.stats.range, |_| true),
            };
            let e = &mut w.entities[i];
            e.engage = engage;
            e.chase = false;
            e.move_goal = None;
        }
        Order::Move(p) => {
            let e = &mut w.entities[i];
            e.engage = None;
            if e.pos.distance(p) < 3.0 || (e.path.is_empty() && e.move_goal.is_some() && e.stuck_time > 1.0) {
                e.order = Order::Idle;
                e.move_goal = None;
                e.stuck_time = 0.0;
            } else {
                e.move_goal = Some(p);
            }
        }
        Order::Attack(t) => {
            if hostile_target(w, i, t).is_some() {
                let e = &mut w.entities[i];
                e.engage = Some(t);
                e.chase = true;
            } else {
                let e = &mut w.entities[i];
                e.order = Order::Idle;
                e.engage = None;
                e.move_goal = None;
            }
        }
        Order::AttackMove(p) => {
            let e = &w.entities[i];
            let acquire = e.stats.range + 90.0;
            let keep = e
                .engage
                .and_then(|t| hostile_target(w, i, t))
                .filter(|&ti| e.edge_distance(&w.entities[ti]) <= acquire);
            let engage = match keep {
                Some(ti) => Some(w.entities[ti].id),
                None => nearest_enemy(w, i, acquire, |_| true),
            };
            let e = &mut w.entities[i];
            e.engage = engage;
            e.chase = true;
            if engage.is_none() {
                if e.pos.distance(p) < 4.0 {
                    e.order = Order::Idle;
                    e.move_goal = None;
                } else {
                    e.move_goal = Some(p);
                }
            }
        }
        Order::Cast { slot, target } => {
            w.entities[i].engage = None;
            match abilities::try_pending_cast(w, i, slot, target) {
                PendingResult::Invalid | PendingResult::Cast => {
                    let e = &mut w.entities[i];
                    if matches!(e.order, Order::Cast { .. }) {
                        e.order = Order::Idle;
                    }
                    e.move_goal = None;
                }
                PendingResult::Walk(goal) => {
                    w.entities[i].move_goal = Some(goal);
                }
            }
        }
    }
    let e = &w.entities[i];
    if e.rooted() {
        let e = &mut w.entities[i];
        e.move_goal = None;
    }
}

// ------------------------------------------------------------------ minions

fn minion_think(w: &mut World, i: usize) {
    let (team, pos, lane, waypoint) = {
        let e = &w.entities[i];
        let UnitKind::Minion(m) = &e.kind else { return };
        (e.team, e.pos, m.lane, m.waypoint)
    };
    if w.entities[i].stunned() {
        w.entities[i].move_goal = None;
        return;
    }
    // Drop targets that died, vanished or ran too far.
    let current = w.entities[i].engage.and_then(|t| hostile_target(w, i, t)).filter(|&ti| {
        let t = &w.entities[ti];
        t.pos.distance(pos) <= MINION_CHASE && structure_attackable(t)
    });
    let mut target = current.map(|ti| w.entities[ti].id);
    if let Some(h) = call_for_help(w, team, pos, MINION_ACQUIRE + 40.0) {
        target = Some(h);
    }
    if target.is_none() {
        target = nearest_enemy(w, i, MINION_ACQUIRE, |t| t.is_minion())
            .or_else(|| nearest_enemy(w, i, MINION_ACQUIRE + 30.0, |t| t.is_structure() && structure_attackable(t)))
            .or_else(|| nearest_enemy(w, i, MINION_ACQUIRE, |t| t.is_hero()));
    }
    let path = &w.map.lanes[team.index()][lane as usize];
    let mut wp = waypoint.min(path.len() - 1);
    if wp + 1 < path.len() && pos.distance(path[wp]) < 40.0 {
        wp += 1;
    }
    let goal = path[wp];
    let e = &mut w.entities[i];
    if let UnitKind::Minion(m) = &mut e.kind {
        m.waypoint = wp;
    }
    e.engage = target;
    e.chase = true;
    e.move_goal = if target.is_some() { None } else { Some(goal) };
}

/// Structures protected by an earlier tower in the lane can't be attacked.
fn structure_attackable(t: &Entity) -> bool {
    !t.is_structure() || !t.invulnerable
}

// ------------------------------------------------------------------ towers

fn tower_think(w: &mut World, i: usize) {
    // Update protection state first (cheap, all structures share one rule).
    let invuln = structure_protected(w, i);
    let e = &mut w.entities[i];
    e.invulnerable = invuln;
    let UnitKind::Structure(s) = &e.kind else {
        return;
    };
    if !matches!(s.kind, StructureKind::Tower { .. }) {
        return;
    }
    let (team, pos, radius) = (e.team, e.pos, e.radius);
    let in_range = |w: &World, ti: usize| -> bool {
        let t = &w.entities[ti];
        t.targetable()
            && t.team != team
            && t.team != Team::Neutral
            && t.pos.distance(pos) <= TOWER_RANGE + t.radius + radius
    };
    let current = w.entities[i].engage.and_then(|t| w.idx(t)).filter(|&ti| in_range(w, ti) && w.visible_to(team, ti));
    let mut target = current.map(|ti| w.entities[ti].id);
    // Champions attacking allied champions under the tower take priority.
    if let Some(h) = call_for_help(w, team, pos, TOWER_RANGE) {
        let is_current_hero = current.map(|ti| w.entities[ti].is_hero()).unwrap_or(false);
        if !is_current_hero {
            target = Some(h);
        }
    }
    if target.is_none() {
        let mut best: Option<(u8, f32, EntityId)> = None;
        for (ti, t) in w.entities.iter().enumerate() {
            if !in_range(w, ti) || !w.visible_to(team, ti) {
                continue;
            }
            let prio = if t.is_minion() {
                0
            } else if t.is_hero() {
                1
            } else {
                2
            };
            let d = t.pos.distance(pos);
            if best.is_none_or(|(bp, bd, _)| (prio, d) < (bp, bd)) {
                best = Some((prio, d, t.id));
            }
        }
        target = best.map(|b| b.2);
    }
    let e = &mut w.entities[i];
    if target != e.engage {
        if let UnitKind::Structure(s) = &mut e.kind {
            s.streak = 0;
        }
    }
    e.engage = target;
    e.chase = false;
    e.move_goal = None;
}

/// LoL structure rules: inner towers need the outer one dead, etc.
fn structure_protected(w: &World, i: usize) -> bool {
    let e = &w.entities[i];
    let Some(s) = e.structure() else { return false };
    let team = e.team;
    let dead = |pred: &dyn Fn(StructureKind) -> bool| {
        w.entities.iter().any(|o| o.team == team && !o.alive && o.structure().map(|x| pred(x.kind)).unwrap_or(false))
    };
    match s.kind {
        StructureKind::Tower { lane, tier } if lane < 3 && tier > 1 => {
            !dead(&|k| k == StructureKind::Tower { lane, tier: tier - 1 })
        }
        StructureKind::Inhibitor { lane } => !dead(&|k| k == StructureKind::Tower { lane, tier: 3 }),
        StructureKind::Tower { tier: 4, .. } => !dead(&|k| matches!(k, StructureKind::Inhibitor { .. })),
        StructureKind::Nexus => {
            let alive_nexus_towers = w
                .entities
                .iter()
                .filter(|o| {
                    o.team == team
                        && o.alive
                        && matches!(o.structure(), Some(x) if matches!(x.kind, StructureKind::Tower { tier: 4, .. }))
                })
                .count();
            alive_nexus_towers > 0
        }
        _ => false,
    }
}

// ------------------------------------------------------------------ monsters

fn monster_think(w: &mut World, i: usize) {
    let (camp, home, resetting, pos) = {
        let e = &w.entities[i];
        let UnitKind::Monster(m) = &e.kind else {
            return;
        };
        (m.camp, m.home, m.resetting, e.pos)
    };
    if resetting {
        let e = &mut w.entities[i];
        e.engage = None;
        e.invulnerable = true;
        e.hp = (e.hp + e.stats.max_hp * 0.25 * DT).min(e.stats.max_hp);
        if pos.distance(home) < 6.0 {
            e.hp = e.stats.max_hp;
            e.invulnerable = false;
            e.move_goal = None;
            e.statuses.clear();
            if let UnitKind::Monster(m) = &mut e.kind {
                m.resetting = false;
            }
        } else {
            e.move_goal = Some(home);
        }
        return;
    }
    if w.entities[i].stunned() {
        return;
    }
    let aggro = w.camps[camp].aggro;
    let target_ok = aggro.and_then(|t| w.idx(t)).filter(|&ti| {
        let t = &w.entities[ti];
        t.targetable() && t.pos.distance(home) <= MONSTER_LEASH + 60.0
    });
    let too_far = pos.distance(home) > MONSTER_LEASH;
    match (target_ok, aggro) {
        (Some(ti), _) if !too_far => {
            let id = w.entities[ti].id;
            let e = &mut w.entities[i];
            e.engage = Some(id);
            e.chase = true;
        }
        (_, Some(_)) => {
            // Leash: everyone in the camp walks home and heals.
            w.camps[camp].aggro = None;
            let members = w.camps[camp].members.clone();
            for m in members {
                if let Some(mi) = w.idx(m) {
                    if let UnitKind::Monster(ms) = &mut w.entities[mi].kind {
                        ms.resetting = true;
                    }
                    w.entities[mi].engage = None;
                }
            }
        }
        _ => {
            let e = &mut w.entities[i];
            e.engage = None;
            e.move_goal = if pos.distance(home) > 8.0 { Some(home) } else { None };
        }
    }
}
