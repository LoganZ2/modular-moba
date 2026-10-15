//! Skill casting: validation, wind-ups, the generic [`Action`] interpreter,
//! effect application, delayed zones and traps, summoner spells, the shop
//! and skill level-ups.

use super::combat::{self, Hit};
use super::*;
use crate::items::{try_item, ItemTier, POTION, POTION_HEAL, POTION_SECS, SELL_RATIO};
use crate::map::FOUNTAIN_RADIUS;
use crate::math::angle_diff;
use crate::protocol::ProjectileStyle;
use crate::skills::{skill, Action, Effect, SkillDef, Targeting};
use crate::stats::haste_cooldown;
use crate::summoners::SummonerId;

/// Range slack so clicks right at the edge still count.
const RANGE_SLACK: f32 = 6.0;

fn skill_of(e: &Entity, slot: usize) -> Option<(&'static SkillDef, u8)> {
    let h = e.hero()?;
    if slot >= 4 {
        return None;
    }
    Some((skill(h.skills[slot]), h.ranks[slot]))
}

/// Is `unit` a legal target for `targeting` when cast by entity `i`?
fn valid_unit_target(w: &World, i: usize, targeting: &Targeting, unit: EntityId) -> bool {
    let Some(ti) = w.idx(unit) else { return false };
    let (e, t) = (&w.entities[i], &w.entities[ti]);
    if !t.targetable() || t.is_structure() {
        return false;
    }
    match targeting {
        Targeting::Enemy { .. } => t.team != e.team && w.visible_to(e.team, ti),
        Targeting::EnemyHero { .. } => t.team != e.team && t.is_hero() && w.visible_to(e.team, ti),
        Targeting::Ally { .. } => t.team == e.team && t.is_hero(),
        _ => false,
    }
}

/// Player pressed a skill key.
pub(crate) fn command_cast(w: &mut World, i: usize, slot: u8, mut target: CastTarget) {
    let slot_u = slot as usize;
    let Some((sk, rank)) = skill_of(&w.entities[i], slot_u) else {
        return;
    };
    {
        let e = &w.entities[i];
        let h = e.hero().unwrap();
        if rank == 0
            || h.cooldowns[slot_u] > 0.0
            || e.mana < sk.mana_at(rank)
            || h.cast.is_some()
            || e.stunned()
            || e.silenced()
            || e.taunted_by().is_some()
            || e.dash.is_some()
        {
            return;
        }
        let moves_self = matches!(sk.action, Action::Dash { .. } | Action::Blink { .. });
        if moves_self && e.rooted() {
            return;
        }
    }
    let pos = w.entities[i].pos;
    match sk.targeting {
        Targeting::Instant | Targeting::Direction { .. } => begin_cast(w, i, slot, target),
        Targeting::Point { range } => {
            let clamps = matches!(sk.action, Action::Dash { .. } | Action::Blink { .. });
            if clamps || pos.distance(target.point) <= range + RANGE_SLACK {
                begin_cast(w, i, slot, target);
            } else {
                w.entities[i].order = Order::Cast { slot, target };
            }
        }
        Targeting::Ally { range } => {
            let self_id = w.entities[i].id;
            let unit = target.unit.filter(|u| *u != self_id && valid_unit_target(w, i, &sk.targeting, *u));
            match unit {
                None => {
                    target.unit = Some(self_id);
                    begin_cast(w, i, slot, target);
                }
                Some(u) => {
                    let ti = w.idx(u).unwrap();
                    if w.entities[i].edge_distance(&w.entities[ti]) <= range + RANGE_SLACK {
                        begin_cast(w, i, slot, target);
                    } else {
                        w.entities[i].order = Order::Cast { slot, target };
                    }
                }
            }
        }
        Targeting::Enemy { range } | Targeting::EnemyHero { range } => {
            let Some(u) = target.unit.filter(|u| valid_unit_target(w, i, &sk.targeting, *u)) else {
                return;
            };
            let ti = w.idx(u).unwrap();
            if w.entities[i].edge_distance(&w.entities[ti]) <= range + RANGE_SLACK {
                begin_cast(w, i, slot, target);
            } else {
                w.entities[i].order = Order::Cast { slot, target };
            }
        }
    }
}

/// A queued `Order::Cast` — returns true once the skill went off (or the order
/// became invalid) so the caller can clear the order.
pub(crate) fn try_pending_cast(w: &mut World, i: usize, slot: u8, target: CastTarget) -> PendingResult {
    let Some((sk, rank)) = skill_of(&w.entities[i], slot as usize) else {
        return PendingResult::Invalid;
    };
    {
        let e = &w.entities[i];
        let h = e.hero().unwrap();
        if rank == 0 || h.cooldowns[slot as usize] > 0.0 || e.mana < sk.mana_at(rank) || e.silenced() {
            return PendingResult::Invalid;
        }
    }
    let range = sk.range();
    let goal = match target.unit {
        Some(u) if sk.targeting.needs_unit() => {
            if !valid_unit_target(w, i, &sk.targeting, u) {
                return PendingResult::Invalid;
            }
            let ti = w.idx(u).unwrap();
            if w.entities[i].edge_distance(&w.entities[ti]) <= range {
                begin_cast(w, i, slot, target);
                return PendingResult::Cast;
            }
            w.entities[ti].pos
        }
        _ => {
            if w.entities[i].pos.distance(target.point) <= range {
                begin_cast(w, i, slot, target);
                return PendingResult::Cast;
            }
            target.point
        }
    };
    PendingResult::Walk(goal)
}

pub(crate) enum PendingResult {
    Invalid,
    Cast,
    Walk(Vec2),
}

fn begin_cast(w: &mut World, i: usize, slot: u8, target: CastTarget) {
    let slot_u = slot as usize;
    let Some((sk, rank)) = skill_of(&w.entities[i], slot_u) else {
        return;
    };
    let e = &mut w.entities[i];
    let haste = e.stats.haste;
    e.mana -= sk.mana_at(rank);
    e.windup = None;
    let cd = haste_cooldown(sk.cooldown_at(rank), haste);
    let h = e.hero_mut().unwrap();
    h.cooldowns[slot_u] = cd;
    h.cooldown_max[slot_u] = cd;
    h.recall = None;
    if matches!(e.order, Order::Cast { .. }) {
        e.order = Order::Idle;
    }
    if sk.cast_time > 0.0 {
        let total = sk.cast_time;
        let h = e.hero_mut().unwrap();
        h.cast = Some(PendingCast { slot, target, remaining: total, total });
        e.path.clear();
        e.move_goal = None;
        let face = target.point - e.pos;
        if face.length_sq() > 1.0 {
            e.facing = face.angle();
        }
    } else {
        execute(w, i, slot_u, target);
    }
}

/// Advance wind-ups and recalls.
pub(crate) fn update_casts(w: &mut World) {
    for i in 0..w.entities.len() {
        let e = &mut w.entities[i];
        if !e.alive || !e.is_hero() {
            continue;
        }
        let interrupted = e.stunned() || e.silenced();
        let h = e.hero_mut().unwrap();
        if let Some(mut c) = h.cast {
            if interrupted {
                h.cast = None;
            } else {
                c.remaining -= DT;
                if c.remaining <= 0.0 {
                    h.cast = None;
                    execute(w, i, c.slot as usize, c.target);
                } else {
                    h.cast = Some(c);
                }
            }
        }
        let e = &mut w.entities[i];
        let team = e.team;
        let h = e.hero_mut().unwrap();
        if let Some(r) = h.recall {
            let r = r - DT;
            if r <= 0.0 {
                h.recall = None;
                let n = h.player % 5;
                let from = e.pos;
                let to = w.map.spawn_point(team, n);
                e.pos = to;
                e.path.clear();
                e.move_goal = None;
                e.order = Order::Idle;
                w.event(Audience::At(from), FxEvent::Blink { from, to, color: 0x8080ff });
            } else {
                h.recall = Some(r);
            }
        }
    }
}

fn projectile_style(sk: &SkillDef, on_hit: &[Effect], width: f32) -> ProjectileStyle {
    if on_hit.iter().any(|e| matches!(e, Effect::Pull)) {
        ProjectileStyle::Hook
    } else if width >= 20.0 {
        ProjectileStyle::Beam
    } else if sk.roles.contains(&crate::roles::Role::Tank) && !sk.ultimate {
        ProjectileStyle::Shard
    } else if on_hit.iter().any(|e| matches!(e, Effect::Damage(DamageKind::Physical, _))) {
        ProjectileStyle::Arrow
    } else {
        ProjectileStyle::Bolt
    }
}

fn clamp_point(from: Vec2, to: Vec2, range: f32) -> Vec2 {
    from + (to - from).clamp_length(range)
}

/// Make the skill in `slot` happen now.
fn execute(w: &mut World, i: usize, slot: usize, target: CastTarget) {
    let Some((sk, rank)) = skill_of(&w.entities[i], slot) else {
        return;
    };
    let (id, team, pos, stats, facing) = {
        let e = &w.entities[i];
        (e.id, e.team, e.pos, e.stats, e.facing)
    };
    if !w.entities[i].alive {
        return;
    }
    let range = sk.range();
    let aim = match (target.unit, sk.targeting.needs_unit()) {
        (Some(u), true) => w.get(u).map(|t| t.pos).unwrap_or(target.point),
        _ => target.point,
    };
    let dir = (aim - pos).normalize_or(Vec2::from_angle(facing));
    w.entities[i].facing = dir.angle();
    let to = match sk.targeting {
        Targeting::Instant => pos,
        _ => clamp_point(pos, aim, range.max(1.0)),
    };
    w.event(Audience::At(pos), FxEvent::Cast { caster: id, skill: sk.id, from: pos, to });
    apply_effects(w, id, team, &stats, rank, id, sk.self_effects, pos);

    match sk.action {
        Action::None => {}
        Action::Projectile { speed, width, pierce, on_hit } => {
            let homing = if sk.targeting.needs_unit() { target.unit } else { None };
            if sk.targeting.needs_unit() && homing.and_then(|u| w.get(u)).map(|t| !t.alive).unwrap_or(true) {
                return;
            }
            let pid = w.alloc_misc_id();
            w.projectiles.push(Projectile {
                id: pid,
                owner: id,
                team,
                pos,
                dir,
                speed,
                target: homing,
                remaining: range,
                width,
                pierce,
                style: projectile_style(sk, on_hit, width),
                color: sk.color,
                payload: Payload::Skill { skill: sk.id, rank, stats },
                hit: Vec::new(),
                dead: false,
            });
        }
        Action::Area { radius, delay, at_target, enemy, ally } => {
            let center = if at_target { clamp_point(pos, target.point, range) } else { pos };
            if delay > 0.0 {
                spawn_zone(w, id, team, center, ZoneArea::Circle(radius), ZoneKind::Delayed, delay, sk, rank, stats);
            } else {
                area_apply(w, id, team, &stats, rank, center, radius, enemy, ally);
                w.event(Audience::At(center), FxEvent::Burst { pos: center, radius, color: sk.color });
            }
        }
        Action::Cone { half_angle_deg, enemy } => {
            let half = half_angle_deg.to_radians();
            let a = dir.angle();
            let victims: Vec<EntityId> = w
                .entities
                .iter()
                .filter(|e| {
                    if !e.alive || !e.targetable() || e.is_structure() || e.team == team {
                        return false;
                    }
                    let d = e.pos - pos;
                    let dist = d.length();
                    if dist > range + e.radius {
                        return false;
                    }
                    let slack = if dist > 1.0 { (e.radius / dist).atan() } else { std::f32::consts::PI };
                    angle_diff(d.angle(), a).abs() <= half + slack
                })
                .map(|e| e.id)
                .collect();
            for v in victims {
                apply_effects(w, id, team, &stats, rank, v, enemy, pos);
            }
            w.event(Audience::At(pos), FxEvent::Cone { pos, angle: a, half_angle: half, range, color: sk.color });
        }
        Action::Line { width, delay, enemy, ally } => {
            let area = ZoneArea::Line { dir, length: range, width };
            if delay > 0.0 {
                spawn_zone(w, id, team, pos, area, ZoneKind::Delayed, delay, sk, rank, stats);
            } else {
                line_apply(w, id, team, &stats, rank, pos, dir, range, width, enemy, ally);
                w.event(Audience::At(pos), FxEvent::Beam { from: pos, to: pos + dir * range, width, color: sk.color });
            }
        }
        Action::Dash { speed, unstoppable, .. } => {
            let unit = if sk.targeting.needs_unit() { target.unit } else { None };
            let mut dest = match sk.targeting {
                Targeting::Direction { range } => pos + dir * range,
                _ => clamp_point(pos, target.point, range),
            };
            if unit.is_none() {
                dest = last_walkable_on_line(w, pos, dest);
            }
            let dist = pos.distance(unit.and_then(|u| w.get(u)).map(|t| t.pos).unwrap_or(dest));
            let e = &mut w.entities[i];
            e.path.clear();
            e.move_goal = None;
            e.windup = None;
            e.dash = Some(Dash {
                to: dest,
                target: unit,
                speed,
                kind: DashKind::Skill { skill: sk.id, rank, stats, hit: Vec::new() },
                time_left: dist / speed + 0.5,
            });
            if unstoppable {
                e.add_status(StatusKind::Unstoppable, dist / speed + 0.15);
            }
        }
        Action::Blink { arrive_radius, arrive } => {
            let dest = w.map.nearest_walkable(clamp_point(pos, target.point, range));
            let e = &mut w.entities[i];
            e.pos = dest;
            e.path.clear();
            e.move_goal = None;
            w.event(Audience::At(pos), FxEvent::Blink { from: pos, to: dest, color: sk.color });
            if arrive_radius > 0.0 {
                area_apply(w, id, team, &stats, rank, dest, arrive_radius, arrive, &[]);
                w.event(Audience::At(dest), FxEvent::Burst { pos: dest, radius: arrive_radius, color: sk.color });
            }
        }
        Action::Unit { effects } => {
            if let Some(u) = target.unit {
                if u != id {
                    apply_effects(w, id, team, &stats, rank, u, effects, pos);
                }
            }
        }
        Action::Trap { radius, lifetime, .. } => {
            let at = w.map.nearest_walkable(clamp_point(pos, target.point, range));
            // At most three traps per champion.
            let mine: Vec<u32> =
                w.zones.iter().filter(|z| z.owner == id && z.kind == ZoneKind::Trap && !z.dead).map(|z| z.id).collect();
            if mine.len() >= 3 {
                if let Some(z) = w.zones.iter_mut().find(|z| z.id == mine[0]) {
                    z.dead = true;
                }
            }
            spawn_zone(w, id, team, at, ZoneArea::Circle(radius), ZoneKind::Trap, lifetime, sk, rank, stats);
        }
    }
}

/// Walk back from `to` toward `from` until the spot is walkable.
fn last_walkable_on_line(w: &World, from: Vec2, to: Vec2) -> Vec2 {
    if w.map.walkable(to) {
        return to;
    }
    let d = to - from;
    let steps = (d.length() / 8.0).ceil().max(1.0) as usize;
    for s in (0..steps).rev() {
        let p = from + d * (s as f32 / steps as f32);
        if w.map.walkable(p) {
            return p;
        }
    }
    from
}

#[allow(clippy::too_many_arguments)]
fn spawn_zone(
    w: &mut World,
    owner: EntityId,
    team: Team,
    pos: Vec2,
    area: ZoneArea,
    kind: ZoneKind,
    timer: f32,
    sk: &SkillDef,
    rank: u8,
    stats: Stats,
) {
    let id = w.alloc_misc_id();
    w.zones.push(Zone {
        id,
        owner,
        team,
        pos,
        area,
        kind,
        timer,
        total: timer,
        skill: sk.id,
        rank,
        stats,
        dead: false,
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn area_apply(
    w: &mut World,
    caster: EntityId,
    team: Team,
    stats: &Stats,
    rank: u8,
    center: Vec2,
    radius: f32,
    enemy: &[Effect],
    ally: &[Effect],
) {
    let mut enemies = Vec::new();
    let mut allies = Vec::new();
    for e in &w.entities {
        if !e.alive || e.is_structure() || e.pos.distance(center) > radius + e.radius {
            continue;
        }
        if e.team != team && e.targetable() {
            enemies.push(e.id);
        } else if e.team == team && e.is_hero() {
            allies.push(e.id);
        }
    }
    if !enemy.is_empty() {
        for v in enemies {
            apply_effects(w, caster, team, stats, rank, v, enemy, center);
        }
    }
    if !ally.is_empty() {
        for a in allies {
            apply_effects(w, caster, team, stats, rank, a, ally, center);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn line_apply(
    w: &mut World,
    caster: EntityId,
    team: Team,
    stats: &Stats,
    rank: u8,
    from: Vec2,
    dir: Vec2,
    length: f32,
    width: f32,
    enemy: &[Effect],
    ally: &[Effect],
) {
    let to = from + dir * length;
    let mut enemies = Vec::new();
    let mut allies = Vec::new();
    for e in &w.entities {
        if !e.alive || e.is_structure() {
            continue;
        }
        if crate::math::dist_point_segment(e.pos, from, to) > width / 2.0 + e.radius {
            continue;
        }
        if e.team != team && e.targetable() {
            enemies.push(e.id);
        } else if e.team == team && e.is_hero() {
            allies.push(e.id);
        }
    }
    for v in enemies {
        apply_effects(w, caster, team, stats, rank, v, enemy, from);
    }
    for a in allies {
        apply_effects(w, caster, team, stats, rank, a, ally, from);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_effects(
    w: &mut World,
    caster: EntityId,
    team: Team,
    stats: &Stats,
    rank: u8,
    target: EntityId,
    effects: &[Effect],
    origin: Vec2,
) {
    for eff in effects {
        apply_effect(w, caster, team, stats, rank, target, eff, origin);
    }
}

/// Stop channels, recalls, wind-ups and (stoppable) dashes.
fn interrupt(e: &mut Entity) {
    e.windup = None;
    if !e.unstoppable() {
        if let Some(Dash { kind: DashKind::Skill { .. }, .. }) = e.dash {
            e.dash = None;
        }
    }
    if let Some(h) = e.hero_mut() {
        h.recall = None;
        h.cast = None;
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_effect(
    w: &mut World,
    caster: EntityId,
    _team: Team,
    stats: &Stats,
    rank: u8,
    target: EntityId,
    eff: &Effect,
    origin: Vec2,
) {
    let Some(ti) = w.idx(target) else { return };
    if !w.entities[ti].alive {
        return;
    }
    let cc_immune = {
        let t = &w.entities[ti];
        t.is_structure() || t.is_epic_monster() || t.unstoppable()
    };
    match *eff {
        Effect::Damage(kind, sc) => {
            let t = &w.entities[ti];
            let missing = t.stats.max_hp - t.hp;
            let amount = sc.eval_vs(rank, stats, missing);
            combat::deal_damage(w, caster, target, amount, kind, Hit::SKILL);
        }
        Effect::Heal(sc) => combat::heal(w, ti, sc.eval(rank, stats)),
        Effect::Shield { amount, secs } => {
            let v = amount.eval(rank, stats);
            w.entities[ti].add_status(StatusKind::Shield(v), secs);
        }
        Effect::Buff { stat, amount, secs } => {
            let v = amount.eval(rank, stats);
            w.entities[ti].add_status(StatusKind::Buff(stat, v), secs);
        }
        Effect::Empower(sc) => {
            let v = sc.eval(rank, stats);
            if let Some(h) = w.entities[ti].hero_mut() {
                h.empower += v;
            }
        }
        Effect::Vulnerable { pct, secs } => {
            if !w.entities[ti].is_structure() {
                w.entities[ti].add_status(StatusKind::Vulnerable(pct), secs);
            }
        }
        _ if cc_immune => {}
        Effect::Slow { pct, secs } => w.entities[ti].add_status(StatusKind::Slow(pct), secs),
        Effect::Stun(secs) => {
            let t = &mut w.entities[ti];
            t.add_status(StatusKind::Stun, secs);
            interrupt(t);
        }
        Effect::Root(secs) => {
            let t = &mut w.entities[ti];
            t.add_status(StatusKind::Root, secs);
            t.path.clear();
        }
        Effect::Knockup(secs) => {
            let t = &mut w.entities[ti];
            t.add_status(StatusKind::Airborne, secs);
            interrupt(t);
        }
        Effect::Silence(secs) => {
            let t = &mut w.entities[ti];
            t.add_status(StatusKind::Silence, secs);
            if let Some(h) = t.hero_mut() {
                h.cast = None;
            }
        }
        Effect::Taunt(secs) => {
            let t = &mut w.entities[ti];
            t.add_status(StatusKind::Taunt(caster), secs);
            if let Some(h) = t.hero_mut() {
                h.cast = None;
                h.recall = None;
            }
            t.engage = Some(caster);
        }
        Effect::Blind(secs) => w.entities[ti].add_status(StatusKind::Blind, secs),
        Effect::Knockback { dist, wall_stun } => {
            let t = &mut w.entities[ti];
            let dir = (t.pos - origin).normalize_or(Vec2::from_angle(t.facing + std::f32::consts::PI));
            let speed = 450.0;
            let to = t.pos + dir * dist;
            interrupt(t);
            t.dash = Some(Dash {
                to,
                target: None,
                speed,
                kind: DashKind::Knockback { wall_stun },
                time_left: dist / speed + 0.1,
            });
            t.add_status(StatusKind::Airborne, dist / speed);
        }
        Effect::Pull => {
            let Some(cpos) = w.get(caster).map(|c| (c.pos, c.radius)) else {
                return;
            };
            let t = &mut w.entities[ti];
            let dir = (t.pos - cpos.0).normalize_or(Vec2::new(1.0, 0.0));
            let to = cpos.0 + dir * (cpos.1 + t.radius + 4.0);
            let dist = t.pos.distance(to);
            let speed = 600.0;
            interrupt(t);
            t.dash = Some(Dash { to, target: None, speed, kind: DashKind::Pull, time_left: dist / speed + 0.1 });
            t.add_status(StatusKind::Airborne, dist / speed);
        }
    }
}

/// Delayed blasts and traps.
pub(crate) fn update_zones(w: &mut World) {
    let mut zones = std::mem::take(&mut w.zones);
    for z in zones.iter_mut() {
        if z.dead {
            continue;
        }
        z.timer -= DT;
        let sk = skill(z.skill);
        match z.kind {
            ZoneKind::Delayed => {
                if z.timer > 0.0 {
                    continue;
                }
                z.dead = true;
                match (&z.area, sk.action) {
                    (ZoneArea::Circle(r), Action::Area { enemy, ally, .. }) => {
                        area_apply(w, z.owner, z.team, &z.stats, z.rank, z.pos, *r, enemy, ally);
                        w.event(Audience::At(z.pos), FxEvent::Burst { pos: z.pos, radius: *r, color: sk.color });
                    }
                    (ZoneArea::Line { dir, length, width }, Action::Line { enemy, ally, .. }) => {
                        line_apply(w, z.owner, z.team, &z.stats, z.rank, z.pos, *dir, *length, *width, enemy, ally);
                        w.event(
                            Audience::At(z.pos),
                            FxEvent::Beam { from: z.pos, to: z.pos + *dir * *length, width: *width, color: sk.color },
                        );
                    }
                    _ => {}
                }
            }
            ZoneKind::Trap => {
                if z.timer <= 0.0 {
                    z.dead = true;
                    continue;
                }
                let ZoneArea::Circle(r) = z.area else {
                    continue;
                };
                let victim = w
                    .entities
                    .iter()
                    .find(|e| {
                        e.alive
                            && e.is_hero()
                            && e.team != z.team
                            && e.targetable()
                            && e.pos.distance(z.pos) <= r + e.radius
                    })
                    .map(|e| e.id);
                if let (Some(v), Action::Trap { effects, .. }) = (victim, sk.action) {
                    z.dead = true;
                    apply_effects(w, z.owner, z.team, &z.stats, z.rank, v, effects, z.pos);
                    w.event(Audience::At(z.pos), FxEvent::Burst { pos: z.pos, radius: r * 1.5, color: sk.color });
                }
            }
        }
    }
    zones.append(&mut w.zones);
    w.zones = zones;
}

/// Finish a dash: apply landing effects.
pub(crate) fn finish_dash(w: &mut World, i: usize, dash: &Dash) {
    if let DashKind::Skill { skill: sid, rank, stats, .. } = &dash.kind {
        let sk = skill(*sid);
        if let Action::Dash { arrive_radius, arrive, .. } = sk.action {
            if arrive_radius > 0.0 {
                let (id, team, pos) = {
                    let e = &w.entities[i];
                    (e.id, e.team, e.pos)
                };
                area_apply(w, id, team, stats, *rank, pos, arrive_radius, arrive, &[]);
                w.event(Audience::At(pos), FxEvent::Burst { pos, radius: arrive_radius, color: sk.color });
            }
        }
    }
}

// ------------------------------------------------------------------ summoners

pub(crate) fn cast_summoner(w: &mut World, i: usize, slot: u8, target: CastTarget) {
    let slot = slot as usize;
    if slot > 1 {
        return;
    }
    let (spell, cd, level, team, pos, id) = {
        let e = &w.entities[i];
        let Some(h) = e.hero() else { return };
        if e.stunned() || h.cast.is_some() {
            return;
        }
        (h.summoners[slot], h.summoner_cd[slot], h.level as f32, e.team, e.pos, e.id)
    };
    if cd > 0.0 {
        return;
    }
    // Ignite / Exhaust need an enemy champion near the cursor.
    let enemy_target = if spell.needs_enemy_hero() {
        let range = spell.range();
        let pick = w
            .entities
            .iter()
            .enumerate()
            .filter(|(ti, t)| t.alive && t.is_hero() && t.team != team && t.targetable() && w.visible_to(team, *ti))
            .filter(|(_, t)| t.edge_distance(&w.entities[i]) <= range)
            .min_by(|a, b| {
                let da = if Some(a.1.id) == target.unit { -1.0 } else { a.1.pos.distance(target.point) };
                let db = if Some(b.1.id) == target.unit { -1.0 } else { b.1.pos.distance(target.point) };
                da.total_cmp(&db)
            })
            .map(|(ti, _)| ti);
        match pick {
            Some(t) => Some(t),
            None => return,
        }
    } else {
        None
    };
    match spell {
        SummonerId::Flash => {
            if w.entities[i].dash.is_some() {
                return;
            }
            let dest = last_walkable_on_line(w, pos, pos + (target.point - pos).clamp_length(spell.range()));
            let e = &mut w.entities[i];
            e.pos = dest;
            e.path.clear();
            e.repath = 0.0;
            w.event(Audience::At(pos), FxEvent::Blink { from: pos, to: dest, color: spell.color() });
        }
        SummonerId::Heal => {
            let amount = 80.0 + 15.0 * level;
            combat::heal(w, i, amount);
            w.entities[i].add_status(StatusKind::Buff(crate::skills::BuffStat::MoveSpeed, 0.3), 1.0);
            let ally = (0..w.entities.len())
                .filter(|&a| a != i)
                .filter(|&a| {
                    let e = &w.entities[a];
                    e.alive && e.is_hero() && e.team == team && e.pos.distance(pos) <= 220.0
                })
                .min_by(|&a, &b| w.entities[a].hp_frac().total_cmp(&w.entities[b].hp_frac()));
            if let Some(a) = ally {
                combat::heal(w, a, amount);
                w.entities[a].add_status(StatusKind::Buff(crate::skills::BuffStat::MoveSpeed, 0.3), 1.0);
            }
            w.event(Audience::At(pos), FxEvent::Burst { pos, radius: 30.0, color: spell.color() });
        }
        SummonerId::Ghost => w.entities[i].add_status(StatusKind::Ghost, 8.0),
        SummonerId::Barrier => {
            w.entities[i].add_status(StatusKind::Shield(100.0 + 18.0 * level), 2.5);
            w.event(Audience::At(pos), FxEvent::Burst { pos, radius: 22.0, color: spell.color() });
        }
        SummonerId::Ignite => {
            let t = enemy_target.unwrap();
            let total = 50.0 + 20.0 * level;
            w.entities[t].add_status(StatusKind::Burn { dps: total / 5.0, source: id }, 5.0);
            let tp = w.entities[t].pos;
            w.event(Audience::At(tp), FxEvent::Burst { pos: tp, radius: 16.0, color: spell.color() });
        }
        SummonerId::Exhaust => {
            let t = enemy_target.unwrap();
            let te = &mut w.entities[t];
            te.add_status(StatusKind::Slow(0.35), 2.5);
            te.add_status(StatusKind::Exhausted, 2.5);
            let tp = te.pos;
            w.event(Audience::At(tp), FxEvent::Burst { pos: tp, radius: 16.0, color: spell.color() });
        }
    }
    let e = &mut w.entities[i];
    cancel_recall(e);
    if let Some(h) = e.hero_mut() {
        h.summoner_cd[slot] = spell.cooldown();
    }
}

// ------------------------------------------------------------------ progression & shop

/// Can this hero put a point into `slot` right now?
pub fn can_level(h: &HeroState, slot: usize) -> bool {
    if slot >= 4 || h.skill_points == 0 {
        return false;
    }
    let rank = h.ranks[slot];
    let max = skill(h.skills[slot]).max_rank();
    if rank >= max {
        return false;
    }
    if slot == 3 {
        let need = [6, 11, 16][rank as usize];
        h.level >= need
    } else {
        rank < h.level.div_ceil(2)
    }
}

pub(crate) fn level_up(w: &mut World, i: usize, slot: u8) {
    if let Some(h) = w.entities[i].hero_mut() {
        let slot = slot as usize;
        if can_level(h, slot) {
            h.ranks[slot] += 1;
            h.skill_points -= 1;
        }
    }
}

pub fn in_shop_range(w: &World, e: &Entity) -> bool {
    !e.alive || e.pos.distance(w.map.fountain(e.team)) <= FOUNTAIN_RADIUS + 40.0
}

pub(crate) fn buy(w: &mut World, i: usize, item: crate::items::ItemId) {
    let Some(def) = try_item(item) else { return };
    if !in_shop_range(w, &w.entities[i]) {
        return;
    }
    let Some(h) = w.entities[i].hero_mut() else {
        return;
    };
    let cost = def.cost as f32;
    if h.gold + 0.01 < cost {
        return;
    }
    if def.stack > 1 {
        if let Some(slot) = h.items.iter_mut().flatten().find(|(id, n)| *id == item && *n < def.stack) {
            slot.1 += 1;
            h.gold -= cost;
            return;
        }
    }
    if let Some(slot) = h.items.iter_mut().find(|s| s.is_none()) {
        *slot = Some((item, 1));
        h.gold -= cost;
    }
}

pub(crate) fn sell(w: &mut World, i: usize, slot: u8) {
    if !in_shop_range(w, &w.entities[i]) {
        return;
    }
    let Some(h) = w.entities[i].hero_mut() else {
        return;
    };
    let Some(Some((id, n))) = h.items.get(slot as usize).copied() else {
        return;
    };
    let def = crate::items::item(id);
    h.gold += def.cost as f32 * SELL_RATIO * n as f32;
    h.items[slot as usize] = None;
}

pub(crate) fn use_item(w: &mut World, i: usize, slot: u8) {
    let e = &mut w.entities[i];
    if e.has(|k| matches!(k, StatusKind::Regen(_))) {
        return;
    }
    let Some(h) = e.hero_mut() else { return };
    let Some(Some((id, n))) = h.items.get(slot as usize).copied() else {
        return;
    };
    if id != POTION || crate::items::item(id).tier != ItemTier::Consumable {
        return;
    }
    h.items[slot as usize] = if n > 1 { Some((id, n - 1)) } else { None };
    e.add_status(StatusKind::Regen(POTION_HEAL / POTION_SECS), POTION_SECS);
}
