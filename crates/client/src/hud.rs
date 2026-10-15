//! Heads-up display: skill bar, stats, items, minimap, score, kill feed,
//! shop, scoreboard and overlays.

use crate::art::Frame;
use crate::game::GameView;
use crate::gfx::{Gfx, SpriteKey};
use crate::input::Input;
use crate::render::TerrainCache;
use crate::ui::{self, Lines};
use moba_core::heroes::hero;
use moba_core::items::{item, ItemTier, ITEMS};
use moba_core::map::MAP_SIZE;
use moba_core::protocol::{flags, Command, SelfView, UnitView};
use moba_core::skills::skill;
use moba_core::units::StructureKind;
use moba_core::{Team, Vec2};

pub const PANEL_W: f64 = 236.0;
pub const PANEL_H: f64 = 50.0;
pub const MINIMAP: f64 = 96.0;

/// Mirrors the server's level-up rules for the "+" buttons.
pub fn can_level(me: &SelfView, slot: usize, max_rank: u8) -> bool {
    if me.skill_points == 0 || slot >= 4 {
        return false;
    }
    let rank = me.ranks[slot];
    if rank >= max_rank {
        return false;
    }
    if slot == 3 {
        me.level >= [6u8, 11, 16][rank as usize]
    } else {
        rank < me.level.div_ceil(2)
    }
}

fn fmt_time(t: f32) -> String {
    let s = t.max(0.0) as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}

pub enum HudAction {
    None,
    Leave,
}

impl GameView {
    pub fn hud(&mut self, g: &Gfx, inp: &mut Input, terrain: &TerrainCache) -> HudAction {
        let mut tip: Option<(Lines, f64)> = None;
        let mut action = HudAction::None;
        let Some(cur) = self.cur.clone() else {
            g.text_center("Loading match...", g.w / 2.0, g.h / 2.0, ui::TEXT, 2.0, true);
            return action;
        };
        let my_team = self.my_team();
        let me = cur.me.clone();

        // ---------------------------------------------------------- top bar
        let (bk, rk) = (cur.score[0].kills, cur.score[1].kills);
        let sx = g.w - 150.0;
        g.panel(sx, 2.0, 148.0, 22.0);
        g.text_s(&format!("{bk}"), sx + 6.0, 6.0, ui::BLUE_TEAM);
        g.text_s("vs", sx + 24.0, 6.0, ui::DIM);
        g.text_s(&format!("{rk}"), sx + 40.0, 6.0, ui::RED_TEAM);
        g.text_s(&format!("T {}-{}", cur.score[0].towers, cur.score[1].towers), sx + 58.0, 6.0, ui::DIM);
        g.text_right(&fmt_time(cur.time), sx + 144.0, 6.0, ui::TEXT);
        if let (Some(m), Some(row)) = (&me, self.me.and_then(|i| cur.players.get(i as usize))) {
            g.text_s(&format!("{}/{}/{}  CS {}", row.kills, row.deaths, row.assists, row.cs), sx + 6.0, 15.0, ui::TEXT);
            let _ = m;
        } else {
            g.text_s("Spectating", sx + 6.0, 15.0, ui::DIM);
        }
        let mut fy = 28.0;
        for f in self.feed.iter().rev() {
            let w: f64 = f.parts.iter().map(|(s, _)| crate::font::text_width(s) + 1.0).sum::<f64>() + 6.0;
            let x = g.w - w - 2.0;
            g.rect_a(x, fy, w, 10.0, 0x101018, 0.7);
            let mut cx = x + 3.0;
            for (s, c) in &f.parts {
                cx += g.text_s(s, cx, fy + 1.0, *c) + 1.0;
            }
            fy += 11.0;
        }
        // Top-left hints.
        let lock = if self.cam_lock { "locked" } else { "free" };
        g.text_s(&format!("Cam {lock} (Y)  P shop  Tab score"), 3.0, 3.0, ui::DIM);
        if let Some(p) = self.ping {
            g.text_s(&format!("ping {:.0}ms", p), 3.0, 12.0, ui::DIM);
        } else if self.is_local() {
            g.text_s("Practice (offline)", 3.0, 12.0, ui::DIM);
        }

        // ---------------------------------------------------------- minimap
        let mmx = g.w - MINIMAP - 2.0;
        let mmy = g.h - MINIMAP - 2.0;
        g.rect(mmx - 2.0, mmy - 2.0, MINIMAP + 4.0, MINIMAP + 4.0, 0x5a5070);
        g.draw_scaled(&terrain.minimap, mmx, mmy, MINIMAP, MINIMAP);
        let k = MINIMAP / MAP_SIZE as f64;
        if self.has_fog {
            g.set_alpha(0.8);
            g.draw_smooth(&self.fog_canvas, mmx, mmy, MINIMAP, MINIMAP);
            g.set_alpha(1.0);
        }
        for e in &cur.entities {
            let p = self.pos_of(e);
            let (x, y) = (mmx + p.x as f64 * k, mmy + p.y as f64 * k);
            let c = if e.team == my_team {
                ui::BLUE_TEAM
            } else if e.team == Team::Neutral {
                0xd0b040
            } else {
                ui::RED_TEAM
            };
            match e.unit {
                UnitView::Structure(s) => {
                    if !e.alive() {
                        g.rect(x - 1.0, y - 1.0, 3.0, 3.0, 0x404040);
                        continue;
                    }
                    let sz = match s {
                        StructureKind::Nexus => 5.0,
                        StructureKind::Inhibitor { .. } => 3.0,
                        _ => 4.0,
                    };
                    g.rect(x - sz / 2.0 - 1.0, y - sz / 2.0 - 1.0, sz + 2.0, sz + 2.0, 0x101010);
                    g.rect(x - sz / 2.0, y - sz / 2.0, sz, sz, c);
                }
                UnitView::Minion(_) => g.rect(x, y, 1.0, 1.0, c),
                UnitView::Monster(_) => g.rect(x - 1.0, y - 1.0, 2.0, 2.0, c),
                UnitView::Hero { .. } => {}
            }
        }
        for e in &cur.entities {
            if let UnitView::Hero { .. } = e.unit {
                if !e.alive() {
                    continue;
                }
                let p = self.pos_of(e);
                let (x, y) = (mmx + p.x as f64 * k, mmy + p.y as f64 * k);
                let mine = me.as_ref().map(|m| m.entity) == Some(e.id);
                let c = if mine {
                    0x70f070
                } else if e.team == my_team {
                    ui::BLUE_TEAM
                } else {
                    ui::RED_TEAM
                };
                g.rect(x - 3.0, y - 3.0, 6.0, 6.0, 0x101010);
                g.rect(x - 2.0, y - 2.0, 4.0, 4.0, c);
                if mine {
                    g.rect(x - 1.0, y - 1.0, 2.0, 2.0, 0xffffff);
                }
            }
        }
        let (ox, oy) = self.view_origin(g);
        g.frame(mmx + ox * k, mmy + oy * k, g.w * k, g.h * k, 0xf0f0f0);
        if inp.in_rect(mmx, mmy, MINIMAP, MINIMAP) {
            let world = Vec2::new(((inp.mx - mmx) / k) as f32, ((inp.my - mmy) / k) as f32);
            if (inp.left || inp.left_pressed) && !self.menu && !inp.click_consumed {
                self.cam = world;
                self.cam_lock = false;
                inp.click_consumed = true;
            }
            if inp.right_pressed && !inp.click_consumed {
                self.out.push(Command::Move(world));
                self.marker = Some((world, self.now, false));
                inp.click_consumed = true;
            }
        }

        // ---------------------------------------------------------- bottom panel
        if let (Some(m), Some(pinfo)) = (me.clone(), self.me.and_then(|i| self.players.get(i as usize)).cloned()) {
            let px = ((g.w - PANEL_W) / 2.0).floor().max(70.0);
            let py = g.h - PANEL_H;
            g.panel(px, py, PANEL_W, PANEL_H + 2.0);
            let my_e = self.entity(m.entity).cloned();
            // Portrait.
            g.rect(px + 4.0, py + 4.0, 30.0, 30.0, 0x24202e);
            g.draw_sprite(SpriteKey::Hero(pinfo.loadout.hero, Frame::Idle, false), px + 7.0, py + 6.0);
            g.rect(px + 4.0, py + 35.0, 30.0, 10.0, 0x101018);
            g.text_center(&format!("Lv{}", m.level), px + 19.0, py + 36.0, ui::GOLD, 1.0, false);
            let xp_frac = if m.xp_next > 0.0 { m.xp / m.xp_next } else { 1.0 };
            ui::bar(g, px + 4.0, py + 46.0, 30.0, 2.0, xp_frac as f64, 0xb070f0, 0x302040);
            // Skills.
            let keys = ["Q", "W", "E", "R"];
            let skills = [
                pinfo.loadout.skills[0],
                pinfo.loadout.skills[1],
                pinfo.loadout.skills[2],
                hero(pinfo.loadout.hero).ultimate,
            ];
            let mana = my_e.as_ref().map(|e| e.mana).unwrap_or(0.0);
            for i in 0..4 {
                let sx = px + 40.0 + i as f64 * 22.0;
                let sy = py + 5.0;
                let sk = skill(skills[i]);
                let rank = m.ranks[i];
                g.rect(
                    sx - 1.0,
                    sy - 1.0,
                    18.0,
                    18.0,
                    if self.held_skill == Some(i as u8) { 0xffffff } else { 0x101018 },
                );
                g.draw_sprite(SpriteKey::Skill(skills[i]), sx, sy);
                if rank == 0 {
                    g.rect_a(sx, sy, 16.0, 16.0, 0x000000, 0.6);
                } else if m.cooldowns[i] > 0.0 {
                    let f = (m.cooldowns[i] / m.cooldown_max[i].max(0.01)).clamp(0.0, 1.0) as f64;
                    g.rect_a(sx, sy + 16.0 * (1.0 - f), 16.0, 16.0 * f, 0x000000, 0.65);
                    let s = if m.cooldowns[i] >= 1.0 {
                        format!("{:.0}", m.cooldowns[i])
                    } else {
                        format!("{:.1}", m.cooldowns[i])
                    };
                    g.text_center(&s, sx + 8.0, sy + 4.0, ui::TEXT, 1.0, true);
                } else if mana < m.mana_costs[i] {
                    g.rect_a(sx, sy, 16.0, 16.0, 0x2040c0, 0.5);
                }
                g.text_s(keys[i], sx + 1.0, sy + 18.0, ui::DIM);
                let max = sk.max_rank();
                for r in 0..max {
                    let c = if r < rank { ui::GOLD } else { 0x404050 };
                    g.rect(sx + 8.0 + r as f64 * 2.0, sy + 20.0, 1.0, 3.0, c);
                }
                if can_level(&m, i, max) {
                    let bx = sx + 4.0;
                    let by = sy - 12.0;
                    let hover = inp.in_rect(bx, by, 9.0, 9.0);
                    g.rect(bx, by, 9.0, 9.0, if hover { 0xfff080 } else { 0xd0a020 });
                    g.text("+", bx + 2.0, by + 1.0, 0x201000);
                    if inp.clicked(bx, by, 9.0, 9.0) {
                        self.out.push(Command::LevelUp(i as u8));
                    }
                }
                if inp.in_rect(sx, sy, 16.0, 16.0) {
                    let stats = my_e.as_ref().map(|_| ());
                    let _ = stats;
                    let mut lines = ui::skill_lines(sk, rank.max(1), None, m.stats.haste);
                    lines.push((
                        format!("Rank {rank}/{max}  -  key {}  (Alt/Ctrl+{} to level)", keys[i], keys[i]),
                        ui::DIM,
                    ));
                    tip = Some((lines, 200.0));
                }
            }
            // Summoners.
            for i in 0..2 {
                let sx = px + 130.0 + i as f64 * 16.0;
                let sy = py + 6.0;
                let s = pinfo.loadout.summoners[i];
                g.draw_sprite(SpriteKey::Summoner(s), sx, sy);
                let cd = m.summoner_cd[i];
                if cd > 0.0 {
                    let f = (cd / s.cooldown()).clamp(0.0, 1.0) as f64;
                    g.rect_a(sx, sy + 14.0 * (1.0 - f), 14.0, 14.0 * f, 0x000000, 0.65);
                    g.text_center(&format!("{:.0}", cd), sx + 7.0, sy + 3.0, ui::TEXT, 1.0, true);
                }
                g.text_s(["D", "F"][i], sx + 4.0, sy + 16.0, ui::DIM);
                if inp.in_rect(sx, sy, 14.0, 14.0) {
                    tip = Some((ui::summoner_lines(s), 160.0));
                }
            }
            // HP / mana bars.
            if let Some(e) = &my_e {
                let bx = px + 40.0;
                let by = py + 31.0;
                let bw = 120.0;
                let total = (e.max_hp + e.shield).max(1.0) as f64;
                g.rect(bx, by, bw, 7.0, 0x301818);
                let hw = (bw * e.hp as f64 / total).round();
                g.rect(bx, by, hw, 7.0, 0x48b040);
                g.rect(bx + hw, by, (bw * e.shield as f64 / total).round(), 7.0, 0xe8e8e8);
                g.text_center(&format!("{:.0}/{:.0}", e.hp, e.max_hp), bx + bw / 2.0, by, ui::TEXT, 1.0, true);
                ui::bar(g, bx, by + 9.0, bw, 5.0, (e.mana / e.max_mana.max(1.0)) as f64, 0x4070e0, 0x182040);
                g.text_center(
                    &format!("{:.0}/{:.0}", e.mana, e.max_mana),
                    bx + bw / 2.0,
                    by + 8.0,
                    0xd0d8ff,
                    1.0,
                    false,
                );
            }
            // Items + gold.
            for i in 0..6 {
                let ix = px + 166.0 + (i % 3) as f64 * 17.0;
                let iy = py + 5.0 + (i / 3) as f64 * 17.0;
                g.rect(ix - 1.0, iy - 1.0, 16.0, 16.0, 0x101018);
                if let Some((id, n)) = m.items[i] {
                    g.draw_sprite(SpriteKey::Item(id), ix, iy);
                    if n > 1 {
                        g.text_s(&n.to_string(), ix + 9.0, iy + 7.0, ui::TEXT);
                    }
                    if inp.in_rect(ix, iy, 14.0, 14.0) {
                        let mut lines = ui::item_lines(item(id));
                        lines.push((format!("Key {}  -  right-click in shop to sell", i + 1), ui::DIM));
                        tip = Some((lines, 160.0));
                    }
                    if inp.clicked(ix, iy, 14.0, 14.0) {
                        self.out.push(Command::UseItem(i as u8));
                    }
                    if m.can_shop && inp.right_clicked(ix, iy, 14.0, 14.0) {
                        self.out.push(Command::Sell(i as u8));
                    }
                }
            }
            g.text_s(&format!("{:.0}g", m.gold), px + 166.0, py + 40.0, ui::GOLD);
            let shop_hover = inp.in_rect(px + 208.0, py + 38.0, 25.0, 10.0);
            g.rect(px + 208.0, py + 38.0, 25.0, 10.0, if shop_hover { 0x6a5a30 } else { 0x40381c });
            g.text("SHOP", px + 210.0, py + 39.0, ui::GOLD);
            if inp.clicked(px + 208.0, py + 38.0, 25.0, 10.0) {
                self.shop = !self.shop;
            }

            // Stats panel.
            let st = m.stats;
            let tx = px - 68.0;
            if tx > 2.0 {
                g.panel(tx, py + 4.0, 64.0, PANEL_H - 2.0);
                let rows = [
                    ("AD", format!("{:.0}", st.ad), 0xffb070),
                    ("AP", format!("{:.0}", st.ap), 0x9090ff),
                    ("AR", format!("{:.0}", st.armor), 0xe0c080),
                    ("MR", format!("{:.0}", st.mr), 0xc080e0),
                    ("AS", format!("{:.2}", st.attack_speed), 0xf0e080),
                ];
                let rows2 = [
                    ("MS", format!("{:.0}", st.move_speed), 0xc0f0c0),
                    ("CR", format!("{:.0}", st.crit * 100.0), 0xff8080),
                    ("AH", format!("{:.0}", st.haste), 0x80e0ff),
                    ("RG", format!("{:.0}", st.range), 0xd0d0d0),
                    ("LS", format!("{:.0}", st.lifesteal * 100.0), 0xff6060),
                ];
                for (r, (label, val, c)) in rows.iter().enumerate() {
                    g.text(label, tx + 3.0, py + 7.0 + r as f64 * 8.0, *c);
                    g.text(val, tx + 15.0, py + 7.0 + r as f64 * 8.0, ui::TEXT);
                }
                for (r, (label, val, c)) in rows2.iter().enumerate() {
                    g.text(label, tx + 34.0, py + 7.0 + r as f64 * 8.0, *c);
                    g.text(val, tx + 46.0, py + 7.0 + r as f64 * 8.0, ui::TEXT);
                }
            }

            // Cast / recall progress.
            if let Some((slot, prog)) = m.casting {
                let w = 80.0;
                let bx = (g.w - w) / 2.0;
                let by = py - 26.0;
                ui::bar(g, bx, by, w, 4.0, prog as f64, 0xf0d060, 0x302810);
                g.text_center(skill(skills[slot as usize]).name, g.w / 2.0, by - 9.0, ui::TEXT, 1.0, true);
            }
            if m.recall > 0.0 {
                let w = 80.0;
                let bx = (g.w - w) / 2.0;
                let by = py - 26.0;
                let total = if my_e.as_ref().map(|e| e.has(flags::BARON)).unwrap_or(false) { 4.0 } else { 8.0 };
                ui::bar(g, bx, by, w, 4.0, (1.0 - m.recall / total) as f64, 0x6080ff, 0x182040);
                g.text_center("Recalling", g.w / 2.0, by - 9.0, 0xb0c0ff, 1.0, true);
            }
            if self.amove {
                g.text_center(
                    "Attack move: left-click a target or location",
                    g.w / 2.0,
                    py - 40.0,
                    0xff9080,
                    1.0,
                    true,
                );
            }
            if m.skill_points > 0 {
                g.text_center(
                    &format!("{} skill point(s): click + or Alt+QWER", m.skill_points),
                    g.w / 2.0,
                    py - 22.0,
                    ui::GOLD,
                    1.0,
                    true,
                );
            }
            // Death overlay.
            if m.respawn > 0.0 {
                g.rect_a(0.0, 0.0, g.w, g.h - PANEL_H - 2.0, 0x101010, 0.35);
                g.text_center(
                    &format!("Respawning in {:.0}", m.respawn.ceil()),
                    g.w / 2.0,
                    g.h / 2.0 - 30.0,
                    0xd0d0d0,
                    2.0,
                    true,
                );
                g.text_center("You can shop while dead (P)", g.w / 2.0, g.h / 2.0 - 10.0, ui::DIM, 1.0, true);
            }
        }

        // ---------------------------------------------------------- banner
        if let Some((text, color, at)) = &self.banner {
            let age = (self.now - at) / 3000.0;
            g.set_alpha((1.0 - age).clamp(0.0, 1.0) * 1.5f64.min(1.0));
            g.text_center(text, g.w / 2.0, 46.0, *color, 2.0, true);
            g.set_alpha(1.0);
        }

        // ---------------------------------------------------------- shop
        if self.shop {
            if let Some(m) = &me {
                if let Some(t) = self.shop_window(g, inp, m) {
                    tip = Some(t);
                }
            }
        }
        if self.scoreboard {
            self.scoreboard_window(g, &cur);
        }

        // ---------------------------------------------------------- game over
        if let Some(w) = cur.winner {
            g.rect_a(0.0, 0.0, g.w, g.h, 0x000000, 0.45);
            let (text, c) = if self.me.is_none() {
                (format!("{} WINS", w.name().to_uppercase()), 0xf0e0b0)
            } else if w == my_team {
                ("VICTORY".to_string(), 0xf0d040)
            } else {
                ("DEFEAT".to_string(), 0xf05050)
            };
            self.scoreboard_window(g, &cur);
            // Above the scoreboard (which is centred and ~200px tall).
            let ty = (((g.h - 200.0) / 2.0 - 10.0) - 34.0).max(2.0);
            g.text_center(&text, g.w / 2.0, ty, c, 3.0, true);
            if ui::button(g, inp, g.w / 2.0 - 50.0, g.h - 70.0, 100.0, 16.0, "CONTINUE", true) {
                action = HudAction::Leave;
            }
            inp.click_consumed = true;
        }

        // ---------------------------------------------------------- escape menu
        if self.menu {
            let w = 140.0;
            let h = 84.0;
            let x = (g.w - w) / 2.0;
            let y = (g.h - h) / 2.0;
            g.rect_a(0.0, 0.0, g.w, g.h, 0x000000, 0.4);
            g.panel(x, y, w, h);
            g.text_center(if self.is_local() { "PAUSED" } else { "MENU" }, g.w / 2.0, y + 6.0, ui::GOLD, 1.0, false);
            if ui::button(g, inp, x + 10.0, y + 20.0, w - 20.0, 16.0, "RESUME", true) || inp.key("Escape") {
                self.menu = false;
            }
            let leave_label = if self.is_local() { "SURRENDER & QUIT" } else { "LEAVE MATCH" };
            if ui::button(g, inp, x + 10.0, y + 42.0, w - 20.0, 16.0, leave_label, true) {
                action = HudAction::Leave;
            }
            g.text_center("Esc to close", g.w / 2.0, y + 68.0, ui::DIM, 1.0, false);
            inp.click_consumed = true;
        }

        if let Some((lines, w)) = tip {
            ui::tooltip(g, inp.mx, inp.my, &lines, w);
        }
        action
    }

    fn shop_window(&mut self, g: &Gfx, inp: &mut Input, m: &SelfView) -> Option<(Lines, f64)> {
        let cols = 7;
        let cell_w = 32.0;
        let cell_h = 30.0;
        let rows = ITEMS.len().div_ceil(cols);
        let w = cols as f64 * cell_w + 12.0;
        let h = rows as f64 * cell_h + 44.0;
        let x = ((g.w - w) / 2.0).floor();
        let y = ((g.h - PANEL_H - h) / 2.0).floor().max(14.0);
        g.panel(x, y, w, h);
        g.text_s("SHOP", x + 6.0, y + 5.0, ui::GOLD);
        g.text_s(&format!("{:.0} gold", m.gold), x + 40.0, y + 5.0, ui::GOLD);
        if !m.can_shop {
            g.text_s("Return to base to buy", x + w - 130.0, y + 5.0, ui::BAD);
        }
        let mut tip = None;
        let mut sorted: Vec<_> = ITEMS.iter().collect();
        sorted.sort_by_key(|i| {
            (i.tier != ItemTier::Consumable, i.tier != ItemTier::Basic, i.tier != ItemTier::Epic, i.cost)
        });
        for (n, it) in sorted.iter().enumerate() {
            let cx = x + 6.0 + (n % cols) as f64 * cell_w;
            let cy = y + 18.0 + (n / cols) as f64 * cell_h;
            let afford = m.gold + 0.01 >= it.cost as f32 && m.can_shop;
            let hover = inp.in_rect(cx, cy, cell_w - 2.0, cell_h - 2.0);
            g.rect(cx, cy, cell_w - 2.0, cell_h - 2.0, if hover { 0x3a3458 } else { 0x1c1a28 });
            g.draw_sprite(SpriteKey::Item(it.id), cx + 8.0, cy + 2.0);
            if !afford {
                g.rect_a(cx + 8.0, cy + 2.0, 14.0, 14.0, 0x000000, 0.55);
            }
            g.text_center(
                &it.cost.to_string(),
                cx + 15.0,
                cy + 18.0,
                if afford { ui::GOLD } else { ui::DIM },
                1.0,
                false,
            );
            if hover {
                let mut lines = ui::item_lines(it);
                lines.push(("Left-click to buy".into(), ui::DIM));
                tip = Some((lines, 160.0));
            }
            if inp.clicked(cx, cy, cell_w - 2.0, cell_h - 2.0) && afford {
                self.out.push(Command::Buy(it.id));
            }
        }
        g.text_s("P/Esc: close   R-click own item: sell", x + 6.0, y + h - 12.0, ui::DIM);
        if inp.in_rect(x, y, w, h) && (inp.left_pressed || inp.right_pressed) {
            inp.click_consumed = true;
        }
        tip
    }

    fn scoreboard_window(&self, g: &Gfx, cur: &moba_core::protocol::Snapshot) {
        let w = 344.0;
        let row_h = 16.0;
        let h = 32.0 + row_h * cur.players.len() as f64 + 8.0;
        let x = ((g.w - w) / 2.0).floor();
        let y = ((g.h - h) / 2.0 - 10.0).floor().max(4.0);
        g.panel(x, y, w, h);
        g.text_s(
            &format!("BLUE {}  dragons {}  barons {}", cur.score[0].kills, cur.score[0].dragons, cur.score[0].barons),
            x + 6.0,
            y + 5.0,
            ui::BLUE_TEAM,
        );
        g.text_s(
            &format!("RED {}  dragons {}  barons {}", cur.score[1].kills, cur.score[1].dragons, cur.score[1].barons),
            x + 6.0,
            y + 15.0,
            ui::RED_TEAM,
        );
        g.text_right(&fmt_time(cur.time), x + w - 6.0, y + 5.0, ui::TEXT);
        let mut rows: Vec<_> = cur.players.iter().collect();
        rows.sort_by_key(|r| (r.team.index(), r.index));
        for (n, r) in rows.iter().enumerate() {
            let ry = y + 28.0 + n as f64 * row_h;
            let mine = Some(r.index) == self.me;
            g.rect(
                x + 4.0,
                ry,
                w - 8.0,
                row_h - 2.0,
                if mine {
                    0x2c3a2c
                } else if r.team == Team::Blue {
                    0x1c2438
                } else {
                    0x381c20
                },
            );
            let c = if r.team == Team::Blue { ui::BLUE_TEAM } else { ui::RED_TEAM };
            let def = hero(r.hero);
            let name = self.player_name(r.index);
            let name: String = name.chars().take(12).collect();
            g.text_s(&name, x + 8.0, ry + 3.0, c);
            g.text_s(def.name, x + 84.0, ry + 3.0, ui::TEXT);
            g.text_s(&format!("{}", r.level), x + 136.0, ry + 3.0, ui::GOLD);
            g.text_s(&format!("{}/{}/{}", r.kills, r.deaths, r.assists), x + 152.0, ry + 3.0, ui::TEXT);
            g.text_s(&format!("{}", r.cs), x + 202.0, ry + 3.0, ui::DIM);
            for (i, it) in r.items.iter().enumerate() {
                if let Some(id) = it {
                    g.draw_sprite(SpriteKey::Item(*id), x + 222.0 + i as f64 * 15.0, ry);
                }
            }
            if r.respawn > 0.0 {
                g.text_right(&format!("{:.0}", r.respawn.ceil()), x + w - 6.0, ry + 3.0, ui::BAD);
            }
        }
    }
}
