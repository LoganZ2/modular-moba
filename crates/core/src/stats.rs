use serde::{Deserialize, Serialize};
use std::ops::AddAssign;

/// Flat stat bonuses (items, buffs). `attack_speed` is a fraction (+0.25 = +25%).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StatBlock {
    pub hp: f32,
    pub mana: f32,
    pub ad: f32,
    pub ap: f32,
    pub armor: f32,
    pub mr: f32,
    pub attack_speed: f32,
    pub move_speed: f32,
    pub crit: f32,
    pub lifesteal: f32,
    pub haste: f32,
    pub hp_regen: f32,
    pub mana_regen: f32,
}

impl StatBlock {
    pub const ZERO: StatBlock = StatBlock {
        hp: 0.0,
        mana: 0.0,
        ad: 0.0,
        ap: 0.0,
        armor: 0.0,
        mr: 0.0,
        attack_speed: 0.0,
        move_speed: 0.0,
        crit: 0.0,
        lifesteal: 0.0,
        haste: 0.0,
        hp_regen: 0.0,
        mana_regen: 0.0,
    };

    /// Human readable lines, e.g. `+40 AD`.
    pub fn describe(&self) -> Vec<String> {
        let mut v = Vec::new();
        let mut push = |val: f32, fmt: &dyn Fn(f32) -> String| {
            if val != 0.0 {
                v.push(fmt(val));
            }
        };
        push(self.hp, &|x| format!("+{x:.0} Health"));
        push(self.mana, &|x| format!("+{x:.0} Mana"));
        push(self.ad, &|x| format!("+{x:.0} Attack Dmg"));
        push(self.ap, &|x| format!("+{x:.0} Ability Pwr"));
        push(self.armor, &|x| format!("+{x:.0} Armor"));
        push(self.mr, &|x| format!("+{x:.0} Magic Res"));
        push(self.attack_speed, &|x| format!("+{:.0}% Attack Spd", x * 100.0));
        push(self.move_speed, &|x| format!("+{x:.0} Move Spd"));
        push(self.crit, &|x| format!("+{:.0}% Crit", x * 100.0));
        push(self.lifesteal, &|x| format!("+{:.0}% Lifesteal", x * 100.0));
        push(self.haste, &|x| format!("+{x:.0} Ability Haste"));
        push(self.hp_regen, &|x| format!("+{x:.1} HP/s"));
        push(self.mana_regen, &|x| format!("+{x:.1} Mana/s"));
        v
    }
}

impl AddAssign for StatBlock {
    fn add_assign(&mut self, o: StatBlock) {
        self.hp += o.hp;
        self.mana += o.mana;
        self.ad += o.ad;
        self.ap += o.ap;
        self.armor += o.armor;
        self.mr += o.mr;
        self.attack_speed += o.attack_speed;
        self.move_speed += o.move_speed;
        self.crit += o.crit;
        self.lifesteal += o.lifesteal;
        self.haste += o.haste;
        self.hp_regen += o.hp_regen;
        self.mana_regen += o.mana_regen;
    }
}

/// Final, fully computed stats of a unit at a point in time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub max_hp: f32,
    pub max_mana: f32,
    pub ad: f32,
    /// AD from level only (used for "bonus AD" ratios).
    pub base_ad: f32,
    pub ap: f32,
    pub armor: f32,
    pub mr: f32,
    /// Attacks per second.
    pub attack_speed: f32,
    pub range: f32,
    pub move_speed: f32,
    pub crit: f32,
    pub lifesteal: f32,
    pub haste: f32,
    pub hp_regen: f32,
    pub mana_regen: f32,
}

impl Stats {
    pub fn bonus_ad(&self) -> f32 {
        (self.ad - self.base_ad).max(0.0)
    }
}

/// Damage multiplier for a resist value (armor or magic resist), LoL style.
pub fn resist_multiplier(resist: f32) -> f32 {
    if resist >= 0.0 {
        100.0 / (100.0 + resist)
    } else {
        2.0 - 100.0 / (100.0 - resist)
    }
}

/// Cooldown after ability haste.
pub fn haste_cooldown(cd: f32, haste: f32) -> f32 {
    cd * 100.0 / (100.0 + haste.max(0.0))
}
