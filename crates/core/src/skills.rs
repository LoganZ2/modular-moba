//! Data-driven skills.
//!
//! A skill is a combination of a [`Targeting`] mode, an [`Action`] (the shape
//! of what happens: projectile, area, dash, ...) and lists of [`Effect`]s
//! (damage, crowd control, shields, buffs ...). The simulation interprets these
//! building blocks generically, so adding a new skill means adding one entry
//! to [`SKILLS`] — no new code.
//!
//! Basic skills (ids 1..=99) carry two roles and can be slotted into Q/W/E by
//! any hero sharing at least one of those roles. Ultimates (ids 100+) belong to
//! exactly one hero and always sit in R.

use crate::roles::{Role, RolePair};
use crate::stats::Stats;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SkillId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DamageKind {
    Physical,
    Magic,
    True,
}

impl DamageKind {
    pub fn name(self) -> &'static str {
        match self {
            DamageKind::Physical => "phys",
            DamageKind::Magic => "magic",
            DamageKind::True => "true",
        }
    }
}

/// A number that grows with skill rank and with the caster's stats.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scaling {
    pub base: f32,
    pub per_rank: f32,
    /// Ratio of total attack damage.
    pub ad: f32,
    /// Ratio of bonus attack damage (items/buffs).
    pub bonus_ad: f32,
    /// Ratio of ability power.
    pub ap: f32,
    /// Ratio of the caster's maximum health.
    pub max_hp: f32,
    /// Ratio of the *target's* missing health (execute style).
    pub missing_hp: f32,
}

/// Shorthand constructor for [`Scaling`].
pub const fn s(base: f32, per_rank: f32) -> Scaling {
    Scaling { base, per_rank, ad: 0.0, bonus_ad: 0.0, ap: 0.0, max_hp: 0.0, missing_hp: 0.0 }
}

impl Scaling {
    pub const fn ad(self, r: f32) -> Self {
        Scaling { ad: r, ..self }
    }
    pub const fn bonus_ad(self, r: f32) -> Self {
        Scaling { bonus_ad: r, ..self }
    }
    pub const fn ap(self, r: f32) -> Self {
        Scaling { ap: r, ..self }
    }
    pub const fn max_hp(self, r: f32) -> Self {
        Scaling { max_hp: r, ..self }
    }
    pub const fn missing_hp(self, r: f32) -> Self {
        Scaling { missing_hp: r, ..self }
    }

    /// Value at `rank` (1-based) without target dependent parts.
    pub fn eval(&self, rank: u8, caster: &Stats) -> f32 {
        let r = rank.max(1) as f32 - 1.0;
        self.base
            + self.per_rank * r
            + self.ad * caster.ad
            + self.bonus_ad * caster.bonus_ad()
            + self.ap * caster.ap
            + self.max_hp * caster.max_hp
    }

    /// Value including the part that depends on the target's missing health.
    pub fn eval_vs(&self, rank: u8, caster: &Stats, target_missing_hp: f32) -> f32 {
        self.eval(rank, caster) + self.missing_hp * target_missing_hp.max(0.0)
    }

    /// Text such as `95 (+50% AD)`.
    pub fn describe(&self, rank: u8, caster: Option<&Stats>, pct: bool) -> String {
        let mut out = match caster {
            Some(st) => fmt_num(self.eval(rank, st), pct),
            None => {
                let r = rank.max(1) as f32 - 1.0;
                fmt_num(self.base + self.per_rank * r, pct)
            }
        };
        let mut ratios = Vec::new();
        if self.ad != 0.0 {
            ratios.push(format!("{:.0}% AD", self.ad * 100.0));
        }
        if self.bonus_ad != 0.0 {
            ratios.push(format!("{:.0}% bonus AD", self.bonus_ad * 100.0));
        }
        if self.ap != 0.0 {
            ratios.push(format!("{:.0}% AP", self.ap * 100.0));
        }
        if self.max_hp != 0.0 {
            ratios.push(format!("{:.0}% max HP", self.max_hp * 100.0));
        }
        if self.missing_hp != 0.0 {
            ratios.push(format!("{:.0}% target missing HP", self.missing_hp * 100.0));
        }
        if !ratios.is_empty() && caster.is_none() {
            out = format!("{out} (+{})", ratios.join(" +"));
        } else if self.missing_hp != 0.0 {
            out = format!("{out} (+{:.0}% missing HP)", self.missing_hp * 100.0);
        }
        out
    }
}

fn fmt_num(v: f32, pct: bool) -> String {
    if pct {
        format!("{:.0}%", v * 100.0)
    } else {
        format!("{v:.0}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuffStat {
    /// Fraction, e.g. 0.25 = +25% move speed.
    MoveSpeed,
    /// Fraction, e.g. 0.4 = +40% attack speed.
    AttackSpeed,
    Armor,
    MagicResist,
    AttackDamage,
    AbilityPower,
}

impl BuffStat {
    pub fn name(self) -> &'static str {
        match self {
            BuffStat::MoveSpeed => "move speed",
            BuffStat::AttackSpeed => "attack speed",
            BuffStat::Armor => "armor",
            BuffStat::MagicResist => "magic resist",
            BuffStat::AttackDamage => "attack damage",
            BuffStat::AbilityPower => "ability power",
        }
    }
    pub fn is_pct(self) -> bool {
        matches!(self, BuffStat::MoveSpeed | BuffStat::AttackSpeed)
    }
}

/// The atomic things a skill can do to a unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Damage(DamageKind, Scaling),
    Heal(Scaling),
    Shield {
        amount: Scaling,
        secs: f32,
    },
    Slow {
        pct: f32,
        secs: f32,
    },
    Stun(f32),
    Root(f32),
    Knockup(f32),
    Silence(f32),
    /// Forces the target to attack the caster.
    Taunt(f32),
    /// Basic attacks miss.
    Blind(f32),
    /// Push the target away from the effect origin; stun if it hits a wall.
    Knockback {
        dist: f32,
        wall_stun: f32,
    },
    /// Pull the target next to the caster.
    Pull,
    Buff {
        stat: BuffStat,
        amount: Scaling,
        secs: f32,
    },
    /// Target takes `pct` more damage from all sources.
    Vulnerable {
        pct: f32,
        secs: f32,
    },
    /// Next basic attack deals bonus physical damage (self only).
    Empower(Scaling),
}

impl Effect {
    pub fn is_hard_cc(&self) -> bool {
        matches!(
            self,
            Effect::Stun(_) | Effect::Knockup(_) | Effect::Taunt(_) | Effect::Pull | Effect::Knockback { .. }
        )
    }

    pub fn describe(&self, rank: u8, caster: Option<&Stats>) -> String {
        match self {
            Effect::Damage(k, sc) => {
                format!("{} {} damage", sc.describe(rank, caster, false), k.name())
            }
            Effect::Heal(sc) => format!("heal {}", sc.describe(rank, caster, false)),
            Effect::Shield { amount, secs } => format!("{} shield for {secs}s", amount.describe(rank, caster, false)),
            Effect::Slow { pct, secs } => format!("slow {:.0}% for {secs}s", pct * 100.0),
            Effect::Stun(t) => format!("stun {t}s"),
            Effect::Root(t) => format!("root {t}s"),
            Effect::Knockup(t) => format!("knock up {t}s"),
            Effect::Silence(t) => format!("silence {t}s"),
            Effect::Taunt(t) => format!("taunt {t}s"),
            Effect::Blind(t) => format!("blind {t}s"),
            Effect::Knockback { dist, wall_stun } => {
                if *wall_stun > 0.0 {
                    format!("knock back {dist:.0} (stun {wall_stun}s on wall hit)")
                } else {
                    format!("knock back {dist:.0}")
                }
            }
            Effect::Pull => "pull to caster".to_string(),
            Effect::Buff { stat, amount, secs } => {
                format!("+{} {} for {secs}s", amount.describe(rank, caster, stat.is_pct()), stat.name())
            }
            Effect::Vulnerable { pct, secs } => {
                format!("+{:.0}% damage taken for {secs}s", pct * 100.0)
            }
            Effect::Empower(sc) => {
                format!("next attack +{} damage", sc.describe(rank, caster, false))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Targeting {
    /// Cast instantly around / on yourself.
    Instant,
    /// Skillshot toward the cursor (clamped to range).
    Direction { range: f32 },
    /// Ground target. Dashes/blinks clamp to range, areas walk into range.
    Point { range: f32 },
    /// Target an enemy unit.
    Enemy { range: f32 },
    /// Target an enemy champion only.
    EnemyHero { range: f32 },
    /// Target an allied champion (self-cast allowed).
    Ally { range: f32 },
}

impl Targeting {
    pub fn range(&self) -> f32 {
        match *self {
            Targeting::Instant => 0.0,
            Targeting::Direction { range }
            | Targeting::Point { range }
            | Targeting::Enemy { range }
            | Targeting::EnemyHero { range }
            | Targeting::Ally { range } => range,
        }
    }

    pub fn needs_unit(&self) -> bool {
        matches!(self, Targeting::Enemy { .. } | Targeting::EnemyHero { .. } | Targeting::Ally { .. })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Targeting::Instant => "self",
            Targeting::Direction { .. } => "skillshot",
            Targeting::Point { .. } => "ground",
            Targeting::Enemy { .. } => "enemy unit",
            Targeting::EnemyHero { .. } => "enemy champion",
            Targeting::Ally { .. } => "ally",
        }
    }
}

/// What shape the skill takes in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Only `self_effects` apply.
    None,
    /// Linear skillshot (Direction) or homing missile (unit targeted).
    Projectile { speed: f32, width: f32, pierce: bool, on_hit: &'static [Effect] },
    /// Circle around the caster, or at the target point when `at_target`.
    Area { radius: f32, delay: f32, at_target: bool, enemy: &'static [Effect], ally: &'static [Effect] },
    /// Cone in front of the caster; length = targeting range.
    Cone { half_angle_deg: f32, enemy: &'static [Effect] },
    /// Beam from the caster; length = targeting range.
    Line { width: f32, delay: f32, enemy: &'static [Effect], ally: &'static [Effect] },
    /// Physically travel. `hit` applies to the dash target (unit targeted) or to
    /// enemies passed through (direction). `arrive` applies around the landing spot.
    Dash {
        speed: f32,
        stop_on_enemy: bool,
        hit: &'static [Effect],
        arrive_radius: f32,
        arrive: &'static [Effect],
        unstoppable: bool,
    },
    /// Instant teleport, then `arrive` around the landing spot.
    Blink { arrive_radius: f32, arrive: &'static [Effect] },
    /// Instant effect on the targeted unit.
    Unit { effects: &'static [Effect] },
    /// Hidden-ish trap that triggers on the first enemy champion or minion.
    Trap { radius: f32, lifetime: f32, effects: &'static [Effect] },
}

impl Action {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Action::None => "self",
            Action::Projectile { .. } => "projectile",
            Action::Area { .. } => "area",
            Action::Cone { .. } => "cone",
            Action::Line { .. } => "beam",
            Action::Dash { .. } => "dash",
            Action::Blink { .. } => "blink",
            Action::Unit { .. } => "targeted",
            Action::Trap { .. } => "trap",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillDef {
    pub id: SkillId,
    pub name: &'static str,
    pub name_zh: &'static str,
    /// For ultimates this mirrors the owning hero's roles (informational).
    pub roles: RolePair,
    pub ultimate: bool,
    pub desc: &'static str,
    pub targeting: Targeting,
    pub action: Action,
    /// Applied to the caster when the skill goes off.
    pub self_effects: &'static [Effect],
    /// (cooldown at rank 1, reduction per extra rank) in seconds.
    pub cooldown: (f32, f32),
    /// (mana at rank 1, increase per extra rank).
    pub mana: (f32, f32),
    /// Wind-up before the skill fires; the caster can't move meanwhile.
    pub cast_time: f32,
    /// Theme colour for VFX and icon (0xRRGGBB).
    pub color: u32,
}

impl SkillDef {
    pub fn max_rank(&self) -> u8 {
        if self.ultimate {
            3
        } else {
            5
        }
    }

    pub fn cooldown_at(&self, rank: u8) -> f32 {
        let r = rank.max(1) as f32 - 1.0;
        (self.cooldown.0 - self.cooldown.1 * r).max(0.5)
    }

    pub fn mana_at(&self, rank: u8) -> f32 {
        let r = rank.max(1) as f32 - 1.0;
        (self.mana.0 + self.mana.1 * r).max(0.0)
    }

    pub fn range(&self) -> f32 {
        self.targeting.range()
    }

    /// Plain-language breakdown used for tooltips.
    pub fn detail_lines(&self, rank: u8, caster: Option<&Stats>) -> Vec<String> {
        let mut lines = Vec::new();
        let join = |effects: &[Effect]| -> String {
            effects.iter().map(|e| e.describe(rank, caster)).collect::<Vec<_>>().join(", ")
        };
        match self.action {
            Action::None => {}
            Action::Projectile { on_hit, pierce, .. } => {
                let p = if pierce { " (pierces)" } else { "" };
                lines.push(format!("On hit{p}: {}", join(on_hit)));
            }
            Action::Area { radius, delay, enemy, ally, .. } => {
                let d = if delay > 0.0 { format!(" after {delay}s") } else { String::new() };
                if !enemy.is_empty() {
                    lines.push(format!("Enemies in {radius:.0}{d}: {}", join(enemy)));
                }
                if !ally.is_empty() {
                    lines.push(format!("Allies in {radius:.0}{d}: {}", join(ally)));
                }
            }
            Action::Cone { enemy, .. } => lines.push(format!("Enemies in cone: {}", join(enemy))),
            Action::Line { enemy, ally, delay, .. } => {
                let d = if delay > 0.0 { format!(" after {delay}s") } else { String::new() };
                if !enemy.is_empty() {
                    lines.push(format!("Enemies hit{d}: {}", join(enemy)));
                }
                if !ally.is_empty() {
                    lines.push(format!("Allies hit{d}: {}", join(ally)));
                }
            }
            Action::Dash { hit, arrive, unstoppable, .. } => {
                if !hit.is_empty() {
                    lines.push(format!("Hit: {}", join(hit)));
                }
                if !arrive.is_empty() {
                    lines.push(format!("On landing: {}", join(arrive)));
                }
                if unstoppable {
                    lines.push("Unstoppable while dashing".to_string());
                }
            }
            Action::Blink { arrive, .. } => {
                if !arrive.is_empty() {
                    lines.push(format!("On arrival: {}", join(arrive)));
                }
            }
            Action::Unit { effects } => lines.push(format!("Target: {}", join(effects))),
            Action::Trap { lifetime, effects, .. } => lines.push(format!("Trap ({lifetime:.0}s): {}", join(effects))),
        }
        if !self.self_effects.is_empty() {
            lines.push(format!("Self: {}", join(self.self_effects)));
        }
        lines
    }
}

use DamageKind::{Magic, Physical};
use Role::*;

const NO_FX: &[Effect] = &[];

/// The whole skill catalog. Basic skills: 24 (every role appears on exactly 8
/// of them). Ultimates: one per hero.
pub static SKILLS: &[SkillDef] = &[
    // ---------------------------------------------------------------- Tank / Fighter
    SkillDef {
        id: SkillId(1),
        name: "Shield Bash",
        name_zh: "盾击冲锋",
        roles: [Tank, Fighter],
        ultimate: false,
        desc: "Charge forward. The first enemy hit is stunned.",
        targeting: Targeting::Direction { range: 150.0 },
        action: Action::Dash {
            speed: 520.0,
            stop_on_enemy: true,
            hit: &[Effect::Damage(Physical, s(60.0, 35.0).ad(0.5)), Effect::Stun(0.75)],
            arrive_radius: 0.0,
            arrive: NO_FX,
            unstoppable: false,
        },
        self_effects: NO_FX,
        cooldown: (12.0, 1.0),
        mana: (50.0, 5.0),
        cast_time: 0.0,
        color: 0xb8c8e0,
    },
    SkillDef {
        id: SkillId(2),
        name: "Ground Slam",
        name_zh: "大地震击",
        roles: [Tank, Fighter],
        ultimate: false,
        desc: "Slam the ground, damaging and slowing nearby enemies.",
        targeting: Targeting::Instant,
        action: Action::Area {
            radius: 100.0,
            delay: 0.0,
            at_target: false,
            enemy: &[Effect::Damage(Physical, s(70.0, 35.0).ad(0.6)), Effect::Slow { pct: 0.35, secs: 1.5 }],
            ally: NO_FX,
        },
        self_effects: NO_FX,
        cooldown: (9.0, 0.5),
        mana: (40.0, 5.0),
        cast_time: 0.0,
        color: 0xa8804a,
    },
    // ---------------------------------------------------------------- Tank / Support
    SkillDef {
        id: SkillId(3),
        name: "Taunting Roar",
        name_zh: "挑衅怒吼",
        roles: [Tank, Support],
        ultimate: false,
        desc: "Roar to taunt nearby enemies and harden yourself.",
        targeting: Targeting::Instant,
        action: Action::Area {
            radius: 110.0,
            delay: 0.0,
            at_target: false,
            enemy: &[Effect::Taunt(1.0), Effect::Damage(Magic, s(40.0, 25.0).max_hp(0.03))],
            ally: NO_FX,
        },
        self_effects: &[
            Effect::Buff { stat: BuffStat::Armor, amount: s(20.0, 5.0), secs: 3.0 },
            Effect::Buff { stat: BuffStat::MagicResist, amount: s(20.0, 5.0), secs: 3.0 },
        ],
        cooldown: (14.0, 1.0),
        mana: (60.0, 0.0),
        cast_time: 0.0,
        color: 0xe04848,
    },
    SkillDef {
        id: SkillId(4),
        name: "Guardian Link",
        name_zh: "守护链接",
        roles: [Tank, Support],
        ultimate: false,
        desc: "Shield an allied champion and yourself.",
        targeting: Targeting::Ally { range: 200.0 },
        action: Action::Unit { effects: &[Effect::Shield { amount: s(50.0, 30.0).max_hp(0.06), secs: 3.0 }] },
        self_effects: &[Effect::Shield { amount: s(50.0, 30.0).max_hp(0.06), secs: 3.0 }],
        cooldown: (14.0, 1.0),
        mana: (60.0, 5.0),
        cast_time: 0.0,
        color: 0x80d8ff,
    },
    // ---------------------------------------------------------------- Tank / Mage
    SkillDef {
        id: SkillId(5),
        name: "Seismic Shard",
        name_zh: "地震碎片",
        roles: [Tank, Mage],
        ultimate: false,
        desc: "Hurl a shard of earth at an enemy, slowing it and speeding you up.",
        targeting: Targeting::Enemy { range: 250.0 },
        action: Action::Projectile {
            speed: 420.0,
            width: 0.0,
            pierce: false,
            on_hit: &[Effect::Damage(Magic, s(70.0, 45.0).ap(0.6)), Effect::Slow { pct: 0.3, secs: 2.0 }],
        },
        self_effects: &[Effect::Buff { stat: BuffStat::MoveSpeed, amount: s(0.15, 0.02), secs: 2.0 }],
        cooldown: (8.0, 0.5),
        mana: (60.0, 5.0),
        cast_time: 0.0,
        color: 0x9a7848,
    },
    SkillDef {
        id: SkillId(6),
        name: "Stone Skin",
        name_zh: "石肤术",
        roles: [Tank, Mage],
        ultimate: false,
        desc: "Gain a shield that scales with max health, plus armor.",
        targeting: Targeting::Instant,
        action: Action::None,
        self_effects: &[
            Effect::Shield { amount: s(40.0, 30.0).max_hp(0.07).ap(0.4), secs: 4.0 },
            Effect::Buff { stat: BuffStat::Armor, amount: s(10.0, 5.0), secs: 4.0 },
        ],
        cooldown: (15.0, 1.0),
        mana: (60.0, 5.0),
        cast_time: 0.0,
        color: 0xa8a8a8,
    },
    // ---------------------------------------------------------------- Tank / Assassin
    SkillDef {
        id: SkillId(7),
        name: "Hook",
        name_zh: "钩锁",
        roles: [Tank, Assassin],
        ultimate: false,
        desc: "Throw a hook that pulls the first enemy hit to you.",
        targeting: Targeting::Direction { range: 260.0 },
        action: Action::Projectile {
            speed: 460.0,
            width: 12.0,
            pierce: false,
            on_hit: &[Effect::Damage(Magic, s(80.0, 45.0).ap(0.6)), Effect::Pull, Effect::Stun(0.5)],
        },
        self_effects: NO_FX,
        cooldown: (18.0, 1.0),
        mana: (80.0, 0.0),
        cast_time: 0.15,
        color: 0xc8a868,
    },
    // ---------------------------------------------------------------- Tank / Marksman
    SkillDef {
        id: SkillId(8),
        name: "Repel Shot",
        name_zh: "击退射击",
        roles: [Tank, Marksman],
        ultimate: false,
        desc: "Blast an enemy backwards. Stuns if it hits a wall.",
        targeting: Targeting::Enemy { range: 140.0 },
        action: Action::Projectile {
            speed: 600.0,
            width: 0.0,
            pierce: false,
            on_hit: &[
                Effect::Damage(Physical, s(50.0, 35.0).bonus_ad(0.5)),
                Effect::Knockback { dist: 80.0, wall_stun: 1.25 },
            ],
        },
        self_effects: NO_FX,
        cooldown: (12.0, 1.0),
        mana: (50.0, 0.0),
        cast_time: 0.0,
        color: 0xe0e0ff,
    },
    // ---------------------------------------------------------------- Fighter / Assassin
    SkillDef {
        id: SkillId(9),
        name: "Lunge",
        name_zh: "突刺",
        roles: [Fighter, Assassin],
        ultimate: false,
        desc: "Dash to an enemy and strike it.",
        targeting: Targeting::Enemy { range: 150.0 },
        action: Action::Dash {
            speed: 600.0,
            stop_on_enemy: false,
            hit: &[Effect::Damage(Physical, s(50.0, 30.0).ad(0.8))],
            arrive_radius: 0.0,
            arrive: NO_FX,
            unstoppable: false,
        },
        self_effects: NO_FX,
        cooldown: (9.0, 1.0),
        mana: (40.0, 0.0),
        cast_time: 0.0,
        color: 0xff8848,
    },
    SkillDef {
        id: SkillId(10),
        name: "Whirling Blades",
        name_zh: "旋风刃",
        roles: [Fighter, Assassin],
        ultimate: false,
        desc: "Spin, cutting every nearby enemy and healing yourself.",
        targeting: Targeting::Instant,
        action: Action::Area {
            radius: 90.0,
            delay: 0.0,
            at_target: false,
            enemy: &[Effect::Damage(Physical, s(55.0, 30.0).ad(0.9))],
            ally: NO_FX,
        },
        self_effects: &[Effect::Heal(s(20.0, 10.0).ad(0.2))],
        cooldown: (7.0, 0.5),
        mana: (35.0, 0.0),
        cast_time: 0.0,
        color: 0xd8d8d8,
    },
    // ---------------------------------------------------------------- Fighter / Support
    SkillDef {
        id: SkillId(11),
        name: "Rallying Cry",
        name_zh: "集结号令",
        roles: [Fighter, Support],
        ultimate: false,
        desc: "Nearby allies gain move speed and attack speed.",
        targeting: Targeting::Instant,
        action: Action::Area {
            radius: 220.0,
            delay: 0.0,
            at_target: false,
            enemy: NO_FX,
            ally: &[
                Effect::Buff { stat: BuffStat::MoveSpeed, amount: s(0.25, 0.0), secs: 3.0 },
                Effect::Buff { stat: BuffStat::AttackSpeed, amount: s(0.25, 0.05), secs: 3.0 },
            ],
        },
        self_effects: NO_FX,
        cooldown: (18.0, 1.0),
        mana: (60.0, 0.0),
        cast_time: 0.0,
        color: 0xffd848,
    },
    // ---------------------------------------------------------------- Fighter / Marksman
    SkillDef {
        id: SkillId(12),
        name: "Tumble",
        name_zh: "翻滚",
        roles: [Fighter, Marksman],
        ultimate: false,
        desc: "Roll a short distance. Your next attack hits harder.",
        targeting: Targeting::Direction { range: 75.0 },
        action: Action::Dash {
            speed: 500.0,
            stop_on_enemy: false,
            hit: NO_FX,
            arrive_radius: 0.0,
            arrive: NO_FX,
            unstoppable: false,
        },
        self_effects: &[Effect::Empower(s(20.0, 15.0).ad(0.4))],
        cooldown: (6.0, 0.5),
        mana: (30.0, 0.0),
        cast_time: 0.0,
        color: 0xf0f0a0,
    },
    SkillDef {
        id: SkillId(13),
        name: "Rapid Fire",
        name_zh: "急速射击",
        roles: [Fighter, Marksman],
        ultimate: false,
        desc: "Greatly increase attack speed for a few seconds.",
        targeting: Targeting::Instant,
        action: Action::None,
        self_effects: &[
            Effect::Buff { stat: BuffStat::AttackSpeed, amount: s(0.4, 0.1), secs: 4.0 },
            Effect::Buff { stat: BuffStat::MoveSpeed, amount: s(0.1, 0.0), secs: 4.0 },
        ],
        cooldown: (16.0, 1.0),
        mana: (50.0, 0.0),
        cast_time: 0.0,
        color: 0xffb038,
    },
    // ---------------------------------------------------------------- Fighter / Mage
    SkillDef {
        id: SkillId(14),
        name: "Infernal Cleave",
        name_zh: "炼狱横扫",
        roles: [Fighter, Mage],
        ultimate: false,
        desc: "Sweep a burning blade in a cone, slowing enemies.",
        targeting: Targeting::Direction { range: 120.0 },
        action: Action::Cone {
            half_angle_deg: 45.0,
            enemy: &[Effect::Damage(Magic, s(60.0, 40.0).ap(0.5).ad(0.3)), Effect::Slow { pct: 0.25, secs: 1.0 }],
        },
        self_effects: NO_FX,
        cooldown: (8.0, 0.5),
        mana: (50.0, 5.0),
        cast_time: 0.1,
        color: 0xff5828,
    },
    // ---------------------------------------------------------------- Assassin / Mage
    SkillDef {
        id: SkillId(15),
        name: "Shadow Blink",
        name_zh: "暗影闪烁",
        roles: [Assassin, Mage],
        ultimate: false,
        desc: "Teleport and burst enemies where you land.",
        targeting: Targeting::Point { range: 160.0 },
        action: Action::Blink { arrive_radius: 70.0, arrive: &[Effect::Damage(Magic, s(60.0, 40.0).ap(0.6))] },
        self_effects: NO_FX,
        cooldown: (14.0, 1.0),
        mana: (60.0, 5.0),
        cast_time: 0.0,
        color: 0x8848c8,
    },
    SkillDef {
        id: SkillId(16),
        name: "Void Bolt",
        name_zh: "虚空飞弹",
        roles: [Assassin, Mage],
        ultimate: false,
        desc: "Fire a fast bolt of void energy.",
        targeting: Targeting::Direction { range: 250.0 },
        action: Action::Projectile {
            speed: 560.0,
            width: 12.0,
            pierce: false,
            on_hit: &[Effect::Damage(Magic, s(80.0, 50.0).ap(0.75))],
        },
        self_effects: NO_FX,
        cooldown: (7.0, 0.5),
        mana: (50.0, 5.0),
        cast_time: 0.1,
        color: 0xa848ff,
    },
    // ---------------------------------------------------------------- Assassin / Marksman
    SkillDef {
        id: SkillId(17),
        name: "Piercing Arrow",
        name_zh: "穿透之箭",
        roles: [Assassin, Marksman],
        ultimate: false,
        desc: "An arrow that passes through every enemy in a line.",
        targeting: Targeting::Direction { range: 280.0 },
        action: Action::Projectile {
            speed: 620.0,
            width: 14.0,
            pierce: true,
            on_hit: &[Effect::Damage(Physical, s(50.0, 40.0).ad(0.9))],
        },
        self_effects: NO_FX,
        cooldown: (9.0, 0.5),
        mana: (55.0, 5.0),
        cast_time: 0.15,
        color: 0xe0f088,
    },
    SkillDef {
        id: SkillId(18),
        name: "Marked for Death",
        name_zh: "死亡印记",
        roles: [Assassin, Marksman],
        ultimate: false,
        desc: "Mark an enemy: it takes more damage from everything.",
        targeting: Targeting::Enemy { range: 200.0 },
        action: Action::Unit {
            effects: &[
                Effect::Vulnerable { pct: 0.15, secs: 4.0 },
                Effect::Damage(Physical, s(30.0, 20.0).bonus_ad(0.5)),
            ],
        },
        self_effects: NO_FX,
        cooldown: (14.0, 1.0),
        mana: (40.0, 0.0),
        cast_time: 0.0,
        color: 0xff3868,
    },
    // ---------------------------------------------------------------- Assassin / Support
    SkillDef {
        id: SkillId(19),
        name: "Smoke Bomb",
        name_zh: "烟雾弹",
        roles: [Assassin, Support],
        ultimate: false,
        desc: "Smoke blinds and slows enemies; allies inside speed up.",
        targeting: Targeting::Point { range: 200.0 },
        action: Action::Area {
            radius: 85.0,
            delay: 0.0,
            at_target: true,
            enemy: &[Effect::Blind(1.5), Effect::Slow { pct: 0.3, secs: 1.5 }],
            ally: &[Effect::Buff { stat: BuffStat::MoveSpeed, amount: s(0.2, 0.03), secs: 2.0 }],
        },
        self_effects: NO_FX,
        cooldown: (16.0, 1.0),
        mana: (60.0, 0.0),
        cast_time: 0.0,
        color: 0x9090a0,
    },
    // ---------------------------------------------------------------- Mage / Support
    SkillDef {
        id: SkillId(20),
        name: "Healing Wave",
        name_zh: "治疗波",
        roles: [Mage, Support],
        ultimate: false,
        desc: "A burst of light heals allies and hurts enemies.",
        targeting: Targeting::Point { range: 220.0 },
        action: Action::Area {
            radius: 85.0,
            delay: 0.0,
            at_target: true,
            enemy: &[Effect::Damage(Magic, s(40.0, 30.0).ap(0.3))],
            ally: &[Effect::Heal(s(50.0, 30.0).ap(0.35))],
        },
        self_effects: NO_FX,
        cooldown: (10.0, 0.5),
        mana: (70.0, 5.0),
        cast_time: 0.1,
        color: 0x68ff98,
    },
    SkillDef {
        id: SkillId(21),
        name: "Frost Nova",
        name_zh: "冰霜新星",
        roles: [Mage, Support],
        ultimate: false,
        desc: "After a short delay, freeze enemies in place.",
        targeting: Targeting::Point { range: 230.0 },
        action: Action::Area {
            radius: 75.0,
            delay: 0.6,
            at_target: true,
            enemy: &[Effect::Damage(Magic, s(70.0, 40.0).ap(0.6)), Effect::Root(1.25)],
            ally: NO_FX,
        },
        self_effects: NO_FX,
        cooldown: (12.0, 0.5),
        mana: (70.0, 5.0),
        cast_time: 0.1,
        color: 0x88e8ff,
    },
    // ---------------------------------------------------------------- Mage / Marksman
    SkillDef {
        id: SkillId(22),
        name: "Mystic Shot",
        name_zh: "秘术射击",
        roles: [Mage, Marksman],
        ultimate: false,
        desc: "A long range bolt that scales with both AD and AP.",
        targeting: Targeting::Direction { range: 280.0 },
        action: Action::Projectile {
            speed: 650.0,
            width: 10.0,
            pierce: false,
            on_hit: &[Effect::Damage(Physical, s(20.0, 25.0).ad(1.1).ap(0.4))],
        },
        self_effects: NO_FX,
        cooldown: (5.5, 0.5),
        mana: (30.0, 3.0),
        cast_time: 0.1,
        color: 0x68a8ff,
    },
    // ---------------------------------------------------------------- Marksman / Support
    SkillDef {
        id: SkillId(23),
        name: "Volley",
        name_zh: "齐射",
        roles: [Marksman, Support],
        ultimate: false,
        desc: "Fire a cone of arrows that slows.",
        targeting: Targeting::Direction { range: 220.0 },
        action: Action::Cone {
            half_angle_deg: 25.0,
            enemy: &[Effect::Damage(Physical, s(30.0, 25.0).ad(0.8)), Effect::Slow { pct: 0.25, secs: 2.0 }],
        },
        self_effects: NO_FX,
        cooldown: (10.0, 1.0),
        mana: (50.0, 0.0),
        cast_time: 0.1,
        color: 0xc8e8ff,
    },
    SkillDef {
        id: SkillId(24),
        name: "Snare Trap",
        name_zh: "捕兽夹",
        roles: [Marksman, Support],
        ultimate: false,
        desc: "Set a trap that roots the first enemy to step on it.",
        targeting: Targeting::Point { range: 150.0 },
        action: Action::Trap {
            radius: 22.0,
            lifetime: 30.0,
            effects: &[Effect::Root(1.5), Effect::Damage(Physical, s(40.0, 30.0).bonus_ad(0.4))],
        },
        self_effects: NO_FX,
        cooldown: (14.0, 1.0),
        mana: (40.0, 0.0),
        cast_time: 0.0,
        color: 0xb88848,
    },
    // ================================================================ Ultimates (R)
    SkillDef {
        id: SkillId(101),
        name: "Earthshatter",
        name_zh: "裂地",
        roles: [Tank, Fighter],
        ultimate: true,
        desc: "Wind up, then shatter the ground, knocking up all nearby enemies.",
        targeting: Targeting::Instant,
        action: Action::Area {
            radius: 140.0,
            delay: 0.0,
            at_target: false,
            enemy: &[Effect::Damage(Physical, s(150.0, 100.0).bonus_ad(0.6).max_hp(0.04)), Effect::Knockup(1.0)],
            ally: NO_FX,
        },
        self_effects: NO_FX,
        cooldown: (90.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 0.4,
        color: 0xc88848,
    },
    SkillDef {
        id: SkillId(102),
        name: "Sanctuary",
        name_zh: "圣域",
        roles: [Tank, Support],
        ultimate: true,
        desc: "Shield every nearby ally and heavily slow nearby enemies.",
        targeting: Targeting::Instant,
        action: Action::Area {
            radius: 260.0,
            delay: 0.0,
            at_target: false,
            enemy: &[Effect::Slow { pct: 0.5, secs: 2.0 }],
            ally: &[Effect::Shield { amount: s(120.0, 80.0).max_hp(0.08), secs: 4.0 }],
        },
        self_effects: NO_FX,
        cooldown: (100.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 0.0,
        color: 0xfff0a0,
    },
    SkillDef {
        id: SkillId(103),
        name: "Unstoppable Force",
        name_zh: "势不可挡",
        roles: [Tank, Mage],
        ultimate: true,
        desc: "Charge to a location, knocking up everyone where you land.",
        targeting: Targeting::Point { range: 300.0 },
        action: Action::Dash {
            speed: 700.0,
            stop_on_enemy: false,
            hit: NO_FX,
            arrive_radius: 110.0,
            arrive: &[Effect::Damage(Magic, s(200.0, 100.0).ap(0.8)), Effect::Knockup(1.25)],
            unstoppable: true,
        },
        self_effects: NO_FX,
        cooldown: (100.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 0.0,
        color: 0x9898b0,
    },
    SkillDef {
        id: SkillId(104),
        name: "Execution",
        name_zh: "处决",
        roles: [Fighter, Assassin],
        ultimate: true,
        desc: "Leap at an enemy champion. Deals more damage the lower its health.",
        targeting: Targeting::EnemyHero { range: 170.0 },
        action: Action::Dash {
            speed: 750.0,
            stop_on_enemy: false,
            hit: &[Effect::Damage(Physical, s(100.0, 100.0).bonus_ad(0.8).missing_hp(0.25))],
            arrive_radius: 0.0,
            arrive: NO_FX,
            unstoppable: true,
        },
        self_effects: NO_FX,
        cooldown: (90.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 0.0,
        color: 0xff2828,
    },
    SkillDef {
        id: SkillId(105),
        name: "Skyfall",
        name_zh: "天降神兵",
        roles: [Fighter, Support],
        ultimate: true,
        desc: "Channel, then drop from the sky onto a distant location.",
        targeting: Targeting::Point { range: 700.0 },
        action: Action::Blink {
            arrive_radius: 120.0,
            arrive: &[Effect::Damage(Magic, s(200.0, 100.0).ad(0.8)), Effect::Slow { pct: 0.5, secs: 1.5 }],
        },
        self_effects: NO_FX,
        cooldown: (110.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 1.25,
        color: 0xffe068,
    },
    SkillDef {
        id: SkillId(106),
        name: "Void Rift",
        name_zh: "虚空裂隙",
        roles: [Assassin, Mage],
        ultimate: true,
        desc: "Tear through space, silencing and blasting enemies at the exit.",
        targeting: Targeting::Point { range: 220.0 },
        action: Action::Blink {
            arrive_radius: 100.0,
            arrive: &[Effect::Damage(Magic, s(150.0, 100.0).ap(0.8)), Effect::Silence(1.0)],
        },
        self_effects: NO_FX,
        cooldown: (80.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 0.0,
        color: 0x6828a8,
    },
    SkillDef {
        id: SkillId(107),
        name: "Shadow Rush",
        name_zh: "影袭",
        roles: [Assassin, Marksman],
        ultimate: true,
        desc: "Dash through enemies, cutting everyone in your path.",
        targeting: Targeting::Direction { range: 220.0 },
        action: Action::Dash {
            speed: 800.0,
            stop_on_enemy: false,
            hit: &[Effect::Damage(Physical, s(150.0, 100.0).bonus_ad(1.0))],
            arrive_radius: 0.0,
            arrive: NO_FX,
            unstoppable: true,
        },
        self_effects: NO_FX,
        cooldown: (80.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 0.0,
        color: 0xe8e8e8,
    },
    SkillDef {
        id: SkillId(108),
        name: "Arc Cannon",
        name_zh: "奥术巨炮",
        roles: [Marksman, Mage],
        ultimate: true,
        desc: "Charge up and fire a huge piercing blast across the map.",
        targeting: Targeting::Direction { range: 1600.0 },
        action: Action::Projectile {
            speed: 650.0,
            width: 28.0,
            pierce: true,
            on_hit: &[Effect::Damage(Magic, s(250.0, 150.0).ap(0.9).bonus_ad(0.8))],
        },
        self_effects: NO_FX,
        cooldown: (100.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 0.75,
        color: 0x48ffd8,
    },
    SkillDef {
        id: SkillId(109),
        name: "Radiant Beam",
        name_zh: "光辉射线",
        roles: [Mage, Support],
        ultimate: true,
        desc: "A long beam of light: burns enemies, heals allies.",
        targeting: Targeting::Direction { range: 380.0 },
        action: Action::Line {
            width: 36.0,
            delay: 0.6,
            enemy: &[Effect::Damage(Magic, s(250.0, 100.0).ap(0.75))],
            ally: &[Effect::Heal(s(100.0, 50.0).ap(0.4))],
        },
        self_effects: NO_FX,
        cooldown: (80.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 0.0,
        color: 0xfff8c8,
    },
    SkillDef {
        id: SkillId(110),
        name: "Deadeye",
        name_zh: "致命狙击",
        roles: [Marksman, Fighter],
        ultimate: true,
        desc: "Take aim at a distant champion, then fire an unmissable shot.",
        targeting: Targeting::EnemyHero { range: 550.0 },
        action: Action::Projectile {
            speed: 900.0,
            width: 0.0,
            pierce: false,
            on_hit: &[Effect::Damage(Physical, s(250.0, 175.0).bonus_ad(1.5))],
        },
        self_effects: NO_FX,
        cooldown: (90.0, 15.0),
        mana: (100.0, 0.0),
        cast_time: 1.0,
        color: 0xff6868,
    },
];

pub fn try_skill(id: SkillId) -> Option<&'static SkillDef> {
    SKILLS.iter().find(|s| s.id == id)
}

/// Panics on unknown ids — only use with ids that passed validation.
pub fn skill(id: SkillId) -> &'static SkillDef {
    try_skill(id).unwrap_or_else(|| panic!("unknown skill {id:?}"))
}

/// All skills that can be slotted into Q/W/E.
pub fn basic_skills() -> impl Iterator<Item = &'static SkillDef> {
    SKILLS.iter().filter(|s| !s.ultimate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_unique_and_roles_distinct() {
        let mut ids = HashSet::new();
        for sk in SKILLS {
            assert!(ids.insert(sk.id), "duplicate id {:?}", sk.id);
            assert_ne!(sk.roles[0], sk.roles[1], "{} must have two distinct roles", sk.name);
            assert_eq!(sk.ultimate, sk.id.0 >= 100, "{}: ultimates use ids >= 100", sk.name);
            assert!(sk.cooldown_at(sk.max_rank()) > 0.0);
        }
    }

    #[test]
    fn every_role_has_eight_basic_skills() {
        for role in Role::ALL {
            let n = basic_skills().filter(|s| s.roles.contains(&role)).count();
            assert_eq!(n, 8, "role {role:?} appears on {n} basic skills");
        }
    }

    #[test]
    fn descriptions_render() {
        for sk in SKILLS {
            let lines = sk.detail_lines(1, None);
            assert!(!lines.is_empty() || matches!(sk.action, Action::None), "{}", sk.name);
        }
    }
}
