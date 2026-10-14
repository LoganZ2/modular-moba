//! Simulation-side entity representation.

use crate::heroes::{hero, HeroId};
use crate::items::ItemId;
use crate::math::Vec2;
use crate::protocol::ProjectileStyle;
use crate::skills::{BuffStat, SkillId};
use crate::stats::Stats;
use crate::summoners::SummonerId;
use crate::units::{MinionKind, MonsterKind, StructureKind};
use crate::{EntityId, Team};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StatusKind {
    Slow(f32),
    Stun,
    Root,
    Airborne,
    Silence,
    Taunt(EntityId),
    Blind,
    Buff(BuffStat, f32),
    Vulnerable(f32),
    /// Remaining absorb amount.
    Shield(f32),
    /// Health per second (potions, fountain is handled separately).
    Regen(f32),
    /// True damage per second from `source`.
    Burn {
        dps: f32,
        source: EntityId,
    },
    BlueBuff,
    RedBuff,
    BaronBuff,
    Ghost,
    /// Deals 35% less damage.
    Exhausted,
    Unstoppable,
}

impl StatusKind {
    pub fn is_cc(&self) -> bool {
        matches!(
            self,
            StatusKind::Slow(_)
                | StatusKind::Stun
                | StatusKind::Root
                | StatusKind::Airborne
                | StatusKind::Silence
                | StatusKind::Taunt(_)
                | StatusKind::Blind
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Status {
    pub kind: StatusKind,
    pub remaining: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CastTarget {
    pub point: Vec2,
    pub unit: Option<EntityId>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    Idle,
    Move(Vec2),
    Attack(EntityId),
    AttackMove(Vec2),
    /// Walking into range to cast.
    Cast {
        slot: u8,
        target: CastTarget,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingCast {
    pub slot: u8,
    pub target: CastTarget,
    pub remaining: f32,
    pub total: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeroState {
    pub player: usize,
    pub hero: HeroId,
    /// Q, W, E, R.
    pub skills: [SkillId; 4],
    pub summoners: [SummonerId; 2],
    pub level: u8,
    pub xp: f32,
    pub gold: f32,
    pub ranks: [u8; 4],
    pub skill_points: u8,
    pub cooldowns: [f32; 4],
    pub cooldown_max: [f32; 4],
    pub summoner_cd: [f32; 2],
    pub items: [Option<(ItemId, u8)>; 6],
    pub respawn: f32,
    pub recall: Option<f32>,
    pub cast: Option<PendingCast>,
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub cs: u16,
    /// Bonus physical damage on the next basic attack.
    pub empower: f32,
    /// Enemy heroes that damaged us recently: (hero entity, time).
    pub damagers: Vec<(EntityId, f32)>,
    /// Last time this hero damaged an enemy hero, and which one
    /// (towers and minions answer such "calls for help").
    pub aggression: Option<(EntityId, f32)>,
}

impl HeroState {
    pub fn def(&self) -> &'static crate::heroes::HeroDef {
        hero(self.hero)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MinionState {
    pub kind: MinionKind,
    pub lane: u8,
    pub waypoint: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructureState {
    pub kind: StructureKind,
    /// Consecutive shots on the same champion (tower damage ramps up).
    pub streak: u32,
    pub respawn_at: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonsterState {
    pub kind: MonsterKind,
    pub camp: usize,
    pub home: Vec2,
    pub resetting: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnitKind {
    Hero(Box<HeroState>),
    Minion(MinionState),
    Structure(StructureState),
    Monster(MonsterState),
}

#[derive(Clone, Debug, PartialEq)]
pub enum DashKind {
    Skill { skill: SkillId, rank: u8, stats: Stats, hit: Vec<EntityId> },
    Knockback { wall_stun: f32 },
    Pull,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dash {
    pub to: Vec2,
    /// Follow a unit (targeted dashes).
    pub target: Option<EntityId>,
    pub speed: f32,
    pub kind: DashKind,
    /// Safety timer so a dash can never last forever.
    pub time_left: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub team: Team,
    pub kind: UnitKind,
    pub pos: Vec2,
    /// Movement during the last tick, in units per second.
    pub vel: Vec2,
    pub radius: f32,
    pub facing: f32,
    pub hp: f32,
    pub mana: f32,
    /// Spawn-time stats for non-hero units (heroes recompute from their def).
    pub base: Stats,
    /// Current stats after level, items and buffs.
    pub stats: Stats,
    pub alive: bool,
    pub statuses: Vec<Status>,
    pub attack_cd: f32,
    /// Attack wind-up: (target, seconds left).
    pub windup: Option<(EntityId, f32)>,
    pub ranged: bool,
    pub projectile_speed: f32,
    pub attack_style: ProjectileStyle,
    pub attack_color: u32,
    pub order: Order,
    /// Unit we currently want to attack.
    pub engage: Option<EntityId>,
    /// Whether `engage` may be chased out of attack range.
    pub chase: bool,
    pub move_goal: Option<Vec2>,
    pub path: Vec<Vec2>,
    pub path_goal: Vec2,
    pub repath: f32,
    pub dash: Option<Dash>,
    pub revealed_until: f32,
    pub invulnerable: bool,
    /// Minions and monsters are removed from the world after dying.
    pub remove: bool,
    pub stuck_time: f32,
}

impl Entity {
    pub fn hero(&self) -> Option<&HeroState> {
        match &self.kind {
            UnitKind::Hero(h) => Some(h),
            _ => None,
        }
    }

    pub fn hero_mut(&mut self) -> Option<&mut HeroState> {
        match &mut self.kind {
            UnitKind::Hero(h) => Some(h),
            _ => None,
        }
    }

    pub fn is_hero(&self) -> bool {
        matches!(self.kind, UnitKind::Hero(_))
    }

    pub fn is_minion(&self) -> bool {
        matches!(self.kind, UnitKind::Minion(_))
    }

    pub fn is_structure(&self) -> bool {
        matches!(self.kind, UnitKind::Structure(_))
    }

    pub fn is_monster(&self) -> bool {
        matches!(self.kind, UnitKind::Monster(_))
    }

    pub fn structure(&self) -> Option<&StructureState> {
        match &self.kind {
            UnitKind::Structure(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_epic_monster(&self) -> bool {
        matches!(&self.kind, UnitKind::Monster(m) if m.kind.is_epic())
    }

    pub fn has(&self, pred: impl Fn(&StatusKind) -> bool) -> bool {
        self.statuses.iter().any(|s| pred(&s.kind))
    }

    pub fn stunned(&self) -> bool {
        self.has(|k| matches!(k, StatusKind::Stun | StatusKind::Airborne))
    }

    pub fn rooted(&self) -> bool {
        self.has(|k| matches!(k, StatusKind::Root))
    }

    pub fn silenced(&self) -> bool {
        self.has(|k| matches!(k, StatusKind::Silence))
    }

    pub fn taunted_by(&self) -> Option<EntityId> {
        self.statuses.iter().find_map(|s| match s.kind {
            StatusKind::Taunt(src) => Some(src),
            _ => None,
        })
    }

    pub fn unstoppable(&self) -> bool {
        self.has(|k| matches!(k, StatusKind::Unstoppable))
    }

    pub fn shield(&self) -> f32 {
        self.statuses
            .iter()
            .map(|s| match s.kind {
                StatusKind::Shield(v) => v,
                _ => 0.0,
            })
            .sum()
    }

    pub fn can_move(&self) -> bool {
        self.alive && !self.stunned() && !self.rooted() && self.dash.is_none()
    }

    /// Can be selected as an attack / skill target.
    pub fn targetable(&self) -> bool {
        self.alive && !self.invulnerable
    }

    /// Distance between the edges of two units.
    pub fn edge_distance(&self, other: &Entity) -> f32 {
        (self.pos.distance(other.pos) - self.radius - other.radius).max(0.0)
    }

    pub fn in_attack_range(&self, other: &Entity) -> bool {
        self.edge_distance(other) <= self.stats.range
    }

    pub fn add_status(&mut self, kind: StatusKind, secs: f32) {
        self.statuses.push(Status { kind, remaining: secs });
    }

    pub fn hp_frac(&self) -> f32 {
        if self.stats.max_hp > 0.0 {
            self.hp / self.stats.max_hp
        } else {
            0.0
        }
    }
}
