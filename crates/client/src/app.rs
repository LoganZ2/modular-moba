//! Top-level client state machine: main menu -> champion select (practice
//! or online lobby) -> match.

use crate::art::{CursorKind, Frame};
use crate::font;
use crate::game::{Backend, GameView, LocalSim};
use crate::gfx::{Gfx, SpriteKey};
use crate::hud::HudAction;
use crate::input::Input;
use crate::net::{default_url, Net, NetState};
use crate::render::TerrainCache;
use crate::ui::{self, Lines};
use moba_core::heroes::{hero, HeroId, HEROES};
use moba_core::loadout::{can_equip, Loadout};
use moba_core::protocol::{ClientMsg, LobbyState, ServerMsg, PROTOCOL_VERSION, TEAM_SIZE};
use moba_core::sim::{LaneRole, PlayerSetup};
use moba_core::skills::{basic_skills, skill, SkillId};
use moba_core::summoners::SummonerId;
use moba_core::{Team, Vec2};
use std::cell::RefCell;
use std::rc::Rc;
use web_sys::HtmlCanvasElement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Practice,
    Online,
}

pub enum Screen {
    Menu,
    Select(Mode),
    Game(Box<GameView>),
}

pub struct App {
    pub gfx: Gfx,
    input: Rc<RefCell<Input>>,
    terrain: TerrainCache,
    screen: Screen,
    name: String,
    room: String,
    loadout: Loadout,
    field_focus: Option<u8>,
    focus_slot: usize,
    practice_role: usize,
    practice_team: Team,
    net: Option<Net>,
    my_id: u32,
    lobby: Option<LobbyState>,
    toast: Option<(String, u32, f64)>,
    last: f64,
    now: f64,
    server_url: Option<String>,
    seed: u64,
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn load_profile() -> (String, Loadout) {
    let st = storage();
    let name = st.as_ref().and_then(|s| s.get_item("moba.name").ok().flatten()).unwrap_or_default();
    let loadout = st
        .as_ref()
        .and_then(|s| s.get_item("moba.loadout").ok().flatten())
        .and_then(|v| {
            let n: Vec<u16> = v.split(',').filter_map(|x| x.parse().ok()).collect();
            if n.len() != 6 {
                return None;
            }
            let sum = |i: u16| SummonerId::ALL.get(i as usize).copied();
            Some(Loadout {
                hero: HeroId(n[0] as u8),
                skills: [SkillId(n[1]), SkillId(n[2]), SkillId(n[3])],
                summoners: [sum(n[4])?, sum(n[5])?],
            })
        })
        .map(|l| l.sanitized())
        .unwrap_or_else(|| Loadout::suggested(HeroId(0)));
    let name = if name.trim().is_empty() {
        format!("Summoner{}", (js_sys::Math::random() * 900.0) as u32 + 100)
    } else {
        name
    };
    (name, loadout)
}

fn save_profile(name: &str, l: &Loadout) {
    if let Some(s) = storage() {
        let _ = s.set_item("moba.name", name);
        let idx = |x: SummonerId| SummonerId::ALL.iter().position(|s| *s == x).unwrap_or(0);
        let v = format!(
            "{},{},{},{},{},{}",
            l.hero.0,
            l.skills[0].0,
            l.skills[1].0,
            l.skills[2].0,
            idx(l.summoners[0]),
            idx(l.summoners[1])
        );
        let _ = s.set_item("moba.loadout", &v);
    }
}

impl App {
    pub fn new(canvas: HtmlCanvasElement, input: Rc<RefCell<Input>>) -> App {
        let (name, loadout) = load_profile();
        let room = web_sys::window()
            .and_then(|w| w.location().hash().ok())
            .map(|h| h.trim_start_matches('#').to_string())
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "lobby".to_string());
        App {
            gfx: Gfx::new(canvas),
            input,
            terrain: TerrainCache::new(),
            screen: Screen::Menu,
            name,
            room,
            loadout,
            field_focus: None,
            focus_slot: 0,
            practice_role: 2,
            practice_team: Team::Blue,
            net: None,
            my_id: 0,
            lobby: None,
            toast: None,
            last: 0.0,
            now: 0.0,
            server_url: default_url(),
            seed: (js_sys::Math::random() * 1e15) as u64,
        }
    }

    fn toast(&mut self, s: impl Into<String>, color: u32) {
        self.toast = Some((s.into(), color, self.now));
    }

    pub fn frame(&mut self, now: f64) {
        let dt_ms = if self.last == 0.0 { 16.0 } else { (now - self.last).clamp(0.0, 100.0) };
        self.last = now;
        self.now = now;
        self.gfx.resize();
        let input = self.input.clone();
        let mut inp = input.borrow_mut();
        self.poll_net(now);
        let mut cursor = CursorKind::Normal;
        let select_mode = match &self.screen {
            Screen::Select(m) => Some(*m),
            _ => None,
        };
        let in_game = matches!(self.screen, Screen::Game(_));
        if let Some(mode) = select_mode {
            self.select_screen(&mut inp, mode);
        } else if in_game {
            cursor = self.game_screen(&mut inp, now, dt_ms);
        } else {
            self.menu_screen(&mut inp);
        }
        if !matches!(self.screen, Screen::Game(_)) {
            self.terrain.prefetch_one();
        }
        let g = &self.gfx;
        if let Some((s, c, at)) = &self.toast {
            if now - at < 4500.0 {
                let w = font::text_width(s) + 12.0;
                g.panel((g.w - w) / 2.0, g.h - 82.0, w, 14.0);
                g.text_s(s, (g.w - w) / 2.0 + 6.0, g.h - 79.0, *c);
            }
        }
        if inp.mouse_in {
            let k = if cursor == CursorKind::Target { 5.0 } else { 0.0 };
            g.draw_sprite(SpriteKey::Cursor(cursor), inp.mx - k, inp.my - k);
        }
        inp.end_frame();
    }

    // -------------------------------------------------------------- network

    fn connect(&mut self) {
        let Some(url) = self.server_url.clone() else {
            self.toast("Online play needs the page to be served by moba-server", ui::BAD);
            return;
        };
        let hello = ClientMsg::Hello { name: self.name.clone(), room: self.room.clone(), version: PROTOCOL_VERSION };
        match Net::connect(&url, hello) {
            Ok(n) => {
                self.net = Some(n);
                self.lobby = None;
                self.screen = Screen::Select(Mode::Online);
            }
            Err(_) => self.toast("Could not open a connection", ui::BAD),
        }
    }

    fn send(&self, msg: ClientMsg) {
        if let Some(n) = &self.net {
            n.send(&msg);
        }
    }

    fn disconnect(&mut self) {
        if let Some(n) = self.net.take() {
            n.close();
        }
        self.lobby = None;
    }

    fn poll_net(&mut self, now: f64) {
        let Some(net) = &self.net else { return };
        let msgs = net.poll();
        let closed = net.state() == NetState::Closed;
        for m in msgs {
            match m {
                ServerMsg::Welcome { player_id } => {
                    self.my_id = player_id;
                    self.send(ClientMsg::SetLoadout(self.loadout));
                }
                ServerMsg::Lobby(state) => {
                    let back_to_lobby = !state.in_game && matches!(&self.screen, Screen::Game(g) if !g.is_local());
                    self.lobby = Some(state);
                    if back_to_lobby {
                        self.screen = Screen::Select(Mode::Online);
                    }
                }
                ServerMsg::GameStart(gs) => {
                    let gv = GameView::new(Backend::Online, gs.players, gs.you);
                    self.screen = Screen::Game(Box::new(gv));
                }
                ServerMsg::Snapshot(s) => {
                    if let Screen::Game(g) = &mut self.screen {
                        g.push_snapshot(*s, now);
                    }
                }
                ServerMsg::Notice(text) => self.toast(text, 0xffd080),
                ServerMsg::Pong(sent) => {
                    if let Screen::Game(g) = &mut self.screen {
                        g.ping = Some((now as u32).wrapping_sub(sent) as f64);
                    }
                }
            }
        }
        if closed {
            self.net = None;
            self.lobby = None;
            if !matches!(&self.screen, Screen::Game(g) if g.is_local()) {
                self.screen = Screen::Menu;
            }
            self.toast("Disconnected from server", ui::BAD);
        }
    }

    // -------------------------------------------------------------- menu

    fn background(&mut self, dim: f64) {
        let t = self.now / 1000.0;
        let cx = 1600.0 + (t * 0.05).cos() * 1100.0;
        let cy = 1600.0 + (t * 0.037).sin() * 1100.0;
        let g = &self.gfx;
        let (ox, oy) = (cx - g.w / 2.0, cy - g.h / 2.0);
        self.terrain.draw(&self.gfx, ox, oy);
        let g = &self.gfx;
        g.rect_a(0.0, 0.0, g.w, g.h, 0x0a0812, dim);
    }

    fn menu_screen(&mut self, inp: &mut Input) {
        self.background(0.55);
        let g = &self.gfx;
        let cx = g.w / 2.0;
        g.text_center("MODULAR MOBA", cx, 26.0, 0xf0d060, 4.0, true);
        g.text_center("Pixel MOBA in Rust + WebAssembly", cx, 64.0, ui::TEXT, 1.0, true);
        g.text_center(
            "Every champion has 2 roles - every skill has 2 roles - build your own kit",
            cx,
            74.0,
            ui::DIM,
            1.0,
            true,
        );

        let pw = 230.0;
        let ph = 112.0;
        let px = (cx - pw / 2.0).floor();
        let py = 92.0;
        g.panel(px, py, pw, ph);
        g.text_s("NAME", px + 10.0, py + 12.0, ui::DIM);
        g.text_s("ROOM", px + 10.0, py + 30.0, ui::DIM);
        let t = self.now / 1000.0;
        let mut name = self.name.clone();
        let mut room = self.room.clone();
        let f0 = ui::text_field(g, inp, px + 44.0, py + 9.0, 176.0, &mut name, self.field_focus == Some(0), 16, t);
        let f1 = ui::text_field(g, inp, px + 44.0, py + 27.0, 176.0, &mut room, self.field_focus == Some(1), 24, t);
        self.field_focus = if f0 {
            Some(0)
        } else if f1 {
            Some(1)
        } else {
            None
        };
        if name != self.name {
            self.name = name;
            save_profile(&self.name, &self.loadout);
        }
        self.room = room;
        let g = &self.gfx;
        let practice = ui::button(g, inp, px + 10.0, py + 50.0, pw - 20.0, 18.0, "PRACTICE VS BOTS (offline)", true);
        let online_ok = self.server_url.is_some();
        let online = ui::button(g, inp, px + 10.0, py + 72.0, pw - 20.0, 18.0, "PLAY ONLINE", online_ok);
        if !online_ok {
            g.text_center("(start moba-server to play online)", cx, py + 95.0, ui::DIM, 1.0, false);
        } else {
            g.text_center(
                &format!("room '{}' - share the URL #{} with friends", self.room, self.room),
                cx,
                py + 95.0,
                ui::DIM,
                1.0,
                false,
            );
        }

        let help = [
            "Right-click: move / attack     A + click: attack-move     S: stop",
            "Q W E R: skills (hold to aim, release to cast)     Alt/Ctrl + QWER: level up",
            "D F: summoner spells     B: recall     P: shop     1-6: items",
            "Tab: scoreboard     Y: lock camera     Space: center camera     Esc: menu",
        ];
        let hy = g.h - 52.0;
        g.rect_a(0.0, hy - 6.0, g.w, 52.0, 0x000000, 0.45);
        for (i, l) in help.iter().enumerate() {
            g.text_center(l, cx, hy + i as f64 * 11.0, 0xd0c8e0, 1.0, true);
        }
        if practice {
            self.screen = Screen::Select(Mode::Practice);
        }
        if online {
            if self.room.trim().is_empty() {
                self.room = "lobby".into();
            }
            self.connect();
        }
    }

    // -------------------------------------------------------------- champion select

    fn set_loadout(&mut self, l: Loadout) {
        self.loadout = l;
        save_profile(&self.name, &self.loadout);
        if self.net.is_some() && self.loadout.validate().is_ok() {
            self.send(ClientMsg::SetLoadout(self.loadout));
        }
    }

    fn select_screen(&mut self, inp: &mut Input, mode: Mode) {
        self.background(0.8);
        let mut tip: Option<(Lines, f64)> = None;
        let w = self.gfx.w;
        let h = self.gfx.h;
        {
            let g = &self.gfx;
            g.rect(0.0, 0.0, w, 13.0, 0x1c1828);
            g.text_s("CHAMPION SELECT", 4.0, 3.0, ui::GOLD);
            let sub = match mode {
                Mode::Practice => "Practice vs bots (runs entirely in your browser)".to_string(),
                Mode::Online => format!("Online room '{}'", self.room),
            };
            g.text_s(&sub, 110.0, 3.0, ui::DIM);
        }

        // ------------------------------------------------ heroes
        let hx = 4.0;
        let mut hy = 17.0;
        let row = ((h - 110.0) / HEROES.len() as f64).clamp(20.0, 25.0).floor();
        for hd in HEROES {
            let g = &self.gfx;
            let selected = hd.id == self.loadout.hero;
            let hover = inp.in_rect(hx, hy, 122.0, row - 1.0);
            g.rect(
                hx,
                hy,
                122.0,
                row - 1.0,
                if selected {
                    0x3c3466
                } else if hover {
                    0x2a2440
                } else {
                    0x16141e
                },
            );
            if selected {
                g.frame(hx, hy, 122.0, row - 1.0, ui::GOLD);
            }
            g.draw_sprite(SpriteKey::Hero(hd.id, Frame::Idle, false), hx, hy + row - 27.0);
            g.text_s(hd.name, hx + 26.0, hy + 2.0, if selected { ui::GOLD } else { ui::TEXT });
            ui::role_tags(g, hx + 26.0, hy + 11.0, &hd.roles);
            if inp.clicked(hx, hy, 122.0, row - 1.0) && !selected {
                let mut l = Loadout::suggested(hd.id);
                l.summoners = self.loadout.summoners;
                self.focus_slot = 0;
                self.set_loadout(l);
            }
            hy += row;
        }

        // ------------------------------------------------ hero details
        let hd = hero(self.loadout.hero);
        let bx = 130.0;
        {
            let g = &self.gfx;
            g.panel(bx, 17.0, 168.0, h - 108.0);
            g.rect(bx + 4.0, 21.0, 52.0, 56.0, 0x24202e);
            g.draw_sprite_k(SpriteKey::Hero(hd.id, Frame::Idle, false), bx + 6.0, 23.0, 2.0);
            g.text_sk(hd.name, bx + 60.0, 22.0, ui::GOLD, 2.0);
            g.text_s(hd.title, bx + 60.0, 40.0, ui::DIM);
            ui::role_tags(g, bx + 60.0, 50.0, &hd.roles);
            let b = &hd.base;
            let lines = [
                format!("HP {:.0}  MP {:.0}", b.hp, b.mana),
                format!("AD {:.0}  AS {:.2}", b.ad, b.attack_speed),
                format!("AR {:.0}  MR {:.0}", b.armor, b.mr),
                format!("{} {:.0}  MS {:.0}", if b.ranged { "Ranged" } else { "Melee" }, b.range, b.move_speed),
            ];
            for (i, l) in lines.iter().enumerate() {
                g.text(l, bx + 60.0, 62.0 + i as f64 * 9.0, ui::TEXT);
            }
            let ult = skill(hd.ultimate);
            let uy = 104.0;
            g.text_s("R - BUILT-IN ULTIMATE", bx + 4.0, uy, ui::GOLD);
            g.draw_sprite(SpriteKey::Skill(ult.id), bx + 4.0, uy + 10.0);
            g.text_s(ult.name, bx + 24.0, uy + 14.0, ui::TEXT);
            let mut y = uy + 30.0;
            for l in font::wrap(ult.desc, 158.0) {
                g.text(&l, bx + 5.0, y, 0xd0c8e0);
                y += 9.0;
            }
            for d in ult.detail_lines(1, None) {
                for l in font::wrap(&d, 158.0) {
                    if y < h - 100.0 {
                        g.text(&l, bx + 5.0, y, 0xa8c8f0);
                    }
                    y += 9.0;
                }
            }
            if inp.in_rect(bx + 4.0, uy + 10.0, 160.0, 16.0) {
                tip = Some((ui::skill_lines(ult, 1, None, 0.0), 200.0));
            }
        }

        // ------------------------------------------------ Q/W/E slots + pool
        let sx = 302.0;
        let sw = (w - sx - 4.0).max(200.0);
        {
            let g = &self.gfx;
            g.text_s("YOUR SKILLS", sx, 17.0, ui::GOLD);
            g.text_s("(click a slot, then a skill)", sx + 70.0, 17.0, ui::DIM);
        }
        let keys = ["Q", "W", "E"];
        let slot_w = ((sw - 8.0) / 3.0).floor();
        for (i, key) in keys.iter().enumerate() {
            let g = &self.gfx;
            let x = sx + i as f64 * (slot_w + 4.0);
            let y = 27.0;
            let focus = self.focus_slot == i;
            g.rect(x, y, slot_w, 20.0, if focus { 0x3c3466 } else { 0x1c1a28 });
            g.frame(x, y, slot_w, 20.0, if focus { ui::GOLD } else { 0x4a4460 });
            g.text_s(key, x + 3.0, y + 7.0, ui::GOLD);
            let sk = skill(self.loadout.skills[i]);
            g.draw_sprite(SpriteKey::Skill(sk.id), x + 11.0, y + 2.0);
            let name: String = sk.name.chars().take(((slot_w - 32.0) / 6.0) as usize).collect();
            g.text(&name, x + 30.0, y + 7.0, ui::TEXT);
            if inp.clicked(x, y, slot_w, 20.0) {
                self.focus_slot = i;
            }
            if inp.in_rect(x, y, slot_w, 20.0) {
                tip = Some((ui::skill_lines(sk, 1, None, 0.0), 200.0));
            }
        }
        {
            let g = &self.gfx;
            let r = hd.roles;
            g.text_s("SKILL POOL - must share a role with", sx, 52.0, ui::DIM);
            ui::role_tags(g, sx + 214.0, 51.0, &r);
        }
        let pool: Vec<_> = basic_skills().collect();
        let col_w = ((sw - 4.0) / 2.0).floor();
        let rows = pool.len().div_ceil(2);
        let cell_h = ((h - 170.0) / rows as f64).clamp(14.0, 18.0).floor();
        for (n, sk) in pool.iter().enumerate() {
            let g = &self.gfx;
            let x = sx + (n / rows) as f64 * (col_w + 4.0);
            let y = 63.0 + (n % rows) as f64 * cell_h;
            let ok = can_equip(hd, sk);
            let slot = self.loadout.skills.iter().position(|s| *s == sk.id);
            let hover = inp.in_rect(x, y, col_w, cell_h - 1.0);
            let bg = if slot.is_some() {
                0x3a3460
            } else if hover && ok {
                0x2a2440
            } else {
                0x14121c
            };
            g.rect(x, y, col_w, cell_h - 1.0, bg);
            if cell_h >= 17.0 {
                g.draw_sprite(SpriteKey::Skill(sk.id), x, y);
            } else {
                g.draw_scaled(&g.sprite(SpriteKey::Skill(sk.id)).canvas, x, y, cell_h - 1.0, cell_h - 1.0);
            }
            let name_color = if !ok {
                0x5a5468
            } else if slot.is_some() {
                ui::GOLD
            } else {
                ui::TEXT
            };
            g.text(sk.name, x + cell_h + 2.0, y + 1.0, name_color);
            let tag_y = y + cell_h - 10.0;
            if cell_h >= 17.0 {
                ui::role_tags(g, x + cell_h + 2.0, tag_y, &sk.roles);
            } else {
                let t = format!("{}/{}", sk.roles[0].short(), sk.roles[1].short());
                g.text(&t, x + col_w - font::text_width(&t) - 2.0, y + 1.0, if ok { ui::DIM } else { 0x403a50 });
            }
            if !ok {
                g.rect_a(x, y, cell_h - 1.0, cell_h - 1.0, 0x000000, 0.6);
            }
            if let Some(s) = slot {
                g.rect(x + col_w - 9.0, y + cell_h - 10.0, 9.0, 9.0, ui::GOLD);
                g.text(keys[s], x + col_w - 7.0, y + cell_h - 9.0, 0x201000);
            }
            if hover {
                let mut lines = ui::skill_lines(sk, 1, None, 0.0);
                if !ok {
                    lines.insert(1, (format!("{} can't use this: no shared role", hd.name), ui::BAD));
                }
                tip = Some((lines, 200.0));
            }
            if inp.clicked(x, y, col_w, cell_h - 1.0) && ok {
                let mut l = self.loadout;
                match slot {
                    Some(s) => self.focus_slot = s,
                    None => {
                        l.skills[self.focus_slot] = sk.id;
                        self.focus_slot = (self.focus_slot + 1) % 3;
                        self.set_loadout(l);
                    }
                }
            }
        }

        // ------------------------------------------------ summoners
        let spy = 63.0 + rows as f64 * cell_h + 3.0;
        {
            let g = &self.gfx;
            g.text_s("SPELLS", sx, spy + 3.0, ui::DIM);
            for i in 0..2 {
                let x = sx + 40.0 + i as f64 * 26.0;
                g.text_s(["D", "F"][i], x, spy + 3.0, ui::GOLD);
                g.draw_sprite(SpriteKey::Summoner(self.loadout.summoners[i]), x + 8.0, spy);
            }
            g.text_s("|", sx + 96.0, spy + 3.0, ui::DIM);
        }
        for (i, s) in SummonerId::ALL.iter().enumerate() {
            let g = &self.gfx;
            let x = sx + 104.0 + i as f64 * 16.0;
            let y = spy;
            let equipped = self.loadout.summoners.contains(s);
            g.draw_sprite(SpriteKey::Summoner(*s), x, y);
            if equipped {
                g.frame(x - 1.0, y - 1.0, 16.0, 16.0, ui::GOLD);
            }
            if inp.in_rect(x, y, 14.0, 14.0) {
                let mut lines = ui::summoner_lines(*s);
                lines.push(("Left-click: put on D   Right-click: put on F".into(), ui::DIM));
                tip = Some((lines, 170.0));
            }
            let mut l = self.loadout;
            if inp.clicked(x, y, 14.0, 14.0) {
                if l.summoners[1] == *s {
                    l.summoners[1] = l.summoners[0];
                }
                l.summoners[0] = *s;
                self.set_loadout(l);
            } else if inp.right_clicked(x, y, 14.0, 14.0) {
                if l.summoners[0] == *s {
                    l.summoners[0] = l.summoners[1];
                }
                l.summoners[1] = *s;
                self.set_loadout(l);
            }
        }

        // ------------------------------------------------ bottom bar
        let by = h - 84.0;
        {
            let g = &self.gfx;
            g.panel(2.0, by, w - 4.0, 82.0);
            match self.loadout.validate() {
                Ok(()) => {
                    g.text_s("Loadout valid - every skill shares a role with your champion.", 8.0, by + 4.0, ui::GOOD)
                }
                Err(e) => g.text_s(&e.to_string(), 8.0, by + 4.0, ui::BAD),
            };
        }
        match mode {
            Mode::Practice => self.practice_bar(inp, by),
            Mode::Online => self.lobby_bar(inp, by),
        }
        if let Some((lines, tw)) = tip {
            ui::tooltip(&self.gfx, inp.mx, inp.my, &lines, tw);
        }
        if inp.key("Escape") {
            self.leave_select(mode);
        }
    }

    fn leave_select(&mut self, mode: Mode) {
        if mode == Mode::Online {
            self.disconnect();
        }
        self.screen = Screen::Menu;
    }

    fn practice_bar(&mut self, inp: &mut Input, by: f64) {
        let g = &self.gfx;
        let w = g.w;
        g.text_s("ROLE", 8.0, by + 20.0, ui::DIM);
        let mut x = 40.0;
        for (i, r) in LaneRole::ORDER.iter().enumerate() {
            let label = r.name().to_uppercase();
            if ui::chip(g, inp, x, by + 18.0, &label, self.practice_role == i, ui::GOLD) {
                self.practice_role = i;
            }
            x += font::text_width(&label) + 12.0;
        }
        g.text_s("SIDE", 8.0, by + 36.0, ui::DIM);
        if ui::chip(g, inp, 40.0, by + 34.0, "BLUE (bottom-left)", self.practice_team == Team::Blue, ui::BLUE_TEAM) {
            self.practice_team = Team::Blue;
        }
        if ui::chip(g, inp, 160.0, by + 34.0, "RED (top-right)", self.practice_team == Team::Red, ui::RED_TEAM) {
            self.practice_team = Team::Red;
        }
        g.text_s("9 bots fill the other seats. The whole match is simulated in WebAssembly.", 8.0, by + 52.0, ui::DIM);
        let back = ui::button(g, inp, w - 196.0, by + 58.0, 70.0, 18.0, "BACK", true);
        let start =
            ui::button(g, inp, w - 120.0, by + 58.0, 114.0, 18.0, "START PRACTICE", self.loadout.validate().is_ok());
        if back {
            self.screen = Screen::Menu;
        }
        if start {
            self.start_practice();
        }
    }

    fn start_practice(&mut self) {
        let mut others: Vec<HeroId> = HEROES.iter().map(|h| h.id).filter(|h| *h != self.loadout.hero).collect();
        // Shuffle bot heroes.
        for i in (1..others.len()).rev() {
            self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (self.seed >> 33) as usize % (i + 1);
            others.swap(i, j);
        }
        let mut setups = Vec::new();
        let mut me = 0;
        let mut k = 0;
        for team in Team::PLAYABLE {
            for i in 0..TEAM_SIZE {
                if team == self.practice_team && i == self.practice_role {
                    me = setups.len();
                    setups.push(PlayerSetup { name: self.name.clone(), team, loadout: self.loadout, bot: false });
                } else {
                    let h = others[k % others.len()];
                    k += 1;
                    setups.push(PlayerSetup {
                        name: format!("{} Bot", hero(h).name),
                        team,
                        loadout: Loadout::suggested(h),
                        bot: true,
                    });
                }
            }
        }
        let sim = LocalSim::new(setups, me, self.seed);
        let players = sim.players();
        let gv = GameView::new(Backend::Local(Box::new(sim)), players, Some(me as u8));
        self.screen = Screen::Game(Box::new(gv));
    }

    fn lobby_bar(&mut self, inp: &mut Input, by: f64) {
        let g = &self.gfx;
        let w = g.w;
        let Some(lobby) = self.lobby.clone() else {
            g.text_s("Connecting...", 8.0, by + 20.0, ui::DIM);
            if ui::button(g, inp, w - 76.0, by + 58.0, 70.0, 18.0, "LEAVE", true) {
                self.leave_select(Mode::Online);
            }
            return;
        };
        let seat_w = ((w - 140.0) / 5.0).floor();
        let mut take: Option<(Team, u8)> = None;
        for (row, team) in Team::PLAYABLE.iter().enumerate() {
            let y = by + 16.0 + row as f64 * 21.0;
            let c = if *team == Team::Blue { ui::BLUE_TEAM } else { ui::RED_TEAM };
            g.text_s(if *team == Team::Blue { "BLUE" } else { "RED" }, 6.0, y + 6.0, c);
            for seat in lobby.seats.iter().filter(|s| s.team == *team) {
                let x = 32.0 + seat.index as f64 * (seat_w + 2.0);
                let hover = inp.in_rect(x, y, seat_w, 19.0);
                let mine = seat.player.as_ref().map(|p| p.id == self.my_id).unwrap_or(false);
                g.rect(
                    x,
                    y,
                    seat_w,
                    19.0,
                    if mine {
                        0x30402c
                    } else if hover && seat.player.is_none() {
                        0x2a2440
                    } else {
                        0x16141e
                    },
                );
                g.frame(x, y, seat_w, 19.0, crate::art::shade(c, 0.6));
                let role = LaneRole::ORDER[seat.index as usize].name();
                g.text(role, x + 2.0, y + 2.0, ui::DIM);
                match &seat.player {
                    Some(p) => {
                        let n: String = p.name.chars().take(((seat_w - 4.0) / 6.0) as usize).collect();
                        g.text(&n, x + 2.0, y + 10.0, if mine { 0xc8ffc0 } else { ui::TEXT });
                        let hn = hero(p.loadout.hero).name;
                        g.text(hn, x + seat_w - font::text_width(hn) - 2.0, y + 2.0, ui::GOLD);
                    }
                    None => {
                        g.text("bot", x + 2.0, y + 10.0, 0x5a5468);
                        if inp.clicked(x, y, seat_w, 19.0) {
                            take = Some((*team, seat.index));
                        }
                    }
                }
            }
        }
        if let Some((team, index)) = take {
            self.send(ClientMsg::TakeSeat { team, index });
        }
        let host = lobby.host == self.my_id;
        let g = &self.gfx;
        if !lobby.spectators.is_empty() {
            g.text_s(&format!("{} spectating", lobby.spectators.len()), w - 104.0, by + 18.0, ui::DIM);
        }
        if lobby.in_game {
            g.text_s("Match in progress...", w - 104.0, by + 30.0, ui::GOLD);
        }
        let leave = ui::button(g, inp, w - 104.0, by + 40.0, 98.0, 16.0, "LEAVE", true);
        let start = if host {
            ui::button(
                g,
                inp,
                w - 104.0,
                by + 60.0,
                98.0,
                18.0,
                "START GAME",
                !lobby.in_game && self.loadout.validate().is_ok(),
            )
        } else {
            g.text_s("Waiting for host", w - 104.0, by + 64.0, ui::DIM);
            false
        };
        if leave {
            self.leave_select(Mode::Online);
        }
        if start {
            self.send(ClientMsg::StartGame);
        }
    }

    // -------------------------------------------------------------- game

    fn game_screen(&mut self, inp: &mut Input, now: f64, dt_ms: f64) -> CursorKind {
        let Screen::Game(gv) = &mut self.screen else {
            return CursorKind::Normal;
        };
        let g = &self.gfx;
        let dt = dt_ms / 1000.0;
        gv.update(now, dt_ms, g);
        gv.update_camera(g, inp, dt);
        gv.render_world(g, &mut self.terrain);
        let action = gv.hud(g, inp, &self.terrain);
        gv.handle_input(g, inp, dt);
        if !gv.is_local() {
            if let Some(n) = &self.net {
                for c in gv.out.drain(..) {
                    n.send(&ClientMsg::Command(c));
                }
                if now - gv.last_ping > 2000.0 {
                    gv.last_ping = now;
                    n.send(&ClientMsg::Ping(now as u32));
                }
            }
        }
        let my_team = gv.my_team();
        let cursor = if gv.amove || gv.held_skill.is_some() {
            CursorKind::Target
        } else {
            match gv.hover.and_then(|id| gv.entity(id)) {
                Some(e) if e.team != my_team => CursorKind::Attack,
                Some(_) => CursorKind::Ally,
                None => CursorKind::Normal,
            }
        };
        if let HudAction::Leave = action {
            let local = gv.is_local();
            let finished = gv.cur.as_ref().and_then(|c| c.winner).is_some();
            if local {
                self.screen = Screen::Select(Mode::Practice);
            } else if finished {
                self.send(ClientMsg::ReturnToLobby);
                self.screen = Screen::Select(Mode::Online);
            } else {
                self.disconnect();
                self.screen = Screen::Menu;
                self.toast("You left the match - a bot took over your champion", ui::DIM);
            }
        }
        let _ = Vec2::ZERO;
        cursor
    }
}
