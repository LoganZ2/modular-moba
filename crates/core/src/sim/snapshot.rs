//! Build per-player [`Snapshot`]s: only what that player's team can see.

use super::vision::point_visible;
use super::*;
use crate::protocol::{
    flags, EntityView, PlayerRow, ProjectileView, SelfView, Snapshot, StatsView, UnitView, ZoneShape, ZoneView,
};
use crate::skills::skill;

pub(crate) fn build(w: &World, player: Option<usize>) -> Snapshot {
    let team = player.map(|p| w.players[p].team).unwrap_or(Team::Neutral);
    let my_entity = player.map(|p| w.players[p].entity);

    let mut entities = Vec::with_capacity(w.entities.len());
    for (i, e) in w.entities.iter().enumerate() {
        let mine = Some(e.id) == my_entity;
        if e.is_hero() && !e.alive && !mine {
            continue;
        }
        if !mine && !w.visible_to(team, i) {
            continue;
        }
        entities.push(entity_view(w, e));
    }

    let projectiles = w
        .projectiles
        .iter()
        .filter(|p| !p.dead && (p.team == team || point_visible(w, team, p.pos)))
        .map(|p| ProjectileView {
            id: p.id,
            pos: p.pos,
            angle: p.dir.angle(),
            style: p.style,
            color: p.color,
            team: p.team,
            owner: p.owner,
            width: p.width,
        })
        .collect();

    let zones = w
        .zones
        .iter()
        .filter(|z| !z.dead && (z.team == team || point_visible(w, team, z.pos)))
        .map(|z| ZoneView {
            id: z.id,
            pos: z.pos,
            shape: match z.area {
                ZoneArea::Circle(r) => ZoneShape::Circle { radius: r },
                ZoneArea::Line { dir, length, width } => ZoneShape::Line { angle: dir.angle(), length, width },
            },
            kind: z.kind,
            team: z.team,
            color: skill(z.skill).color,
            progress: if z.total > 0.0 { 1.0 - (z.timer / z.total).clamp(0.0, 1.0) } else { 1.0 },
        })
        .collect();

    let events = w
        .events
        .iter()
        .filter(|(aud, _)| match aud {
            Audience::All => true,
            Audience::Team(t) => *t == team || team == Team::Neutral,
            Audience::Player(p) => Some(*p) == player,
            Audience::At(pos) => point_visible(w, team, *pos),
        })
        .map(|(_, ev)| ev.clone())
        .collect();

    let me = my_entity.and_then(|id| w.get(id)).map(|e| self_view(w, e));

    // The scoreboard changes slowly: send it three times a second.
    let players = if !w.tick.is_multiple_of(10) && w.winner.is_none() {
        Vec::new()
    } else {
        w.players
            .iter()
            .map(|p| {
                let e = w.get(p.entity);
                let h = e.and_then(|e| e.hero());
                PlayerRow {
                    index: p.index as u8,
                    team: p.team,
                    hero: p.loadout.hero,
                    level: h.map(|h| h.level).unwrap_or(1),
                    kills: h.map(|h| h.kills).unwrap_or(0),
                    deaths: h.map(|h| h.deaths).unwrap_or(0),
                    assists: h.map(|h| h.assists).unwrap_or(0),
                    cs: h.map(|h| h.cs).unwrap_or(0),
                    items: h.map(|h| h.items.map(|s| s.map(|(id, _)| id))).unwrap_or([None; 6]),
                    respawn: h
                        .map(|h| if e.map(|e| e.alive).unwrap_or(true) { 0.0 } else { h.respawn.max(0.0) })
                        .unwrap_or(0.0),
                }
            })
            .collect()
    };

    let fog = if w.vision_dirty && team != Team::Neutral { Some(w.vision[team.index()].packed()) } else { None };

    Snapshot {
        tick: w.tick,
        time: w.time,
        entities,
        projectiles,
        zones,
        events,
        me,
        score: [w.teams[0].score, w.teams[1].score],
        players,
        fog,
        winner: w.winner,
    }
}

fn entity_view(w: &World, e: &Entity) -> EntityView {
    let mut f = 0u32;
    for s in &e.statuses {
        f |= match s.kind {
            StatusKind::Slow(_) => flags::SLOWED,
            StatusKind::Stun => flags::STUNNED,
            StatusKind::Root => flags::ROOTED,
            StatusKind::Airborne => flags::AIRBORNE,
            StatusKind::Silence => flags::SILENCED,
            StatusKind::Taunt(_) => flags::TAUNTED,
            StatusKind::Blind => flags::BLINDED,
            StatusKind::Vulnerable(_) => flags::VULNERABLE,
            StatusKind::Burn { .. } => flags::BURNING,
            StatusKind::BlueBuff => flags::BLUE_BUFF,
            StatusKind::RedBuff => flags::RED_BUFF,
            StatusKind::BaronBuff => flags::BARON,
            StatusKind::Ghost => flags::HASTED,
            StatusKind::Buff(crate::skills::BuffStat::MoveSpeed, _) => flags::HASTED,
            StatusKind::Unstoppable => flags::UNSTOPPABLE,
            _ => 0,
        };
    }
    if !e.alive {
        f |= flags::DEAD;
    }
    if e.invulnerable {
        f |= flags::INVULNERABLE;
    }
    if w.map.brush_at(e.pos) != 0 {
        f |= flags::IN_BRUSH;
    }
    if e.dash.is_some() {
        f |= flags::DASHING;
    }
    if e.windup.is_some() {
        f |= flags::ATTACKING;
    }
    if e.vel.length_sq() > 1.0 {
        f |= flags::MOVING;
    }
    let mut cast = None;
    let mut unit = match &e.kind {
        UnitKind::Minion(m) => UnitView::Minion(m.kind),
        UnitKind::Structure(s) => UnitView::Structure(s.kind),
        UnitKind::Monster(m) => UnitView::Monster(m.kind),
        UnitKind::Hero(_) => UnitView::Minion(crate::units::MinionKind::Melee),
    };
    if let Some(h) = e.hero() {
        unit = UnitView::Hero { hero: h.hero, level: h.level, player: h.player as u8 };
        if h.recall.is_some() {
            f |= flags::RECALLING;
        }
        if h.empower > 0.0 {
            f |= flags::EMPOWERED;
        }
        if let Some(c) = h.cast {
            f |= flags::CHANNELING;
            let aim = c.target.unit.and_then(|u| w.get(u)).map(|t| t.pos).unwrap_or(c.target.point);
            cast = Some((h.skills[c.slot as usize], aim));
        }
    }
    EntityView {
        id: e.id,
        team: e.team,
        unit,
        pos: e.pos,
        facing: e.facing,
        hp: e.hp.max(0.0),
        max_hp: e.stats.max_hp,
        shield: e.shield(),
        mana: e.mana,
        max_mana: e.stats.max_mana,
        flags: f,
        target: e.windup.map(|(t, _)| t).or(e.engage),
        cast,
    }
}

fn self_view(w: &World, e: &Entity) -> SelfView {
    let Some(h) = e.hero() else {
        return SelfView::default();
    };
    let mut mana_costs = [0.0; 4];
    for (s, cost) in mana_costs.iter_mut().enumerate() {
        *cost = skill(h.skills[s]).mana_at(h.ranks[s].max(1));
    }
    SelfView {
        entity: e.id,
        gold: h.gold,
        xp: h.xp,
        xp_next: xp_to_next(h.level),
        level: h.level,
        ranks: h.ranks,
        skill_points: h.skill_points,
        cooldowns: h.cooldowns,
        cooldown_max: h.cooldown_max,
        mana_costs,
        summoner_cd: h.summoner_cd,
        items: h.items,
        stats: StatsView {
            ad: e.stats.ad,
            ap: e.stats.ap,
            armor: e.stats.armor,
            mr: e.stats.mr,
            attack_speed: e.stats.attack_speed,
            move_speed: e.stats.move_speed,
            crit: e.stats.crit,
            haste: e.stats.haste,
            range: e.stats.range,
            lifesteal: e.stats.lifesteal,
        },
        respawn: if e.alive { 0.0 } else { h.respawn.max(0.0) },
        recall: h.recall.unwrap_or(0.0),
        can_shop: abilities::in_shop_range(w, e),
        casting: h.cast.map(|c| (c.slot, 1.0 - c.remaining / c.total.max(1e-3))),
        empower: h.empower,
    }
}
