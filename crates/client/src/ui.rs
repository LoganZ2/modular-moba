//! Immediate-mode pixel UI widgets and tooltip builders.

// Immediate-mode widgets naturally take a rectangle plus state.
#![allow(clippy::too_many_arguments)]

use crate::font;
use crate::gfx::Gfx;
use crate::input::Input;
use moba_core::items::ItemDef;
use moba_core::roles::Role;
use moba_core::skills::SkillDef;
use moba_core::stats::Stats;
use moba_core::summoners::SummonerId;

pub const TEXT: u32 = 0xf0eadc;
pub const DIM: u32 = 0x8a8498;
pub const GOLD: u32 = 0xf0c838;
pub const GOOD: u32 = 0x68e070;
pub const BAD: u32 = 0xf06060;
pub const BLUE_TEAM: u32 = 0x58a0ff;
pub const RED_TEAM: u32 = 0xff6858;

pub fn button(g: &Gfx, inp: &mut Input, x: f64, y: f64, w: f64, h: f64, label: &str, enabled: bool) -> bool {
    let hover = enabled && inp.in_rect(x, y, w, h);
    let bg = if !enabled {
        0x262230
    } else if hover {
        if inp.left {
            0x2c2448
        } else {
            0x5a4c8a
        }
    } else {
        0x3c3262
    };
    g.rect(x, y, w, h, bg);
    g.frame(x, y, w, h, if enabled { 0x9a8cd0 } else { 0x403a50 });
    if enabled {
        g.rect(x + 1.0, y + 1.0, w - 2.0, 1.0, if hover { 0x8a7cc0 } else { 0x5c4e90 });
    }
    let tw = font::text_width(label);
    g.text_s(label, x + (w - tw) / 2.0, y + (h - 7.0) / 2.0, if enabled { TEXT } else { DIM });
    enabled && inp.clicked(x, y, w, h)
}

/// Small toggle chip; returns true when clicked.
pub fn chip(g: &Gfx, inp: &mut Input, x: f64, y: f64, label: &str, on: bool, color: u32) -> bool {
    let w = font::text_width(label) + 8.0;
    let h = 11.0;
    let hover = inp.in_rect(x, y, w, h);
    g.rect(
        x,
        y,
        w,
        h,
        if on {
            color
        } else if hover {
            0x3a3450
        } else {
            0x24202e
        },
    );
    g.frame(x, y, w, h, if on { 0xffffff } else { 0x5a5070 });
    g.text(label, x + 4.0, y + 2.0, if on { 0x101018 } else { TEXT });
    inp.clicked(x, y, w, h)
}

pub fn role_tag(g: &Gfx, x: f64, y: f64, r: Role) -> f64 {
    let w = font::text_width(r.short()) + 4.0;
    g.rect(x, y, w, 9.0, crate::art::shade(r.color(), 0.55));
    g.text(r.short(), x + 2.0, y + 1.0, r.color());
    w
}

pub fn role_tags(g: &Gfx, x: f64, y: f64, roles: &[Role; 2]) -> f64 {
    let a = role_tag(g, x, y, roles[0]);
    let b = role_tag(g, x + a + 2.0, y, roles[1]);
    a + b + 2.0
}

pub fn bar(g: &Gfx, x: f64, y: f64, w: f64, h: f64, frac: f64, color: u32, bg: u32) {
    g.rect(x, y, w, h, bg);
    let fw = (w * frac.clamp(0.0, 1.0)).round();
    g.rect(x, y, fw, h, color);
}

pub type Lines = Vec<(String, u32)>;

/// Draw a tooltip near (x, y), kept on screen.
pub fn tooltip(g: &Gfx, x: f64, y: f64, lines: &Lines, max_w: f64) {
    let mut wrapped: Vec<(String, u32)> = Vec::new();
    for (s, c) in lines {
        if s.is_empty() {
            wrapped.push((String::new(), *c));
            continue;
        }
        for l in font::wrap(s, max_w) {
            wrapped.push((l, *c));
        }
    }
    let w = wrapped.iter().map(|(s, _)| font::text_width(s)).fold(0.0, f64::max) + 8.0;
    let h = wrapped.len() as f64 * 9.0 + 6.0;
    let mut tx = x + 10.0;
    let mut ty = y + 10.0;
    if tx + w > g.w - 2.0 {
        tx = (x - w - 6.0).max(2.0);
    }
    if ty + h > g.h - 2.0 {
        ty = (g.h - h - 2.0).max(2.0);
    }
    g.panel(tx, ty, w, h);
    for (i, (s, c)) in wrapped.iter().enumerate() {
        g.text(s, tx + 4.0, ty + 4.0 + i as f64 * 9.0, *c);
    }
}

pub fn skill_lines(sk: &SkillDef, rank: u8, stats: Option<&Stats>, haste: f32) -> Lines {
    let mut v: Lines = Vec::new();
    let r = rank.max(1);
    let title = if sk.ultimate { format!("{} (Ultimate)", sk.name) } else { sk.name.to_string() };
    v.push((title, GOLD));
    v.push((format!("{} / {}", sk.roles[0].name(), sk.roles[1].name()), DIM));
    v.push((sk.desc.to_string(), TEXT));
    for l in sk.detail_lines(r, stats) {
        v.push((l, 0xb8d8ff));
    }
    let cd = moba_core::stats::haste_cooldown(sk.cooldown_at(r), haste);
    let mut meta = format!("CD {:.1}s  Mana {:.0}", cd, sk.mana_at(r));
    if sk.range() > 0.0 {
        meta.push_str(&format!("  Range {:.0}", sk.range()));
    }
    if sk.cast_time > 0.0 {
        meta.push_str(&format!("  Cast {:.2}s", sk.cast_time));
    }
    v.push((meta, DIM));
    let ranks = (1..=sk.max_rank()).map(|k| format!("{:.0}", sk.cooldown_at(k))).collect::<Vec<_>>().join("/");
    v.push((format!("Cooldown by rank: {ranks}"), DIM));
    v
}

pub fn item_lines(it: &ItemDef) -> Lines {
    let mut v: Lines = vec![(it.name.to_string(), GOLD), (format!("{} gold", it.cost), DIM)];
    if it.id == moba_core::items::POTION {
        v.push((
            format!(
                "Restores {:.0} health over {:.0}s. Stacks to {}.",
                moba_core::items::POTION_HEAL,
                moba_core::items::POTION_SECS,
                it.stack
            ),
            TEXT,
        ));
    }
    for l in it.stats.describe() {
        v.push((l, 0xb8d8ff));
    }
    v
}

pub fn summoner_lines(s: SummonerId) -> Lines {
    vec![(s.name().to_string(), GOLD), (s.desc().to_string(), TEXT), (format!("Cooldown {:.0}s", s.cooldown()), DIM)]
}

/// Editable single-line text field. Returns true when it has focus.
pub fn text_field(
    g: &Gfx,
    inp: &mut Input,
    x: f64,
    y: f64,
    w: f64,
    value: &mut String,
    focused: bool,
    max: usize,
    t: f64,
) -> bool {
    let h = 13.0;
    let mut focus = focused;
    if inp.left_pressed {
        focus = inp.in_rect(x, y, w, h);
        if focus {
            inp.click_consumed = true;
        }
    }
    g.rect(x, y, w, h, if focus { 0x1c1830 } else { 0x16141e });
    g.frame(x, y, w, h, if focus { 0xb0a0f0 } else { 0x4a4460 });
    if focus {
        for c in inp.typed.drain(..) {
            if value.chars().count() < max && (c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_') {
                value.push(c);
            }
        }
        for _ in 0..inp.backspace {
            value.pop();
        }
    }
    g.text(value, x + 3.0, y + 3.0, TEXT);
    if focus && (t * 2.0) as i64 % 2 == 0 {
        g.rect(x + 4.0 + font::text_width(value), y + 2.0, 1.0, 9.0, TEXT);
    }
    focus
}
