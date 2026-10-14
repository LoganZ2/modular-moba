//! Walking (with A* for champions), dashes / displacements and unit
//! separation.

use super::abilities;
use super::*;
use crate::skills::{skill, Action};

pub(crate) fn update(w: &mut World) {
    for i in 0..w.entities.len() {
        if w.entities[i].dash.is_some() {
            update_dash(w, i);
        }
    }
    let map = w.map;
    for e in &mut w.entities {
        let old = e.pos;
        if !e.alive || e.is_structure() || e.dash.is_some() || !e.can_move() {
            e.vel = Vec2::ZERO;
            continue;
        }
        let Some(goal) = e.move_goal else {
            e.vel = Vec2::ZERO;
            e.path.clear();
            e.stuck_time = 0.0;
            continue;
        };
        let mut budget = e.stats.move_speed * DT;
        if e.is_hero() {
            let goal_changed = e.path_goal.distance(goal) > 10.0;
            let ran_out = e.path.is_empty() && e.pos.distance(goal) > 2.0;
            let need_path = (goal_changed || ran_out) && e.repath <= 0.0 || e.stuck_time > 0.4;
            if need_path {
                e.path = map.find_path(e.pos, goal).unwrap_or_default();
                e.path_goal = goal;
                e.repath = 0.25;
                if e.stuck_time > 0.4 {
                    e.stuck_time = 0.0;
                }
            }
            while budget > 1e-3 && !e.path.is_empty() {
                let wp = e.path[0];
                let d = e.pos.distance(wp);
                if d <= budget {
                    e.pos = map.slide_move(e.pos, wp - e.pos);
                    budget -= d;
                    e.path.remove(0);
                } else {
                    let step = (wp - e.pos) / d * budget;
                    e.pos = map.slide_move(e.pos, step);
                    budget = 0.0;
                }
            }
        } else {
            let step = (goal - e.pos).clamp_length(budget);
            e.pos = map.slide_move(e.pos, step);
        }
        let moved = e.pos - old;
        if moved.length_sq() > 1e-4 {
            e.facing = moved.angle();
        }
        let expected = e.stats.move_speed * DT;
        if moved.length() < expected * 0.2 && e.pos.distance(goal) > 4.0 {
            e.stuck_time += DT;
        } else {
            e.stuck_time = 0.0;
        }
        e.vel = moved / DT;
    }
    separate(w);
}

fn update_dash(w: &mut World, i: usize) {
    let Some(mut dash) = w.entities[i].dash.clone() else {
        return;
    };
    let map = w.map;
    let (id, team, radius) = {
        let e = &w.entities[i];
        (e.id, e.team, e.radius)
    };
    if !w.entities[i].alive {
        w.entities[i].dash = None;
        return;
    }
    let mut dest = dash.to;
    let mut target_alive = true;
    if let Some(t) = dash.target {
        match w.get(t) {
            Some(te) if te.alive => dest = te.pos,
            _ => target_alive = false,
        }
    }
    let old = w.entities[i].pos;
    let step = dash.speed * DT;
    let mut next = old.move_towards(dest, step);
    let mut finished = !target_alive || next.distance(dest) < 0.5;
    dash.time_left -= DT;

    match &mut dash.kind {
        DashKind::Knockback { wall_stun } => {
            if !map.walkable(next) {
                next = old;
                finished = true;
                if *wall_stun > 0.0 {
                    let e = &mut w.entities[i];
                    e.add_status(StatusKind::Stun, *wall_stun);
                    if let Some(h) = e.hero_mut() {
                        h.cast = None;
                    }
                }
            }
        }
        DashKind::Pull => {
            if !map.walkable(next) {
                next = map.slide_move(old, next - old);
            }
        }
        DashKind::Skill { skill: sid, rank, stats, hit } => {
            let sk = skill(*sid);
            let Action::Dash { stop_on_enemy, hit: hit_fx, .. } = sk.action else {
                w.entities[i].dash = None;
                return;
            };
            if let Some(t) = dash.target {
                // Targeted dash: strike when we reach the target.
                let reach = w.get(t).map(|te| te.pos.distance(next) <= te.radius + radius + 4.0).unwrap_or(false);
                if reach {
                    finished = true;
                    let stats = *stats;
                    let rank = *rank;
                    w.entities[i].pos = next;
                    abilities::apply_effects(w, id, team, &stats, rank, t, hit_fx, old);
                }
            } else if !hit_fx.is_empty() {
                let victims: Vec<EntityId> = w
                    .entities
                    .iter()
                    .filter(|e| {
                        e.alive
                            && e.targetable()
                            && !e.is_structure()
                            && e.team != team
                            && !hit.contains(&e.id)
                            && crate::math::dist_point_segment(e.pos, old, next) <= e.radius + radius + 2.0
                    })
                    .map(|e| e.id)
                    .collect();
                let stats = *stats;
                let rank = *rank;
                for v in victims {
                    hit.push(v);
                    abilities::apply_effects(w, id, team, &stats, rank, v, hit_fx, old);
                    if stop_on_enemy {
                        finished = true;
                        if let Some(ve) = w.get(v) {
                            let back = (old - ve.pos).normalize_or_zero();
                            next = ve.pos + back * (ve.radius + radius + 1.0);
                        }
                        break;
                    }
                }
            }
        }
    }
    if dash.time_left <= 0.0 {
        finished = true;
    }
    // The dashing unit may have been stunned / killed by a reaction above.
    let still_dashing = w.entities[i].dash.is_some() && w.entities[i].alive;
    let e = &mut w.entities[i];
    if !still_dashing {
        if !map.walkable(e.pos) {
            e.pos = map.nearest_walkable(e.pos);
        }
        return;
    }
    e.pos = next;
    let mv = next - old;
    if mv.length_sq() > 1e-4 && !matches!(dash.kind, DashKind::Knockback { .. } | DashKind::Pull) {
        e.facing = mv.angle();
    }
    e.vel = mv / DT;
    if finished {
        e.dash = None;
        if !map.walkable(e.pos) {
            e.pos = map.nearest_walkable(e.pos);
        }
        e.path.clear();
        e.repath = 0.0;
        abilities::finish_dash(w, i, &dash);
    } else {
        e.dash = Some(dash);
    }
}

/// Push overlapping units apart so they don't stack on one pixel.
fn separate(w: &mut World) {
    let map = w.map;
    let movers: Vec<usize> = (0..w.entities.len())
        .filter(|&i| {
            let e = &w.entities[i];
            e.alive && !e.is_structure() && e.dash.is_none()
        })
        .collect();
    let structures: Vec<(Vec2, f32)> =
        w.entities.iter().filter(|e| e.is_structure() && e.alive).map(|e| (e.pos, e.radius)).collect();
    let mut push = vec![Vec2::ZERO; w.entities.len()];
    for a in 0..movers.len() {
        for b in a + 1..movers.len() {
            let (i, j) = (movers[a], movers[b]);
            let (ei, ej) = (&w.entities[i], &w.entities[j]);
            let d = ei.pos - ej.pos;
            let min = (ei.radius + ej.radius) * 0.85;
            let dist2 = d.length_sq();
            if dist2 >= min * min {
                continue;
            }
            let dist = dist2.sqrt();
            let n = if dist > 1e-3 { d / dist } else { Vec2::from_angle((i * 7 + j * 13) as f32) };
            let overlap = (min - dist) * 0.35;
            let (wi, wj) = match (ei.is_epic_monster(), ej.is_epic_monster()) {
                (true, false) => (0.0, 1.0),
                (false, true) => (1.0, 0.0),
                _ => (0.5, 0.5),
            };
            push[i] += n * overlap * wi;
            push[j] -= n * overlap * wj;
        }
    }
    for &i in &movers {
        let e = &mut w.entities[i];
        let mut p = e.pos;
        if push[i].length_sq() > 1e-6 {
            p = map.slide_move(p, push[i]);
        }
        for (sp, sr) in &structures {
            let d = p - *sp;
            let min = sr + e.radius * 0.8;
            let dist = d.length();
            if dist < min {
                let n = d.normalize_or(Vec2::new(0.0, 1.0));
                p = map.slide_move(p, n * (min - dist));
            }
        }
        e.pos = p;
    }
}
