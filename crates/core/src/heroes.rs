//! Hero (champion) catalog. Each hero has two roles, base stats and one
//! built-in ultimate. Q/W/E are chosen by the player from the skill pool.

use crate::roles::{Role, RolePair};
use crate::skills::SkillId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HeroId(pub u8);

/// Base stats. Distances use map units (the map is 3200 x 3200).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroBase {
    pub hp: f32,
    pub hp_lvl: f32,
    pub hp_regen: f32,
    pub mana: f32,
    pub mana_lvl: f32,
    pub mana_regen: f32,
    pub ad: f32,
    pub ad_lvl: f32,
    pub armor: f32,
    pub armor_lvl: f32,
    pub mr: f32,
    pub mr_lvl: f32,
    /// Attacks per second at level 1.
    pub attack_speed: f32,
    /// Bonus attack speed fraction gained per level.
    pub as_lvl: f32,
    pub range: f32,
    pub move_speed: f32,
    pub ranged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Weapon {
    Hammer,
    SwordShield,
    Fists,
    Sword,
    Spear,
    Daggers,
    Bow,
    Staff,
    Wand,
    Rifle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Headgear {
    Helm,
    WingedHelm,
    Horns,
    Hair,
    Hood,
    WizardHat,
    Bandana,
    Crown,
}

/// Palette + parts used by the client to paint the pixel sprite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroLook {
    pub body: u32,
    pub trim: u32,
    pub skin: u32,
    pub hair: u32,
    pub weapon: Weapon,
    pub head: Headgear,
    pub cape: bool,
    /// Bulky silhouette (tanks).
    pub broad: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroDef {
    pub id: HeroId,
    pub name: &'static str,
    pub name_zh: &'static str,
    pub title: &'static str,
    pub roles: RolePair,
    pub base: HeroBase,
    pub ultimate: SkillId,
    /// Default / recommended Q, W, E.
    pub suggested: [SkillId; 3],
    pub look: HeroLook,
}

use Role::*;

const MELEE: f32 = 32.0;

#[rustfmt::skip]
pub static HEROES: &[HeroDef] = &[
    HeroDef {
        id: HeroId(0),
        name: "Ironclad",
        name_zh: "铁甲",
        title: "the Iron Bastion",
        roles: [Tank, Fighter],
        base: HeroBase {
            hp: 640.0, hp_lvl: 105.0, hp_regen: 1.6,
            mana: 280.0, mana_lvl: 40.0, mana_regen: 1.4,
            ad: 64.0, ad_lvl: 3.5,
            armor: 36.0, armor_lvl: 4.5, mr: 32.0, mr_lvl: 1.8,
            attack_speed: 0.65, as_lvl: 0.02,
            range: MELEE, move_speed: 84.0, ranged: false,
        },
        ultimate: SkillId(101),
        suggested: [SkillId(1), SkillId(2), SkillId(6)],
        look: HeroLook {
            body: 0x8a96a8, trim: 0xd8b048, skin: 0xe8b890, hair: 0x5a4030,
            weapon: Weapon::Hammer, head: Headgear::Helm, cape: false, broad: true,
        },
    },
    HeroDef {
        id: HeroId(1),
        name: "Aegis",
        name_zh: "神盾",
        title: "the Shieldmaiden",
        roles: [Tank, Support],
        base: HeroBase {
            hp: 620.0, hp_lvl: 100.0, hp_regen: 1.7,
            mana: 320.0, mana_lvl: 45.0, mana_regen: 1.6,
            ad: 58.0, ad_lvl: 3.2,
            armor: 38.0, armor_lvl: 4.8, mr: 32.0, mr_lvl: 2.0,
            attack_speed: 0.64, as_lvl: 0.02,
            range: MELEE, move_speed: 83.0, ranged: false,
        },
        ultimate: SkillId(102),
        suggested: [SkillId(3), SkillId(4), SkillId(7)],
        look: HeroLook {
            body: 0xe8e0c8, trim: 0x48a0e0, skin: 0xf0c8a0, hair: 0xf0d860,
            weapon: Weapon::SwordShield, head: Headgear::WingedHelm, cape: true, broad: true,
        },
    },
    HeroDef {
        id: HeroId(2),
        name: "Colossus",
        name_zh: "巨像",
        title: "the Living Mountain",
        roles: [Tank, Mage],
        base: HeroBase {
            hp: 650.0, hp_lvl: 100.0, hp_regen: 1.5,
            mana: 300.0, mana_lvl: 45.0, mana_regen: 1.5,
            ad: 62.0, ad_lvl: 3.5,
            armor: 37.0, armor_lvl: 4.5, mr: 30.0, mr_lvl: 1.6,
            attack_speed: 0.63, as_lvl: 0.018,
            range: MELEE, move_speed: 82.0, ranged: false,
        },
        ultimate: SkillId(103),
        suggested: [SkillId(5), SkillId(2), SkillId(6)],
        look: HeroLook {
            body: 0x8a7058, trim: 0x68c8e8, skin: 0xa08870, hair: 0x584838,
            weapon: Weapon::Fists, head: Headgear::Horns, cape: false, broad: true,
        },
    },
    HeroDef {
        id: HeroId(3),
        name: "Kaelen",
        name_zh: "凯伦",
        title: "the Duelist",
        roles: [Fighter, Assassin],
        base: HeroBase {
            hp: 600.0, hp_lvl: 100.0, hp_regen: 1.5,
            mana: 280.0, mana_lvl: 40.0, mana_regen: 1.4,
            ad: 66.0, ad_lvl: 3.8,
            armor: 33.0, armor_lvl: 4.2, mr: 32.0, mr_lvl: 1.5,
            attack_speed: 0.68, as_lvl: 0.025,
            range: MELEE, move_speed: 86.0, ranged: false,
        },
        ultimate: SkillId(104),
        suggested: [SkillId(9), SkillId(10), SkillId(14)],
        look: HeroLook {
            body: 0x983040, trim: 0x282838, skin: 0xf0c8a0, hair: 0x281818,
            weapon: Weapon::Sword, head: Headgear::Hair, cape: true, broad: false,
        },
    },
    HeroDef {
        id: HeroId(4),
        name: "Valkyr",
        name_zh: "瓦尔基里",
        title: "the Sky Lancer",
        roles: [Fighter, Support],
        base: HeroBase {
            hp: 610.0, hp_lvl: 100.0, hp_regen: 1.6,
            mana: 300.0, mana_lvl: 42.0, mana_regen: 1.5,
            ad: 63.0, ad_lvl: 3.6,
            armor: 34.0, armor_lvl: 4.4, mr: 32.0, mr_lvl: 1.6,
            attack_speed: 0.66, as_lvl: 0.022,
            range: 38.0, move_speed: 85.0, ranged: false,
        },
        ultimate: SkillId(105),
        suggested: [SkillId(1), SkillId(11), SkillId(4)],
        look: HeroLook {
            body: 0xd8d8e8, trim: 0xe8b838, skin: 0xf0c8a0, hair: 0xf8e8a0,
            weapon: Weapon::Spear, head: Headgear::WingedHelm, cape: true, broad: false,
        },
    },
    HeroDef {
        id: HeroId(5),
        name: "Shade",
        name_zh: "暗影",
        title: "the Void Walker",
        roles: [Assassin, Mage],
        base: HeroBase {
            hp: 570.0, hp_lvl: 92.0, hp_regen: 1.4,
            mana: 330.0, mana_lvl: 45.0, mana_regen: 1.6,
            ad: 60.0, ad_lvl: 3.4,
            armor: 30.0, armor_lvl: 4.2, mr: 32.0, mr_lvl: 1.5,
            attack_speed: 0.67, as_lvl: 0.025,
            range: MELEE, move_speed: 87.0, ranged: false,
        },
        ultimate: SkillId(106),
        suggested: [SkillId(16), SkillId(15), SkillId(19)],
        look: HeroLook {
            body: 0x382848, trim: 0xa848ff, skin: 0xc8b8d8, hair: 0x181020,
            weapon: Weapon::Daggers, head: Headgear::Hood, cape: true, broad: false,
        },
    },
    HeroDef {
        id: HeroId(6),
        name: "Sable",
        name_zh: "黑貂",
        title: "the Shadow Archer",
        roles: [Assassin, Marksman],
        base: HeroBase {
            hp: 570.0, hp_lvl: 90.0, hp_regen: 1.3,
            mana: 300.0, mana_lvl: 40.0, mana_regen: 1.4,
            ad: 60.0, ad_lvl: 3.2,
            armor: 28.0, armor_lvl: 4.0, mr: 30.0, mr_lvl: 1.3,
            attack_speed: 0.66, as_lvl: 0.03,
            range: 125.0, move_speed: 85.0, ranged: true,
        },
        ultimate: SkillId(107),
        suggested: [SkillId(17), SkillId(18), SkillId(12)],
        look: HeroLook {
            body: 0x284030, trim: 0x88a048, skin: 0xe0b088, hair: 0x302020,
            weapon: Weapon::Bow, head: Headgear::Hood, cape: true, broad: false,
        },
    },
    HeroDef {
        id: HeroId(7),
        name: "Vex",
        name_zh: "维克丝",
        title: "the Arcane Gunner",
        roles: [Marksman, Mage],
        base: HeroBase {
            hp: 560.0, hp_lvl: 90.0, hp_regen: 1.3,
            mana: 340.0, mana_lvl: 45.0, mana_regen: 1.6,
            ad: 58.0, ad_lvl: 3.0,
            armor: 26.0, armor_lvl: 4.0, mr: 30.0, mr_lvl: 1.3,
            attack_speed: 0.64, as_lvl: 0.03,
            range: 135.0, move_speed: 84.0, ranged: true,
        },
        ultimate: SkillId(108),
        suggested: [SkillId(22), SkillId(16), SkillId(13)],
        look: HeroLook {
            body: 0x2868a8, trim: 0x48ffd8, skin: 0xf0d0b0, hair: 0xe85890,
            weapon: Weapon::Wand, head: Headgear::Bandana, cape: false, broad: false,
        },
    },
    HeroDef {
        id: HeroId(8),
        name: "Lumen",
        name_zh: "流明",
        title: "the Dawnbringer",
        roles: [Mage, Support],
        base: HeroBase {
            hp: 540.0, hp_lvl: 88.0, hp_regen: 1.2,
            mana: 380.0, mana_lvl: 50.0, mana_regen: 1.8,
            ad: 52.0, ad_lvl: 3.0,
            armor: 24.0, armor_lvl: 4.0, mr: 30.0, mr_lvl: 1.3,
            attack_speed: 0.63, as_lvl: 0.02,
            range: 130.0, move_speed: 83.0, ranged: true,
        },
        ultimate: SkillId(109),
        suggested: [SkillId(20), SkillId(21), SkillId(16)],
        look: HeroLook {
            body: 0xf0e8d0, trim: 0xf0c040, skin: 0xf8d8b8, hair: 0xf8f0c0,
            weapon: Weapon::Staff, head: Headgear::WizardHat, cape: true, broad: false,
        },
    },
    HeroDef {
        id: HeroId(9),
        name: "Hawk",
        name_zh: "鹰眼",
        title: "the Bounty Hunter",
        roles: [Marksman, Fighter],
        base: HeroBase {
            hp: 580.0, hp_lvl: 92.0, hp_regen: 1.4,
            mana: 300.0, mana_lvl: 40.0, mana_regen: 1.4,
            ad: 62.0, ad_lvl: 3.4,
            armor: 28.0, armor_lvl: 4.2, mr: 30.0, mr_lvl: 1.3,
            attack_speed: 0.65, as_lvl: 0.028,
            range: 140.0, move_speed: 84.0, ranged: true,
        },
        ultimate: SkillId(110),
        suggested: [SkillId(23), SkillId(24), SkillId(13)],
        look: HeroLook {
            body: 0x785838, trim: 0xd84838, skin: 0xd8a078, hair: 0x382818,
            weapon: Weapon::Rifle, head: Headgear::Crown, cape: false, broad: false,
        },
    },
];

pub fn try_hero(id: HeroId) -> Option<&'static HeroDef> {
    HEROES.get(id.0 as usize).filter(|h| h.id == id)
}

/// Panics on unknown ids — only use with ids that passed validation.
pub fn hero(id: HeroId) -> &'static HeroDef {
    try_hero(id).unwrap_or_else(|| panic!("unknown hero {id:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::try_skill;

    #[test]
    fn heroes_are_consistent() {
        for (i, h) in HEROES.iter().enumerate() {
            assert_eq!(h.id.0 as usize, i);
            assert_ne!(h.roles[0], h.roles[1]);
            let ult = try_skill(h.ultimate).expect("ultimate exists");
            assert!(ult.ultimate, "{}'s R must be an ultimate", h.name);
            assert_eq!(ult.roles, h.roles, "{}'s ultimate mirrors its roles", h.name);
        }
    }

    #[test]
    fn ultimates_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for h in HEROES {
            assert!(seen.insert(h.ultimate));
        }
    }
}
