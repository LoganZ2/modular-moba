//! Summoner spells (D / F).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SummonerId {
    Flash,
    Heal,
    Ghost,
    Ignite,
    Exhaust,
    Barrier,
}

impl SummonerId {
    pub const ALL: [SummonerId; 6] = [
        SummonerId::Flash,
        SummonerId::Heal,
        SummonerId::Ghost,
        SummonerId::Ignite,
        SummonerId::Exhaust,
        SummonerId::Barrier,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SummonerId::Flash => "Flash",
            SummonerId::Heal => "Heal",
            SummonerId::Ghost => "Ghost",
            SummonerId::Ignite => "Ignite",
            SummonerId::Exhaust => "Exhaust",
            SummonerId::Barrier => "Barrier",
        }
    }

    pub fn name_zh(self) -> &'static str {
        match self {
            SummonerId::Flash => "闪现",
            SummonerId::Heal => "治疗术",
            SummonerId::Ghost => "幽灵疾步",
            SummonerId::Ignite => "点燃",
            SummonerId::Exhaust => "虚弱",
            SummonerId::Barrier => "屏障",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            SummonerId::Flash => "Teleport a short distance toward the cursor.",
            SummonerId::Heal => "Heal yourself and the nearest ally, gain speed.",
            SummonerId::Ghost => "Gain a large move speed boost for 8s.",
            SummonerId::Ignite => "Burn an enemy champion for true damage over 5s.",
            SummonerId::Exhaust => "Slow an enemy champion and cut its damage.",
            SummonerId::Barrier => "Shield yourself for 2.5s.",
        }
    }

    pub fn cooldown(self) -> f32 {
        match self {
            SummonerId::Flash => 150.0,
            SummonerId::Heal => 120.0,
            SummonerId::Ghost => 120.0,
            SummonerId::Ignite => 100.0,
            SummonerId::Exhaust => 120.0,
            SummonerId::Barrier => 100.0,
        }
    }

    /// Cast range (0 for self-cast spells).
    pub fn range(self) -> f32 {
        match self {
            SummonerId::Flash => 100.0,
            SummonerId::Ignite => 150.0,
            SummonerId::Exhaust => 165.0,
            _ => 0.0,
        }
    }

    pub fn color(self) -> u32 {
        match self {
            SummonerId::Flash => 0xfff070,
            SummonerId::Heal => 0x60ff80,
            SummonerId::Ghost => 0x80c8ff,
            SummonerId::Ignite => 0xff7030,
            SummonerId::Exhaust => 0xb08850,
            SummonerId::Barrier => 0xf0e0a0,
        }
    }

    pub fn needs_enemy_hero(self) -> bool {
        matches!(self, SummonerId::Ignite | SummonerId::Exhaust)
    }
}
