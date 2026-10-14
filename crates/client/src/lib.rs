//! Modular MOBA browser client (Rust -> WebAssembly).
//!
//! Everything is drawn on one low-resolution canvas that CSS scales up with
//! `image-rendering: pixelated`, so the whole game — world, HUD and text —
//! shares the same chunky pixel grid.

pub mod app;
pub mod art;
pub mod font;
pub mod game;
pub mod gfx;
pub mod hud;
pub mod input;
pub mod net;
pub mod render;
pub mod terrain;
pub mod ui;

use input::{Input, KeyPress};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};

fn window() -> web_sys::Window {
    web_sys::window().expect("window")
}

fn request_animation_frame(f: &Closure<dyn FnMut(f64)>) {
    let _ = window().request_animation_frame(f.as_ref().unchecked_ref());
}

/// Keys whose browser default (scrolling, focus change, ...) we suppress.
fn should_prevent(code: &str, ctrl: bool, alt: bool) -> bool {
    matches!(code, "Tab" | "Space" | "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "Backspace" | "F1")
        || ((ctrl || alt) && matches!(code, "KeyQ" | "KeyW" | "KeyE" | "KeyR" | "KeyD" | "KeyF" | "KeyS" | "KeyP"))
        || (alt && code.starts_with("Key"))
}

fn install_listeners(canvas: &HtmlCanvasElement, input: Rc<RefCell<Input>>) {
    let to_internal = {
        let canvas = canvas.clone();
        move |ev: &MouseEvent| -> (f64, f64) {
            let r = canvas.get_bounding_client_rect();
            let sx = canvas.width() as f64 / r.width().max(1.0);
            let sy = canvas.height() as f64 / r.height().max(1.0);
            ((ev.client_x() as f64 - r.left()) * sx, (ev.client_y() as f64 - r.top()) * sy)
        }
    };
    let add = |target: &web_sys::EventTarget, name: &str, f: Closure<dyn FnMut(JsValue)>| {
        let _ = target.add_event_listener_with_callback(name, f.as_ref().unchecked_ref());
        f.forget();
    };
    let win: web_sys::EventTarget = window().into();
    let cv: web_sys::EventTarget = canvas.clone().into();
    {
        let input = input.clone();
        let conv = to_internal.clone();
        add(
            &win,
            "mousemove",
            Closure::new(move |e: JsValue| {
                let e: MouseEvent = e.unchecked_into();
                let (x, y) = conv(&e);
                let mut i = input.borrow_mut();
                i.mx = x;
                i.my = y;
                i.mouse_in = true;
            }),
        );
    }
    {
        let input = input.clone();
        let conv = to_internal.clone();
        add(
            &cv,
            "mousedown",
            Closure::new(move |e: JsValue| {
                let e: MouseEvent = e.unchecked_into();
                e.prevent_default();
                let (x, y) = conv(&e);
                let mut i = input.borrow_mut();
                i.mx = x;
                i.my = y;
                i.mouse_in = true;
                match e.button() {
                    0 => {
                        i.left = true;
                        i.left_pressed = true;
                    }
                    2 => {
                        i.right = true;
                        i.right_pressed = true;
                    }
                    _ => {}
                }
            }),
        );
    }
    {
        let input = input.clone();
        add(
            &win,
            "mouseup",
            Closure::new(move |e: JsValue| {
                let e: MouseEvent = e.unchecked_into();
                let mut i = input.borrow_mut();
                match e.button() {
                    0 => {
                        i.left = false;
                        i.left_released = true;
                    }
                    2 => {
                        i.right = false;
                        i.right_released = true;
                    }
                    _ => {}
                }
            }),
        );
    }
    add(
        &cv,
        "contextmenu",
        Closure::new(move |e: JsValue| {
            let e: web_sys::Event = e.unchecked_into();
            e.prevent_default();
        }),
    );
    {
        let input = input.clone();
        add(
            &win,
            "mouseout",
            Closure::new(move |e: JsValue| {
                let e: MouseEvent = e.unchecked_into();
                if e.related_target().is_none() {
                    input.borrow_mut().mouse_in = false;
                }
            }),
        );
    }
    {
        let input = input.clone();
        add(
            &cv,
            "wheel",
            Closure::new(move |e: JsValue| {
                let e: WheelEvent = e.unchecked_into();
                e.prevent_default();
                input.borrow_mut().wheel += e.delta_y();
            }),
        );
    }
    {
        let input = input.clone();
        add(
            &win,
            "keydown",
            Closure::new(move |e: JsValue| {
                let e: KeyboardEvent = e.unchecked_into();
                let code = e.code();
                if should_prevent(&code, e.ctrl_key(), e.alt_key()) {
                    e.prevent_default();
                }
                let mut i = input.borrow_mut();
                let key = e.key();
                if key == "Backspace" {
                    i.backspace += 1;
                } else if key.chars().count() == 1 && !e.ctrl_key() && !e.alt_key() && !e.meta_key() {
                    i.typed.push(key.chars().next().unwrap());
                }
                if e.repeat() {
                    return;
                }
                i.held.insert(code.clone());
                i.pressed.push(KeyPress {
                    code,
                    ctrl: e.ctrl_key() || e.meta_key(),
                    alt: e.alt_key(),
                    shift: e.shift_key(),
                });
            }),
        );
    }
    {
        let input = input.clone();
        add(
            &win,
            "keyup",
            Closure::new(move |e: JsValue| {
                let e: KeyboardEvent = e.unchecked_into();
                let code = e.code();
                if should_prevent(&code, e.ctrl_key(), e.alt_key()) {
                    e.prevent_default();
                }
                let mut i = input.borrow_mut();
                i.held.remove(&code);
                i.released.push(code);
            }),
        );
    }
    {
        let input = input.clone();
        add(
            &win,
            "blur",
            Closure::new(move |_e: JsValue| {
                let mut i = input.borrow_mut();
                let held: Vec<String> = i.held.drain().collect();
                i.released.extend(held);
                i.left = false;
                i.right = false;
            }),
        );
    }
}

/// Entry point, called automatically when the wasm module loads.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let doc = gfx::document();
    let canvas: HtmlCanvasElement = match doc.get_element_by_id("game") {
        Some(el) => el.dyn_into()?,
        None => {
            let c: HtmlCanvasElement = doc.create_element("canvas")?.dyn_into()?;
            doc.body().unwrap().append_child(&c)?;
            c
        }
    };
    if let Some(loading) = doc.get_element_by_id("loading") {
        loading.remove();
    }
    let input = Rc::new(RefCell::new(Input::default()));
    install_listeners(&canvas, input.clone());
    let app = Rc::new(RefCell::new(app::App::new(canvas, input)));

    type FrameLoop = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;
    let f: FrameLoop = Rc::new(RefCell::new(None));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::new(move |t: f64| {
        app.borrow_mut().frame(t);
        request_animation_frame(f.borrow().as_ref().unwrap());
    }));
    request_animation_frame(g.borrow().as_ref().unwrap());
    Ok(())
}
