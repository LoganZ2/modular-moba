//! Non-hero unit archetypes: minions, jungle monsters and structures.

use serde::{Deserialize, Serialize};

/// Shared shape of the stat tables below.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitStats {
    pub hp: f32,
    pub ad: f32,
    pub armor: f32,
    pub mr: f32,
    pub attack_speed: f32,
    pub range: f32,
    pub move_speed: f32,
    pub radius: f32,
    pub gold: f32,
    pub xp: f32,
    pub ranged: bool,
    pub projectile_speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MinionKind {
    Melee,
    Caster,
    Siege,
    Super,
}

impl MinionKind {
    #[rustfmt::skip]
    pub fn stats(self) -> UnitStats {
        match self {
            MinionKind::Melee => UnitStats {
                hp: 477.0, ad: 12.0, armor: 0.0, mr: 0.0, attack_speed: 1.25, range: 28.0,
                move_speed: 82.0, radius: 9.0, gold: 21.0, xp: 60.0, ranged: false, projectile_speed: 0.0,
            },
            MinionKind::Caster => UnitStats {
                hp: 296.0, ad: 24.0, armor: 0.0, mr: 0.0, attack_speed: 0.667, range: 120.0,
                move_speed: 82.0, radius: 8.0, gold: 14.0, xp: 30.0, ranged: true, projectile_speed: 170.0,
            },
            MinionKind::Siege => UnitStats {
                hp: 900.0, ad: 41.0, armor: 0.0, mr: 0.0, attack_speed: 1.0, range: 75.0,
                move_speed: 82.0, radius: 12.0, gold: 60.0, xp: 93.0, ranged: true, projectile_speed: 220.0,
            },
            MinionKind::Super => UnitStats {
                hp: 1600.0, ad: 110.0, armor: 50.0, mr: 30.0, attack_speed: 0.85, range: 32.0,
                move_speed: 82.0, radius: 14.0, gold: 60.0, xp: 97.0, ranged: false, projectile_speed: 0.0,
            },
        }
    }

    /// (hp, ad) added every 90 seconds of game time.
    pub fn growth(self) -> (f32, f32) {
        match self {
            MinionKind::Melee => (22.0, 1.0),
            MinionKind::Caster => (8.0, 1.5),
            MinionKind::Siege => (45.0, 2.5),
            MinionKind::Super => (100.0, 5.0),
        }
    }

    /// Fraction of max HP a tower shot removes.
    pub fn tower_shot_pct(self) -> f32 {
        match self {
            MinionKind::Melee => 0.45,
            MinionKind::Caster => 0.7,
            MinionKind::Siege => 0.14,
            MinionKind::Super => 0.08,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MonsterKind {
    Gromp,
    BigWolf,
    Wolf,
    BigRaptor,
    Raptor,
    BigKrug,
    Krug,
    BlueSentinel,
    RedBrambleback,
    Dragon,
    Baron,
}

impl MonsterKind {
    pub fn name(self) -> &'static str {
        match self {
            MonsterKind::Gromp => "Gromp",
            MonsterKind::BigWolf => "Greater Wolf",
            MonsterKind::Wolf => "Wolf",
            MonsterKind::BigRaptor => "Crimson Raptor",
            MonsterKind::Raptor => "Raptor",
            MonsterKind::BigKrug => "Ancient Krug",
            MonsterKind::Krug => "Krug",
            MonsterKind::BlueSentinel => "Blue Sentinel",
            MonsterKind::RedBrambleback => "Red Brambleback",
            MonsterKind::Dragon => "Dragon",
            MonsterKind::Baron => "Baron",
        }
    }

    pub fn is_epic(self) -> bool {
        matches!(self, MonsterKind::Dragon | MonsterKind::Baron)
    }

    #[rustfmt::skip]
    pub fn stats(self) -> UnitStats {
        let melee = |hp, ad, aspd, radius, gold, xp| UnitStats {
            hp, ad, armor: 20.0, mr: 20.0, attack_speed: aspd, range: 30.0, move_speed: 90.0,
            radius, gold, xp, ranged: false, projectile_speed: 0.0,
        };
        match self {
            MonsterKind::Gromp => UnitStats { range: 70.0, ranged: true, projectile_speed: 250.0, ..melee(1300.0, 40.0, 0.45, 16.0, 80.0, 120.0) },
            MonsterKind::BigWolf => melee(1100.0, 24.0, 0.62, 13.0, 55.0, 95.0),
            MonsterKind::Wolf => melee(350.0, 9.0, 0.62, 9.0, 15.0, 25.0),
            MonsterKind::BigRaptor => UnitStats { range: 80.0, ranged: true, projectile_speed: 300.0, ..melee(900.0, 16.0, 0.66, 12.0, 40.0, 40.0) },
            MonsterKind::Raptor => melee(260.0, 7.0, 0.7, 8.0, 10.0, 12.0),
            MonsterKind::BigKrug => melee(1200.0, 38.0, 0.6, 16.0, 80.0, 100.0),
            MonsterKind::Krug => melee(450.0, 16.0, 0.6, 10.0, 20.0, 20.0),
            MonsterKind::BlueSentinel => melee(1800.0, 45.0, 0.49, 18.0, 90.0, 140.0),
            MonsterKind::RedBrambleback => melee(1800.0, 45.0, 0.49, 18.0, 90.0, 140.0),
            MonsterKind::Dragon => UnitStats { armor: 30.0, mr: 30.0, range: 110.0, ranged: true, projectile_speed: 350.0, ..melee(3500.0, 90.0, 0.5, 26.0, 100.0, 250.0) },
            MonsterKind::Baron => UnitStats { armor: 60.0, mr: 60.0, range: 90.0, ranged: true, projectile_speed: 400.0, ..melee(8000.0, 150.0, 0.6, 34.0, 300.0, 600.0) },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StructureKind {
    /// tier 1 = outer, 2 = inner, 3 = inhibitor tower, 4 = nexus tower.
    /// lane 0 top, 1 mid, 2 bot, 3 = nexus towers.
    Tower {
        lane: u8,
        tier: u8,
    },
    Inhibitor {
        lane: u8,
    },
    Nexus,
}

impl StructureKind {
    pub fn max_hp(self) -> f32 {
        match self {
            StructureKind::Tower { tier: 1, .. } => 3500.0,
            StructureKind::Tower { tier: 2, .. } => 3300.0,
            StructureKind::Tower { tier: 3, .. } => 3300.0,
            StructureKind::Tower { .. } => 2700.0,
            StructureKind::Inhibitor { .. } => 3000.0,
            StructureKind::Nexus => 5500.0,
        }
    }

    pub fn radius(self) -> f32 {
        match self {
            StructureKind::Tower { .. } => 18.0,
            StructureKind::Inhibitor { .. } => 20.0,
            StructureKind::Nexus => 36.0,
        }
    }

    pub fn armor(self) -> f32 {
        match self {
            StructureKind::Tower { .. } => 40.0,
            _ => 20.0,
        }
    }
}

/// Tower combat constants.
pub const TOWER_RANGE: f32 = 190.0;
pub const TOWER_AD: f32 = 160.0;
pub const TOWER_ATTACK_SPEED: f32 = 0.83;
pub const TOWER_PROJECTILE_SPEED: f32 = 320.0;
