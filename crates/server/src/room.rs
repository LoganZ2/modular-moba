//! A room = one lobby (ten seats) plus, while playing, one authoritative
//! [`World`]. Rooms are plain synchronous state machines; `main.rs` drives
//! them from a tokio task, which keeps this file easy to unit test.

use moba_core::heroes::{HeroId, HEROES};
use moba_core::loadout::Loadout;
use moba_core::protocol::{
    encode, ClientMsg, Command, GameStart, LobbyPlayer, LobbyState, PlayerInfo, Seat, ServerMsg, TEAM_SIZE,
};
use moba_core::sim::bot::BotBrain;
use moba_core::sim::{PlayerSetup, World};
use moba_core::{Team, TICK_RATE};
use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;

/// Seconds the scoreboard stays up after a nexus falls before the room
/// returns to the lobby automatically.
const POST_GAME_SECONDS: f32 = 30.0;

pub enum RoomEvent {
    Join { id: u32, name: String, tx: UnboundedSender<Vec<u8>> },
    Leave { id: u32 },
    Msg { id: u32, msg: ClientMsg },
}

struct Member {
    id: u32,
    name: String,
    tx: UnboundedSender<Vec<u8>>,
    loadout: Loadout,
    seat: Option<(Team, u8)>,
}

struct Game {
    world: World,
    /// member id -> player index in the world
    players: HashMap<u32, usize>,
    pending: Vec<(usize, Command)>,
    infos: Vec<PlayerInfo>,
    post_game: f32,
}

pub struct Room {
    pub name: String,
    members: Vec<Member>,
    host: u32,
    game: Option<Game>,
    seed: u64,
}

impl Room {
    pub fn new(name: String, seed: u64) -> Room {
        Room { name, members: Vec::new(), host: 0, game: None, seed }
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn in_game(&self) -> bool {
        self.game.is_some()
    }

    fn send(&self, id: u32, msg: &ServerMsg) {
        if let Some(m) = self.members.iter().find(|m| m.id == id) {
            let _ = m.tx.send(encode(msg));
        }
    }

    fn broadcast(&self, msg: &ServerMsg) {
        let bytes = encode(msg);
        for m in &self.members {
            let _ = m.tx.send(bytes.clone());
        }
    }

    fn seat_taken(&self, team: Team, index: u8) -> bool {
        self.members.iter().any(|m| m.seat == Some((team, index)))
    }

    /// First free seat on the emptier team.
    fn free_seat(&self) -> Option<(Team, u8)> {
        let count = |t: Team| self.members.iter().filter(|m| m.seat.map(|s| s.0) == Some(t)).count();
        let mut teams = [Team::Blue, Team::Red];
        if count(Team::Red) < count(Team::Blue) {
            teams.swap(0, 1);
        }
        for t in teams {
            for i in 0..TEAM_SIZE as u8 {
                if !self.seat_taken(t, i) {
                    return Some((t, i));
                }
            }
        }
        None
    }

    pub fn lobby_state(&self) -> LobbyState {
        let mut seats = Vec::new();
        for team in Team::PLAYABLE {
            for index in 0..TEAM_SIZE as u8 {
                let player = self.members.iter().find(|m| m.seat == Some((team, index))).map(|m| LobbyPlayer {
                    id: m.id,
                    name: m.name.clone(),
                    loadout: m.loadout,
                });
                seats.push(Seat { team, index, player });
            }
        }
        let spectators = self
            .members
            .iter()
            .filter(|m| m.seat.is_none())
            .map(|m| LobbyPlayer { id: m.id, name: m.name.clone(), loadout: m.loadout })
            .collect();
        LobbyState { room: self.name.clone(), host: self.host, seats, spectators, in_game: self.game.is_some() }
    }

    fn broadcast_lobby(&self) {
        self.broadcast(&ServerMsg::Lobby(self.lobby_state()));
    }

    pub fn handle(&mut self, ev: RoomEvent) {
        match ev {
            RoomEvent::Join { id, name, tx } => self.join(id, name, tx),
            RoomEvent::Leave { id } => self.leave(id),
            RoomEvent::Msg { id, msg } => self.message(id, msg),
        }
    }

    fn join(&mut self, id: u32, name: String, tx: UnboundedSender<Vec<u8>>) {
        let name = sanitize_name(&name, id);
        let seat = if self.game.is_none() { self.free_seat() } else { None };
        let hero = HeroId((id as usize % HEROES.len()) as u8);
        self.members.push(Member { id, name: name.clone(), tx, loadout: Loadout::suggested(hero), seat });
        if self.members.len() == 1 {
            self.host = id;
        }
        println!("[{}] {name} (#{id}) joined", self.name);
        self.send(id, &ServerMsg::Welcome { player_id: id });
        self.broadcast_lobby();
        if let Some(g) = &self.game {
            // Late joiners spectate the running match.
            let start = GameStart { players: g.infos.clone(), you: None };
            self.send(id, &ServerMsg::GameStart(start));
        }
    }

    fn leave(&mut self, id: u32) {
        let Some(pos) = self.members.iter().position(|m| m.id == id) else {
            return;
        };
        let m = self.members.remove(pos);
        println!("[{}] {} (#{id}) left", self.name, m.name);
        if let Some(g) = &mut self.game {
            // A bot takes over the abandoned champion.
            if let Some(p) = g.players.remove(&id) {
                let role = g.world.players[p].role;
                g.world.players[p].bot = Some(BotBrain::new(role, p));
                g.world.players[p].name = format!("{} (bot)", g.world.players[p].name);
            }
            if g.players.is_empty() {
                self.game = None;
            }
        }
        if self.host == id {
            self.host = self.members.first().map(|m| m.id).unwrap_or(0);
        }
        if !self.members.is_empty() {
            self.broadcast_lobby();
        }
    }

    fn message(&mut self, id: u32, msg: ClientMsg) {
        match msg {
            ClientMsg::Hello { .. } => {}
            ClientMsg::Ping(n) => self.send(id, &ServerMsg::Pong(n)),
            ClientMsg::TakeSeat { team, index } => {
                if self.game.is_some()
                    || team == Team::Neutral
                    || index as usize >= TEAM_SIZE
                    || self.seat_taken(team, index)
                {
                    return;
                }
                if let Some(m) = self.members.iter_mut().find(|m| m.id == id) {
                    m.seat = Some((team, index));
                }
                self.broadcast_lobby();
            }
            ClientMsg::SetLoadout(l) => {
                if let Err(e) = l.validate() {
                    self.send(id, &ServerMsg::Notice(format!("Invalid loadout: {e}")));
                }
                if let Some(m) = self.members.iter_mut().find(|m| m.id == id) {
                    m.loadout = l.sanitized();
                }
                self.broadcast_lobby();
            }
            ClientMsg::StartGame => {
                if id != self.host {
                    self.send(id, &ServerMsg::Notice("Only the host can start the game".into()));
                } else if self.game.is_none() {
                    self.start_game();
                }
            }
            ClientMsg::Command(c) => {
                if let Some(g) = &mut self.game {
                    if let Some(&p) = g.players.get(&id) {
                        if g.pending.len() < 4096 {
                            g.pending.push((p, c));
                        }
                    }
                }
            }
            ClientMsg::ReturnToLobby => {
                if self.game.as_ref().map(|g| g.world.winner.is_some()).unwrap_or(false) {
                    self.game = None;
                    self.broadcast_lobby();
                }
            }
        }
    }

    pub fn start_game(&mut self) {
        let mut setups = Vec::new();
        let mut owners: Vec<Option<u32>> = Vec::new();
        let mut used: Vec<HeroId> = self.members.iter().filter(|m| m.seat.is_some()).map(|m| m.loadout.hero).collect();
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut pick = self.seed;
        for team in Team::PLAYABLE {
            for index in 0..TEAM_SIZE as u8 {
                match self.members.iter().find(|m| m.seat == Some((team, index))) {
                    Some(m) => {
                        setups.push(PlayerSetup {
                            name: m.name.clone(),
                            team,
                            loadout: m.loadout.sanitized(),
                            bot: false,
                        });
                        owners.push(Some(m.id));
                    }
                    None => {
                        // Bots pick a hero nobody else is playing.
                        pick = pick.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
                        let start = (pick >> 33) as usize % HEROES.len();
                        let hero = (0..HEROES.len())
                            .map(|k| HeroId(((start + k) % HEROES.len()) as u8))
                            .find(|h| !used.contains(h))
                            .unwrap_or(HeroId(start as u8));
                        used.push(hero);
                        setups.push(PlayerSetup {
                            name: format!("{} Bot", HEROES[hero.0 as usize].name),
                            team,
                            loadout: Loadout::suggested(hero),
                            bot: true,
                        });
                        owners.push(None);
                    }
                }
            }
        }
        let world = World::new(setups, self.seed);
        let mut players = HashMap::new();
        let infos: Vec<PlayerInfo> = world
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
            .collect();
        for (p, owner) in owners.iter().enumerate() {
            if let Some(id) = owner {
                players.insert(*id, p);
            }
        }
        println!("[{}] game started with {} human player(s)", self.name, players.len());
        for m in &self.members {
            let you = players.get(&m.id).map(|&p| p as u8);
            let _ = m.tx.send(encode(&ServerMsg::GameStart(GameStart { players: infos.clone(), you })));
        }
        self.game = Some(Game { world, players, pending: Vec::new(), infos, post_game: 0.0 });
        self.broadcast_lobby();
    }

    /// One simulation tick + snapshots for everyone.
    pub fn tick(&mut self) {
        let Some(g) = &mut self.game else { return };
        let cmds = std::mem::take(&mut g.pending);
        g.world.step(&cmds);
        for m in &self.members {
            let player = g.players.get(&m.id).copied();
            let snap = g.world.snapshot_for(player);
            let _ = m.tx.send(encode(&ServerMsg::Snapshot(Box::new(snap))));
        }
        if g.world.winner.is_some() {
            g.post_game += 1.0 / TICK_RATE as f32;
            if g.post_game >= POST_GAME_SECONDS {
                println!("[{}] game over, back to lobby", self.name);
                self.game = None;
                self.broadcast_lobby();
            }
        }
    }
}

fn sanitize_name(name: &str, id: u32) -> String {
    let clean: String = name.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(16).collect();
    let clean = clean.trim().to_string();
    if clean.is_empty() {
        format!("Player{id}")
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moba_core::protocol::decode;
    use moba_core::skills::SkillId;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    fn drain(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<ServerMsg> {
        let mut out = Vec::new();
        while let Ok(b) = rx.try_recv() {
            out.push(decode::<ServerMsg>(&b).expect("valid server message"));
        }
        out
    }

    #[test]
    fn lobby_seats_and_game_flow() {
        let mut room = Room::new("test".into(), 1);
        let (tx1, mut rx1) = unbounded_channel();
        let (tx2, mut rx2) = unbounded_channel();
        room.handle(RoomEvent::Join { id: 1, name: "Alice".into(), tx: tx1 });
        room.handle(RoomEvent::Join { id: 2, name: "Bob".into(), tx: tx2 });
        let lobby = room.lobby_state();
        let seated: Vec<_> = lobby.seats.iter().filter(|s| s.player.is_some()).map(|s| s.team).collect();
        assert_eq!(seated.len(), 2);
        assert!(seated.contains(&Team::Blue) && seated.contains(&Team::Red), "players are balanced across teams");

        // Invalid loadouts are rejected with a notice and sanitized.
        let mut bad = Loadout::suggested(HeroId(0));
        bad.skills[0] = SkillId(20); // Healing Wave: no role shared with Ironclad
        room.handle(RoomEvent::Msg { id: 1, msg: ClientMsg::SetLoadout(bad) });
        let msgs = drain(&mut rx1);
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Notice(_))));
        assert!(room.members[0].loadout.validate().is_ok());

        // Only the host may start.
        room.handle(RoomEvent::Msg { id: 2, msg: ClientMsg::StartGame });
        assert!(!room.in_game());
        room.handle(RoomEvent::Msg { id: 1, msg: ClientMsg::StartGame });
        assert!(room.in_game());
        let msgs = drain(&mut rx2);
        let start =
            msgs.iter().find_map(|m| if let ServerMsg::GameStart(g) = m { Some(g.clone()) } else { None }).unwrap();
        assert_eq!(start.players.len(), 10);
        assert_eq!(start.players.iter().filter(|p| !p.bot).count(), 2);
        let me = start.you.expect("Bob plays");

        room.handle(RoomEvent::Msg {
            id: 2,
            msg: ClientMsg::Command(Command::Move(moba_core::Vec2::new(1600.0, 1600.0))),
        });
        for _ in 0..10 {
            room.tick();
        }
        let msgs = drain(&mut rx2);
        let snaps: Vec<_> =
            msgs.iter().filter_map(|m| if let ServerMsg::Snapshot(s) = m { Some(s) } else { None }).collect();
        assert_eq!(snaps.len(), 10);
        let last = snaps.last().unwrap();
        assert_eq!(last.me.as_ref().unwrap().entity, start.players[me as usize].entity);

        // Leaving hands the champion to a bot; the game ends when no humans remain.
        room.handle(RoomEvent::Leave { id: 1 });
        assert!(room.in_game());
        room.handle(RoomEvent::Leave { id: 2 });
        assert!(!room.in_game());
        assert!(room.is_empty());
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("  ", 5), "Player5");
        assert_eq!(sanitize_name("abcdefghijklmnopqrstuvwxyz", 1).len(), 16);
        assert_eq!(sanitize_name("Zoë<script>", 1), "Zo<script>");
    }
}
