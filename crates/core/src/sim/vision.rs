//! Fog of war. Each team sees what its units, structures and fountain can
//! see; walls block sight, and brush hides units from anyone outside it.

use super::*;
use crate::map::FOUNTAIN_RADIUS;
use crate::protocol::{VISION_CELL, VISION_GRID};

#[derive(Clone, Debug)]
pub struct TeamVision {
    pub cells: Vec<bool>,
    /// Brush regions that have a friendly unit inside.
    pub brushes: Vec<u16>,
}

impl TeamVision {
    pub fn new() -> Self {
        TeamVision { cells: vec![false; VISION_GRID * VISION_GRID], brushes: Vec::new() }
    }

    pub fn cell_visible(&self, p: Vec2) -> bool {
        match cell_index(p) {
            Some(i) => self.cells[i],
            None => false,
        }
    }

    /// Packed bitmap for the network.
    pub fn packed(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.cells.len().div_ceil(8)];
        for (i, v) in self.cells.iter().enumerate() {
            if *v {
                out[i / 8] |= 1 << (i % 8);
            }
        }
        out
    }
}

impl Default for TeamVision {
    fn default() -> Self {
        Self::new()
    }
}

pub fn cell_index(p: Vec2) -> Option<usize> {
    let x = (p.x / VISION_CELL).floor();
    let y = (p.y / VISION_CELL).floor();
    if x < 0.0 || y < 0.0 || x >= VISION_GRID as f32 || y >= VISION_GRID as f32 {
        return None;
    }
    Some(y as usize * VISION_GRID + x as usize)
}

pub fn sight_radius(e: &Entity) -> f32 {
    match &e.kind {
        UnitKind::Hero(_) => 340.0,
        UnitKind::Minion(_) => 220.0,
        UnitKind::Structure(s) => match s.kind {
            StructureKind::Tower { .. } => 280.0,
            _ => 220.0,
        },
        UnitKind::Monster(_) => 0.0,
    }
}

pub(crate) fn update(w: &mut World) {
    let map = w.map;
    for (t, team) in Team::PLAYABLE.into_iter().enumerate() {
        let mut v = TeamVision::new();
        // The base around the fountain is always visible.
        let f = map.fountain(team);
        mark_disc(&mut v, f, FOUNTAIN_RADIUS + 120.0);
        for e in &w.entities {
            if !e.alive || e.team != team {
                continue;
            }
            let r = sight_radius(e);
            if r <= 0.0 {
                continue;
            }
            let b = map.brush_at(e.pos);
            if b != 0 && !v.brushes.contains(&b) {
                v.brushes.push(b);
            }
            cast_rays(&mut v, map, e.pos, r);
        }
        w.vision[t] = v;
    }
    w.vision_dirty = true;
}

fn mark_disc(v: &mut TeamVision, c: Vec2, r: f32) {
    let steps = (r / VISION_CELL).ceil() as i32;
    for dy in -steps..=steps {
        for dx in -steps..=steps {
            let p = c + Vec2::new(dx as f32, dy as f32) * VISION_CELL;
            if p.distance(c) <= r {
                if let Some(i) = cell_index(p) {
                    v.cells[i] = true;
                }
            }
        }
    }
}

fn cast_rays(v: &mut TeamVision, map: &GameMap, origin: Vec2, r: f32) {
    if let Some(i) = cell_index(origin) {
        v.cells[i] = true;
    }
    let rays = ((std::f32::consts::TAU * r) / 18.0).ceil() as usize;
    let step = 10.0;
    for k in 0..rays {
        let dir = Vec2::from_angle(k as f32 / rays as f32 * std::f32::consts::TAU);
        let mut s = step;
        while s <= r {
            let p = origin + dir * s;
            let Some(i) = cell_index(p) else { break };
            v.cells[i] = true;
            if !map.walkable(p) {
                break;
            }
            s += step;
        }
    }
}

/// Is entity `i` visible to `team`? (Neutral = spectator: sees everything.)
pub(crate) fn entity_visible(w: &World, team: Team, i: usize) -> bool {
    let e = &w.entities[i];
    if team == Team::Neutral || e.team == team || e.is_structure() {
        return true;
    }
    let v = &w.vision[team.index()];
    if !v.cell_visible(e.pos) {
        return false;
    }
    let b = w.map.brush_at(e.pos);
    if b != 0 && !v.brushes.contains(&b) && w.time > e.revealed_until {
        return false;
    }
    true
}

pub(crate) fn point_visible(w: &World, team: Team, p: Vec2) -> bool {
    team == Team::Neutral || w.vision[team.index().min(1)].cell_visible(p)
}
