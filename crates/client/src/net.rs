//! WebSocket connection to the game server.

use moba_core::protocol::{decode, encode, ClientMsg, ServerMsg};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, MessageEvent, WebSocket};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetState {
    Connecting,
    Open,
    Closed,
}

pub struct Net {
    ws: WebSocket,
    inbox: Rc<RefCell<VecDeque<ServerMsg>>>,
    state: Rc<Cell<NetState>>,
    _handlers: Vec<Closure<dyn FnMut(JsValue)>>,
}

/// `ws://host/ws` (or `wss://` when the page is served over https).
pub fn default_url() -> Option<String> {
    let loc = web_sys::window()?.location();
    let protocol = loc.protocol().ok()?;
    if protocol != "http:" && protocol != "https:" {
        return None;
    }
    let host = loc.host().ok()?;
    let scheme = if protocol == "https:" { "wss" } else { "ws" };
    Some(format!("{scheme}://{host}/ws"))
}

impl Net {
    pub fn connect(url: &str, hello: ClientMsg) -> Result<Net, JsValue> {
        let ws = WebSocket::new(url)?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        let inbox: Rc<RefCell<VecDeque<ServerMsg>>> = Rc::new(RefCell::new(VecDeque::new()));
        let state = Rc::new(Cell::new(NetState::Connecting));
        let mut handlers = Vec::new();

        {
            let ws2 = ws.clone();
            let state = state.clone();
            let hello = encode(&hello);
            let on_open = Closure::<dyn FnMut(JsValue)>::new(move |_| {
                state.set(NetState::Open);
                let _ = ws2.send_with_u8_array(&hello);
            });
            ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
            handlers.push(on_open);
        }
        {
            let inbox = inbox.clone();
            let on_msg = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
                let ev: MessageEvent = ev.unchecked_into();
                if let Ok(buf) = ev.data().dyn_into::<js_sys::ArrayBuffer>() {
                    let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                    if let Some(msg) = decode::<ServerMsg>(&bytes) {
                        inbox.borrow_mut().push_back(msg);
                    }
                }
            });
            ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
            handlers.push(on_msg);
        }
        for setter in [0, 1] {
            let state = state.clone();
            let h = Closure::<dyn FnMut(JsValue)>::new(move |_| state.set(NetState::Closed));
            if setter == 0 {
                ws.set_onclose(Some(h.as_ref().unchecked_ref()));
            } else {
                ws.set_onerror(Some(h.as_ref().unchecked_ref()));
            }
            handlers.push(h);
        }
        Ok(Net { ws, inbox, state, _handlers: handlers })
    }

    pub fn state(&self) -> NetState {
        self.state.get()
    }

    pub fn send(&self, msg: &ClientMsg) {
        if self.state.get() == NetState::Open {
            let _ = self.ws.send_with_u8_array(&encode(msg));
        }
    }

    pub fn poll(&self) -> Vec<ServerMsg> {
        self.inbox.borrow_mut().drain(..).collect()
    }

    pub fn close(&self) {
        let _ = self.ws.close();
        self.state.set(NetState::Closed);
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onclose(None);
        self.ws.set_onerror(None);
        let _ = self.ws.close();
    }
}
