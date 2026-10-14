//! Procedural pixel art. Every sprite is painted in code from a handful of
//! primitives, then gets an automatic dark outline. Heroes are assembled from
//! their [`HeroLook`] (palette + headgear + weapon), so new heroes need no
//! hand-drawn assets.

use moba_core::heroes::{Headgear, HeroLook, Weapon};
use moba_core::items::{ItemDef, ItemGlyph};
use moba_core::skills::{Action, Effect, SkillDef};
use moba_core::summoners::SummonerId;
use moba_core::units::{MinionKind, MonsterKind};
use moba_core::Team;

pub const OUTLINE: u32 = 0x1a1424;

/// A small RGBA image; pixels are 0xAARRGGBB (0 = transparent).
#[derive(Clone, Debug)]
pub struct Pix {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Pix {
    pub fn new(w: usize, h: usize) -> Pix {
        Pix { w, h, px: vec![0; w * h] }
    }

    pub fn set(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = 0xff00_0000 | (c & 0xff_ffff);
        }
    }

    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize]
        } else {
            0
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.set(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: u32) {
        let x0 = (cx - rx).floor() as i32;
        let x1 = (cx + rx).ceil() as i32;
        let y0 = (cy - ry).floor() as i32;
        let y1 = (cy + ry).ceil() as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = (x as f32 + 0.5 - cx) / rx.max(0.01);
                let dy = (y as f32 + 0.5 - cy) / ry.max(0.01);
                if dx * dx + dy * dy <= 1.0 {
                    self.set(x, y, c);
                }
            }
        }
    }

    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, c: u32) {
        self.ellipse(cx, cy, r, r, c);
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, c: u32) {
        let steps = (r * 8.0).max(12.0) as i32;
        for i in 0..steps {
            let a = i as f32 / steps as f32 * std::f32::consts::TAU;
            self.set((cx + a.cos() * r).round() as i32, (cy + a.sin() * r).round() as i32, c);
        }
    }

    /// Filled triangle.
    pub fn tri(&mut self, a: (i32, i32), b: (i32, i32), c: (i32, i32), col: u32) {
        let minx = a.0.min(b.0).min(c.0);
        let maxx = a.0.max(b.0).max(c.0);
        let miny = a.1.min(b.1).min(c.1);
        let maxy = a.1.max(b.1).max(c.1);
        let edge = |p: (i32, i32), q: (i32, i32), x: i32, y: i32| (q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0);
        for y in miny..=maxy {
            for x in minx..=maxx {
                let w0 = edge(b, c, x, y);
                let w1 = edge(c, a, x, y);
                let w2 = edge(a, b, x, y);
                if (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0) {
                    self.set(x, y, col);
                }
            }
        }
    }

    /// Surround every opaque pixel with an outline pixel.
    pub fn outline(&mut self, c: u32) {
        let src = self.px.clone();
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                if src[y as usize * self.w + x as usize] != 0 {
                    continue;
                }
                let near = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                    let (nx, ny) = (x + dx, y + dy);
                    nx >= 0
                        && ny >= 0
                        && (nx as usize) < self.w
                        && (ny as usize) < self.h
                        && src[ny as usize * self.w + nx as usize] != 0
                });
                if near {
                    self.set(x, y, c);
                }
            }
        }
    }

    pub fn flipped(&self) -> Pix {
        let mut out = Pix::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                out.px[y * self.w + (self.w - 1 - x)] = self.px[y * self.w + x];
            }
        }
        out
    }

    /// Multiply every opaque pixel's colour (used for "dead"/grey variants).
    pub fn tinted(&self, f: impl Fn(u32) -> u32) -> Pix {
        let mut out = self.clone();
        for p in out.px.iter_mut() {
            if *p != 0 {
                *p = 0xff00_0000 | f(*p & 0xff_ffff);
            }
        }
        out
    }

    pub fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.w * self.h * 4);
        for p in &self.px {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
        }
        out
    }
}

pub fn shade(c: u32, f: f32) -> u32 {
    let ch = |s: u32| (((c >> s) & 0xff) as f32 * f).clamp(0.0, 255.0) as u32;
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let x = ((a >> s) & 0xff) as f32;
        let y = ((b >> s) & 0xff) as f32;
        (x + (y - x) * t).clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

pub fn grey(c: u32) -> u32 {
    let l = (((c >> 16) & 0xff) * 3 + ((c >> 8) & 0xff) * 5 + (c & 0xff) * 2) / 10;
    let l = (l as f32 * 0.6) as u32;
    (l << 16) | (l << 8) | l
}

pub fn team_colors(team: Team) -> (u32, u32, u32) {
    match team {
        Team::Blue => (0x3c78d8, 0x8cc4ff, 0x1c3c78),
        Team::Red => (0xd04040, 0xff9a8a, 0x701c24),
        Team::Neutral => (0xc8b040, 0xf8e890, 0x605010),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frame {
    Idle,
    Walk,
    Attack,
}

const STEEL: u32 = 0xd0d8e4;
const WOOD: u32 = 0x8a5a30;
const DARK: u32 = 0x302830;

// ------------------------------------------------------------------ heroes

pub const HERO_W: usize = 24;
pub const HERO_H: usize = 26;
/// Sprite pixel that sits on the unit's position.
pub const HERO_ANCHOR: (f64, f64) = (12.0, 23.0);

pub fn hero(look: &HeroLook, frame: Frame) -> Pix {
    let mut p = Pix::new(HERO_W, HERO_H);
    let (x0, x1) = if look.broad { (7, 16) } else { (8, 15) };
    let body = look.body;
    let trim = look.trim;
    let skin = look.skin;
    let legc = shade(body, 0.5);
    let attack = frame == Frame::Attack;

    if look.cape {
        p.rect(x0 - 2, 13, 3, 9, shade(trim, 0.7));
        p.rect(x0 - 1, 12, 2, 1, shade(trim, 0.7));
    }
    if look.weapon == Weapon::SwordShield {
        // Shield on the back arm.
        p.rect(3, 13, 5, 7, trim);
        p.rect(4, 14, 3, 5, shade(trim, 0.75));
        p.set(5, 16, 0xf0e8c0);
    }
    // Legs.
    match frame {
        Frame::Walk => {
            p.rect(x0 + 1, 20, 2, 3, legc);
            p.rect(x1 - 1, 19, 2, 3, legc);
            p.rect(x0 + 1, 22, 2, 1, shade(legc, 0.6));
        }
        _ => {
            p.rect(x0 + 1, 20, 2, 3, legc);
            p.rect(x1 - 2, 20, 2, 3, legc);
            p.rect(x0 + 1, 22, 2, 1, shade(legc, 0.6));
            p.rect(x1 - 2, 22, 2, 1, shade(legc, 0.6));
        }
    }
    // Torso.
    p.rect(x0, 13, x1 - x0 + 1, 7, body);
    p.rect(x0, 13, 1, 7, shade(body, 0.8));
    p.rect(x1, 14, 1, 5, shade(body, 1.15));
    p.rect(x0, 18, x1 - x0 + 1, 1, trim);
    p.rect(x0 + 2, 13, x1 - x0 - 3, 1, trim);
    p.set((x0 + x1) / 2 + 1, 15, shade(trim, 1.2));
    // Back arm.
    p.rect(x0 - 2, 14, 2, 4, shade(body, 0.75));
    p.rect(x0 - 2, 18, 2, 1, shade(skin, 0.85));
    // Head.
    p.rect(8, 5, 8, 8, skin);
    p.rect(8, 12, 8, 1, shade(skin, 0.85));
    p.set(12, 9, 0x201820);
    p.set(14, 9, 0x201820);
    p.set(15, 11, shade(skin, 0.8));
    headgear(&mut p, look);
    // Front arm + weapon.
    if attack {
        p.rect(x1 + 1, 14, 4, 2, body);
        p.rect(x1 + 5, 14, 1, 2, skin);
    } else {
        p.rect(x1 + 1, 14, 2, 4, body);
        p.rect(x1 + 1, 18, 2, 1, skin);
    }
    weapon(&mut p, look, attack, x0, x1);
    p.outline(OUTLINE);
    p
}

fn headgear(p: &mut Pix, look: &HeroLook) {
    let hair = look.hair;
    let trim = look.trim;
    let metal = mix(0x9aa4b4, trim, 0.25);
    match look.head {
        Headgear::Hair => {
            p.rect(8, 4, 8, 3, hair);
            p.rect(7, 5, 1, 5, hair);
            p.rect(8, 7, 2, 2, hair);
            p.set(15, 7, hair);
        }
        Headgear::Helm | Headgear::WingedHelm => {
            p.rect(7, 4, 10, 4, metal);
            p.rect(7, 8, 3, 4, metal);
            p.rect(8, 4, 8, 1, shade(metal, 1.2));
            p.rect(11, 2, 2, 2, trim);
            p.rect(10, 8, 6, 1, shade(metal, 0.7));
            if look.head == Headgear::WingedHelm {
                p.rect(4, 3, 3, 3, 0xf4f4f4);
                p.set(3, 2, 0xf4f4f4);
                p.rect(17, 3, 3, 3, 0xf4f4f4);
                p.set(20, 2, 0xf4f4f4);
            }
        }
        Headgear::Horns => {
            p.rect(8, 4, 8, 2, hair);
            let bone = 0xe8dcc0;
            p.rect(6, 2, 2, 3, bone);
            p.set(5, 1, bone);
            p.rect(16, 2, 2, 3, bone);
            p.set(18, 1, bone);
            p.rect(9, 6, 6, 1, shade(hair, 0.8));
        }
        Headgear::Hood => {
            let hood = shade(look.body, 0.85);
            p.rect(7, 4, 10, 9, hood);
            p.rect(6, 6, 1, 7, hood);
            p.rect(11, 7, 5, 5, look.skin);
            p.rect(11, 7, 5, 1, shade(look.skin, 0.6));
            p.set(12, 9, 0x201820);
            p.set(14, 9, 0x201820);
            p.rect(8, 3, 6, 1, hood);
        }
        Headgear::WizardHat => {
            p.rect(8, 6, 1, 4, hair);
            p.rect(6, 5, 12, 1, trim);
            p.rect(8, 4, 8, 1, shade(trim, 0.7));
            p.rect(9, 3, 6, 1, trim);
            p.rect(10, 2, 4, 1, trim);
            p.rect(11, 1, 3, 1, trim);
            p.set(13, 0, trim);
            p.set(12, 3, 0xfff0a0);
        }
        Headgear::Bandana => {
            p.rect(8, 4, 8, 2, hair);
            p.rect(7, 5, 1, 4, hair);
            p.rect(8, 6, 8, 1, trim);
            p.set(6, 6, trim);
            p.set(5, 7, trim);
        }
        Headgear::Crown => {
            p.rect(8, 4, 8, 2, hair);
            p.rect(7, 5, 1, 4, hair);
            let gold = 0xf0c838;
            p.rect(9, 3, 6, 1, gold);
            p.set(9, 2, gold);
            p.set(12, 2, gold);
            p.set(14, 2, gold);
            p.set(12, 3, 0xe03040);
        }
    }
}

fn weapon(p: &mut Pix, look: &HeroLook, attack: bool, x0: i32, x1: i32) {
    let trim = look.trim;
    match look.weapon {
        Weapon::Sword | Weapon::SwordShield => {
            let len = if look.weapon == Weapon::Sword { 9 } else { 6 };
            if attack {
                p.rect(x1 + 6, 14, len, 1, STEEL);
                p.rect(x1 + 6, 15, len - 1, 1, shade(STEEL, 0.8));
                p.rect(x1 + 5, 13, 1, 4, trim);
            } else {
                p.rect(18, 17 - len, 1, len, STEEL);
                p.rect(19, 18 - len, 1, len - 1, shade(STEEL, 0.8));
                p.rect(17, 17, 4, 1, trim);
                p.set(18, 18, WOOD);
            }
        }
        Weapon::Hammer => {
            if attack {
                p.rect(x1 + 5, 15, 4, 1, WOOD);
                p.rect(20, 11, 4, 7, STEEL);
                p.rect(20, 11, 4, 1, shade(STEEL, 1.1));
                p.rect(23, 12, 1, 5, shade(STEEL, 0.7));
            } else {
                p.rect(18, 9, 1, 11, WOOD);
                p.rect(16, 5, 6, 5, STEEL);
                p.rect(16, 9, 6, 1, shade(STEEL, 0.7));
                p.rect(17, 6, 2, 1, 0xffffff);
            }
        }
        Weapon::Fists => {
            if attack {
                p.rect(x1 + 5, 13, 4, 4, trim);
                p.rect(x1 + 5, 13, 4, 1, shade(trim, 1.2));
            } else {
                p.rect(x1 + 1, 17, 3, 3, trim);
            }
            p.rect(x0 - 3, 17, 3, 3, shade(trim, 0.8));
        }
        Weapon::Spear => {
            if attack {
                p.rect(x1 - 2, 15, 13, 1, WOOD);
                p.rect(21, 14, 3, 3, STEEL);
                p.set(23, 15, 0xffffff);
            } else {
                p.rect(18, 3, 1, 19, WOOD);
                p.rect(17, 1, 3, 3, STEEL);
                p.set(18, 0, STEEL);
                p.rect(17, 5, 3, 1, trim);
            }
        }
        Weapon::Daggers => {
            if attack {
                p.rect(x1 + 6, 14, 4, 1, STEEL);
            } else {
                p.rect(18, 15, 1, 4, STEEL);
            }
            p.rect(x0 - 2, 15, 1, 4, STEEL);
            p.set(x0 - 2, 19, trim);
        }
        Weapon::Bow => {
            let bx = 19;
            for (y, dx) in [
                (8, -1),
                (9, 0),
                (10, 0),
                (11, 1),
                (12, 1),
                (13, 1),
                (14, 1),
                (15, 1),
                (16, 1),
                (17, 0),
                (18, 0),
                (19, -1),
            ] {
                p.set(bx + dx, y, WOOD);
            }
            if attack {
                p.line(bx - 1, 8, 15, 14, 0xe8e8e8);
                p.line(15, 14, bx - 1, 19, 0xe8e8e8);
                p.rect(15, 14, 9, 1, STEEL);
                p.set(23, 13, trim);
                p.set(23, 15, trim);
            } else {
                p.rect(bx - 2, 9, 1, 10, 0xe8e8e8);
            }
        }
        Weapon::Staff => {
            if attack {
                p.line(16, 20, 21, 7, WOOD);
                p.disc(21.5, 5.5, 2.2, trim);
                p.set(21, 4, 0xffffff);
            } else {
                p.rect(18, 6, 1, 16, WOOD);
                p.disc(18.5, 4.0, 2.2, trim);
                p.set(18, 3, 0xffffff);
            }
        }
        Weapon::Wand => {
            if attack {
                p.rect(x1 + 5, 15, 4, 1, DARK);
                p.rect(21, 13, 3, 3, trim);
                p.set(22, 14, 0xffffff);
            } else {
                p.rect(18, 12, 1, 7, DARK);
                p.rect(17, 9, 3, 3, trim);
                p.set(18, 10, 0xffffff);
            }
        }
        Weapon::Rifle => {
            let y = if attack { 14 } else { 16 };
            p.rect(13, y, 10, 2, DARK);
            p.rect(10, y + 1, 4, 2, WOOD);
            p.rect(15, y - 1, 3, 1, trim);
            if attack {
                p.set(23, y, 0xfff060);
                p.set(23, y + 1, 0xffb030);
            }
        }
    }
}

// ------------------------------------------------------------------ minions

pub fn minion(kind: MinionKind, team: Team, frame: Frame) -> Pix {
    let (main, light, dark) = team_colors(team);
    let walk = frame == Frame::Walk;
    let attack = frame == Frame::Attack;
    match kind {
        MinionKind::Melee | MinionKind::Super => {
            let big = kind == MinionKind::Super;
            let s = if big { 22 } else { 16 };
            let mut p = Pix::new(s, s);
            let o = if big { 3 } else { 0 };
            let armor = if big { light } else { 0xa8b0bc };
            // legs
            p.rect(5 + o, 12 + o, 2, 3, DARK);
            p.rect(9 + o, if walk { 11 + o } else { 12 + o }, 2, 3, DARK);
            // body
            p.rect(4 + o, 7 + o, 8, 6, main);
            p.rect(4 + o, 7 + o, 8, 1, light);
            p.rect(5 + o, 10 + o, 6, 1, dark);
            // head (helmet)
            p.rect(5 + o, 2 + o, 6, 5, armor);
            p.rect(8 + o, 4 + o, 3, 1, 0x181018);
            p.rect(7 + o, 1 + o, 2, 1, main);
            if big {
                p.rect(3, 1, 2, 3, light);
                p.rect(17, 1, 2, 3, light);
                p.rect(1, 10, 4, 6, light);
            }
            // sword
            if attack {
                p.rect(12 + o, 9 + o, 4, 1, STEEL);
            } else {
                p.rect(13 + o, 4 + o, 1, 7, STEEL);
                p.rect(12 + o, 10 + o, 3, 1, WOOD);
            }
            p.outline(OUTLINE);
            p
        }
        MinionKind::Caster => {
            let mut p = Pix::new(16, 16);
            p.tri((3, 14), (12, 14), (8, 5), main);
            p.rect(5, 6, 6, 8, main);
            p.rect(5, 12, 6, 1, dark);
            p.rect(5, 2, 6, 5, shade(main, 0.8));
            p.rect(7, 4, 3, 2, 0xf0c8a0);
            p.set(9, 4, 0x181018);
            if walk {
                p.rect(5, 14, 2, 1, DARK);
            } else {
                p.rect(5, 14, 2, 1, DARK);
                p.rect(9, 14, 2, 1, DARK);
            }
            let (sx, sy) = if attack { (13, 4) } else { (12, 3) };
            p.rect(12, 6, 1, 8, WOOD);
            p.disc(sx as f32 + 0.5, sy as f32 + 0.5, 1.6, light);
            p.set(sx, sy, 0xffffff);
            p.outline(OUTLINE);
            p
        }
        MinionKind::Siege => {
            let mut p = Pix::new(20, 16);
            p.rect(3, 7, 13, 5, WOOD);
            p.rect(3, 7, 13, 1, shade(WOOD, 1.3));
            p.rect(3, 9, 13, 1, main);
            let bx = if attack { 12 } else { 11 };
            p.rect(bx, 4, 7, 3, 0x505860);
            p.rect(bx, 4, 7, 1, 0x8890a0);
            if attack {
                p.set(19, 5, 0xfff060);
            }
            p.rect(6, 1, 1, 6, WOOD);
            p.rect(7, 1, 4, 3, main);
            let off = if walk { 1 } else { 0 };
            p.disc(5.5, 13.0, 2.2, DARK);
            p.disc(13.5, 13.0, 2.2, DARK);
            p.set(5 + off, 12, 0x909090);
            p.set(13 + off, 12, 0x909090);
            p.outline(OUTLINE);
            p
        }
    }
}

// ------------------------------------------------------------------ structures

pub fn tower(team: Team, alive: bool) -> Pix {
    let (main, light, dark) = team_colors(team);
    let mut p = Pix::new(28, 46);
    let stone = 0x8c8c98;
    if !alive {
        p.ellipse(14.0, 40.0, 12.0, 5.0, shade(stone, 0.6));
        for (x, y, w, h) in [(5, 36, 6, 4), (12, 34, 7, 5), (18, 37, 5, 4), (9, 39, 9, 3)] {
            p.rect(x, y, w, h, shade(stone, 0.8));
            p.rect(x, y, w, 1, stone);
        }
        p.rect(14, 33, 3, 2, dark);
        p.outline(OUTLINE);
        return p;
    }
    // Base.
    p.rect(4, 36, 20, 8, shade(stone, 0.85));
    p.rect(4, 36, 20, 1, stone);
    // Column.
    p.rect(8, 12, 12, 25, stone);
    p.rect(8, 12, 2, 25, shade(stone, 0.75));
    p.rect(18, 12, 2, 25, shade(stone, 1.15));
    for y in (15..36).step_by(5) {
        p.rect(10, y, 8, 1, shade(stone, 0.7));
    }
    // Banner.
    p.rect(11, 17, 6, 10, main);
    p.rect(11, 17, 6, 1, light);
    p.tri((11, 27), (16, 27), (13, 30), main);
    // Platform.
    p.rect(5, 9, 18, 4, shade(stone, 0.7));
    p.rect(5, 9, 18, 1, stone);
    p.rect(6, 7, 2, 2, shade(stone, 0.7));
    p.rect(20, 7, 2, 2, shade(stone, 0.7));
    // Crystal.
    p.tri((14, 0), (9, 5), (19, 5), light);
    p.tri((9, 5), (19, 5), (14, 10), main);
    p.set(13, 3, 0xffffff);
    p.set(12, 4, 0xffffff);
    p.outline(OUTLINE);
    p
}

pub fn inhibitor(team: Team, alive: bool) -> Pix {
    let (main, light, dark) = team_colors(team);
    let mut p = Pix::new(30, 30);
    let stone = 0x9090a0;
    p.ellipse(15.0, 24.0, 13.0, 5.0, shade(stone, 0.7));
    p.ellipse(15.0, 23.0, 11.0, 4.0, stone);
    if alive {
        p.disc(15.0, 12.0, 8.5, main);
        p.disc(13.5, 10.5, 5.0, light);
        p.disc(12.5, 9.5, 2.0, 0xffffff);
        p.ring(15.0, 12.0, 10.5, dark);
    } else {
        for (x, y) in [(9, 18), (17, 17), (13, 20)] {
            p.tri((x, y + 4), (x + 4, y + 4), (x + 2, y), shade(main, 0.6));
        }
    }
    p.outline(OUTLINE);
    p
}

pub fn nexus(team: Team, alive: bool) -> Pix {
    let (main, light, dark) = team_colors(team);
    let mut p = Pix::new(56, 56);
    let stone = 0x9898a8;
    p.ellipse(28.0, 46.0, 25.0, 9.0, shade(stone, 0.6));
    p.ellipse(28.0, 44.0, 22.0, 7.0, stone);
    p.ellipse(28.0, 43.0, 15.0, 4.0, shade(stone, 1.15));
    if alive {
        p.tri((28, 2), (16, 22), (40, 22), light);
        p.tri((16, 22), (40, 22), (28, 42), main);
        p.tri((28, 2), (28, 42), (16, 22), shade(main, 1.1));
        p.line(28, 4, 28, 40, light);
        p.set(24, 12, 0xffffff);
        p.set(23, 14, 0xffffff);
        for (x, y) in [(8, 18), (46, 20), (12, 34), (44, 32)] {
            p.tri((x, y - 4), (x - 2, y), (x + 2, y), light);
            p.tri((x - 2, y), (x + 2, y), (x, y + 4), dark);
        }
    } else {
        for (x, y, s) in [(16, 34, 6), (26, 30, 8), (36, 35, 5), (22, 38, 4)] {
            p.tri((x, y + s), (x + s, y + s), (x + s / 2, y), shade(main, 0.5));
        }
    }
    p.outline(OUTLINE);
    p
}

// ------------------------------------------------------------------ monsters

pub fn monster(kind: MonsterKind, frame: Frame) -> Pix {
    let attack = frame == Frame::Attack;
    let walk = frame == Frame::Walk;
    let mut p;
    match kind {
        MonsterKind::Gromp => {
            p = Pix::new(26, 22);
            let c = 0x6a8a3a;
            p.ellipse(13.0, 13.0, 11.0, 7.5, c);
            p.ellipse(13.0, 15.0, 7.0, 4.0, 0xd8c870);
            p.disc(8.0, 6.0, 3.2, c);
            p.disc(18.0, 6.0, 3.2, c);
            p.disc(8.0, 5.5, 1.8, 0xfff8e0);
            p.disc(18.0, 5.5, 1.8, 0xfff8e0);
            p.set(8, 5, 0x101010);
            p.set(18, 5, 0x101010);
            for (x, y) in [(5, 12), (20, 10), (16, 9), (9, 15)] {
                p.set(x, y, shade(c, 0.7));
            }
            if attack {
                p.rect(9, 10, 8, 2, 0xa02838);
            } else {
                p.rect(10, 10, 6, 1, shade(c, 0.6));
            }
            p.rect(3, 18, 5, 2, shade(c, 0.8));
            p.rect(18, 18, 5, 2, shade(c, 0.8));
        }
        MonsterKind::Wolf | MonsterKind::BigWolf => {
            let big = kind == MonsterKind::BigWolf;
            let (w, h) = if big { (24, 18) } else { (18, 13) };
            p = Pix::new(w, h);
            let c = if big { 0x585868 } else { 0x8a8a98 };
            let s = if big { 1.35 } else { 1.0 };
            p.ellipse(8.0 * s, 7.0 * s, 6.0 * s, 3.5 * s, c);
            p.ellipse(13.5 * s, 4.5 * s, 3.2 * s, 2.8 * s, c);
            p.rect((15.0 * s) as i32, (4.5 * s) as i32, (3.0 * s) as i32, (1.5 * s).max(1.0) as i32, shade(c, 1.2));
            p.set((12.0 * s) as i32, (1.5 * s) as i32, c);
            p.set((14.0 * s) as i32, (1.5 * s) as i32, c);
            p.set((14.0 * s) as i32, (4.0 * s) as i32, if big { 0xff3030 } else { 0xffe060 });
            p.line(2, (5.0 * s) as i32, 0, (2.0 * s) as i32, c);
            let leg = (9.0 * s) as i32;
            let lh = (3.0 * s) as i32;
            let off = if walk { 1 } else { 0 };
            p.rect((4.0 * s) as i32 + off, leg, 2, lh, shade(c, 0.7));
            p.rect((11.0 * s) as i32 - off, leg, 2, lh, shade(c, 0.7));
            if attack {
                p.rect((15.0 * s) as i32, (6.0 * s) as i32, 3, 1, 0xffffff);
            }
        }
        MonsterKind::Raptor | MonsterKind::BigRaptor => {
            let big = kind == MonsterKind::BigRaptor;
            let s = if big { 1.35 } else { 1.0 };
            p = Pix::new((14.0 * s) as usize + 2, (16.0 * s) as usize + 2);
            let c = if big { 0xd84828 } else { 0xe87838 };
            p.ellipse(6.0 * s, 9.0 * s, 4.5 * s, 3.5 * s, c);
            p.ellipse(9.5 * s, 5.0 * s, 2.5 * s, 2.2 * s, c);
            p.rect((11.0 * s) as i32, (5.0 * s) as i32, 3, 1, 0xf0c040);
            p.set((10.0 * s) as i32, (4.0 * s) as i32, 0x101010);
            p.rect((8.0 * s) as i32, (1.5 * s) as i32, 2, 2, 0xf0d040);
            p.line(1, (8.0 * s) as i32, 0, (5.0 * s) as i32, shade(c, 0.8));
            let off = if walk { 1 } else { 0 };
            p.rect((4.0 * s) as i32 + off, (12.0 * s) as i32, 1, (3.0 * s) as i32, 0xf0c040);
            p.rect((7.0 * s) as i32 - off, (12.0 * s) as i32, 1, (3.0 * s) as i32, 0xf0c040);
            if attack {
                p.set((13.0 * s) as i32, (6.0 * s) as i32, 0xffffff);
            }
        }
        MonsterKind::Krug | MonsterKind::BigKrug => {
            let big = kind == MonsterKind::BigKrug;
            let s = if big { 1.5 } else { 1.0 };
            p = Pix::new((16.0 * s) as usize, (16.0 * s) as usize);
            let c = 0x8a6a48;
            p.disc(8.0 * s, 9.0 * s, 6.0 * s, c);
            p.disc(6.0 * s, 7.0 * s, 3.0 * s, shade(c, 1.2));
            p.disc(11.0 * s, 11.0 * s, 2.5 * s, shade(c, 0.75));
            p.rect((4.0 * s) as i32, (4.0 * s) as i32, (4.0 * s) as i32, 1, 0x58a048);
            p.set((10.0 * s) as i32, (7.0 * s) as i32, 0xffa030);
            p.set((12.0 * s) as i32, (7.0 * s) as i32, 0xffa030);
            if attack {
                p.disc(14.0 * s, 9.0 * s, 2.0 * s, shade(c, 0.9));
            }
        }
        MonsterKind::BlueSentinel | MonsterKind::RedBrambleback => {
            let blue = kind == MonsterKind::BlueSentinel;
            p = Pix::new(30, 30);
            let c = if blue { 0x4868b8 } else { 0xa83828 };
            let glow = if blue { 0x70e0ff } else { 0xffb030 };
            p.rect(8, 22, 4, 6, shade(c, 0.6));
            p.rect(18, if walk { 21 } else { 22 }, 4, 6, shade(c, 0.6));
            p.ellipse(15.0, 15.0, 10.0, 9.0, c);
            p.ellipse(15.0, 18.0, 6.0, 4.0, shade(c, 1.25));
            p.disc(15.0, 7.0, 5.0, shade(c, 0.85));
            p.set(13, 6, glow);
            p.set(17, 6, glow);
            p.rect(4, 12, 4, 9, shade(c, 0.8));
            let ax = if attack { 25 } else { 22 };
            p.rect(ax, 12, 4, 9, shade(c, 0.8));
            if blue {
                p.tri((9, 6), (11, 0), (12, 6), glow);
                p.tri((18, 6), (20, 0), (21, 6), glow);
                p.rect(13, 14, 4, 4, glow);
            } else {
                for (x, y) in [(7, 9), (11, 5), (19, 5), (23, 9), (15, 3)] {
                    p.tri((x - 1, y + 2), (x + 1, y + 2), (x, y - 2), 0xe8d0a0);
                }
            }
        }
        MonsterKind::Dragon => {
            p = Pix::new(48, 38);
            let c = 0xe06828;
            let belly = 0xf0c068;
            let wing = 0xa03818;
            let wy = if walk { 2 } else { 0 };
            p.tri((14, 16), (4, 2 + wy), (22, 12), wing);
            p.tri((26, 12), (40, 1 + wy), (32, 18), wing);
            p.ellipse(22.0, 22.0, 12.0, 8.0, c);
            p.ellipse(22.0, 25.0, 8.0, 4.0, belly);
            p.line(10, 22, 2, 30, c);
            p.line(10, 23, 2, 31, c);
            p.tri((1, 30), (4, 28), (3, 34), wing);
            p.ellipse(36.0, 14.0, 6.0, 4.5, c);
            let jaw = if attack { 3 } else { 1 };
            p.rect(40, 15, 6, jaw, shade(c, 0.8));
            p.set(37, 12, 0xfff060);
            p.tri((33, 10), (34, 5), (36, 10), 0xe8dcc0);
            p.rect(15, 28, 4, 6, shade(c, 0.7));
            p.rect(26, 28, 4, 6, shade(c, 0.7));
            if attack {
                p.rect(46, 15, 2, 2, 0xffd040);
            }
        }
        MonsterKind::Baron => {
            p = Pix::new(56, 48);
            let c = 0x7038a0;
            let light = 0xb070e0;
            p.ellipse(28.0, 40.0, 24.0, 7.0, 0x381848);
            p.ellipse(28.0, 26.0, 13.0, 16.0, c);
            p.ellipse(28.0, 28.0, 8.0, 11.0, light);
            for i in 0..5 {
                let y = 18 + i * 5;
                p.rect(20, y, 16, 1, shade(c, 0.7));
            }
            p.ellipse(28.0, 11.0, 10.0, 7.0, c);
            let mouth = if attack { 5 } else { 2 };
            p.rect(22, 12, 12, mouth, 0x200818);
            p.set(23, 12, 0xffffff);
            p.set(32, 12, 0xffffff);
            p.line(21, 7, 12, 1, c);
            p.line(35, 7, 44, 1, c);
            p.disc(12.0, 1.5, 1.8, 0x60ff60);
            p.disc(44.0, 1.5, 1.8, 0x60ff60);
            p.set(25, 8, 0x60ff60);
            p.set(31, 8, 0x60ff60);
            for (x, y) in [(10, 30), (46, 30), (14, 22), (42, 22)] {
                p.tri((x - 2, y + 3), (x + 2, y + 3), (x, y - 4), shade(c, 0.6));
            }
        }
    }
    p.outline(OUTLINE);
    p
}

// ------------------------------------------------------------------ icons

/// 16x16 skill icon: role-coloured background + a symbol for the action.
pub fn skill_icon(sk: &SkillDef) -> Pix {
    let mut p = Pix::new(16, 16);
    let a = shade(sk.roles[0].color(), 0.45);
    let b = shade(sk.roles[1].color(), 0.45);
    for y in 0..16 {
        for x in 0..16 {
            p.set(x, y, if x + y < 16 { a } else { b });
        }
    }
    let c = mix(sk.color, 0xffffff, 0.15);
    let d = shade(sk.color, 0.6);
    match sk.action {
        Action::Projectile { pierce, .. } => {
            p.line(3, 12, 11, 4, c);
            p.line(4, 12, 12, 4, d);
            p.tri((12, 2), (13, 7), (8, 3), c);
            if pierce {
                p.line(2, 9, 6, 5, c);
            }
        }
        Action::Area { at_target, .. } => {
            p.ring(8.0, 8.0, 5.5, c);
            p.ring(8.0, 8.0, 3.0, d);
            p.disc(8.0, 8.0, 1.5, c);
            if at_target {
                p.set(8, 1, c);
                p.set(8, 2, c);
            }
        }
        Action::Cone { .. } => {
            p.tri((2, 13), (13, 3), (13, 13), d);
            p.tri((2, 13), (13, 6), (10, 13), c);
        }
        Action::Line { .. } => {
            p.line(2, 13, 13, 2, c);
            p.line(3, 13, 13, 3, c);
            p.line(2, 12, 12, 2, d);
        }
        Action::Dash { .. } => {
            for o in [0, 5] {
                p.line(2 + o, 3, 7 + o, 8, c);
                p.line(7 + o, 8, 2 + o, 13, c);
            }
        }
        Action::Blink { .. } => {
            p.line(8, 1, 8, 14, c);
            p.line(1, 8, 14, 8, c);
            p.line(4, 4, 11, 11, d);
            p.line(11, 4, 4, 11, d);
            p.disc(8.0, 8.0, 2.0, 0xffffff);
        }
        Action::Unit { .. } => {
            p.ring(8.0, 8.0, 5.0, c);
            p.line(8, 1, 8, 5, c);
            p.line(8, 11, 8, 14, c);
            p.line(1, 8, 5, 8, c);
            p.line(11, 8, 14, 8, c);
        }
        Action::Trap { .. } => {
            p.rect(3, 10, 10, 2, d);
            for x in (3..13).step_by(3) {
                p.tri((x, 10), (x + 2, 10), (x + 1, 5), c);
            }
        }
        Action::None => {
            if sk.self_effects.iter().any(|e| matches!(e, Effect::Shield { .. })) {
                p.rect(4, 3, 8, 6, c);
                p.tri((4, 9), (11, 9), (8, 13), c);
                p.rect(7, 4, 2, 6, d);
            } else {
                p.tri((8, 2), (3, 8), (13, 8), c);
                p.rect(6, 8, 4, 6, c);
            }
        }
    }
    let border = if sk.ultimate { 0xf0c838 } else { 0x101018 };
    for i in 0..16 {
        p.set(i, 0, border);
        p.set(i, 15, border);
        p.set(0, i, border);
        p.set(15, i, border);
    }
    p
}

pub fn item_icon(it: &ItemDef) -> Pix {
    let mut p = Pix::new(14, 14);
    p.rect(0, 0, 14, 14, 0x20202c);
    let c = it.color;
    let d = shade(c, 0.6);
    let l = mix(c, 0xffffff, 0.4);
    match it.glyph {
        ItemGlyph::Potion => {
            p.disc(7.0, 9.0, 4.0, c);
            p.rect(5, 2, 4, 4, 0xd0e0f0);
            p.rect(6, 1, 2, 1, WOOD);
            p.set(5, 8, l);
        }
        ItemGlyph::Sword => {
            p.line(3, 11, 10, 2, c);
            p.line(4, 11, 11, 3, d);
            p.line(2, 8, 6, 12, 0xc0a040);
            p.set(2, 12, WOOD);
        }
        ItemGlyph::Book => {
            p.rect(3, 2, 8, 10, c);
            p.rect(3, 2, 2, 10, d);
            p.rect(6, 5, 4, 1, l);
        }
        ItemGlyph::Gem => {
            p.tri((7, 2), (2, 6), (12, 6), l);
            p.tri((2, 6), (12, 6), (7, 12), c);
        }
        ItemGlyph::Armor => {
            p.rect(3, 3, 8, 8, c);
            p.rect(1, 3, 2, 3, d);
            p.rect(11, 3, 2, 3, d);
            p.rect(5, 3, 4, 1, 0x20202c);
            p.rect(4, 6, 6, 1, l);
        }
        ItemGlyph::Cloak => {
            p.tri((7, 1), (2, 12), (12, 12), c);
            p.line(7, 2, 7, 11, d);
        }
        ItemGlyph::Dagger => {
            p.line(4, 10, 9, 3, c);
            p.set(10, 2, l);
            p.rect(3, 9, 3, 1, 0xc0a040);
            p.set(3, 11, WOOD);
        }
        ItemGlyph::Boots => {
            p.rect(4, 2, 4, 8, c);
            p.rect(4, 9, 8, 3, c);
            p.rect(4, 11, 8, 1, d);
        }
        ItemGlyph::Bow => {
            p.line(4, 1, 9, 6, c);
            p.line(9, 6, 4, 12, c);
            p.line(4, 1, 4, 12, l);
        }
        ItemGlyph::Staff => {
            p.line(3, 12, 9, 4, WOOD);
            p.disc(10.0, 3.0, 2.5, c);
            p.set(9, 2, l);
        }
        ItemGlyph::Belt => {
            p.rect(1, 5, 12, 4, c);
            p.rect(5, 4, 4, 6, 0xd0b050);
            p.rect(6, 5, 2, 4, 0x20202c);
        }
        ItemGlyph::Hat => {
            p.rect(1, 10, 12, 2, d);
            p.tri((3, 10), (11, 10), (8, 1), c);
            p.set(7, 6, 0xfff080);
        }
        ItemGlyph::Heart => {
            p.disc(4.5, 5.0, 2.8, c);
            p.disc(9.5, 5.0, 2.8, c);
            p.tri((2, 6), (12, 6), (7, 12), c);
            p.set(4, 4, l);
        }
        ItemGlyph::Fang => {
            p.tri((3, 2), (7, 2), (5, 12), l);
            p.tri((7, 2), (11, 2), (9, 9), c);
        }
    }
    p
}

pub fn summoner_icon(s: SummonerId) -> Pix {
    let mut p = Pix::new(14, 14);
    p.rect(0, 0, 14, 14, 0x20202c);
    let c = s.color();
    let d = shade(c, 0.6);
    match s {
        SummonerId::Flash => {
            p.tri((8, 1), (3, 8), (8, 8), c);
            p.tri((6, 6), (11, 6), (5, 13), c);
        }
        SummonerId::Heal => {
            p.rect(5, 2, 4, 10, c);
            p.rect(2, 5, 10, 4, c);
        }
        SummonerId::Ghost => {
            p.disc(7.0, 6.0, 4.5, c);
            p.rect(3, 6, 9, 6, c);
            p.set(5, 5, 0x20202c);
            p.set(9, 5, 0x20202c);
            p.set(4, 12, 0x20202c);
            p.set(7, 12, 0x20202c);
            p.set(10, 12, 0x20202c);
        }
        SummonerId::Ignite => {
            p.tri((7, 1), (2, 12), (12, 12), c);
            p.tri((7, 6), (5, 12), (9, 12), 0xfff060);
        }
        SummonerId::Exhaust => {
            p.disc(7.0, 7.0, 5.0, d);
            p.line(3, 3, 11, 11, c);
            p.line(4, 3, 11, 10, c);
        }
        SummonerId::Barrier => {
            p.ring(7.0, 7.0, 5.0, c);
            p.ring(7.0, 7.0, 4.0, d);
            p.disc(7.0, 7.0, 1.5, c);
        }
    }
    p
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CursorKind {
    Normal,
    Attack,
    Ally,
    Target,
}

pub fn cursor(kind: CursorKind) -> Pix {
    let mut p = Pix::new(14, 14);
    match kind {
        CursorKind::Normal | CursorKind::Ally => {
            let c = if kind == CursorKind::Ally { 0x70e870 } else { 0xf0d070 };
            p.tri((1, 1), (1, 11), (8, 8), c);
            p.line(4, 8, 7, 12, c);
            p.line(5, 8, 8, 12, c);
            p.set(2, 3, 0xffffff);
        }
        CursorKind::Attack => {
            p.line(1, 1, 9, 9, 0xff5040);
            p.line(2, 1, 10, 9, 0xd02820);
            p.line(7, 11, 11, 7, 0xc0a040);
            p.rect(10, 10, 2, 2, WOOD);
        }
        CursorKind::Target => {
            p.ring(6.0, 6.0, 4.5, 0xff5040);
            p.line(6, 0, 6, 3, 0xff5040);
            p.line(6, 9, 6, 12, 0xff5040);
            p.line(0, 6, 3, 6, 0xff5040);
            p.line(9, 6, 12, 6, 0xff5040);
        }
    }
    p.outline(OUTLINE);
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use moba_core::heroes::HEROES;
    use moba_core::items::ITEMS;
    use moba_core::skills::SKILLS;

    #[test]
    fn all_art_renders() {
        for h in HEROES {
            for f in [Frame::Idle, Frame::Walk, Frame::Attack] {
                let p = hero(&h.look, f);
                assert!(p.px.iter().filter(|&&x| x != 0).count() > 80, "{} looks empty", h.name);
            }
        }
        for t in [Team::Blue, Team::Red] {
            for k in [MinionKind::Melee, MinionKind::Caster, MinionKind::Siege, MinionKind::Super] {
                minion(k, t, Frame::Walk);
            }
            tower(t, true);
            tower(t, false);
            inhibitor(t, true);
            nexus(t, false);
        }
        for s in SKILLS {
            skill_icon(s);
        }
        for i in ITEMS {
            item_icon(i);
        }
        for s in SummonerId::ALL {
            summoner_icon(s);
        }
        let p = hero(&HEROES[0].look, Frame::Idle);
        let f = p.flipped().flipped();
        assert_eq!(f.px, p.px);
        assert_eq!(p.rgba().len(), HERO_W * HERO_H * 4);
    }
}
