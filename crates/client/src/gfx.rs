//! Thin pixel-oriented wrapper around a 2D canvas: solid rectangles, pixel
//! lines and circles, the bitmap font and a lazily built sprite cache.

use crate::art::{self, CursorKind, Frame, Pix};
use crate::font;
use moba_core::heroes::{hero, HeroId};
use moba_core::items::{item, ItemId};
use moba_core::skills::{skill, SkillId};
use moba_core::summoners::SummonerId;
use moba_core::units::{MinionKind, MonsterKind};
use moba_core::Team;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

pub fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

pub fn make_canvas(w: u32, h: u32) -> (HtmlCanvasElement, CanvasRenderingContext2d) {
    let c: HtmlCanvasElement = document().create_element("canvas").unwrap().dyn_into().unwrap();
    c.set_width(w.max(1));
    c.set_height(h.max(1));
    let ctx: CanvasRenderingContext2d = c.get_context("2d").unwrap().unwrap().dyn_into().unwrap();
    ctx.set_image_smoothing_enabled(false);
    (c, ctx)
}

pub fn canvas_from_rgba(w: usize, h: usize, rgba: &[u8]) -> HtmlCanvasElement {
    let (c, ctx) = make_canvas(w as u32, h as u32);
    if let Ok(img) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(rgba), w as u32, h as u32) {
        let _ = ctx.put_image_data(&img, 0.0, 0.0);
    }
    c
}

pub fn css(c: u32) -> String {
    format!("#{:06x}", c & 0xff_ffff)
}

pub struct Sprite {
    pub canvas: HtmlCanvasElement,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpriteKey {
    Hero(HeroId, Frame, bool),
    Minion(MinionKind, Team, Frame, bool),
    Tower(Team, bool),
    Inhibitor(Team, bool),
    Nexus(Team, bool),
    Monster(MonsterKind, Frame, bool),
    Skill(SkillId),
    Item(ItemId),
    Summoner(SummonerId),
    Cursor(CursorKind),
}

fn build_sprite(key: SpriteKey) -> Pix {
    let flip = |p: Pix, f: bool| if f { p.flipped() } else { p };
    match key {
        SpriteKey::Hero(id, frame, f) => flip(art::hero(&hero(id).look, frame), f),
        SpriteKey::Minion(k, t, frame, f) => flip(art::minion(k, t, frame), f),
        SpriteKey::Tower(t, alive) => art::tower(t, alive),
        SpriteKey::Inhibitor(t, alive) => art::inhibitor(t, alive),
        SpriteKey::Nexus(t, alive) => art::nexus(t, alive),
        SpriteKey::Monster(k, frame, f) => flip(art::monster(k, frame), f),
        SpriteKey::Skill(id) => art::skill_icon(skill(id)),
        SpriteKey::Item(id) => art::item_icon(item(id)),
        SpriteKey::Summoner(s) => art::summoner_icon(s),
        SpriteKey::Cursor(k) => art::cursor(k),
    }
}

pub struct Gfx {
    pub canvas: HtmlCanvasElement,
    pub ctx: CanvasRenderingContext2d,
    /// Internal (pixel art) resolution.
    pub w: f64,
    pub h: f64,
    /// CSS pixels per internal pixel.
    pub scale: f64,
    fill: Cell<u32>,
    alpha: Cell<f64>,
    fonts: RefCell<HashMap<u32, HtmlCanvasElement>>,
    sprites: RefCell<HashMap<SpriteKey, Rc<Sprite>>>,
}

impl Gfx {
    pub fn new(canvas: HtmlCanvasElement) -> Gfx {
        let ctx: CanvasRenderingContext2d = canvas.get_context("2d").unwrap().unwrap().dyn_into().unwrap();
        let mut g = Gfx {
            canvas,
            ctx,
            w: 640.0,
            h: 360.0,
            scale: 2.0,
            fill: Cell::new(u32::MAX),
            alpha: Cell::new(1.0),
            fonts: RefCell::new(HashMap::new()),
            sprites: RefCell::new(HashMap::new()),
        };
        g.resize();
        g
    }

    /// Pick an integer scale so the internal resolution stays near 640x360.
    pub fn resize(&mut self) {
        let win = web_sys::window().unwrap();
        let ww = win.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1280.0);
        let wh = win.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(720.0);
        let scale = (wh / 340.0).min(ww / 560.0).floor().max(1.0);
        let w = (ww / scale).floor();
        let h = (wh / scale).floor();
        if w != self.w || h != self.h || scale != self.scale || self.canvas.width() != w as u32 {
            self.w = w;
            self.h = h;
            self.scale = scale;
            self.canvas.set_width(w as u32);
            self.canvas.set_height(h as u32);
            let style = self.canvas.style();
            let _ = style.set_property("width", &format!("{}px", w * scale));
            let _ = style.set_property("height", &format!("{}px", h * scale));
            self.ctx.set_image_smoothing_enabled(false);
            self.fill.set(u32::MAX);
            self.alpha.set(1.0);
        }
    }

    pub fn set_alpha(&self, a: f64) {
        let a = a.clamp(0.0, 1.0);
        if (self.alpha.get() - a).abs() > 1e-3 {
            self.ctx.set_global_alpha(a);
            self.alpha.set(a);
        }
    }

    fn set_fill(&self, c: u32) {
        if self.fill.get() != c {
            self.ctx.set_fill_style_str(&css(c));
            self.fill.set(c);
        }
    }

    pub fn clear(&self, c: u32) {
        self.set_alpha(1.0);
        self.rect(0.0, 0.0, self.w, self.h, c);
    }

    pub fn rect(&self, x: f64, y: f64, w: f64, h: f64, c: u32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.set_fill(c);
        self.ctx.fill_rect(x.floor(), y.floor(), w.round(), h.round());
    }

    pub fn rect_a(&self, x: f64, y: f64, w: f64, h: f64, c: u32, a: f64) {
        let old = self.alpha.get();
        self.set_alpha(old * a);
        self.rect(x, y, w, h, c);
        self.set_alpha(old);
    }

    /// 1px rectangle outline.
    pub fn frame(&self, x: f64, y: f64, w: f64, h: f64, c: u32) {
        let (x, y, w, h) = (x.floor(), y.floor(), w.round(), h.round());
        self.rect(x, y, w, 1.0, c);
        self.rect(x, y + h - 1.0, w, 1.0, c);
        self.rect(x, y, 1.0, h, c);
        self.rect(x + w - 1.0, y, 1.0, h, c);
    }

    /// Bevelled pixel panel.
    pub fn panel(&self, x: f64, y: f64, w: f64, h: f64) {
        self.rect_a(x, y, w, h, 0x12101c, 0.92);
        self.frame(x, y, w, h, 0x5a5070);
        self.rect(x + 1.0, y + 1.0, w - 2.0, 1.0, 0x8a80a8);
    }

    /// Pixel line (Bresenham). Long lines are drawn with fewer, longer rects.
    pub fn line(&self, x0: f64, y0: f64, x1: f64, y1: f64, c: u32) {
        self.set_fill(c);
        let (mut x, mut y) = (x0.floor() as i64, y0.floor() as i64);
        let (x1, y1) = (x1.floor() as i64, y1.floor() as i64);
        let dx = (x1 - x).abs();
        let dy = -(y1 - y).abs();
        if dx.max(-dy) > 4000 {
            return;
        }
        let sx = if x < x1 { 1 } else { -1 };
        let sy = if y < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.ctx.fill_rect(x as f64, y as f64, 1.0, 1.0);
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

    /// Thick line made of a stroked path (used for beams).
    pub fn thick_line(&self, x0: f64, y0: f64, x1: f64, y1: f64, width: f64, c: u32) {
        self.ctx.set_stroke_style_str(&css(c));
        self.ctx.set_line_width(width);
        self.ctx.begin_path();
        self.ctx.move_to(x0, y0);
        self.ctx.line_to(x1, y1);
        self.ctx.stroke();
    }

    /// Pixel circle outline.
    pub fn ring(&self, cx: f64, cy: f64, r: f64, c: u32) {
        if r < 1.0 {
            return;
        }
        self.set_fill(c);
        let (cx, cy) = (cx.floor(), cy.floor());
        let r = r.round() as i64;
        let (mut x, mut y, mut err) = (r, 0i64, 1 - r);
        while x >= y {
            for (px, py) in [(x, y), (y, x), (-y, x), (-x, y), (-x, -y), (-y, -x), (y, -x), (x, -y)] {
                self.ctx.fill_rect(cx + px as f64, cy + py as f64, 1.0, 1.0);
            }
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }

    /// Filled pixel circle (scanlines), optionally squashed vertically.
    pub fn ellipse(&self, cx: f64, cy: f64, rx: f64, ry: f64, c: u32) {
        if rx < 0.5 || ry < 0.5 {
            return;
        }
        self.set_fill(c);
        let ry_i = ry.round() as i64;
        for dy in -ry_i..=ry_i {
            let t = dy as f64 / ry;
            let half = (rx * (1.0 - t * t).max(0.0).sqrt()).round();
            if half <= 0.0 {
                continue;
            }
            self.ctx.fill_rect((cx - half).floor(), (cy + dy as f64).floor(), half * 2.0, 1.0);
        }
    }

    pub fn disc(&self, cx: f64, cy: f64, r: f64, c: u32) {
        self.ellipse(cx, cy, r, r, c);
    }

    // -------------------------------------------------------------- text

    fn font_atlas(&self, c: u32) -> HtmlCanvasElement {
        if let Some(a) = self.fonts.borrow().get(&c) {
            return a.clone();
        }
        let (w, h, px) = font::atlas_rgba(c);
        let canvas = canvas_from_rgba(w, h, &px);
        self.fonts.borrow_mut().insert(c, canvas.clone());
        canvas
    }

    /// Draw text at integer scale `k`; returns its width.
    pub fn text_k(&self, s: &str, x: f64, y: f64, c: u32, k: f64) -> f64 {
        let atlas = self.font_atlas(c);
        let mut cx = x.floor();
        let y = y.floor();
        for ch in s.chars() {
            if ch != ' ' {
                let gi = font::glyph_index(ch) as f64;
                let _ = self.ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
                    &atlas,
                    gi * 6.0,
                    0.0,
                    6.0,
                    8.0,
                    cx,
                    y,
                    6.0 * k,
                    8.0 * k,
                );
            }
            cx += font::ADVANCE * k;
        }
        font::text_width(s) * k
    }

    pub fn text(&self, s: &str, x: f64, y: f64, c: u32) -> f64 {
        self.text_k(s, x, y, c, 1.0)
    }

    /// Text with a 1px dark drop shadow (readable on the map).
    pub fn text_s(&self, s: &str, x: f64, y: f64, c: u32) -> f64 {
        self.text_k(s, x + 1.0, y + 1.0, 0x0a0810, 1.0);
        self.text_k(s, x, y, c, 1.0)
    }

    pub fn text_sk(&self, s: &str, x: f64, y: f64, c: u32, k: f64) -> f64 {
        self.text_k(s, x + k, y + k, 0x0a0810, k);
        self.text_k(s, x, y, c, k)
    }

    pub fn text_center(&self, s: &str, cx: f64, y: f64, c: u32, k: f64, shadow: bool) {
        let w = font::text_width(s) * k;
        if shadow {
            self.text_sk(s, cx - w / 2.0, y, c, k);
        } else {
            self.text_k(s, cx - w / 2.0, y, c, k);
        }
    }

    pub fn text_right(&self, s: &str, rx: f64, y: f64, c: u32) {
        let w = font::text_width(s);
        self.text_s(s, rx - w, y, c);
    }

    // -------------------------------------------------------------- sprites

    pub fn sprite(&self, key: SpriteKey) -> Rc<Sprite> {
        if let Some(s) = self.sprites.borrow().get(&key) {
            return s.clone();
        }
        let pix = build_sprite(key);
        let s =
            Rc::new(Sprite { canvas: canvas_from_rgba(pix.w, pix.h, &pix.rgba()), w: pix.w as f64, h: pix.h as f64 });
        self.sprites.borrow_mut().insert(key, s.clone());
        s
    }

    pub fn draw(&self, canvas: &HtmlCanvasElement, x: f64, y: f64) {
        let _ = self.ctx.draw_image_with_html_canvas_element(canvas, x.floor(), y.floor());
    }

    pub fn draw_scaled(&self, canvas: &HtmlCanvasElement, x: f64, y: f64, w: f64, h: f64) {
        let _ = self.ctx.draw_image_with_html_canvas_element_and_dw_and_dh(canvas, x.floor(), y.floor(), w, h);
    }

    pub fn draw_sprite(&self, key: SpriteKey, x: f64, y: f64) {
        let s = self.sprite(key);
        self.draw(&s.canvas, x, y);
    }

    pub fn draw_sprite_k(&self, key: SpriteKey, x: f64, y: f64, k: f64) {
        let s = self.sprite(key);
        self.draw_scaled(&s.canvas, x, y, s.w * k, s.h * k);
    }

    /// Draw a canvas with smoothing (fog of war).
    pub fn draw_smooth(&self, canvas: &HtmlCanvasElement, x: f64, y: f64, w: f64, h: f64) {
        self.ctx.set_image_smoothing_enabled(true);
        let _ = self.ctx.draw_image_with_html_canvas_element_and_dw_and_dh(canvas, x, y, w, h);
        self.ctx.set_image_smoothing_enabled(false);
    }
}
