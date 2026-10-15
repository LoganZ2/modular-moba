//! In-match client state: snapshot interpolation, visual effects, camera and
//! the League-style control scheme (input -> [`Command`]s).

use crate::gfx::{make_canvas, Gfx};
use crate::input::Input;
use moba_core::heroes::hero;
use moba_core::map::MAP_SIZE;
use moba_core::protocol::{
    flags, Command, EntityView, FxEvent, Killer, PlayerInfo, SelfView, Snapshot, UnitView, VISION_GRID,
};
use moba_core::sim::{PlayerSetup, World};
use moba_core::skills::{skill, DamageKind, Targeting};
use moba_core::{EntityId, Team, Vec2, TICK_RATE};
use std::collections::{HashMap, VecDeque};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

pub const TICK_MS: f64 = 1000.0 / TICK_RATE as f64;

/// The in-browser practice simulation.
pub struct LocalSim {
    pub world: World,
    pub me: usize,
    pending: Vec<Command>,
    acc: f64,
    pub paused: bool,
    /// Fast-forward factor (debug: `?speed=N` in the page URL).
    pub speed: f64,
}

/// `?speed=N` from the page URL (1 when absent).
pub fn url_speed() -> f64 {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|q| q.trim_start_matches('?').split('&').find_map(|kv| kv.strip_prefix("speed=").map(str::to_string)))
        .and_then(|v| v.parse::<f64>().ok())
        .map(|v| v.clamp(0.25, 30.0))
        .unwrap_or(1.0)
}

impl LocalSim {
    pub fn new(setups: Vec<PlayerSetup>, me: usize, seed: u64) -> LocalSim {
        LocalSim {
            world: World::new(setups, seed),
            me,
            pending: Vec::new(),
            acc: 0.0,
            paused: false,
            speed: url_speed(),
        }
    }

    pub fn push(&mut self, c: Command) {
        self.pending.push(c);
    }

    pub fn advance(&mut self, dt_ms: f64) -> Vec<Snapshot> {
        let mut out = Vec::new();
        if self.paused {
            return out;
        }
        self.acc += dt_ms.min(250.0) * self.speed;
        let max_steps = (6.0 * self.speed).ceil() as usize;
        let mut n = 0;
        while self.acc >= TICK_MS && n < max_steps {
            let me = self.me;
            let cmds: Vec<(usize, Command)> = self.pending.drain(..).map(|c| (me, c)).collect();
            self.world.step(&cmds);
            out.push(self.world.snapshot_for(Some(me)));
            self.acc -= TICK_MS;
            n += 1;
        }
        if n == max_steps {
            self.acc = 0.0;
        }
        out
    }

    pub fn players(&self) -> Vec<PlayerInfo> {
        self.world
            .players
            .iter()
            .map(|p| PlayerInfo {
                index: p.index as u8,
                name: p.name.clone(),
                team: p.team,
                loadout: p.loadout,
                bot: p.bot.is_some(),
                entity: p.entity,
            })
            .collect()
    }
}

pub enum Backend {
    Local(Box<LocalSim>),
    Online,
}

#[derive(Clone, Debug)]
pub enum FxKind {
    Text { s: String, color: u32, big: bool },
    Burst { radius: f64, color: u32 },
    Cone { angle: f64, half: f64, range: f64, color: u32 },
    Beam { to: Vec2, width: f64, color: u32 },
    Spark { vx: f64, vy: f64, color: u32 },
    Ring { radius: f64, color: u32 },
    Trail { to: Vec2, color: u32 },
}

#[derive(Clone, Debug)]
pub struct Fx {
    pub pos: Vec2,
    pub age: f64,
    pub life: f64,
    pub kind: FxKind,
}

pub struct FeedEntry {
    pub parts: Vec<(String, u32)>,
    pub at: f64,
}

pub struct GameView {
    pub backend: Backend,
    pub players: Vec<PlayerInfo>,
    pub me: Option<u8>,
    pub prev: Option<Snapshot>,
    pub cur: Option<Snapshot>,
    pub cur_at: f64,
    pub prev_pos: HashMap<EntityId, Vec2>,
    pub cam: Vec2,
    pub cam_lock: bool,
    pub fx: Vec<Fx>,
    pub feed: VecDeque<FeedEntry>,
    pub banner: Option<(String, u32, f64)>,
    pub fog_canvas: HtmlCanvasElement,
    fog_ctx: CanvasRenderingContext2d,
    pub has_fog: bool,
    pub shop: bool,
    pub menu: bool,
    pub scoreboard: bool,
    pub amove: bool,
    pub held_skill: Option<u8>,
    pub marker: Option<(Vec2, f64, bool)>,
    hold_right: f64,
    pub out: Vec<Command>,
    pub ping: Option<f64>,
    pub last_ping: f64,
    pub now: f64,
    pub hover: Option<EntityId>,
    pub mouse_world: Vec2,
    pub leave: bool,
    rng: u32,
    seen_tick: u32,
}

impl GameView {
    pub fn new(backend: Backend, players: Vec<PlayerInfo>, me: Option<u8>) -> GameView {
        let (fog_canvas, fog_ctx) = make_canvas(VISION_GRID as u32, VISION_GRID as u32);
        let start = me
            .and_then(|m| players.get(m as usize))
            .map(|p| moba_core::map::GameMap::get().spawn_point(p.team, 0))
            .unwrap_or(Vec2::new(MAP_SIZE / 2.0, MAP_SIZE / 2.0));
        GameView {
            backend,
            players,
            me,
            prev: None,
            cur: None,
            cur_at: 0.0,
            prev_pos: HashMap::new(),
            cam: start,
            cam_lock: true,
            fx: Vec::new(),
            feed: VecDeque::new(),
            banner: None,
            fog_canvas,
            fog_ctx,
            has_fog: false,
            shop: false,
            menu: false,
            scoreboard: false,
            amove: false,
            held_skill: None,
            marker: None,
            hold_right: 0.0,
            out: Vec::new(),
            ping: None,
            last_ping: 0.0,
            now: 0.0,
            hover: None,
            mouse_world: Vec2::ZERO,
            leave: false,
            rng: 0x1234_5678,
            seen_tick: 0,
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self.backend, Backend::Local(_))
    }

    pub fn my_team(&self) -> Team {
        self.me.and_then(|m| self.players.get(m as usize)).map(|p| p.team).unwrap_or(Team::Neutral)
    }

    pub fn me_view(&self) -> Option<&SelfView> {
        self.cur.as_ref().and_then(|s| s.me.as_ref())
    }

    pub fn my_entity(&self) -> Option<&EntityView> {
        let id = self.me_view()?.entity;
        self.entity(id)
    }

    pub fn entity(&self, id: EntityId) -> Option<&EntityView> {
        self.cur.as_ref()?.entities.iter().find(|e| e.id == id)
    }

    pub fn player_name(&self, idx: u8) -> String {
        self.players.get(idx as usize).map(|p| p.name.clone()).unwrap_or_else(|| "?".into())
    }

    fn player_team(&self, idx: u8) -> Team {
        self.players.get(idx as usize).map(|p| p.team).unwrap_or(Team::Neutral)
    }

    fn team_color(&self, team: Team) -> u32 {
        if team == self.my_team() {
            crate::ui::BLUE_TEAM
        } else if team == Team::Neutral {
            0xd0c080
        } else {
            crate::ui::RED_TEAM
        }
    }

    fn rand(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng % 10_000) as f64 / 10_000.0
    }

    /// Interpolated render position.
    pub fn pos_of(&self, e: &EntityView) -> Vec2 {
        let a = ((self.now - self.cur_at) / TICK_MS).clamp(0.0, 1.0) as f32;
        match self.prev_pos.get(&e.id) {
            Some(p) if p.distance(e.pos) < 60.0 => p.lerp(e.pos, a),
            _ => e.pos,
        }
    }

    // -------------------------------------------------------------- snapshots

    pub fn push_snapshot(&mut self, snap: Snapshot, now: f64) {
        if snap.tick <= self.seen_tick && self.cur.is_some() {
            return;
        }
        self.seen_tick = snap.tick;
        for ev in snap.events.clone() {
            self.on_event(ev, &snap);
        }
        if let Some(bits) = &snap.fog {
            self.update_fog(bits);
        }
        // Positions as currently rendered become the interpolation origin.
        let mut prev_pos = HashMap::new();
        if let Some(cur) = &self.cur {
            for e in &cur.entities {
                prev_pos.insert(e.id, self.pos_of(e));
            }
        }
        self.prev_pos = prev_pos;
        let mut snap = snap;
        if snap.players.is_empty() {
            // Scoreboard rows only arrive every few ticks; keep the last ones.
            if let Some(cur) = &self.cur {
                snap.players = cur.players.clone();
            }
        }
        self.prev = self.cur.take();
        self.cur = Some(snap);
        self.cur_at = now;
    }

    fn update_fog(&mut self, bits: &[u8]) {
        let n = VISION_GRID * VISION_GRID;
        let mut rgba = vec![0u8; n * 4];
        for i in 0..n {
            let visible = bits.get(i / 8).map(|b| b & (1 << (i % 8)) != 0).unwrap_or(false);
            let o = i * 4;
            rgba[o] = 6;
            rgba[o + 1] = 8;
            rgba[o + 2] = 22;
            rgba[o + 3] = if visible { 0 } else { 150 };
        }
        if let Ok(img) = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(&rgba[..]),
            VISION_GRID as u32,
            VISION_GRID as u32,
        ) {
            let _ = self.fog_ctx.put_image_data(&img, 0.0, 0.0);
            self.has_fog = true;
        }
    }

    fn add_fx(&mut self, pos: Vec2, life: f64, kind: FxKind) {
        if self.fx.len() < 600 {
            self.fx.push(Fx { pos, age: 0.0, life, kind });
        }
    }

    fn sparks(&mut self, pos: Vec2, n: usize, color: u32, speed: f64) {
        for _ in 0..n {
            let a = self.rand() * std::f64::consts::TAU;
            let s = speed * (0.4 + self.rand() * 0.8);
            let life = 0.3 + self.rand() * 0.4;
            self.add_fx(pos, life, FxKind::Spark { vx: a.cos() * s, vy: a.sin() * s - speed * 0.3, color });
        }
    }

    fn text_fx(&mut self, pos: Vec2, s: String, color: u32, big: bool) {
        let jitter = Vec2::new((self.rand() as f32 - 0.5) * 10.0, 0.0);
        self.add_fx(pos + jitter, if big { 1.2 } else { 0.9 }, FxKind::Text { s, color, big });
    }

    fn feed(&mut self, parts: Vec<(String, u32)>) {
        self.feed.push_back(FeedEntry { parts, at: self.now });
        while self.feed.len() > 6 {
            self.feed.pop_front();
        }
    }

    pub fn set_banner(&mut self, s: impl Into<String>, color: u32) {
        self.banner = Some((s.into(), color, self.now));
    }

    fn on_event(&mut self, ev: FxEvent, snap: &Snapshot) {
        let my_id = snap.me.as_ref().map(|m| m.entity);
        let hero_ids: Vec<EntityId> = self.players.iter().map(|p| p.entity).collect();
        match ev {
            FxEvent::Damage { source, target, pos, amount, kind, crit } => {
                let involves_me = Some(source) == my_id || Some(target) == my_id;
                let hero_target = hero_ids.contains(&target);
                if involves_me || (hero_target && amount >= 5.0) {
                    let color = match kind {
                        _ if Some(target) == my_id => 0xff5050,
                        DamageKind::Physical => 0xffb070,
                        DamageKind::Magic => 0x90a0ff,
                        DamageKind::True => 0xffffff,
                    };
                    let s = if crit { format!("{:.0}!", amount) } else { format!("{:.0}", amount) };
                    self.text_fx(pos - Vec2::new(0.0, 22.0), s, color, crit);
                }
            }
            FxEvent::Heal { target, pos, amount } => {
                if Some(target) == my_id || hero_ids.contains(&target) {
                    self.text_fx(pos - Vec2::new(0.0, 22.0), format!("+{:.0}", amount), 0x70f070, false);
                }
                self.sparks(pos, 4, 0x80ff90, 30.0);
            }
            FxEvent::Cast { skill: sid, from, to, .. } => {
                let c = skill(sid).color;
                self.sparks(from, 5, c, 40.0);
                if matches!(skill(sid).targeting, Targeting::Point { .. }) {
                    self.add_fx(to, 0.3, FxKind::Ring { radius: 10.0, color: c });
                }
            }
            FxEvent::Burst { pos, radius, color } => {
                self.add_fx(pos, 0.45, FxKind::Burst { radius: radius as f64, color });
                let n = (radius / 8.0).clamp(4.0, 24.0) as usize;
                self.sparks(pos, n, color, radius as f64 * 0.9);
            }
            FxEvent::Cone { pos, angle, half_angle, range, color } => {
                self.add_fx(
                    pos,
                    0.35,
                    FxKind::Cone { angle: angle as f64, half: half_angle as f64, range: range as f64, color },
                );
            }
            FxEvent::Beam { from, to, width, color } => {
                self.add_fx(from, 0.4, FxKind::Beam { to, width: width as f64, color });
            }
            FxEvent::Blink { from, to, color } => {
                self.sparks(from, 10, color, 50.0);
                self.sparks(to, 10, color, 50.0);
                self.add_fx(from, 0.35, FxKind::Trail { to, color });
            }
            FxEvent::Death { id, pos } => {
                let hero_death = hero_ids.contains(&id);
                self.sparks(pos, if hero_death { 22 } else { 6 }, 0x9890a0, 45.0);
            }
            FxEvent::Gold { pos, amount } => {
                self.text_fx(pos - Vec2::new(0.0, 30.0), format!("+{:.0}g", amount), 0xf0c838, false);
            }
            FxEvent::LevelUp { id, level } => {
                if let Some(e) = snap.entities.iter().find(|e| e.id == id) {
                    self.add_fx(e.pos, 0.8, FxKind::Ring { radius: 22.0, color: 0xf0d860 });
                    if Some(id) == my_id {
                        self.text_fx(e.pos - Vec2::new(0.0, 34.0), format!("LEVEL {level}!"), 0xf0d860, true);
                    }
                }
            }
            FxEvent::Kill { killer, victim, assists } => {
                let victim_name = self.player_name(victim);
                let victim_team = self.player_team(victim);
                let (killer_name, killer_team) = match killer {
                    Killer::Player(p) => (self.player_name(p), self.player_team(p)),
                    Killer::Tower(t) => ("Turret".to_string(), t),
                    Killer::Minion(t) => ("Minions".to_string(), t),
                    Killer::Monster => ("Monster".to_string(), Team::Neutral),
                    Killer::Fountain => ("Fountain".to_string(), victim_team.enemy()),
                };
                let mut parts = vec![
                    (killer_name, self.team_color(killer_team)),
                    (" > ".to_string(), crate::ui::DIM),
                    (victim_name, self.team_color(victim_team)),
                ];
                if !assists.is_empty() {
                    parts.push((format!(" +{}", assists.len()), crate::ui::DIM));
                }
                self.feed(parts);
                let me = self.me;
                if Some(victim) == me {
                    self.set_banner("You have been slain", 0xff6060);
                } else if matches!(killer, Killer::Player(p) if Some(p) == me) {
                    self.set_banner("You have slain an enemy!", 0xf0d860);
                } else if me.is_some() && victim_team == self.my_team() {
                    self.set_banner("An ally has been slain", 0xff9080);
                } else if me.is_some() {
                    self.set_banner("An enemy has been slain", 0x90c0ff);
                }
            }
            FxEvent::Announce { text, team } => {
                let color = match team {
                    Some(t) if t == self.my_team() => 0x90c8ff,
                    Some(Team::Neutral) | None => 0xf0e0b0,
                    Some(_) => 0xff9080,
                };
                self.feed(vec![(text.clone(), color)]);
                self.set_banner(text, color);
            }
            FxEvent::Miss { pos, .. } => self.text_fx(pos - Vec2::new(0.0, 22.0), "MISS".into(), 0xc0c0c0, false),
        }
    }

    // -------------------------------------------------------------- per frame

    /// Advance local simulation, effects and camera.
    pub fn update(&mut self, now: f64, dt_ms: f64, g: &Gfx) {
        self.now = now;
        let snaps = match &mut self.backend {
            Backend::Local(sim) => {
                for c in self.out.drain(..) {
                    sim.push(c);
                }
                let paused = self.menu;
                sim.paused = paused;
                sim.advance(dt_ms)
            }
            Backend::Online => Vec::new(),
        };
        for s in snaps {
            self.push_snapshot(s, now);
        }
        let dt = dt_ms / 1000.0;
        for f in &mut self.fx {
            f.age += dt;
            if let FxKind::Spark { vx, vy, .. } = &mut f.kind {
                f.pos.x += (*vx * dt) as f32;
                f.pos.y += (*vy * dt) as f32;
                *vy += 160.0 * dt;
            }
        }
        self.fx.retain(|f| f.age < f.life);
        if let Some((_, _, at)) = self.banner {
            if now - at > 3000.0 {
                self.banner = None;
            }
        }
        while self.feed.front().map(|f| now - f.at > 12_000.0).unwrap_or(false) {
            self.feed.pop_front();
        }
        let _ = g;
    }

    /// Screen-space top-left of the camera view.
    pub fn view_origin(&self, g: &Gfx) -> (f64, f64) {
        ((self.cam.x as f64 - g.w / 2.0).floor(), (self.cam.y as f64 - g.h / 2.0).floor())
    }

    pub fn to_screen(&self, g: &Gfx, p: Vec2) -> (f64, f64) {
        let (ox, oy) = self.view_origin(g);
        (p.x as f64 - ox, p.y as f64 - oy)
    }

    pub fn update_camera(&mut self, g: &Gfx, inp: &Input, dt: f64) {
        let my_pos = self.my_entity().filter(|e| e.alive()).map(|e| self.pos_of(e));
        let center = inp.is_held("Space") || self.cam_lock;
        if let (true, Some(p)) = (center, my_pos) {
            self.cam = p;
        } else {
            let speed = 700.0 * dt;
            let mut d = Vec2::ZERO;
            let edge = 3.0;
            if inp.mouse_in && !self.shop && !self.menu {
                if inp.mx <= edge {
                    d.x -= 1.0;
                }
                if inp.mx >= g.w - edge - 1.0 {
                    d.x += 1.0;
                }
                if inp.my <= edge {
                    d.y -= 1.0;
                }
                if inp.my >= g.h - edge - 1.0 {
                    d.y += 1.0;
                }
            }
            for (k, v) in [
                ("ArrowLeft", (-1.0, 0.0)),
                ("ArrowRight", (1.0, 0.0)),
                ("ArrowUp", (0.0, -1.0)),
                ("ArrowDown", (0.0, 1.0)),
            ] {
                if inp.is_held(k) {
                    d.x += v.0;
                    d.y += v.1;
                }
            }
            self.cam += d * speed as f32;
        }
        let hw = (g.w / 2.0) as f32;
        let hh = (g.h / 2.0) as f32;
        self.cam.x = self.cam.x.clamp(hw.min(MAP_SIZE / 2.0), (MAP_SIZE - hw).max(MAP_SIZE / 2.0));
        self.cam.y = self.cam.y.clamp(hh.min(MAP_SIZE / 2.0), (MAP_SIZE - hh + 70.0).max(MAP_SIZE / 2.0));
    }

    /// Visual hit box: (half width, height above the feet).
    pub fn hit_box(e: &EntityView) -> (f32, f32) {
        match e.unit {
            UnitView::Hero { .. } => (8.0, 22.0),
            UnitView::Minion(k) => match k {
                moba_core::units::MinionKind::Super => (9.0, 18.0),
                moba_core::units::MinionKind::Siege => (9.0, 14.0),
                _ => (6.0, 14.0),
            },
            UnitView::Structure(s) => match s {
                moba_core::units::StructureKind::Tower { .. } => (12.0, 42.0),
                moba_core::units::StructureKind::Inhibitor { .. } => (12.0, 24.0),
                moba_core::units::StructureKind::Nexus => (24.0, 48.0),
            },
            UnitView::Monster(m) => {
                let r = moba_core::units::MonsterKind::stats(m).radius;
                (r, r * 2.0)
            }
        }
    }

    fn find_hover(&self) -> Option<EntityId> {
        let cur = self.cur.as_ref()?;
        let m = self.mouse_world;
        let my_team = self.my_team();
        let mut best: Option<(f32, bool, EntityId)> = None;
        for e in &cur.entities {
            if !e.alive() {
                continue;
            }
            let p = self.pos_of(e);
            let (hw, hh) = Self::hit_box(e);
            if m.x < p.x - hw - 2.0 || m.x > p.x + hw + 2.0 || m.y < p.y - hh || m.y > p.y + 4.0 {
                continue;
            }
            let enemy = e.team != my_team;
            let d = m.distance(p - Vec2::new(0.0, hh / 2.0));
            let better = match best {
                None => true,
                Some((bd, be, _)) => (enemy && !be) || (enemy == be && d < bd),
            };
            if better {
                best = Some((d, enemy, e.id));
            }
        }
        best.map(|b| b.2)
    }

    /// Best unit for a targeted skill near the cursor.
    fn skill_unit(&self, targeting: &Targeting) -> Option<EntityId> {
        let cur = self.cur.as_ref()?;
        let my_team = self.my_team();
        let ok = |e: &EntityView| -> bool {
            if !e.alive() || matches!(e.unit, UnitView::Structure(_)) {
                return false;
            }
            match targeting {
                Targeting::Enemy { .. } => e.team != my_team,
                Targeting::EnemyHero { .. } => e.team != my_team && matches!(e.unit, UnitView::Hero { .. }),
                Targeting::Ally { .. } => e.team == my_team && matches!(e.unit, UnitView::Hero { .. }),
                _ => false,
            }
        };
        if let Some(h) = self.hover.and_then(|id| self.entity(id)) {
            if ok(h) {
                return Some(h.id);
            }
        }
        cur.entities
            .iter()
            .filter(|e| ok(e))
            .map(|e| (self.pos_of(e).distance(self.mouse_world), e.id))
            .filter(|(d, _)| *d < 45.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    fn cast(&mut self, slot: u8) {
        let Some(me) = self.me_view() else { return };
        let Some(h) = self.me.and_then(|m| self.players.get(m as usize)) else {
            return;
        };
        let sid = if slot < 3 { h.loadout.skills[slot as usize] } else { hero(h.loadout.hero).ultimate };
        let _ = me;
        let sk = skill(sid);
        let unit = if sk.targeting.needs_unit() { self.skill_unit(&sk.targeting) } else { None };
        self.out.push(Command::Cast { slot, point: self.mouse_world, unit });
    }

    /// Translate this frame's input into commands. Called after the HUD had
    /// a chance to consume clicks.
    pub fn handle_input(&mut self, g: &Gfx, inp: &mut Input, dt: f64) {
        let (ox, oy) = self.view_origin(g);
        self.mouse_world = Vec2::new((inp.mx + ox) as f32, (inp.my + oy) as f32);
        self.hover = if inp.mouse_in { self.find_hover() } else { None };
        if self.menu {
            return;
        }
        self.scoreboard = inp.is_held("Tab");
        if inp.plain("KeyY") {
            self.cam_lock = !self.cam_lock;
        }
        if inp.plain("KeyP") {
            self.shop = !self.shop;
        }
        if inp.key("Escape") {
            if self.shop {
                self.shop = false;
            } else if self.held_skill.is_some() || self.amove {
                self.held_skill = None;
                self.amove = false;
            } else {
                self.menu = true;
            }
        }
        let won = self.cur.as_ref().and_then(|c| c.winner).is_some();
        if won {
            return;
        }
        let my_team = self.my_team();
        let hover_enemy = self
            .hover
            .and_then(|id| self.entity(id))
            .filter(|e| e.team != my_team && !e.has(flags::INVULNERABLE))
            .map(|e| e.id);

        // Level ups (Ctrl/Alt + QWER).
        for (i, k) in ["KeyQ", "KeyW", "KeyE", "KeyR"].iter().enumerate() {
            if inp.chord(k) {
                self.out.push(Command::LevelUp(i as u8));
            }
        }
        // Skills: quick cast with indicator (press = aim, release = cast).
        for (i, k) in ["KeyQ", "KeyW", "KeyE", "KeyR"].iter().enumerate() {
            let slot = i as u8;
            if inp.plain(k) {
                let instant = self
                    .me
                    .and_then(|m| self.players.get(m as usize))
                    .map(|p| {
                        let sid =
                            if slot < 3 { p.loadout.skills[slot as usize] } else { hero(p.loadout.hero).ultimate };
                        skill(sid).targeting == Targeting::Instant
                    })
                    .unwrap_or(false);
                if instant {
                    self.cast(slot);
                } else {
                    self.held_skill = Some(slot);
                }
                self.amove = false;
            }
            if inp.was_released(k) && self.held_skill == Some(slot) {
                self.held_skill = None;
                self.cast(slot);
            }
        }
        if self.held_skill.is_some() && inp.right_pressed {
            self.held_skill = None;
            inp.click_consumed = true;
        }
        if self.held_skill.is_some() && inp.left_pressed && !inp.click_consumed {
            let slot = self.held_skill.take().unwrap();
            self.cast(slot);
            inp.click_consumed = true;
        }
        // Summoners.
        for (i, k) in ["KeyD", "KeyF"].iter().enumerate() {
            if inp.plain(k) {
                let unit = hover_enemy.or_else(|| self.skill_unit(&Targeting::EnemyHero { range: 0.0 }));
                self.out.push(Command::Summoner { slot: i as u8, point: self.mouse_world, unit });
            }
        }
        // Items 1-6.
        for i in 0..6u8 {
            if inp.plain(&format!("Digit{}", i + 1)) {
                self.out.push(Command::UseItem(i));
            }
        }
        if inp.plain("KeyB") {
            self.out.push(Command::Recall);
        }
        if inp.plain("KeyS") {
            self.out.push(Command::Stop);
            self.amove = false;
        }
        if inp.plain("KeyA") {
            self.amove = true;
        }
        // Mouse.
        if inp.left_pressed && !inp.click_consumed && self.amove {
            inp.click_consumed = true;
            self.amove = false;
            match hover_enemy {
                Some(id) => self.out.push(Command::Attack(id)),
                None => self.out.push(Command::AttackMove(self.mouse_world)),
            }
            self.marker = Some((self.mouse_world, self.now, true));
        }
        if inp.right_pressed && !inp.click_consumed {
            inp.click_consumed = true;
            self.amove = false;
            match hover_enemy {
                Some(id) => {
                    self.out.push(Command::Attack(id));
                    self.marker = Some((self.mouse_world, self.now, true));
                }
                None => {
                    self.out.push(Command::Move(self.mouse_world));
                    self.marker = Some((self.mouse_world, self.now, false));
                }
            }
            self.hold_right = 0.0;
        } else if inp.right && hover_enemy.is_none() && !inp.click_consumed {
            // Holding right mouse keeps steering, like in LoL.
            self.hold_right += dt;
            if self.hold_right > 0.15 {
                self.hold_right = 0.0;
                self.out.push(Command::Move(self.mouse_world));
            }
        }
    }
}
