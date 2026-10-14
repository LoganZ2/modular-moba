//! Shop items. Items are pure stat sticks (plus the health potion) so they
//! compose with any hero/skill combination.

use crate::stats::StatBlock;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemId(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemTier {
    Consumable,
    Basic,
    Epic,
    Legendary,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDef {
    pub id: ItemId,
    pub name: &'static str,
    pub cost: u32,
    pub tier: ItemTier,
    pub stats: StatBlock,
    /// Max stack size in one inventory slot.
    pub stack: u8,
    /// Icon colour (0xRRGGBB).
    pub color: u32,
    /// Small 3x3-ish glyph hint for the client icon painter.
    pub glyph: ItemGlyph,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemGlyph {
    Potion,
    Sword,
    Book,
    Gem,
    Armor,
    Cloak,
    Dagger,
    Boots,
    Bow,
    Staff,
    Belt,
    Hat,
    Heart,
    Fang,
}

pub const POTION: ItemId = ItemId(0);
/// Health restored by one potion, spread over [`POTION_SECS`].
pub const POTION_HEAL: f32 = 120.0;
pub const POTION_SECS: f32 = 12.0;
/// Fraction of the cost refunded when selling.
pub const SELL_RATIO: f32 = 0.7;

const Z: StatBlock = StatBlock::ZERO;

#[rustfmt::skip]
pub static ITEMS: &[ItemDef] = &[
    ItemDef { id: ItemId(0), name: "Health Potion", cost: 50, tier: ItemTier::Consumable, stats: Z, stack: 5, color: 0xe03838, glyph: ItemGlyph::Potion },
    ItemDef { id: ItemId(1), name: "Long Sword", cost: 350, tier: ItemTier::Basic, stats: StatBlock { ad: 10.0, ..Z }, stack: 1, color: 0xc8c8d8, glyph: ItemGlyph::Sword },
    ItemDef { id: ItemId(2), name: "Amplifying Tome", cost: 435, tier: ItemTier::Basic, stats: StatBlock { ap: 20.0, ..Z }, stack: 1, color: 0x4878e0, glyph: ItemGlyph::Book },
    ItemDef { id: ItemId(3), name: "Ruby Crystal", cost: 400, tier: ItemTier::Basic, stats: StatBlock { hp: 150.0, ..Z }, stack: 1, color: 0xe83050, glyph: ItemGlyph::Gem },
    ItemDef { id: ItemId(4), name: "Cloth Armor", cost: 300, tier: ItemTier::Basic, stats: StatBlock { armor: 15.0, ..Z }, stack: 1, color: 0xb09070, glyph: ItemGlyph::Armor },
    ItemDef { id: ItemId(5), name: "Null-Magic Mantle", cost: 450, tier: ItemTier::Basic, stats: StatBlock { mr: 25.0, ..Z }, stack: 1, color: 0x9860d0, glyph: ItemGlyph::Cloak },
    ItemDef { id: ItemId(6), name: "Dagger", cost: 300, tier: ItemTier::Basic, stats: StatBlock { attack_speed: 0.12, ..Z }, stack: 1, color: 0xd8d0a0, glyph: ItemGlyph::Dagger },
    ItemDef { id: ItemId(7), name: "Boots", cost: 300, tier: ItemTier::Basic, stats: StatBlock { move_speed: 6.0, ..Z }, stack: 1, color: 0x906040, glyph: ItemGlyph::Boots },
    ItemDef { id: ItemId(8), name: "Sapphire Crystal", cost: 350, tier: ItemTier::Basic, stats: StatBlock { mana: 250.0, ..Z }, stack: 1, color: 0x3890f0, glyph: ItemGlyph::Gem },
    ItemDef { id: ItemId(9), name: "Kindlegem", cost: 800, tier: ItemTier::Epic, stats: StatBlock { hp: 200.0, haste: 10.0, ..Z }, stack: 1, color: 0xf06830, glyph: ItemGlyph::Gem },
    ItemDef { id: ItemId(10), name: "Vampiric Scepter", cost: 900, tier: ItemTier::Epic, stats: StatBlock { ad: 15.0, lifesteal: 0.1, ..Z }, stack: 1, color: 0xa02030, glyph: ItemGlyph::Fang },
    ItemDef { id: ItemId(11), name: "Greatsword", cost: 1300, tier: ItemTier::Epic, stats: StatBlock { ad: 40.0, ..Z }, stack: 1, color: 0xe8e8f0, glyph: ItemGlyph::Sword },
    ItemDef { id: ItemId(12), name: "Blasting Wand", cost: 850, tier: ItemTier::Epic, stats: StatBlock { ap: 40.0, ..Z }, stack: 1, color: 0x6040c8, glyph: ItemGlyph::Staff },
    ItemDef { id: ItemId(13), name: "Chain Vest", cost: 800, tier: ItemTier::Epic, stats: StatBlock { armor: 40.0, ..Z }, stack: 1, color: 0x909aa8, glyph: ItemGlyph::Armor },
    ItemDef { id: ItemId(14), name: "Giant's Belt", cost: 900, tier: ItemTier::Epic, stats: StatBlock { hp: 350.0, ..Z }, stack: 1, color: 0xb87838, glyph: ItemGlyph::Belt },
    ItemDef { id: ItemId(15), name: "Recurve Bow", cost: 1000, tier: ItemTier::Epic, stats: StatBlock { attack_speed: 0.25, ..Z }, stack: 1, color: 0x68a048, glyph: ItemGlyph::Bow },
    ItemDef { id: ItemId(16), name: "Cloak of Agility", cost: 600, tier: ItemTier::Epic, stats: StatBlock { crit: 0.15, ..Z }, stack: 1, color: 0xe8d870, glyph: ItemGlyph::Cloak },
    ItemDef { id: ItemId(17), name: "Swift Boots", cost: 900, tier: ItemTier::Epic, stats: StatBlock { move_speed: 12.0, attack_speed: 0.1, ..Z }, stack: 1, color: 0x58b8e8, glyph: ItemGlyph::Boots },
    ItemDef { id: ItemId(18), name: "Infinity Blade", cost: 3400, tier: ItemTier::Legendary, stats: StatBlock { ad: 70.0, crit: 0.25, ..Z }, stack: 1, color: 0xf8e060, glyph: ItemGlyph::Sword },
    ItemDef { id: ItemId(19), name: "Archmage Hat", cost: 3600, tier: ItemTier::Legendary, stats: StatBlock { ap: 130.0, ..Z }, stack: 1, color: 0x5838c8, glyph: ItemGlyph::Hat },
    ItemDef { id: ItemId(20), name: "Ember Aegis", cost: 2700, tier: ItemTier::Legendary, stats: StatBlock { hp: 450.0, armor: 45.0, ..Z }, stack: 1, color: 0xf07028, glyph: ItemGlyph::Armor },
    ItemDef { id: ItemId(21), name: "Spirit Ward", cost: 2900, tier: ItemTier::Legendary, stats: StatBlock { hp: 400.0, mr: 50.0, haste: 10.0, hp_regen: 2.0, ..Z }, stack: 1, color: 0x58d8a8, glyph: ItemGlyph::Cloak },
    ItemDef { id: ItemId(22), name: "Bloodthirster", cost: 3400, tier: ItemTier::Legendary, stats: StatBlock { ad: 60.0, lifesteal: 0.18, ..Z }, stack: 1, color: 0xd02838, glyph: ItemGlyph::Fang },
    ItemDef { id: ItemId(23), name: "Phantom Edge", cost: 2600, tier: ItemTier::Legendary, stats: StatBlock { attack_speed: 0.45, crit: 0.2, move_speed: 6.0, ..Z }, stack: 1, color: 0xb8a8f8, glyph: ItemGlyph::Dagger },
    ItemDef { id: ItemId(24), name: "Echo Staff", cost: 3000, tier: ItemTier::Legendary, stats: StatBlock { ap: 90.0, mana: 600.0, haste: 20.0, ..Z }, stack: 1, color: 0x48a8f8, glyph: ItemGlyph::Staff },
    ItemDef { id: ItemId(25), name: "Rune Cleaver", cost: 3100, tier: ItemTier::Legendary, stats: StatBlock { ad: 40.0, hp: 400.0, haste: 20.0, ..Z }, stack: 1, color: 0x404858, glyph: ItemGlyph::Sword },
    ItemDef { id: ItemId(26), name: "Titan Heart", cost: 3000, tier: ItemTier::Legendary, stats: StatBlock { hp: 800.0, hp_regen: 5.0, ..Z }, stack: 1, color: 0xd84858, glyph: ItemGlyph::Heart },
    ItemDef { id: ItemId(27), name: "Dawn Charm", cost: 2300, tier: ItemTier::Legendary, stats: StatBlock { ap: 40.0, hp: 200.0, haste: 20.0, mana_regen: 3.0, ..Z }, stack: 1, color: 0xf8d0a0, glyph: ItemGlyph::Gem },
];

pub fn try_item(id: ItemId) -> Option<&'static ItemDef> {
    ITEMS.get(id.0 as usize).filter(|i| i.id == id)
}

pub fn item(id: ItemId) -> &'static ItemDef {
    try_item(id).unwrap_or_else(|| panic!("unknown item {id:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_index() {
        for (i, it) in ITEMS.iter().enumerate() {
            assert_eq!(it.id.0 as usize, i, "{}", it.name);
            assert!(it.stack >= 1);
        }
    }
}
