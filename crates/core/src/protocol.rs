//! Messages exchanged between browser clients and the server.
//!
//! Encoding: [postcard] (compact, varint based) over binary WebSocket frames.
//! The same [`Snapshot`] type is produced by the in-browser practice mode, so
//! the client renders online and offline games through one code path.

use crate::heroes::HeroId;
use crate::items::ItemId;
use crate::loadout::Loadout;
use crate::math::Vec2;
use crate::skills::{DamageKind, SkillId};
use crate::units::{MinionKind, MonsterKind, StructureKind};
use crate::{EntityId, Team};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 2;
pub const TEAM_SIZE: usize = 5;

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).expect("protocol message serializes")
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}

// ------------------------------------------------------------------ client -> server

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        name: String,
        room: String,
        version: u32,
    },
    /// Take a lobby seat (team + index 0..5).
    TakeSeat {
        team: Team,
        index: u8,
    },
    SetLoadout(Loadout),
    /// Host only: start the match; empty seats are filled with bots.
    StartGame,
    Command(Command),
    Ping(u32),
    /// Leave a finished game and return to the lobby.
    ReturnToLobby,
}

/// In-game orders. Identical for humans and bots.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Move(Vec2),
    AttackMove(Vec2),
    Attack(EntityId),
    Stop,
    /// slot 0..4 = Q, W, E, R.
    Cast {
        slot: u8,
        point: Vec2,
        unit: Option<EntityId>,
    },
    /// slot 0 = D, 1 = F.
    Summoner {
        slot: u8,
        point: Vec2,
        unit: Option<EntityId>,
    },
    LevelUp(u8),
    Recall,
    Buy(ItemId),
    Sell(u8),
    UseItem(u8),
}

// ------------------------------------------------------------------ server -> client

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome { player_id: u32 },
    Lobby(LobbyState),
    GameStart(GameStart),
    Snapshot(Box<Snapshot>),
    Notice(String),
    Pong(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LobbyState {
    pub room: String,
    pub host: u32,
    /// Ten seats: blue 0..5 then red 0..5.
    pub seats: Vec<Seat>,
    /// Connected players without a seat.
    pub spectators: Vec<LobbyPlayer>,
    pub in_game: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seat {
    pub team: Team,
    pub index: u8,
    pub player: Option<LobbyPlayer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LobbyPlayer {
    pub id: u32,
    pub name: String,
    pub loadout: Loadout,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameStart {
    pub players: Vec<PlayerInfo>,
    /// Index into `players` of the receiving client (None = spectator).
    pub you: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub index: u8,
    pub name: String,
    pub team: Team,
    pub loadout: Loadout,
    pub bot: bool,
    pub entity: EntityId,
}

/// Everything one client may know about the world at one tick.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u32,
    pub time: f32,
    pub entities: Vec<EntityView>,
    pub projectiles: Vec<ProjectileView>,
    pub zones: Vec<ZoneView>,
    pub events: Vec<FxEvent>,
    pub me: Option<SelfView>,
    pub score: [TeamScore; 2],
    /// Scoreboard rows. Sent every few ticks; empty means "unchanged".
    pub players: Vec<PlayerRow>,
    /// Team vision bitmap (VISION_GRID^2 bits, row major), sent a few times a second.
    pub fog: Option<Vec<u8>>,
    pub winner: Option<Team>,
}

/// Fog-of-war cell size in map units and grid width.
pub const VISION_CELL: f32 = 32.0;
pub const VISION_GRID: usize = 100;

pub mod flags {
    pub const STUNNED: u32 = 1 << 0;
    pub const ROOTED: u32 = 1 << 1;
    pub const SLOWED: u32 = 1 << 2;
    pub const AIRBORNE: u32 = 1 << 3;
    pub const SILENCED: u32 = 1 << 4;
    pub const BLINDED: u32 = 1 << 5;
    pub const TAUNTED: u32 = 1 << 6;
    pub const IN_BRUSH: u32 = 1 << 7;
    pub const RECALLING: u32 = 1 << 8;
    pub const DEAD: u32 = 1 << 9;
    pub const INVULNERABLE: u32 = 1 << 10;
    pub const CHANNELING: u32 = 1 << 11;
    pub const EMPOWERED: u32 = 1 << 12;
    pub const BLUE_BUFF: u32 = 1 << 13;
    pub const RED_BUFF: u32 = 1 << 14;
    pub const BARON: u32 = 1 << 15;
    pub const DASHING: u32 = 1 << 16;
    pub const HASTED: u32 = 1 << 17;
    pub const VULNERABLE: u32 = 1 << 18;
    pub const ATTACKING: u32 = 1 << 19;
    pub const MOVING: u32 = 1 << 20;
    pub const BURNING: u32 = 1 << 21;
    pub const UNSTOPPABLE: u32 = 1 << 22;
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum UnitView {
    Hero { hero: HeroId, level: u8, player: u8 },
    Minion(MinionKind),
    Structure(StructureKind),
    Monster(MonsterKind),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityView {
    pub id: EntityId,
    pub team: Team,
    pub unit: UnitView,
    #[serde(with = "quant::pos")]
    pub pos: Vec2,
    /// Facing angle in radians.
    #[serde(with = "quant::angle")]
    pub facing: f32,
    #[serde(with = "quant::whole")]
    pub hp: f32,
    #[serde(with = "quant::whole")]
    pub max_hp: f32,
    #[serde(with = "quant::whole")]
    pub shield: f32,
    #[serde(with = "quant::whole")]
    pub mana: f32,
    #[serde(with = "quant::whole")]
    pub max_mana: f32,
    pub flags: u32,
    /// Current attack target (for attack animation and lines).
    pub target: Option<EntityId>,
    /// Channel / cast wind-up: skill and aim point.
    pub cast: Option<(SkillId, Vec2)>,
}

impl EntityView {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
    pub fn alive(&self) -> bool {
        !self.has(flags::DEAD)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectileStyle {
    Arrow,
    Bolt,
    Orb,
    Cannon,
    TowerShot,
    Hook,
    Shard,
    Beam,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectileView {
    pub id: u32,
    #[serde(with = "quant::pos")]
    pub pos: Vec2,
    #[serde(with = "quant::angle")]
    pub angle: f32,
    pub style: ProjectileStyle,
    pub color: u32,
    pub team: Team,
    pub owner: EntityId,
    pub width: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ZoneShape {
    Circle { radius: f32 },
    Line { angle: f32, length: f32, width: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneKind {
    /// Telegraphed delayed blast.
    Delayed,
    /// Lingering trap.
    Trap,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoneView {
    pub id: u32,
    pub pos: Vec2,
    pub shape: ZoneShape,
    pub kind: ZoneKind,
    pub team: Team,
    pub color: u32,
    /// 0..1 progress toward detonation (Delayed) or remaining life (Trap).
    pub progress: f32,
}

/// Who got the kill in a kill-feed entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Killer {
    Player(u8),
    Tower(Team),
    Minion(Team),
    Monster,
    Fountain,
}

/// One-shot visual / audio cues and notifications.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FxEvent {
    Damage { source: EntityId, target: EntityId, pos: Vec2, amount: f32, kind: DamageKind, crit: bool },
    Heal { target: EntityId, pos: Vec2, amount: f32 },
    Cast { caster: EntityId, skill: SkillId, from: Vec2, to: Vec2 },
    Burst { pos: Vec2, radius: f32, color: u32 },
    Cone { pos: Vec2, angle: f32, half_angle: f32, range: f32, color: u32 },
    Beam { from: Vec2, to: Vec2, width: f32, color: u32 },
    Blink { from: Vec2, to: Vec2, color: u32 },
    Death { id: EntityId, pos: Vec2 },
    Gold { pos: Vec2, amount: f32 },
    LevelUp { id: EntityId, level: u8 },
    Kill { killer: Killer, victim: u8, assists: Vec<u8> },
    Announce { text: String, team: Option<Team> },
    Miss { target: EntityId, pos: Vec2 },
}

/// Private information for the receiving player's own champion.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SelfView {
    pub entity: EntityId,
    pub gold: f32,
    pub xp: f32,
    pub xp_next: f32,
    pub level: u8,
    pub ranks: [u8; 4],
    pub skill_points: u8,
    pub cooldowns: [f32; 4],
    pub cooldown_max: [f32; 4],
    pub mana_costs: [f32; 4],
    pub summoner_cd: [f32; 2],
    pub items: [Option<(ItemId, u8)>; 6],
    pub stats: StatsView,
    pub respawn: f32,
    pub recall: f32,
    pub can_shop: bool,
    /// (slot, progress 0..1) while winding up a skill.
    pub casting: Option<(u8, f32)>,
    pub empower: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StatsView {
    pub ad: f32,
    pub ap: f32,
    pub armor: f32,
    pub mr: f32,
    pub attack_speed: f32,
    pub move_speed: f32,
    pub crit: f32,
    pub haste: f32,
    pub range: f32,
    pub lifesteal: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamScore {
    pub kills: u16,
    pub towers: u8,
    pub dragons: u8,
    pub barons: u8,
    pub inhibitors: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerRow {
    pub index: u8,
    pub team: Team,
    pub hero: HeroId,
    pub level: u8,
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub cs: u16,
    pub items: [Option<ItemId>; 6],
    pub respawn: f32,
}

/// Lossy-but-plenty compact encodings for the hot snapshot fields
/// (postcard writes integers as varints, so small integers are 1-3 bytes).
pub mod quant {
    /// Positions in 1/4 map units (two u16 varints instead of two f32).
    pub mod pos {
        use crate::math::Vec2;
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        const K: f32 = 4.0;

        pub fn serialize<S: Serializer>(v: &Vec2, s: S) -> Result<S::Ok, S::Error> {
            let q = |x: f32| (x * K).round().clamp(0.0, u16::MAX as f32) as u16;
            (q(v.x), q(v.y)).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec2, D::Error> {
            let (x, y) = <(u16, u16)>::deserialize(d)?;
            Ok(Vec2::new(x as f32 / K, y as f32 / K))
        }
    }

    /// Angles as one byte (1.4 degree steps).
    pub mod angle {
        use serde::{Deserialize, Deserializer, Serialize, Serializer};
        use std::f32::consts::TAU;

        pub fn serialize<S: Serializer>(a: &f32, s: S) -> Result<S::Ok, S::Error> {
            let turns = if a.is_finite() { (a / TAU).rem_euclid(1.0) } else { 0.0 };
            (((turns * 256.0).round() as u32 % 256) as u8).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
            Ok(u8::deserialize(d)? as f32 / 256.0 * TAU)
        }
    }

    /// Non-negative quantities (health, mana, shields) rounded to integers.
    pub mod whole {
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
            (v.max(0.0).round().min(u32::MAX as f32) as u32).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
            Ok(u32::deserialize(d)? as f32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heroes::HeroId;

    #[test]
    fn roundtrip_client_messages() {
        let msgs = vec![
            ClientMsg::Hello { name: "Ana".into(), room: "lobby".into(), version: PROTOCOL_VERSION },
            ClientMsg::SetLoadout(Loadout::suggested(HeroId(3))),
            ClientMsg::Command(Command::Cast { slot: 2, point: Vec2::new(10.0, 20.0), unit: Some(7) }),
            ClientMsg::Command(Command::Buy(ItemId(4))),
        ];
        for m in msgs {
            let bytes = encode(&m);
            let back: ClientMsg = decode(&bytes).expect("decodes");
            assert_eq!(back, m);
        }
    }

    #[test]
    fn roundtrip_snapshot() {
        let snap = Snapshot {
            tick: 99,
            time: 3.3,
            entities: vec![EntityView {
                id: 1,
                team: Team::Red,
                unit: UnitView::Hero { hero: HeroId(1), level: 3, player: 0 },
                pos: Vec2::new(1.0, 2.0),
                facing: 0.0,
                hp: 100.0,
                max_hp: 200.0,
                shield: 0.0,
                mana: 50.0,
                max_mana: 60.0,
                flags: flags::SLOWED,
                target: None,
                cast: None,
            }],
            events: vec![FxEvent::Announce { text: "First Blood".into(), team: None }],
            ..Default::default()
        };
        let bytes = encode(&ServerMsg::Snapshot(Box::new(snap.clone())));
        match decode::<ServerMsg>(&bytes) {
            Some(ServerMsg::Snapshot(s)) => assert_eq!(*s, snap),
            other => panic!("bad decode {other:?}"),
        }
        assert!(decode::<ServerMsg>(&[255, 255, 255]).is_none());
    }
}

#[cfg(test)]
mod quant_tests {
    use super::*;

    #[test]
    fn quantization_is_close() {
        let e = EntityView {
            id: 3,
            team: Team::Blue,
            unit: UnitView::Minion(MinionKind::Caster),
            pos: Vec2::new(1234.56, 3199.9),
            facing: -2.0,
            hp: 99.6,
            max_hp: 300.0,
            shield: 0.2,
            mana: 0.0,
            max_mana: 0.0,
            flags: 0,
            target: Some(9),
            cast: None,
        };
        let back: EntityView = decode(&encode(&e)).unwrap();
        assert!(back.pos.distance(e.pos) < 0.2);
        let da = crate::math::angle_diff(back.facing, e.facing);
        assert!(da.abs() < 0.03, "{da}");
        assert_eq!(back.hp, 100.0);
        assert!(encode(&e).len() < 24, "entity encodes to {} bytes", encode(&e).len());
    }
}
