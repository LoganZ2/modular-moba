//! Collects DOM input events into per-frame state.

use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct KeyPress {
    /// `KeyboardEvent.code`, e.g. "KeyQ", "Digit1", "Space".
    pub code: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Default)]
pub struct Input {
    /// Mouse position in internal (pixel-art) coordinates.
    pub mx: f64,
    pub my: f64,
    pub mouse_in: bool,
    pub left: bool,
    pub right: bool,
    pub left_pressed: bool,
    pub right_pressed: bool,
    pub left_released: bool,
    pub right_released: bool,
    pub wheel: f64,
    pub held: HashSet<String>,
    pub pressed: Vec<KeyPress>,
    pub released: Vec<String>,
    /// Printable characters typed this frame (for text fields).
    pub typed: Vec<char>,
    pub backspace: u32,
    /// Set by UI widgets when they consume the click.
    pub click_consumed: bool,
}

impl Input {
    pub fn end_frame(&mut self) {
        self.left_pressed = false;
        self.right_pressed = false;
        self.left_released = false;
        self.right_released = false;
        self.wheel = 0.0;
        self.pressed.clear();
        self.released.clear();
        self.typed.clear();
        self.backspace = 0;
        self.click_consumed = false;
    }

    pub fn key(&self, code: &str) -> bool {
        self.pressed.iter().any(|k| k.code == code)
    }

    /// Key pressed this frame without Ctrl/Alt.
    pub fn plain(&self, code: &str) -> bool {
        self.pressed.iter().any(|k| k.code == code && !k.ctrl && !k.alt)
    }

    /// Key pressed with Ctrl or Alt held (LoL "level up" chords).
    pub fn chord(&self, code: &str) -> bool {
        self.pressed.iter().any(|k| k.code == code && (k.ctrl || k.alt))
    }

    pub fn is_held(&self, code: &str) -> bool {
        self.held.contains(code)
    }

    pub fn was_released(&self, code: &str) -> bool {
        self.released.iter().any(|k| k == code)
    }

    pub fn in_rect(&self, x: f64, y: f64, w: f64, h: f64) -> bool {
        self.mouse_in && self.mx >= x && self.my >= y && self.mx < x + w && self.my < y + h
    }

    /// Left click inside a rect (and mark it consumed).
    pub fn clicked(&mut self, x: f64, y: f64, w: f64, h: f64) -> bool {
        if self.left_pressed && !self.click_consumed && self.in_rect(x, y, w, h) {
            self.click_consumed = true;
            true
        } else {
            false
        }
    }

    pub fn right_clicked(&mut self, x: f64, y: f64, w: f64, h: f64) -> bool {
        if self.right_pressed && !self.click_consumed && self.in_rect(x, y, w, h) {
            self.click_consumed = true;
            true
        } else {
            false
        }
    }
}
