//! `moba-core` — everything the client and the server agree on.
//!
//! * **content**: [`roles`], [`skills`], [`heroes`], [`items`], [`summoners`]
//! * **rules**: [`loadout`] (the "a skill must share a role with its hero" rule)
//! * **world**: [`map`] (Summoner's-Rift style three-lane map + path finding)
//! * **simulation**: [`sim`] (deterministic, fixed-tick, server authoritative;
//!   also runs inside the browser for offline practice)
//! * **wire format**: [`protocol`]

pub mod heroes;
pub mod items;
pub mod loadout;
pub mod map;
pub mod math;
pub mod protocol;
pub mod roles;
pub mod sim;
pub mod skills;
pub mod stats;
pub mod summoners;
pub mod units;

pub use math::Vec2;

/// Simulation ticks per second.
pub const TICK_RATE: u32 = 30;
/// Seconds per simulation tick.
pub const DT: f32 = 1.0 / TICK_RATE as f32;

/// Unique id of a simulated entity (hero, minion, tower, monster ...).
pub type EntityId = u32;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Team {
    Blue,
    Red,
    Neutral,
}

impl Team {
    pub const PLAYABLE: [Team; 2] = [Team::Blue, Team::Red];

    pub fn enemy(self) -> Team {
        match self {
            Team::Blue => Team::Red,
            Team::Red => Team::Blue,
            Team::Neutral => Team::Neutral,
        }
    }

    /// 0 for blue, 1 for red. Neutral maps to 2.
    pub fn index(self) -> usize {
        match self {
            Team::Blue => 0,
            Team::Red => 1,
            Team::Neutral => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Team::Blue => "Blue",
            Team::Red => "Red",
            Team::Neutral => "Neutral",
        }
    }

    /// True if units of `self` may damage units of `other`.
    pub fn hostile_to(self, other: Team) -> bool {
        self != other
    }
}
