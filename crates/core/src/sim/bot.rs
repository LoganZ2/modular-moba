//! Champion bots. They see only what their team sees and act exclusively
//! through the same [`Command`]s a human sends, so they exercise exactly the
//! same rules (cooldowns, mana, ranges, shop distance ...).

use super::abilities::can_level;
use super::*;
use crate::items::{item, ItemId, ItemTier, POTION};
use crate::map::FOUNTAIN_RADIUS;
use crate::roles::Role;
use crate::skills::{skill, Action, Effect, Targeting};
use crate::summoners::SummonerId;
use crate::units::TOWER_RANGE;

const THINK_INTERVAL: f32 = 0.2;

#[derive(Clone, Debug)]
pub struct BotBrain {
    pub role: LaneRole,
    think_timer: f32,
    retreating: bool,
    build: Vec<ItemId>,
    build_idx: usize,
    jungle_camp: Option<usize>,
}

impl BotBrain {
    pub fn new(role: LaneRole, player_index: usize) -> Self {
        BotBrain {
            role,
            think_timer: (player_index as f32 * 0.047) % THINK_INTERVAL,
            retreating: false,
            build: Vec::new(),
            build_idx: 0,
            jungle_camp: None,
        }
    }
}

pub(crate) fn think_all(w: &mut World, out: &mut Vec<(usize, Command)>) {
    for p in 0..w.players.len() {
        let Some(mut brain) = w.players[p].bot.take() else {
            continue;
        };
        brain.think_timer -= DT;
        if brain.think_timer <= 0.0 {
            brain.think_timer = THINK_INTERVAL;
            think(w, p, &mut brain, out);
        }
        w.players[p].bot = Some(brain);
    }
}

fn build_for(h: &HeroState) -> Vec<ItemId> {
    let def = h.def();
    // Does this loadout lean on AP or AD?
    let (mut ap, mut ad) = (0.0, 0.0);
    for sid in h.skills {
        let sk = skill(sid);
        let mut scan = |fx: &[Effect]| {
            for e in fx {
                if let Effect::Damage(_, sc) = e {
                    ap += sc.ap;
                    ad += sc.ad + sc.bonus_ad;
                }
            }
        };
        match sk.action {
            Action::Projectile { on_hit, .. } => scan(on_hit),
            Action::Area { enemy, .. } | Action::Cone { enemy, .. } | Action::Line { enemy, .. } => scan(enemy),
            Action::Dash { hit, arrive, .. } => {
                scan(hit);
                scan(arrive)
            }
            Action::Blink { arrive, .. } => scan(arrive),
            Action::Unit { effects } | Action::Trap { effects, .. } => scan(effects),
            Action::None => {}
        }
    }
    let ids = |v: &[u8]| v.iter().map(|&i| ItemId(i)).collect::<Vec<_>>();
    match def.roles[0] {
        Role::Tank => ids(&[3, 0, 4, 7, 14, 13, 20, 21, 26, 25]),
        Role::Support => ids(&[2, 0, 3, 7, 9, 27, 21, 24, 20]),
        _ if ap > ad => ids(&[2, 0, 8, 7, 12, 24, 19, 17, 21, 27]),
        Role::Marksman => ids(&[1, 0, 6, 7, 16, 15, 18, 17, 23, 22, 25]),
        _ => ids(&[1, 0, 3, 7, 10, 11, 25, 22, 20, 26]),
    }
}

fn push(out: &mut Vec<(usize, Command)>, p: usize, c: Command) {
    out.push((p, c));
}

fn think(w: &World, p: usize, b: &mut BotBrain, out: &mut Vec<(usize, Command)>) {
    let Some(me_i) = w.idx(w.players[p].entity) else {
        return;
    };
    let me = &w.entities[me_i];
    let Some(h) = me.hero() else { return };
    if b.build.is_empty() {
        b.build = build_for(h);
    }
    level_skills(h, p, out);
    let team = me.team;
    let fountain = w.map.fountain(team);
    let at_fountain = me.pos.distance(fountain) < FOUNTAIN_RADIUS;
    if !me.alive || at_fountain {
        shop(w, p, h, b, out);
    }
    if !me.alive {
        b.retreating = false;
        return;
    }
    let mana_frac = if me.stats.max_mana > 0.0 { me.mana / me.stats.max_mana } else { 1.0 };
    if at_fountain && b.retreating {
        if me.hp_frac() > 0.95 && mana_frac > 0.8 {
            b.retreating = false;
        } else {
            return;
        }
    }

    let enemies = visible_enemy_heroes(w, me_i, 330.0);
    let threat = enemies.len();
    let recalling = h.recall.is_some();
    let low_line = if threat > 0 { 0.33 } else { 0.22 };
    if me.hp_frac() < low_line || (mana_frac < 0.1 && me.hp_frac() < 0.5) {
        b.retreating = true;
    }
    if b.retreating {
        if threat > 0 {
            use_defensive_summoners(w, me_i, p, &enemies, out);
        }
        if threat == 0 && !under_enemy_tower(w, team, me.pos) {
            if !recalling {
                push(out, p, Command::Recall);
            }
            return;
        }
        push(out, p, Command::Move(fountain));
        return;
    }
    // Go shopping when rich and safe.
    if !recalling && threat == 0 && h.gold >= 1400.0 && me.pos.distance(fountain) > 700.0 && next_affordable(h, b) {
        push(out, p, Command::Recall);
        return;
    }
    if recalling && threat == 0 {
        return;
    }

    if let Some(ti) = choose_fight(w, me_i, &enemies) {
        let tid = w.entities[ti].id;
        if let Some(cmd) = choose_skill(w, me_i, ti, threat) {
            push(out, p, cmd);
        }
        use_offensive_summoners(w, me_i, ti, p, out);
        push(out, p, Command::Attack(tid));
        return;
    }
    if support_allies(w, me_i, p, out) {
        // Keep doing lane things as well.
    }
    let role = b.role;
    if role == LaneRole::Jungle {
        if !jungle(w, me_i, p, b, out) {
            lane(w, me_i, p, LaneRole::Mid, out);
        }
    } else {
        lane(w, me_i, p, role, out);
    }
}

fn level_skills(h: &HeroState, p: usize, out: &mut Vec<(usize, Command)>) {
    let mut sim = h.clone();
    let order = [3usize, 0, 1, 2];
    while sim.skill_points > 0 {
        // Prefer R, then the lowest-ranked of Q/W/E with Q first among ties —
        // but max Q first after level 4.
        let pick = order.iter().copied().find(|&s| s == 3 && can_level(&sim, s)).or_else(|| {
            let mut basics: Vec<usize> = (0..3).filter(|&s| can_level(&sim, s)).collect();
            if sim.level <= 3 {
                basics.sort_by_key(|&s| (sim.ranks[s], s));
            }
            basics.first().copied()
        });
        let Some(s) = pick else { break };
        sim.ranks[s] += 1;
        sim.skill_points -= 1;
        out.push((p, Command::LevelUp(s as u8)));
    }
}

fn next_affordable(h: &HeroState, b: &BotBrain) -> bool {
    b.build.get(b.build_idx).map(|id| h.gold >= item(*id).cost as f32).unwrap_or(false)
}

fn shop(w: &World, p: usize, h: &HeroState, b: &mut BotBrain, out: &mut Vec<(usize, Command)>) {
    let _ = w;
    let mut gold = h.gold;
    let mut items = h.items;
    while let Some(&next) = b.build.get(b.build_idx) {
        let def = item(next);
        let cost = def.cost as f32;
        if gold < cost {
            break;
        }
        if next == POTION && items.iter().flatten().any(|(id, _)| *id == POTION) {
            b.build_idx += 1;
            continue;
        }
        let free = items.iter().position(|s| s.is_none());
        let slot = match free {
            Some(s) => s,
            None => {
                // Sell the cheapest non-legendary item to make room.
                let cheapest = items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, s)| s.map(|(id, _)| (i, item(id))))
                    .filter(|(_, d)| d.tier != ItemTier::Legendary)
                    .min_by_key(|(_, d)| d.cost);
                match cheapest {
                    Some((i, d)) => {
                        out.push((p, Command::Sell(i as u8)));
                        gold += d.cost as f32 * crate::items::SELL_RATIO;
                        items[i] = None;
                        i
                    }
                    None => {
                        b.build_idx = b.build.len();
                        break;
                    }
                }
            }
        };
        if gold < cost {
            break;
        }
        out.push((p, Command::Buy(next)));
        gold -= cost;
        items[slot] = Some((next, 1));
        b.build_idx += 1;
    }
    // Use a potion when hurt.
}

fn visible_enemy_heroes(w: &World, me_i: usize, radius: f32) -> Vec<usize> {
    let me = &w.entities[me_i];
    (0..w.entities.len())
        .filter(|&i| {
            let e = &w.entities[i];
            e.alive
                && e.is_hero()
                && e.team != me.team
                && e.targetable()
                && e.pos.distance(me.pos) <= radius
                && w.visible_to(me.team, i)
        })
        .collect()
}

fn allies_near(w: &World, me_i: usize, radius: f32) -> usize {
    let me = &w.entities[me_i];
    w.entities
        .iter()
        .filter(|e| e.alive && e.is_hero() && e.team == me.team && e.pos.distance(me.pos) <= radius)
        .count()
}

fn enemy_tower_covering(w: &World, team: Team, pos: Vec2) -> Option<usize> {
    (0..w.entities.len()).find(|&i| {
        let e = &w.entities[i];
        e.alive
            && e.team == team.enemy()
            && matches!(e.structure(), Some(s) if matches!(s.kind, StructureKind::Tower { .. }))
            && e.pos.distance(pos) <= TOWER_RANGE + 25.0
    })
}

fn under_enemy_tower(w: &World, team: Team, pos: Vec2) -> bool {
    enemy_tower_covering(w, team, pos).is_some()
}

fn choose_fight(w: &World, me_i: usize, enemies: &[usize]) -> Option<usize> {
    let me = &w.entities[me_i];
    let reach = me.stats.range + 150.0;
    let allies = allies_near(w, me_i, 330.0);
    let mut best: Option<(f32, usize)> = None;
    for &ti in enemies {
        let t = &w.entities[ti];
        if me.edge_distance(t) > reach {
            continue;
        }
        let favourable = me.hp_frac() >= t.hp_frac() - 0.1 || t.hp_frac() < 0.3;
        let outnumbered = enemies.len() > allies + 1 && t.hp_frac() > 0.25;
        let dive = under_enemy_tower(w, me.team, t.pos) && !(t.hp_frac() < 0.25 && me.hp_frac() > 0.6);
        if !favourable || outnumbered || dive {
            continue;
        }
        let score = t.hp_frac();
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, ti));
        }
    }
    best.map(|(_, i)| i)
}

/// Pick a skill to throw at target `ti`.
fn choose_skill(w: &World, me_i: usize, ti: usize, threat: usize) -> Option<Command> {
    let me = &w.entities[me_i];
    let h = me.hero()?;
    let t = &w.entities[ti];
    let dist = me.pos.distance(t.pos);
    let edge = me.edge_distance(t);
    for slot in [3usize, 0, 1, 2] {
        let sk = skill(h.skills[slot]);
        let rank = h.ranks[slot];
        if rank == 0 || h.cooldowns[slot] > 0.0 || me.mana < sk.mana_at(rank) {
            continue;
        }
        if slot == 3 && t.is_hero() && t.hp_frac() > 0.6 && threat < 2 {
            continue;
        }
        let ally_only = match sk.action {
            Action::Area { enemy, ally, .. } => enemy.is_empty() && !ally.is_empty(),
            _ => false,
        };
        let lead = |speed: f32| t.pos + t.vel * (dist / speed.max(1.0)).min(0.6);
        let cmd = |point: Vec2, unit: Option<EntityId>| Command::Cast { slot: slot as u8, point, unit };
        let fire = match (sk.targeting, sk.action) {
            (Targeting::Instant, Action::Area { radius, .. }) if !ally_only => {
                (dist <= radius * 0.9).then(|| cmd(me.pos, None))
            }
            (Targeting::Instant, _) => (edge <= me.stats.range + 60.0).then(|| cmd(me.pos, None)),
            (Targeting::Direction { range }, Action::Projectile { speed, .. }) => {
                (dist <= range * 0.92).then(|| cmd(lead(speed), None))
            }
            (Targeting::Direction { range }, Action::Dash { .. }) => {
                if range < 100.0 {
                    // Short mobility: only when the target is just out of reach.
                    (edge > me.stats.range && edge < me.stats.range + range).then(|| cmd(t.pos, None))
                } else {
                    (dist <= range * 0.9).then(|| cmd(t.pos, None))
                }
            }
            (Targeting::Direction { range }, _) => (dist <= range * 0.9).then(|| cmd(lead(900.0), None)),
            (Targeting::Point { range }, Action::Blink { .. } | Action::Dash { .. }) => {
                (dist <= range + 20.0 && t.is_hero()).then(|| cmd(t.pos, None))
            }
            (Targeting::Point { range }, _) => (dist <= range).then(|| cmd(lead(700.0), None)),
            (Targeting::Enemy { range }, _) => (edge <= range).then(|| cmd(t.pos, Some(t.id))),
            (Targeting::EnemyHero { range }, _) => (t.is_hero() && edge <= range).then(|| cmd(t.pos, Some(t.id))),
            (Targeting::Ally { .. }, _) => (me.hp_frac() < 0.75).then(|| cmd(me.pos, Some(me.id))),
        };
        if fire.is_some() {
            return fire;
        }
    }
    None
}

fn use_offensive_summoners(w: &World, me_i: usize, ti: usize, p: usize, out: &mut Vec<(usize, Command)>) {
    let me = &w.entities[me_i];
    let Some(h) = me.hero() else { return };
    let t = &w.entities[ti];
    if !t.is_hero() {
        return;
    }
    for slot in 0..2 {
        if h.summoner_cd[slot] > 0.0 {
            continue;
        }
        let s = h.summoners[slot];
        let use_it = match s {
            SummonerId::Ignite => t.hp_frac() < 0.35 && me.edge_distance(t) <= s.range(),
            SummonerId::Exhaust => me.hp_frac() < 0.6 && me.edge_distance(t) <= s.range(),
            SummonerId::Ghost => t.hp_frac() < 0.3 && me.edge_distance(t) > me.stats.range,
            _ => false,
        };
        if use_it {
            out.push((p, Command::Summoner { slot: slot as u8, point: t.pos, unit: Some(t.id) }));
            return;
        }
    }
}

fn use_defensive_summoners(w: &World, me_i: usize, p: usize, enemies: &[usize], out: &mut Vec<(usize, Command)>) {
    let me = &w.entities[me_i];
    let Some(h) = me.hero() else { return };
    let fountain = w.map.fountain(me.team);
    let closest = enemies.iter().map(|&i| w.entities[i].pos.distance(me.pos)).fold(f32::MAX, f32::min);
    for slot in 0..2 {
        if h.summoner_cd[slot] > 0.0 {
            continue;
        }
        let s = h.summoners[slot];
        let use_it = match s {
            SummonerId::Heal | SummonerId::Barrier => me.hp_frac() < 0.2,
            SummonerId::Flash => me.hp_frac() < 0.15 && closest < 90.0,
            SummonerId::Ghost => me.hp_frac() < 0.25,
            SummonerId::Exhaust => me.hp_frac() < 0.25 && closest < s.range(),
            SummonerId::Ignite => false,
        };
        if use_it {
            let toward = me.pos + (fountain - me.pos).normalize_or_zero() * 120.0;
            let unit = enemies.first().map(|&i| w.entities[i].id);
            out.push((p, Command::Summoner { slot: slot as u8, point: toward, unit }));
            return;
        }
    }
    // Drink a potion if we carry one.
    if let Some(slot) = h.items.iter().position(|s| matches!(s, Some((id, _)) if *id == POTION)) {
        if me.hp_frac() < 0.6 && !me.has(|k| matches!(k, StatusKind::Regen(_))) {
            out.push((p, Command::UseItem(slot as u8)));
        }
    }
}

/// Heal / shield hurt allies with ally-targeted skills. Returns true if a
/// command was issued.
fn support_allies(w: &World, me_i: usize, p: usize, out: &mut Vec<(usize, Command)>) -> bool {
    let me = &w.entities[me_i];
    let Some(h) = me.hero() else { return false };
    for slot in 0..4 {
        let sk = skill(h.skills[slot]);
        let rank = h.ranks[slot];
        if rank == 0 || h.cooldowns[slot] > 0.0 || me.mana < sk.mana_at(rank) {
            continue;
        }
        let range = match (sk.targeting, sk.action) {
            (Targeting::Ally { range }, _) => range,
            (Targeting::Point { range }, Action::Area { ally, .. }) if !ally.is_empty() => range,
            _ => continue,
        };
        let hurt = w
            .entities
            .iter()
            .filter(|e| e.alive && e.is_hero() && e.team == me.team && e.id != me.id && e.hp_frac() < 0.55)
            .filter(|e| me.pos.distance(e.pos) <= range)
            .min_by(|a, b| a.hp_frac().total_cmp(&b.hp_frac()));
        if let Some(a) = hurt {
            out.push((p, Command::Cast { slot: slot as u8, point: a.pos, unit: Some(a.id) }));
            return true;
        }
    }
    false
}

/// Lane play: follow the wave, last hit, push towers, respect towers.
fn lane(w: &World, me_i: usize, p: usize, role: LaneRole, out: &mut Vec<(usize, Command)>) {
    let me = &w.entities[me_i];
    let team = me.team;
    let lane = role.lane();
    let path = &w.map.lanes[team.index()][lane as usize];

    // Support: shadow the bot laner.
    if role == LaneRole::Support {
        let carry = w.players.iter().find(|pl| pl.team == team && pl.role == LaneRole::Bot && pl.index != p);
        if let Some(c) = carry.and_then(|pl| w.get(pl.entity)).filter(|c| c.alive) {
            let back = (path[0] - c.pos).normalize_or_zero() * 30.0;
            let spot = w.map.nearest_walkable(c.pos + back + back.perp() * 0.6);
            if let Some(t) = lane_target(w, me_i, true) {
                out.push((p, Command::Attack(t)));
            } else if me.pos.distance(spot) > 40.0 {
                out.push((p, Command::Move(spot)));
            }
            return;
        }
    }

    // Front of our wave in this lane.
    let mut front: Option<f32> = None;
    for e in &w.entities {
        if let UnitKind::Minion(m) = &e.kind {
            if e.alive && e.team == team && m.lane == lane {
                let prog = GameMap::lane_progress(path, e.pos);
                front = Some(front.map_or(prog, |f: f32| f.max(prog)));
            }
        }
    }
    let mut spot = match front {
        Some(f) => GameMap::point_along(path, f - 45.0),
        None => {
            // Hold behind our outermost living tower in this lane.
            let mut best: Option<f32> = None;
            for e in &w.entities {
                if let Some(s) = e.structure() {
                    if e.alive && e.team == team && matches!(s.kind, StructureKind::Tower { lane: l, .. } if l == lane)
                    {
                        let prog = GameMap::lane_progress(path, e.pos);
                        best = Some(best.map_or(prog, |b: f32| b.max(prog)));
                    }
                }
            }
            GameMap::point_along(path, best.unwrap_or(0.0) - 60.0)
        }
    };
    // Don't stand under an enemy tower without minions to tank it.
    if let Some(ti) = enemy_tower_covering(w, team, spot) {
        let tpos = w.entities[ti].pos;
        let tankers = w
            .entities
            .iter()
            .filter(|e| e.alive && e.is_minion() && e.team == team && e.pos.distance(tpos) <= TOWER_RANGE)
            .count();
        if tankers < 2 {
            let tprog = GameMap::lane_progress(path, tpos);
            spot = GameMap::point_along(path, tprog - TOWER_RANGE - 40.0);
        }
    }
    if let Some(t) = lane_target(w, me_i, role == LaneRole::Support) {
        out.push((p, Command::Attack(t)));
        return;
    }
    let spread = Vec2::new(((p * 37) % 23) as f32 - 11.0, ((p * 53) % 23) as f32 - 11.0);
    let spot = w.map.nearest_walkable(spot + spread);
    if me.pos.distance(spot) > 35.0 {
        let already = matches!(me.order, Order::Move(g) if g.distance(spot) < 25.0);
        if !already {
            out.push((p, Command::Move(spot)));
        }
    }
}

/// Something worth hitting near us in lane: last hits, then towers being
/// tanked, then any minion (pushing).
fn lane_target(w: &World, me_i: usize, support: bool) -> Option<EntityId> {
    let me = &w.entities[me_i];
    let reach = me.stats.range + 50.0;
    let mut last_hit: Option<(f32, EntityId)> = None;
    let mut weakest: Option<(f32, EntityId)> = None;
    for (i, e) in w.entities.iter().enumerate() {
        if !e.targetable() || e.team == me.team || e.team == Team::Neutral || me.edge_distance(e) > reach {
            continue;
        }
        if !w.visible_to(me.team, i) {
            continue;
        }
        if e.is_minion() {
            let dmg = me.stats.ad * crate::stats::resist_multiplier(e.stats.armor);
            if e.hp <= dmg * 1.05 && last_hit.is_none_or(|(h, _)| e.hp < h) {
                last_hit = Some((e.hp, e.id));
            }
            if weakest.is_none_or(|(h, _)| e.hp < h) {
                weakest = Some((e.hp, e.id));
            }
        }
    }
    if let Some((_, id)) = last_hit {
        if !support {
            return Some(id);
        }
    }
    // Towers / inhibitors / nexus being tanked by our minions.
    for e in &w.entities {
        if e.is_structure() && e.targetable() && e.team == me.team.enemy() && me.edge_distance(e) <= reach {
            let tankers = w
                .entities
                .iter()
                .filter(|m| m.alive && m.is_minion() && m.team == me.team && m.pos.distance(e.pos) <= TOWER_RANGE)
                .count();
            if tankers >= 2 || !matches!(e.structure().map(|s| s.kind), Some(StructureKind::Tower { .. })) {
                return Some(e.id);
            }
        }
    }
    if support {
        return None;
    }
    // Don't trade with minions under an enemy tower unless it is busy.
    weakest.filter(|_| !under_enemy_tower(w, me.team, me.pos)).map(|(_, id)| id)
}

/// Clear our own jungle. Returns false when there is nothing to do.
fn jungle(w: &World, me_i: usize, p: usize, b: &mut BotBrain, out: &mut Vec<(usize, Command)>) -> bool {
    let me = &w.entities[me_i];
    let team = me.team;
    let camp_alive = |c: usize| !w.camps[c].members.is_empty();
    let valid = b.jungle_camp.filter(|&c| camp_alive(c));
    let camp = valid.or_else(|| {
        let level = me.hero().map(|h| h.level).unwrap_or(1);
        (0..w.camps.len())
            .filter(|&c| camp_alive(c))
            .filter(|&c| {
                let side = w.map.camps[w.camps[c].spawn].side;
                side == team || (side == Team::Neutral && level >= 11 && me.hp_frac() > 0.8)
            })
            .min_by(|&a, &c| {
                let pa = w.map.camps[w.camps[a].spawn].pos.distance(me.pos);
                let pc = w.map.camps[w.camps[c].spawn].pos.distance(me.pos);
                pa.total_cmp(&pc)
            })
    });
    b.jungle_camp = camp;
    let Some(c) = camp else {
        // Wait for the first spawn near our buff.
        if w.time < 45.0 {
            let first = w.camps.iter().position(|cs| w.map.camps[cs.spawn].side == team);
            if let Some(f) = first {
                let pos = w.map.camps[w.camps[f].spawn].pos;
                if me.pos.distance(pos) > 60.0 {
                    out.push((p, Command::Move(pos)));
                }
                return true;
            }
        }
        return false;
    };
    let members = &w.camps[c].members;
    let target = members
        .iter()
        .filter_map(|id| w.idx(*id))
        .filter(|&mi| w.entities[mi].targetable())
        .min_by(|&a, &b2| w.entities[a].hp.total_cmp(&w.entities[b2].hp));
    match target {
        Some(mi) => {
            let t = &w.entities[mi];
            // Losing the fight against the camp: back off and recall.
            if me.hp_frac() < 0.3 && t.hp > me.stats.ad * 6.0 {
                b.retreating = true;
                out.push((p, Command::Move(w.map.fountain(team))));
                return true;
            }
            if me.pos.distance(t.pos) < 250.0 {
                if let Some(cmd) = choose_skill(w, me_i, mi, 0) {
                    out.push((p, cmd));
                }
                out.push((p, Command::Attack(t.id)));
            } else {
                out.push((p, Command::Move(t.pos)));
            }
        }
        None => {
            // Monsters are resetting; wait nearby.
            let pos = w.map.camps[w.camps[c].spawn].pos;
            if me.pos.distance(pos) > 120.0 {
                out.push((p, Command::Move(pos)));
            }
        }
    }
    true
}
