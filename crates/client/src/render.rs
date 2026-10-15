//! Drawing the battlefield: terrain, telegraphs, units, projectiles, effects,
//! health bars and fog of war.

use crate::art::{self, Frame};
use crate::game::{FxKind, GameView};
use crate::gfx::{canvas_from_rgba, Gfx, SpriteKey};
use crate::terrain::{self, CHUNK, CHUNKS};
use crate::ui;
use moba_core::heroes::hero;
use moba_core::map::{GameMap, MAP_SIZE};
use moba_core::protocol::{flags, EntityView, ProjectileStyle, UnitView, ZoneKind, ZoneShape};
use moba_core::skills::{skill, Action, Targeting};
use moba_core::units::{MonsterKind, StructureKind};
use moba_core::{Team, Vec2};
use web_sys::HtmlCanvasElement;

/// Lazily rasterised terrain chunks (kept across matches).
pub struct TerrainCache {
    chunks: Vec<Option<HtmlCanvasElement>>,
    pub minimap: HtmlCanvasElement,
    pub minimap_size: usize,
    prefetch: usize,
}

impl TerrainCache {
    pub fn new() -> TerrainCache {
        let size = 128;
        let map = GameMap::get();
        let mm = terrain::minimap_rgba(map, size);
        TerrainCache {
            chunks: vec![None; CHUNKS * CHUNKS],
            minimap: canvas_from_rgba(size, size, &mm),
            minimap_size: size,
            prefetch: 0,
        }
    }

    pub fn chunk(&mut self, cx: usize, cy: usize) -> &HtmlCanvasElement {
        let i = cy * CHUNKS + cx;
        if self.chunks[i].is_none() {
            let px = terrain::chunk_rgba(GameMap::get(), cx, cy);
            self.chunks[i] = Some(canvas_from_rgba(CHUNK, CHUNK, &px));
        }
        self.chunks[i].as_ref().unwrap()
    }

    /// Rasterise one not-yet-built chunk (call once per idle frame).
    pub fn prefetch_one(&mut self) {
        while self.prefetch < self.chunks.len() {
            let i = self.prefetch;
            self.prefetch += 1;
            if self.chunks[i].is_none() {
                self.chunk(i % CHUNKS, i / CHUNKS);
                return;
            }
        }
    }

    pub fn draw(&mut self, g: &Gfx, ox: f64, oy: f64) {
        let x0 = (ox.max(0.0) / CHUNK as f64) as usize;
        let y0 = (oy.max(0.0) / CHUNK as f64) as usize;
        let x1 = (((ox + g.w) / CHUNK as f64) as usize).min(CHUNKS - 1);
        let y1 = (((oy + g.h) / CHUNK as f64) as usize).min(CHUNKS - 1);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let c = self.chunk(cx, cy).clone();
                g.draw(&c, (cx * CHUNK) as f64 - ox, (cy * CHUNK) as f64 - oy);
            }
        }
    }
}

impl Default for TerrainCache {
    fn default() -> Self {
        Self::new()
    }
}

fn team_ring(team: Team) -> u32 {
    match team {
        Team::Blue => 0x2c64c8,
        Team::Red => 0xc03030,
        Team::Neutral => 0x907830,
    }
}

impl GameView {
    pub fn render_world(&self, g: &Gfx, terrain: &mut TerrainCache) {
        let (ox, oy) = self.view_origin(g);
        g.clear(0x0c1410);
        terrain.draw(g, ox, oy);
        let Some(cur) = &self.cur else { return };
        let my_team = self.my_team();
        let me_id = self.me_view().map(|m| m.entity);
        let t = self.now / 1000.0;

        // Ground telegraphs: zones.
        for z in &cur.zones {
            let (sx, sy) = self.to_screen(g, z.pos);
            let enemy = z.team != my_team;
            match (&z.shape, z.kind) {
                (ZoneShape::Circle { radius }, ZoneKind::Delayed) => {
                    let r = *radius as f64;
                    g.set_alpha(0.18 + 0.3 * z.progress as f64);
                    g.ellipse(sx, sy, r * z.progress as f64, r * z.progress as f64, z.color);
                    g.set_alpha(1.0);
                    g.ring(sx, sy, r, if enemy { 0xff5040 } else { z.color });
                }
                (ZoneShape::Line { angle, length, width }, _) => {
                    let dir = Vec2::from_angle(*angle);
                    let end = z.pos + dir * *length;
                    let (ex, ey) = self.to_screen(g, end);
                    g.set_alpha(0.25 + 0.4 * z.progress as f64);
                    g.thick_line(sx, sy, ex, ey, *width as f64 * z.progress.max(0.2) as f64, z.color);
                    g.set_alpha(1.0);
                    let n = dir.perp() * (*width / 2.0);
                    for s in [1.0f32, -1.0] {
                        let a = self.to_screen(g, z.pos + n * s);
                        let b = self.to_screen(g, end + n * s);
                        g.line(a.0, a.1, b.0, b.1, if enemy { 0xff5040 } else { z.color });
                    }
                }
                (ZoneShape::Circle { radius }, ZoneKind::Trap) => {
                    let r = *radius as f64;
                    g.set_alpha(0.8);
                    g.ring(sx, sy, r, if enemy { 0xff5040 } else { 0x80ff80 });
                    g.set_alpha(1.0);
                    g.rect(sx - 5.0, sy - 1.0, 11.0, 3.0, 0x504030);
                    for k in 0..4 {
                        g.rect(sx - 5.0 + k as f64 * 3.0, sy - 3.0, 1.0, 2.0, 0xd0d0d0);
                    }
                }
            }
        }

        // Click marker.
        if let Some((p, at, attack)) = self.marker {
            let age = (self.now - at) / 400.0;
            if age < 1.0 {
                let (sx, sy) = self.to_screen(g, p);
                let r = 6.0 * (1.0 - age) + 2.0;
                g.ring(sx, sy, r, if attack { 0xff4040 } else { 0x60f060 });
            }
        }

        // Enemy tower range when we get close (like LoL's red tower circle).
        if let Some(me) = self.my_entity().filter(|e| e.alive()) {
            let mp = self.pos_of(me);
            for t in &cur.entities {
                let is_tower = matches!(t.unit, UnitView::Structure(StructureKind::Tower { .. }));
                if !is_tower || !t.alive() || t.team == my_team {
                    continue;
                }
                let reach = moba_core::units::TOWER_RANGE + 18.0 + 11.0; // tower + hero radius
                let d = t.pos.distance(mp);
                if d < reach + 70.0 {
                    let (tx, ty) = self.to_screen(g, t.pos);
                    let danger = d < reach + 10.0;
                    g.set_alpha(if danger { 0.75 } else { 0.35 });
                    g.ring(tx, ty, reach as f64, 0xff4040);
                    g.set_alpha(1.0);
                }
            }
        }

        // Skill / attack range indicators for my hero.
        if let (Some(me), Some(mev)) = (self.my_entity(), self.me_view()) {
            let mp = self.pos_of(me);
            let (mx, my) = self.to_screen(g, mp);
            if self.amove {
                g.set_alpha(0.6);
                g.ring(mx, my, mev.stats.range as f64 + 11.0, 0xff6050);
                g.set_alpha(1.0);
            }
            if let (Some(slot), Some(p)) = (self.held_skill, self.me.and_then(|m| self.players.get(m as usize))) {
                let sid = if slot < 3 { p.loadout.skills[slot as usize] } else { hero(p.loadout.hero).ultimate };
                let sk = skill(sid);
                let range = sk.range() as f64;
                g.set_alpha(0.7);
                g.ring(mx, my, range, 0x80c8ff);
                let aim = self.mouse_world;
                let dir = (aim - mp).normalize_or(Vec2::new(1.0, 0.0));
                let clamped = mp + (aim - mp).clamp_length(range as f32);
                let (ax, ay) = self.to_screen(g, clamped);
                match (sk.targeting, sk.action) {
                    (Targeting::Direction { range }, Action::Cone { half_angle_deg, .. }) => {
                        for s in [-1.0f32, 1.0] {
                            let e = mp + dir.rotate(half_angle_deg.to_radians() * s) * range;
                            let (ex, ey) = self.to_screen(g, e);
                            g.line(mx, my, ex, ey, 0x80c8ff);
                        }
                    }
                    (Targeting::Direction { range }, _) => {
                        let width = match sk.action {
                            Action::Projectile { width, .. } | Action::Line { width, .. } => width.max(6.0),
                            _ => 8.0,
                        };
                        let e = mp + dir * range;
                        let (ex, ey) = self.to_screen(g, e);
                        g.set_alpha(0.25);
                        g.thick_line(mx, my, ex, ey, width as f64, 0x80c8ff);
                        g.set_alpha(0.7);
                        g.line(mx, my, ex, ey, 0xc0e8ff);
                    }
                    (Targeting::Point { .. }, Action::Area { radius, .. }) => {
                        g.ring(ax, ay, radius as f64, 0xc0e8ff);
                    }
                    (Targeting::Point { .. }, Action::Blink { arrive_radius, .. })
                    | (Targeting::Point { .. }, Action::Dash { arrive_radius, .. }) => {
                        g.ring(ax, ay, (arrive_radius as f64).max(8.0), 0xc0e8ff);
                    }
                    (Targeting::Point { .. }, _) => g.ring(ax, ay, 8.0, 0xc0e8ff),
                    _ => {}
                }
                g.set_alpha(1.0);
            }
        }

        // Units, back to front.
        let mut order: Vec<(f32, &EntityView)> = cur.entities.iter().map(|e| (self.pos_of(e).y, e)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, e) in &order {
            if matches!(e.unit, UnitView::Hero { .. }) && !e.alive() {
                continue;
            }
            self.draw_unit(g, e, Some(e.id) == me_id, my_team, t);
        }

        // Projectiles.
        for p in &cur.projectiles {
            let (sx, sy) = self.to_screen(g, p.pos);
            let dir = Vec2::from_angle(p.angle);
            match p.style {
                ProjectileStyle::Arrow => {
                    let tail = p.pos - dir * 7.0;
                    let (tx, ty) = self.to_screen(g, tail);
                    g.line(tx, ty, sx, sy, 0xe8dcc0);
                    g.rect(sx - 1.0, sy - 1.0, 2.0, 2.0, p.color);
                }
                ProjectileStyle::Bolt => {
                    let tail = p.pos - dir * 6.0;
                    let (tx, ty) = self.to_screen(g, tail);
                    g.line(tx, ty, sx, sy, p.color);
                    g.rect(sx - 1.0, sy - 1.0, 3.0, 3.0, 0xffffff);
                }
                ProjectileStyle::Orb => {
                    g.disc(sx, sy, 2.5, p.color);
                    g.rect(sx - 1.0, sy - 1.0, 1.0, 1.0, 0xffffff);
                }
                ProjectileStyle::Cannon => g.disc(sx, sy, 2.5, 0x303038),
                ProjectileStyle::TowerShot => {
                    g.set_alpha(0.5);
                    g.disc(sx, sy, 5.0, p.color);
                    g.set_alpha(1.0);
                    g.disc(sx, sy, 3.0, p.color);
                    g.rect(sx - 1.0, sy - 1.0, 2.0, 2.0, 0xffffff);
                }
                ProjectileStyle::Hook => {
                    if let Some(owner) = self.entity(p.owner) {
                        let (ox2, oy2) = self.to_screen(g, self.pos_of(owner) - Vec2::new(0.0, 10.0));
                        g.line(ox2, oy2, sx, sy, 0x9a8a70);
                    }
                    g.rect(sx - 2.0, sy - 2.0, 4.0, 4.0, p.color);
                    g.rect(sx - 1.0, sy - 1.0, 2.0, 2.0, 0x404040);
                }
                ProjectileStyle::Shard => {
                    g.rect(sx - 2.0, sy - 2.0, 4.0, 4.0, p.color);
                    g.rect(sx - 1.0, sy - 2.0, 1.0, 1.0, 0xffffff);
                }
                ProjectileStyle::Beam => {
                    let tail = p.pos - dir * 26.0;
                    let (tx, ty) = self.to_screen(g, tail);
                    g.set_alpha(0.4);
                    g.thick_line(tx, ty, sx, sy, p.width as f64, p.color);
                    g.set_alpha(1.0);
                    g.thick_line(tx, ty, sx, sy, (p.width as f64 * 0.4).max(2.0), 0xffffff);
                }
            }
        }

        // Effects.
        for f in &self.fx {
            let k = (f.age / f.life).clamp(0.0, 1.0);
            let (sx, sy) = self.to_screen(g, f.pos);
            match &f.kind {
                FxKind::Burst { radius, color } => {
                    g.set_alpha(0.35 * (1.0 - k));
                    g.disc(sx, sy, radius * (0.6 + 0.4 * k), *color);
                    g.set_alpha(1.0 - k);
                    g.ring(sx, sy, radius * (0.6 + 0.4 * k), *color);
                    g.set_alpha(1.0);
                }
                FxKind::Ring { radius, color } => {
                    g.set_alpha(1.0 - k);
                    g.ring(sx, sy, radius * (0.4 + k), *color);
                    g.set_alpha(1.0);
                }
                FxKind::Cone { angle, half, range, color } => {
                    g.set_alpha(0.5 * (1.0 - k));
                    let steps = 9;
                    for i in 0..=steps {
                        let a = angle - half + 2.0 * half * i as f64 / steps as f64;
                        let e = f.pos + Vec2::from_angle(a as f32) * (*range as f32);
                        let (ex, ey) = self.to_screen(g, e);
                        g.thick_line(sx, sy, ex, ey, 3.0, *color);
                    }
                    g.set_alpha(1.0);
                }
                FxKind::Beam { to, width, color } => {
                    let (ex, ey) = self.to_screen(g, *to);
                    g.set_alpha(0.6 * (1.0 - k));
                    g.thick_line(sx, sy, ex, ey, *width, *color);
                    g.set_alpha(1.0 - k);
                    g.thick_line(sx, sy, ex, ey, (width * 0.3).max(1.0), 0xffffff);
                    g.set_alpha(1.0);
                }
                FxKind::Trail { to, color } => {
                    let (ex, ey) = self.to_screen(g, *to);
                    g.set_alpha(0.7 * (1.0 - k));
                    g.line(sx, sy - 8.0, ex, ey - 8.0, *color);
                    g.set_alpha(1.0);
                }
                FxKind::Spark { color, .. } => {
                    g.set_alpha(1.0 - k * k);
                    g.rect(sx, sy - 8.0, 1.0, 1.0, *color);
                    g.set_alpha(1.0);
                }
                FxKind::Text { .. } => {}
            }
        }

        // Fog of war.
        if self.has_fog {
            g.draw_smooth(&self.fog_canvas, -ox, -oy, MAP_SIZE as f64, MAP_SIZE as f64);
        }

        // Overhead bars on top of the fog so allies are readable.
        for (_, e) in &order {
            self.draw_bars(g, e, Some(e.id) == me_id, my_team);
        }
        // Floating combat text.
        for f in &self.fx {
            if let FxKind::Text { s, color, big } = &f.kind {
                let k = f.age / f.life;
                let (sx, sy) = self.to_screen(g, f.pos);
                g.set_alpha((1.5 - k * 1.5).min(1.0));
                g.text_center(s, sx, sy - k * 14.0, *color, if *big { 2.0 } else { 1.0 }, true);
                g.set_alpha(1.0);
            }
        }
        // Hover outline.
        if let Some(h) = self.hover.and_then(|id| self.entity(id)) {
            let (sx, sy) = self.to_screen(g, self.pos_of(h));
            let (hw, _) = GameView::hit_box(h);
            let c = if h.team == my_team { 0x80ff80 } else { 0xff5050 };
            g.set_alpha(0.9);
            g.ring(sx, sy, hw as f64 + 4.0, c);
            g.set_alpha(1.0);
        }
    }

    fn draw_unit(&self, g: &Gfx, e: &EntityView, mine: bool, my_team: Team, t: f64) {
        let p = self.pos_of(e);
        let (sx, sy) = self.to_screen(g, p);
        let (hw, hh) = GameView::hit_box(e);
        if sx < -60.0 || sy < -40.0 || sx > g.w + 60.0 || sy > g.h + 80.0 {
            return;
        }
        let flip = e.facing.cos() < 0.0;
        let moving = e.has(flags::MOVING) || e.has(flags::DASHING);
        let frame = if e.has(flags::ATTACKING) || e.has(flags::CHANNELING) {
            Frame::Attack
        } else if moving && ((t * 7.0 + e.id as f64 * 0.37) as i64) % 2 == 0 {
            Frame::Walk
        } else {
            Frame::Idle
        };
        let lift = if e.has(flags::AIRBORNE) { 7.0 } else { 0.0 };
        let in_brush = e.has(flags::IN_BRUSH);
        let is_structure = matches!(e.unit, UnitView::Structure(_));

        // Shadow + team ring.
        if !is_structure || e.alive() {
            g.set_alpha(0.35);
            g.ellipse(sx, sy + 1.0, hw as f64 + 1.0, (hw as f64 * 0.4).max(2.0), 0x000000);
            g.set_alpha(1.0);
        }
        if matches!(e.unit, UnitView::Hero { .. }) {
            let c = if mine { 0x58e058 } else { team_ring(e.team) };
            g.set_alpha(0.85);
            g.ring(sx, sy + 1.0, hw as f64 + 3.0, c);
            g.set_alpha(1.0);
        }
        // Buff auras.
        if e.has(flags::BARON) {
            g.set_alpha(0.35);
            g.ellipse(sx, sy, 14.0, 6.0, 0xa050ff);
            g.set_alpha(1.0);
        }
        if e.has(flags::RECALLING) {
            let h = 26.0 + (t * 6.0).sin() * 3.0;
            g.set_alpha(0.35);
            g.rect(sx - 7.0, sy - h, 14.0, h, 0x6080ff);
            g.set_alpha(1.0);
        }

        let key = match e.unit {
            UnitView::Hero { hero: h, .. } => SpriteKey::Hero(h, frame, flip),
            UnitView::Minion(k) => SpriteKey::Minion(k, e.team, frame, flip),
            UnitView::Structure(StructureKind::Tower { .. }) => SpriteKey::Tower(e.team, e.alive()),
            UnitView::Structure(StructureKind::Inhibitor { .. }) => SpriteKey::Inhibitor(e.team, e.alive()),
            UnitView::Structure(StructureKind::Nexus) => SpriteKey::Nexus(e.team, e.alive()),
            UnitView::Monster(k) => SpriteKey::Monster(k, frame, flip),
        };
        let spr = g.sprite(key);
        let (ax, ay) = match e.unit {
            UnitView::Hero { .. } => (art::HERO_ANCHOR.0, art::HERO_ANCHOR.1),
            UnitView::Structure(_) => (spr.w / 2.0, spr.h - 6.0),
            _ => (spr.w / 2.0, spr.h - 3.0),
        };
        let ax = if flip && matches!(e.unit, UnitView::Hero { .. }) { spr.w - ax } else { ax };
        if in_brush {
            g.set_alpha(0.55);
        }
        // Crystals bob gently.
        let bob = if is_structure && e.alive() && !matches!(e.unit, UnitView::Structure(StructureKind::Tower { .. })) {
            ((t * 2.0 + e.id as f64).sin() * 1.0).round()
        } else {
            0.0
        };
        g.draw(&spr.canvas, sx - ax, sy - ay - lift + bob);
        g.set_alpha(1.0);

        // Status markers.
        let top = sy - hh as f64 - lift - 4.0;
        if e.has(flags::STUNNED) || e.has(flags::AIRBORNE) {
            for i in 0..3 {
                let a = t * 6.0 + i as f64 * 2.1;
                g.rect(sx + a.cos() * 6.0, top + a.sin() * 2.0, 2.0, 2.0, 0xfff060);
            }
        }
        if e.has(flags::ROOTED) {
            for dx in [-5.0, -1.0, 3.0] {
                g.rect(sx + dx, sy - 3.0, 2.0, 4.0, 0x58b048);
                g.rect(sx + dx - 1.0, sy - 4.0, 1.0, 2.0, 0x78d058);
            }
        }
        if e.has(flags::SLOWED) {
            g.rect(sx - hw as f64 - 3.0, sy - 6.0, 2.0, 1.0, 0x80c0ff);
            g.rect(sx - hw as f64 - 2.0, sy - 5.0, 1.0, 1.0, 0x80c0ff);
        }
        if e.has(flags::SILENCED) {
            g.text_s("...", sx - 8.0, top - 4.0, 0xc080ff);
        }
        if e.has(flags::TAUNTED) {
            g.text_s("!", sx - 2.0, top - 6.0, 0xff4040);
        }
        if e.has(flags::BLINDED) {
            g.set_alpha(0.7);
            g.ellipse(sx, top, 6.0, 3.0, 0x404048);
            g.set_alpha(1.0);
        }
        if e.has(flags::BURNING) && ((t * 10.0) as i64 + e.id as i64) % 3 == 0 {
            g.rect(sx - 3.0 + ((t * 13.0).sin() * 4.0).round(), sy - 10.0, 1.0, 2.0, 0xff8030);
        }
        if e.shield > 0.0 && e.alive() {
            g.set_alpha(0.5);
            g.ring(sx, sy - hh as f64 / 2.0 - lift, hw as f64 + 5.0, 0xf0f0ff);
            g.set_alpha(1.0);
        }
        if e.has(flags::EMPOWERED) {
            g.rect(sx + if flip { -9.0 } else { 7.0 }, sy - 14.0, 2.0, 2.0, 0xfff080);
        }
        if e.has(flags::BLUE_BUFF) {
            g.rect(sx - 1.0, sy + 3.0, 2.0, 2.0, 0x60b0ff);
        }
        if e.has(flags::RED_BUFF) {
            g.rect(sx + 2.0, sy + 3.0, 2.0, 2.0, 0xff6030);
        }
        // Channel indicator (e.g. Deadeye's aim line, Skyfall's landing spot).
        if let Some((sid, aim)) = e.cast {
            let sk = skill(sid);
            let (ex, ey) = self.to_screen(g, aim);
            let c = if e.team == my_team { 0x80c8ff } else { 0xff5040 };
            match sk.targeting {
                Targeting::EnemyHero { .. } | Targeting::Enemy { .. } | Targeting::Direction { .. } => {
                    g.set_alpha(0.6);
                    g.line(sx, sy - 8.0, ex, ey - 8.0, c);
                    g.set_alpha(1.0);
                }
                Targeting::Point { .. } => {
                    let r = match sk.action {
                        Action::Blink { arrive_radius, .. } | Action::Dash { arrive_radius, .. } => {
                            arrive_radius as f64
                        }
                        Action::Area { radius, .. } => radius as f64,
                        _ => 12.0,
                    };
                    g.ring(ex, ey, r, c);
                }
                _ => {
                    if let Action::Area { radius, .. } = sk.action {
                        g.ring(sx, sy, radius as f64, c);
                    }
                }
            }
        }
    }

    fn draw_bars(&self, g: &Gfx, e: &EntityView, mine: bool, my_team: Team) {
        if !e.alive() {
            return;
        }
        let p = self.pos_of(e);
        let (sx, sy) = self.to_screen(g, p);
        if sx < -40.0 || sy < -10.0 || sx > g.w + 40.0 || sy > g.h + 70.0 {
            return;
        }
        let (_, hh) = GameView::hit_box(e);
        let lift = if e.has(flags::AIRBORNE) { 7.0 } else { 0.0 };
        let frac = (e.hp / e.max_hp.max(1.0)) as f64;
        let ally = e.team == my_team;
        let hp_color = if mine {
            0x58d048
        } else if e.team == Team::Neutral {
            0xd8b040
        } else if ally {
            0x4890e8
        } else {
            0xe04848
        };
        match e.unit {
            UnitView::Hero { level, player, .. } => {
                let w = 30.0;
                let x = (sx - w / 2.0).floor();
                let y = (sy - hh as f64 - 9.0 - lift).floor();
                g.rect(x - 1.0, y - 1.0, w + 2.0, 6.0, 0x101018);
                let total = (e.max_hp + e.shield).max(1.0) as f64;
                let hp_w = (w * e.hp as f64 / total).round();
                g.rect(x, y, hp_w, 3.0, hp_color);
                g.rect(x + hp_w, y, (w * e.shield as f64 / total).round(), 3.0, 0xf0f0f0);
                for tick in 1..((e.max_hp / 250.0) as i32).min(30) {
                    let tx = x + (w * tick as f64 * 250.0 / total).round();
                    if tx < x + hp_w {
                        g.rect(tx, y, 1.0, 2.0, crate::art::shade(hp_color, 0.6));
                    }
                }
                if ally && e.max_mana > 0.0 {
                    ui::bar(g, x, y + 3.0, w, 1.0, (e.mana / e.max_mana) as f64, 0x5080f0, 0x202040);
                }
                g.rect(x - 9.0, y - 2.0, 9.0, 8.0, 0x101018);
                let lv = level.to_string();
                g.text(&lv, x - 8.0 + if level < 10 { 2.0 } else { 0.0 }, y - 1.0, ui::GOLD);
                let name = self.player_name(player);
                g.text_center(
                    &name,
                    sx,
                    y - 10.0,
                    if mine {
                        0xc8ffc0
                    } else if ally {
                        0xc0dcff
                    } else {
                        0xffc0c0
                    },
                    1.0,
                    true,
                );
            }
            UnitView::Minion(_) => {
                if frac < 0.999 {
                    let w = 14.0;
                    let x = (sx - w / 2.0).floor();
                    let y = (sy - hh as f64 - 3.0 - lift).floor();
                    g.rect(x - 1.0, y - 1.0, w + 2.0, 3.0, 0x101018);
                    g.rect(x, y, (w * frac).round(), 1.0, hp_color);
                }
            }
            UnitView::Monster(k) => {
                if frac < 0.999 || k.is_epic() {
                    let w = if k.is_epic() { 40.0 } else { 20.0 };
                    let x = (sx - w / 2.0).floor();
                    let y = (sy - hh as f64 - 5.0).floor();
                    g.rect(x - 1.0, y - 1.0, w + 2.0, 4.0, 0x101018);
                    g.rect(x, y, (w * frac).round(), 2.0, hp_color);
                    if matches!(k, MonsterKind::Dragon | MonsterKind::Baron) {
                        g.text_center(k.name(), sx, y - 9.0, 0xf0d0a0, 1.0, true);
                    }
                }
            }
            UnitView::Structure(_) => {
                let w = 30.0;
                let x = (sx - w / 2.0).floor();
                let y = (sy - hh as f64 - 6.0).floor();
                g.rect(x - 1.0, y - 1.0, w + 2.0, 5.0, 0x101018);
                let c = if e.has(flags::INVULNERABLE) { 0x808080 } else { hp_color };
                g.rect(x, y, (w * frac).round(), 3.0, c);
            }
        }
    }
}
