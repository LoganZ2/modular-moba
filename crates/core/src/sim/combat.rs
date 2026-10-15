//! Damage, healing, deaths and rewards, basic attacks and projectiles.

use super::*;
use crate::map::FOUNTAIN_RADIUS;
use crate::protocol::Killer;
use crate::stats::resist_multiplier;
use crate::units::TOWER_RANGE;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Hit {
    pub attack: bool,
    pub crit: bool,
    pub quiet: bool,
}

impl Hit {
    pub const SKILL: Hit = Hit { attack: false, crit: false, quiet: false };
    pub const QUIET: Hit = Hit { attack: false, crit: false, quiet: true };
    pub fn attack(crit: bool) -> Hit {
        Hit { attack: true, crit, quiet: false }
    }
}

/// Apply `raw` damage (before resistances) from `source` to `target`.
/// Returns damage actually dealt (including the part absorbed by shields).
pub(crate) fn deal_damage(
    w: &mut World,
    source: EntityId,
    target: EntityId,
    raw: f32,
    kind: DamageKind,
    hit: Hit,
) -> f32 {
    let Some(ti) = w.idx(target) else { return 0.0 };
    {
        let t = &w.entities[ti];
        if !t.alive || t.invulnerable || raw <= 0.0 {
            return 0.0;
        }
    }
    let si = w.idx(source);
    let (src_team, src_hero, src_exhausted, src_lifesteal) = match si {
        Some(s) => {
            let e = &w.entities[s];
            (Some(e.team), e.is_hero(), e.has(|k| matches!(k, StatusKind::Exhausted)), e.stats.lifesteal)
        }
        None => (None, false, false, 0.0),
    };
    let time = w.time;

    let mut amount = raw;
    {
        let t = &w.entities[ti];
        amount *= match kind {
            DamageKind::Physical => resist_multiplier(t.stats.armor),
            DamageKind::Magic => resist_multiplier(t.stats.mr),
            DamageKind::True => 1.0,
        };
        let vuln =
            t.statuses.iter().map(|s| if let StatusKind::Vulnerable(p) = s.kind { p } else { 0.0 }).fold(0.0, f32::max);
        amount *= 1.0 + vuln;
        if src_exhausted {
            amount *= 0.65;
        }
        // Champions hit jungle camps harder (stands in for LoL's smite /
        // jungle item so solo jungling is viable).
        if src_hero && t.is_monster() && !t.is_epic_monster() {
            amount *= 1.5;
        }
        if let (true, Some(team)) = (src_hero, src_team) {
            if team != Team::Neutral {
                amount *= 1.0 + 0.05 * w.team_state(team).score.dragons as f32;
            }
        }
        // Backdoor protection: structures take far less damage from champions
        // unless friendly minions (of the attacker) are nearby.
        if t.is_structure() && src_hero {
            let enemy = t.team.enemy();
            let tp = t.pos;
            let covered = w
                .entities
                .iter()
                .any(|m| m.alive && m.is_minion() && m.team == enemy && m.pos.distance(tp) <= TOWER_RANGE + 60.0);
            if !covered {
                amount *= 0.4;
            }
        }
    }

    let (pos, target_is_hero, target_is_monster, target_team, dead) = {
        let t = &mut w.entities[ti];
        let mut left = amount;
        for s in &mut t.statuses {
            if let StatusKind::Shield(v) = &mut s.kind {
                let a = v.min(left);
                *v -= a;
                left -= a;
                if left <= 0.0 {
                    break;
                }
            }
        }
        t.hp -= left;
        cancel_recall(t);
        if let (true, Some(h)) = (src_hero, t.hero_mut()) {
            h.damagers.retain(|(id, _)| *id != source);
            h.damagers.push((source, time));
        }
        (t.pos, t.is_hero(), t.is_monster(), t.team, t.hp <= 0.0)
    };

    if !hit.quiet && amount >= 0.5 {
        w.event(Audience::At(pos), FxEvent::Damage { source, target, pos, amount, kind, crit: hit.crit });
    }
    if let Some(s) = si {
        let src = &mut w.entities[s];
        if src_hero && target_is_hero && src.team != target_team {
            if let Some(h) = src.hero_mut() {
                h.aggression = Some((target, time));
            }
        }
        if src_hero && (target_is_hero || !hit.quiet) {
            src.revealed_until = src.revealed_until.max(time + 1.0);
        }
        if hit.attack && src_lifesteal > 0.0 && src.alive {
            src.hp = (src.hp + amount * src_lifesteal).min(src.stats.max_hp);
        }
    }
    if target_is_monster {
        if let UnitKind::Monster(m) = &w.entities[ti].kind {
            let camp = m.camp;
            if w.camps[camp].aggro.is_none() && si.is_some() && src_team != Some(Team::Neutral) {
                w.camps[camp].aggro = Some(source);
            }
        }
    }
    if dead {
        // Source 0 is the fountain; a non-zero id that no longer resolves is a
        // unit that died while its projectile was in flight.
        kill(w, ti, (source != 0).then_some(source));
    }
    amount
}

pub(crate) fn heal(w: &mut World, ti: usize, amount: f32) {
    let e = &mut w.entities[ti];
    if !e.alive || amount <= 0.0 {
        return;
    }
    let before = e.hp;
    e.hp = (e.hp + amount).min(e.stats.max_hp);
    let healed = e.hp - before;
    let (id, pos) = (e.id, e.pos);
    if healed >= 1.0 {
        w.event(Audience::At(pos), FxEvent::Heal { target: id, pos, amount: healed });
    }
}

/// Handle a unit's death: rewards, score, respawn timers, win condition.
pub(crate) fn kill(w: &mut World, vi: usize, killer: Option<EntityId>) {
    let time = w.time;
    {
        let v = &mut w.entities[vi];
        if !v.alive {
            return;
        }
        v.alive = false;
        v.hp = 0.0;
        v.statuses.clear();
        v.windup = None;
        v.dash = None;
        v.order = Order::Idle;
        v.engage = None;
        v.move_goal = None;
        v.path.clear();
        if let Some(h) = v.hero_mut() {
            h.cast = None;
            h.recall = None;
        }
    }
    let (vid, vpos, vteam) = {
        let v = &w.entities[vi];
        (v.id, v.pos, v.team)
    };
    w.event(Audience::At(vpos), FxEvent::Death { id: vid, pos: vpos });
    let killer_idx = killer.and_then(|k| w.idx(k));
    let killer_is_hero = killer_idx.map(|k| w.entities[k].is_hero()).unwrap_or(false);

    match w.entities[vi].kind.clone() {
        UnitKind::Minion(m) => {
            w.entities[vi].remove = true;
            let st = m.kind.stats();
            if let (Some(k), true) = (killer_idx, killer_is_hero) {
                w.give_gold(k, st.gold, true);
                if let Some(h) = w.entities[k].hero_mut() {
                    h.cs += 1;
                }
            }
            share_xp(w, vpos, vteam.enemy(), st.xp);
        }
        UnitKind::Monster(m) => {
            w.entities[vi].remove = true;
            let st = m.kind.stats();
            let camp = m.camp;
            w.camps[camp].members.retain(|id| *id != vid);
            if w.camps[camp].members.is_empty() {
                let spawn = &w.map.camps[w.camps[camp].spawn];
                w.camps[camp].respawn_at = Some(time + spawn.respawn);
                w.camps[camp].aggro = None;
            }
            if let (Some(k), true) = (killer_idx, killer_is_hero) {
                w.give_gold(k, st.gold, true);
                w.give_xp(k, st.xp);
                let team = w.entities[k].team;
                if let Some(h) = w.entities[k].hero_mut() {
                    h.cs += 1;
                }
                match m.kind {
                    MonsterKind::BlueSentinel => w.entities[k].add_status(StatusKind::BlueBuff, 90.0),
                    MonsterKind::RedBrambleback => w.entities[k].add_status(StatusKind::RedBuff, 90.0),
                    MonsterKind::Dragon => {
                        w.team_state_mut(team).score.dragons += 1;
                        for i in 0..w.entities.len() {
                            if w.entities[i].is_hero() && w.entities[i].team == team {
                                w.give_gold(i, 50.0, true);
                            }
                        }
                        let n = w.team_state(team).score.dragons;
                        w.event(
                            Audience::All,
                            FxEvent::Announce {
                                text: format!("{} team slew the Dragon! (+{}% dmg)", team.name(), n * 5),
                                team: Some(team),
                            },
                        );
                    }
                    MonsterKind::Baron => {
                        w.team_state_mut(team).score.barons += 1;
                        for i in 0..w.entities.len() {
                            if w.entities[i].is_hero() && w.entities[i].team == team {
                                w.give_gold(i, 200.0, true);
                                if w.entities[i].alive {
                                    w.entities[i].add_status(StatusKind::BaronBuff, 150.0);
                                }
                            }
                        }
                        w.event(
                            Audience::All,
                            FxEvent::Announce {
                                text: format!("{} team slew the Baron!", team.name()),
                                team: Some(team),
                            },
                        );
                    }
                    _ => {}
                }
            }
        }
        UnitKind::Hero(h) => hero_death(w, vi, &h, killer, killer_idx),
        UnitKind::Structure(s) => {
            let enemy = vteam.enemy();
            match s.kind {
                StructureKind::Tower { .. } => {
                    w.team_state_mut(enemy).score.towers += 1;
                    for i in 0..w.entities.len() {
                        if w.entities[i].is_hero() && w.entities[i].team == enemy {
                            w.give_gold(i, 150.0, true);
                        }
                    }
                    w.event(
                        Audience::All,
                        FxEvent::Announce { text: format!("{} turret destroyed!", vteam.name()), team: Some(enemy) },
                    );
                }
                StructureKind::Inhibitor { .. } => {
                    w.team_state_mut(enemy).score.inhibitors += 1;
                    if let UnitKind::Structure(st) = &mut w.entities[vi].kind {
                        st.respawn_at = Some(time + INHIBITOR_RESPAWN);
                    }
                    for i in 0..w.entities.len() {
                        if w.entities[i].is_hero() && w.entities[i].team == enemy {
                            w.give_gold(i, 50.0, true);
                        }
                    }
                    w.event(
                        Audience::All,
                        FxEvent::Announce { text: format!("{} inhibitor destroyed!", vteam.name()), team: Some(enemy) },
                    );
                }
                StructureKind::Nexus => {
                    w.winner = Some(enemy);
                    w.event(
                        Audience::All,
                        FxEvent::Announce { text: format!("{} team is victorious!", enemy.name()), team: Some(enemy) },
                    );
                }
            }
        }
    }
}

fn share_xp(w: &mut World, pos: Vec2, team: Team, xp: f32) {
    let near: Vec<usize> = (0..w.entities.len())
        .filter(|&i| {
            let e = &w.entities[i];
            e.alive && e.is_hero() && e.team == team && e.pos.distance(pos) <= XP_RANGE
        })
        .collect();
    if near.is_empty() {
        return;
    }
    let n = near.len() as f32;
    let each = xp / n * (1.0 + 0.15 * (n - 1.0));
    for i in near {
        w.give_xp(i, each);
    }
}

fn hero_death(w: &mut World, vi: usize, h: &HeroState, killer: Option<EntityId>, killer_idx: Option<usize>) {
    let time = w.time;
    let vteam = w.entities[vi].team;
    let vpos = w.entities[vi].pos;
    let enemy = vteam.enemy();
    let level = h.level;
    {
        let hm = w.entities[vi].hero_mut().unwrap();
        hm.deaths += 1;
        hm.respawn = 6.0 + 2.2 * level as f32 + (time / 60.0 * 0.4).min(12.0);
    }
    // Recent champion damage, most recent first.
    let mut recent: Vec<(EntityId, f32)> =
        h.damagers.iter().copied().filter(|(_, t)| time - *t <= ASSIST_WINDOW).collect();
    recent.sort_by(|a, b| b.1.total_cmp(&a.1));
    let credited: Option<usize> = match killer_idx {
        Some(k) if w.entities[k].is_hero() && w.entities[k].team == enemy => Some(k),
        _ => recent.first().and_then(|(id, _)| w.idx(*id)),
    };
    let assist_idx: Vec<usize> = recent
        .iter()
        .filter_map(|(id, _)| w.idx(*id))
        .filter(|&i| Some(i) != credited && w.entities[i].team == enemy)
        .collect();

    let victim_player = h.player as u8;
    let killer_tag = match credited {
        Some(k) => Killer::Player(w.entities[k].hero().map(|x| x.player as u8).unwrap_or(0)),
        None => match killer_idx.map(|k| &w.entities[k].kind) {
            Some(UnitKind::Structure(_)) => Killer::Tower(enemy),
            Some(UnitKind::Minion(_)) => Killer::Minion(enemy),
            Some(UnitKind::Monster(_)) => Killer::Monster,
            Some(UnitKind::Hero(_)) => Killer::Minion(enemy),
            None if killer.is_some() => Killer::Minion(enemy),
            None => Killer::Fountain,
        },
    };
    let assists: Vec<u8> = assist_idx.iter().filter_map(|&i| w.entities[i].hero().map(|x| x.player as u8)).collect();
    w.event(Audience::All, FxEvent::Kill { killer: killer_tag, victim: victim_player, assists });

    if let Some(k) = credited {
        w.team_state_mut(enemy).score.kills += 1;
        if let Some(kh) = w.entities[k].hero_mut() {
            kh.kills += 1;
        }
        let mut gold = 300.0;
        if !w.first_blood {
            w.first_blood = true;
            gold += 100.0;
            w.event(Audience::All, FxEvent::Announce { text: "First Blood!".into(), team: Some(enemy) });
        }
        w.give_gold(k, gold, true);
        if !assist_idx.is_empty() {
            let share = 150.0 / assist_idx.len() as f32;
            for &a in &assist_idx {
                if let Some(ah) = w.entities[a].hero_mut() {
                    ah.assists += 1;
                }
                w.give_gold(a, share, true);
            }
        }
        // Experience: killer, assisters and nearby enemies share it.
        let mut takers: Vec<usize> = vec![k];
        takers.extend(assist_idx.iter().copied());
        for i in 0..w.entities.len() {
            let e = &w.entities[i];
            if e.alive && e.is_hero() && e.team == enemy && e.pos.distance(vpos) <= XP_RANGE && !takers.contains(&i) {
                takers.push(i);
            }
        }
        let base = 100.0 + 35.0 * level as f32;
        let n = takers.len() as f32;
        for i in takers {
            w.give_xp(i, base / n * (1.0 + 0.2 * (n - 1.0)));
        }
    }
}

// ------------------------------------------------------------------ attacks

fn windup_time(e: &Entity) -> f32 {
    if e.is_structure() {
        return 0.0;
    }
    (0.3 / e.stats.attack_speed.max(0.1)).clamp(0.08, 0.6)
}

/// Progress attack wind-ups and start new attacks against `engage` targets.
pub(crate) fn update_attacks(w: &mut World) {
    for i in 0..w.entities.len() {
        let (busy, windup, engage) = {
            let e = &w.entities[i];
            let casting = e.hero().map(|h| h.cast.is_some() || h.recall.is_some()).unwrap_or(false);
            (!e.alive || e.stunned() || e.dash.is_some() || casting, e.windup, e.engage)
        };
        if busy {
            w.entities[i].windup = None;
            continue;
        }
        if let Some((t, rem)) = windup {
            let valid = w.idx(t).map(|ti| {
                let te = &w.entities[ti];
                te.targetable() && w.entities[i].edge_distance(te) <= w.entities[i].stats.range + 40.0
            });
            if valid != Some(true) {
                w.entities[i].windup = None;
                continue;
            }
            let rem = rem - DT;
            if rem <= 0.0 {
                w.entities[i].windup = None;
                fire_attack(w, i, t);
            } else {
                w.entities[i].windup = Some((t, rem));
            }
            continue;
        }
        let Some(t) = engage else { continue };
        let Some(ti) = w.idx(t) else { continue };
        let (in_range, tpos) = {
            let (e, te) = (&w.entities[i], &w.entities[ti]);
            (te.targetable() && e.in_attack_range(te), te.pos)
        };
        if !in_range {
            continue;
        }
        let e = &mut w.entities[i];
        e.facing = (tpos - e.pos).angle();
        if e.attack_cd <= 0.0 {
            e.attack_cd = 1.0 / e.stats.attack_speed.max(0.1);
            let wt = windup_time(e);
            if wt <= 0.0 {
                fire_attack(w, i, t);
            } else {
                e.windup = Some((t, wt));
            }
        }
    }
}

fn fire_attack(w: &mut World, i: usize, target: EntityId) {
    let Some(ti) = w.idx(target) else { return };
    let time = w.time;
    let crit_roll = w.rng.next_f32();
    let (aid, apos, team, ranged, speed, style, color) = {
        let a = &w.entities[i];
        (a.id, a.pos, a.team, a.ranged, a.projectile_speed, a.attack_style, a.attack_color)
    };
    if w.entities[i].has(|k| matches!(k, StatusKind::Blind)) {
        if let Some(h) = w.entities[i].hero_mut() {
            h.empower = 0.0;
        }
        let tpos = w.entities[ti].pos;
        w.event(Audience::At(tpos), FxEvent::Miss { target, pos: tpos });
        return;
    }
    let mut damage = w.entities[i].stats.ad;
    let mut kind = DamageKind::Physical;
    let mut crit = false;
    let target_minion = match &w.entities[ti].kind {
        UnitKind::Minion(m) => Some(m.kind),
        _ => None,
    };
    let target_is_hero = w.entities[ti].is_hero();
    let target_max_hp = w.entities[ti].stats.max_hp;
    let crit_chance = w.entities[i].stats.crit;
    if w.entities[i].is_hero() {
        if crit_roll < crit_chance {
            damage *= 1.75;
            crit = true;
        }
        if let Some(h) = w.entities[i].hero_mut() {
            damage += h.empower;
            h.empower = 0.0;
        }
    } else if let UnitKind::Structure(s) = &mut w.entities[i].kind {
        let ad = TOWER_AD + 4.0 * (time / 60.0);
        if let Some(mk) = target_minion {
            damage = target_max_hp * mk.tower_shot_pct();
            kind = DamageKind::True;
            s.streak = 0;
        } else if target_is_hero {
            damage = ad * (1.0 + 0.4 * s.streak.min(3) as f32);
            s.streak += 1;
        } else {
            damage = ad;
        }
    }
    if ranged {
        let id = w.alloc_misc_id();
        let tpos = w.entities[ti].pos;
        w.projectiles.push(Projectile {
            id,
            owner: aid,
            team,
            pos: apos,
            dir: (tpos - apos).normalize_or_zero(),
            speed: speed.max(100.0),
            target: Some(target),
            remaining: 0.0,
            width: 0.0,
            pierce: false,
            style,
            color,
            payload: Payload::Attack { damage, kind, crit },
            hit: Vec::new(),
            dead: false,
        });
    } else {
        attack_hit(w, aid, target, damage, kind, crit);
    }
}

fn attack_hit(w: &mut World, source: EntityId, target: EntityId, damage: f32, kind: DamageKind, crit: bool) {
    deal_damage(w, source, target, damage, kind, Hit::attack(crit));
    // Red buff: attacks burn and slow.
    let red_level = w.get(source).and_then(|s| {
        if s.has(|k| matches!(k, StatusKind::RedBuff)) {
            Some(s.hero().map(|h| h.level).unwrap_or(1))
        } else {
            None
        }
    });
    if let (Some(lvl), Some(t)) = (red_level, w.get_mut(target)) {
        if t.alive && !t.is_structure() {
            t.statuses.retain(|s| !matches!(s.kind, StatusKind::Burn { source: src, .. } if src == source));
            t.add_status(StatusKind::Burn { dps: (8.0 + 2.0 * lvl as f32) / 3.0 * 2.0, source }, 3.0);
            if !t.is_epic_monster() {
                t.add_status(StatusKind::Slow(0.1), 1.0);
            }
        }
    }
}

// ------------------------------------------------------------------ projectiles

pub(crate) fn update_projectiles(w: &mut World) {
    let mut projs = std::mem::take(&mut w.projectiles);
    for p in projs.iter_mut() {
        if p.dead {
            continue;
        }
        let step = p.speed * DT;
        if let Some(t) = p.target {
            let Some(ti) = w.idx(t) else {
                p.dead = true;
                continue;
            };
            let (tpos, tr, alive) = {
                let te = &w.entities[ti];
                (te.pos, te.radius, te.alive)
            };
            if !alive {
                p.dead = true;
                continue;
            }
            let d = tpos - p.pos;
            p.dir = d.normalize_or(p.dir);
            if d.length() <= step + tr {
                p.pos = tpos;
                p.dead = true;
                match p.payload.clone() {
                    Payload::Attack { damage, kind, crit } => attack_hit(w, p.owner, t, damage, kind, crit),
                    Payload::Skill { skill, rank, stats } => {
                        if let crate::skills::Action::Projectile { on_hit, .. } = crate::skills::skill(skill).action {
                            let origin = p.pos - p.dir * 10.0;
                            abilities::apply_effects(w, p.owner, p.team, &stats, rank, t, on_hit, origin);
                        }
                    }
                }
            } else {
                p.pos += p.dir * step;
            }
            continue;
        }
        // Linear skillshot.
        let from = p.pos;
        let to = p.pos + p.dir * step.min(p.remaining);
        let mut hits: Vec<(f32, EntityId)> = Vec::new();
        for e in &w.entities {
            if !e.alive || e.is_structure() || !e.targetable() || e.team == p.team || p.hit.contains(&e.id) {
                continue;
            }
            let d = crate::math::dist_point_segment(e.pos, from, to);
            if d <= p.width / 2.0 + e.radius {
                hits.push(((e.pos - from).dot(p.dir), e.id));
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, id) in hits {
            p.hit.push(id);
            if let Payload::Skill { skill, rank, stats } = p.payload.clone() {
                if let crate::skills::Action::Projectile { on_hit, .. } = crate::skills::skill(skill).action {
                    let origin = w.get(id).map(|e| e.pos - p.dir * 10.0).unwrap_or(from);
                    abilities::apply_effects(w, p.owner, p.team, &stats, rank, id, on_hit, origin);
                }
            }
            if !p.pierce {
                p.dead = true;
                break;
            }
        }
        p.remaining -= step;
        p.pos = to;
        if p.remaining <= 0.0 {
            p.dead = true;
        }
    }
    projs.append(&mut w.projectiles);
    w.projectiles = projs;
}

// ------------------------------------------------------------------ fountains

pub(crate) fn update_fountains(w: &mut World) {
    for team in Team::PLAYABLE {
        let f = w.map.fountain(team);
        let mut victims = Vec::new();
        for e in &mut w.entities {
            if !e.alive || e.pos.distance(f) > FOUNTAIN_RADIUS {
                continue;
            }
            if e.team == team && e.is_hero() {
                e.hp = (e.hp + e.stats.max_hp * 0.12 * DT).min(e.stats.max_hp);
                e.mana = (e.mana + e.stats.max_mana * 0.12 * DT).min(e.stats.max_mana);
            } else if e.team == team.enemy() {
                victims.push(e.id);
            }
        }
        for id in victims {
            deal_damage(w, 0, id, 600.0 * DT, DamageKind::True, Hit::QUIET);
        }
    }
}
