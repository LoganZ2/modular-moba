//! Roles ("定位"). Every hero has exactly two roles and so does every basic
//! skill. A hero may equip a skill in Q/W/E only when the two share at least
//! one role — see [`crate::loadout`].

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Role {
    Tank,
    Fighter,
    Assassin,
    Mage,
    Marksman,
    Support,
}

/// Two distinct roles.
pub type RolePair = [Role; 2];

impl Role {
    pub const ALL: [Role; 6] = [Role::Tank, Role::Fighter, Role::Assassin, Role::Mage, Role::Marksman, Role::Support];

    pub fn name(self) -> &'static str {
        match self {
            Role::Tank => "Tank",
            Role::Fighter => "Fighter",
            Role::Assassin => "Assassin",
            Role::Mage => "Mage",
            Role::Marksman => "Marksman",
            Role::Support => "Support",
        }
    }

    pub fn name_zh(self) -> &'static str {
        match self {
            Role::Tank => "坦克",
            Role::Fighter => "战士",
            Role::Assassin => "刺客",
            Role::Mage => "法师",
            Role::Marksman => "射手",
            Role::Support => "辅助",
        }
    }

    /// Three-letter tag used in the compact pixel UI.
    pub fn short(self) -> &'static str {
        match self {
            Role::Tank => "TNK",
            Role::Fighter => "FTR",
            Role::Assassin => "ASN",
            Role::Mage => "MAG",
            Role::Marksman => "MRK",
            Role::Support => "SUP",
        }
    }

    /// UI colour (0xRRGGBB).
    pub fn color(self) -> u32 {
        match self {
            Role::Tank => 0x8fa3bf,
            Role::Fighter => 0xe0873a,
            Role::Assassin => 0xa65fd6,
            Role::Mage => 0x4a90f0,
            Role::Marksman => 0xf0cc4a,
            Role::Support => 0x52c98e,
        }
    }
}

/// True when the two role pairs have at least one role in common.
pub fn shares_role(a: &RolePair, b: &RolePair) -> bool {
    a.iter().any(|r| b.contains(r))
}

/// The roles both pairs have in common (0, 1 or 2 of them).
pub fn common_roles(a: &RolePair, b: &RolePair) -> Vec<Role> {
    let mut out: Vec<Role> = a.iter().copied().filter(|r| b.contains(r)).collect();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap() {
        assert!(shares_role(&[Role::Tank, Role::Mage], &[Role::Mage, Role::Support]));
        assert!(!shares_role(&[Role::Tank, Role::Fighter], &[Role::Mage, Role::Support]));
        assert_eq!(common_roles(&[Role::Tank, Role::Mage], &[Role::Mage, Role::Tank]).len(), 2);
    }
}
