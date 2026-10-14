//! Modular MOBA game server.
//!
//! * `GET /ws`  — WebSocket endpoint (binary postcard frames, see `moba_core::protocol`)
//! * `GET /*`   — serves the built browser client from the `web/` directory
//!
//! Environment: `PORT` (default 8080), `MOBA_WEB_DIR` (default `./web`).

mod room;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use moba_core::protocol::{decode, encode, ClientMsg, ServerMsg, PROTOCOL_VERSION};
use moba_core::TICK_RATE;
use room::{Room, RoomEvent};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tower_http::services::ServeDir;

#[derive(Clone, Default)]
struct AppState {
    rooms: Arc<Mutex<HashMap<String, UnboundedSender<RoomEvent>>>>,
    next_id: Arc<AtomicU32>,
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args()
        .skip_while(|a| a != "--port")
        .nth(1)
        .or_else(|| std::env::var("PORT").ok())
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let web_dir = find_web_dir();
    if !web_dir.join("pkg").exists() {
        eprintln!(
            "warning: {} has no pkg/ directory — build the client first with scripts/build-client.sh",
            web_dir.display()
        );
    }
    let state = AppState { next_id: Arc::new(AtomicU32::new(1)), ..Default::default() };
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(|| async { "ok" }))
        .fallback_service(ServeDir::new(&web_dir))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.expect("bind port");
    println!("Modular MOBA server on http://localhost:{port}  (serving {})", web_dir.display());
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server");
}

fn find_web_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MOBA_WEB_DIR") {
        return PathBuf::from(dir);
    }
    let local = PathBuf::from("web");
    if local.join("index.html").exists() {
        return local;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web")
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.max_message_size(64 * 1024).on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sink, mut stream) = socket.split();

    // The first frame must be a Hello.
    let (name, room_name) = loop {
        match stream.next().await {
            Some(Ok(Message::Binary(b))) => match decode::<ClientMsg>(&b) {
                Some(ClientMsg::Hello { name, room, version }) => {
                    if version != PROTOCOL_VERSION {
                        let msg = ServerMsg::Notice(format!(
                            "Version mismatch (server {PROTOCOL_VERSION}, client {version}). Please reload."
                        ));
                        let _ = sink.send(Message::Binary(encode(&msg).into())).await;
                        return;
                    }
                    break (name, room);
                }
                _ => return,
            },
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
            Some(Ok(_)) => continue,
        }
    };
    let room_name = sanitize_room(&room_name);
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let room = join_room(&state, &room_name, RoomEvent::Join { id, name, tx });

    let writer = tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if sink.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Binary(b) => {
                if let Some(msg) = decode::<ClientMsg>(&b) {
                    if room.send(RoomEvent::Msg { id, msg }).is_err() {
                        break;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    let _ = room.send(RoomEvent::Leave { id });
    writer.abort();
}

fn sanitize_room(name: &str) -> String {
    let s: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(24).collect();
    if s.is_empty() {
        "lobby".to_string()
    } else {
        s.to_lowercase()
    }
}

/// Send `join` to the room, creating the room task if needed. The registry
/// lock is held while sending so a room can't shut down underneath us.
fn join_room(state: &AppState, name: &str, join: RoomEvent) -> UnboundedSender<RoomEvent> {
    let mut rooms = state.rooms.lock().unwrap();
    if let Some(tx) = rooms.get(name) {
        if !tx.is_closed() {
            // `join` is moved only if the send succeeds; on failure we fall
            // through and create a fresh room with the returned event.
            match tx.send(join) {
                Ok(()) => return tx.clone(),
                Err(mpsc::error::SendError(ev)) => return spawn_room(state, &mut rooms, name, ev),
            }
        }
    }
    spawn_room(state, &mut rooms, name, join)
}

fn spawn_room(
    state: &AppState,
    rooms: &mut HashMap<String, UnboundedSender<RoomEvent>>,
    name: &str,
    first: RoomEvent,
) -> UnboundedSender<RoomEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    tx.send(first).ok();
    rooms.insert(name.to_string(), tx.clone());
    let seed =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7);
    let registry = state.rooms.clone();
    let name = name.to_string();
    println!("[{name}] room created");
    tokio::spawn(run_room(Room::new(name, seed), rx, registry));
    tx
}

async fn run_room(
    mut room: Room,
    mut rx: UnboundedReceiver<RoomEvent>,
    registry: Arc<Mutex<HashMap<String, UnboundedSender<RoomEvent>>>>,
) {
    let mut ticker = tokio::time::interval(Duration::from_micros(1_000_000 / TICK_RATE as u64));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Some(ev) => room.handle(ev),
                None => break,
            },
            _ = ticker.tick(), if room.in_game() => room.tick(),
        }
        if room.is_empty() {
            let mut rooms = registry.lock().unwrap();
            if rx.is_empty() {
                rooms.remove(&room.name);
                println!("[{}] room closed", room.name);
                break;
            }
        }
    }
}
