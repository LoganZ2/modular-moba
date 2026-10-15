//! Deterministic fixed-tick game simulation.
//!
//! The server runs one [`World`] per match and calls [`World::step`] 30 times
//! a second with the commands received from players. The browser runs the
//! very same code for offline practice games against bots.
//!
//! Module overview:
//! * [`entity`]   – unit data
//! * `stats_calc` – level / item / buff stat computation, status timers
//! * `combat`     – damage, deaths, rewards, basic attacks, projectiles
//! * `abilities`  – skill casting, effects, zones, summoner spells, shop
//! * `ai`         – minions, towers, jungle monsters, hero orders
//! * `movement`   – path following, dashes, collision
//! * `vision`     – fog of war and brush
//! * `snapshot`   – per-player network views
//! * [`bot`]      – champion AI that plays through the normal command API

mod abilities;
mod ai;
pub mod bot;
mod combat;
pub mod entity;
mod movement;
mod snapshot;
mod stats_calc;
mod vision;

pub use entity::*;

use crate::heroes::hero;
use crate::loadout::Loadout;
use crate::map::{GameMap, LANE_BOT, LANE_MID, LANE_TOP, MAP_SIZE};
use crate::math::{Rng, Vec2};
use crate::protocol::{Command, FxEvent, ProjectileStyle, TeamScore, ZoneKind};
use crate::skills::{DamageKind, SkillId};
use crate::stats::Stats;
use crate::units::{MinionKind, MonsterKind, StructureKind, UnitStats, TOWER_AD, TOWER_ATTACK_SPEED, TOWER_RANGE};
use crate::{EntityId, Team, DT};
use std::collections::HashMap;

pub const MAX_LEVEL: u8 = 18;
pub const STARTING_GOLD: f32 = 500.0;
/// Passive income per second.
pub const PASSIVE_GOLD: f32 = 2.4;
pub const PASSIVE_GOLD_START: f32 = 15.0;
pub const FIRST_WAVE: f32 = 20.0;
pub const WAVE_INTERVAL: f32 = 30.0;
pub const RECALL_TIME: f32 = 8.0;
pub const INHIBITOR_RESPAWN: f32 = 240.0;
/// Heroes within this distance of a dying enemy unit share its experience.
pub const XP_RANGE: f32 = 400.0;
pub const ASSIST_WINDOW: f32 = 10.0;

/// Experience needed to go from `level` to `level + 1`.
pub fn xp_to_next(level: u8) -> f32 {
    180.0 + 100.0 * level as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneRole {
    Top,
    Jungle,
    Mid,
    Bot,
    Support,
}

impl LaneRole {
    pub const ORDER: [LaneRole; 5] = [LaneRole::Top, LaneRole::Jungle, LaneRole::Mid, LaneRole::Bot, LaneRole::Support];

    pub fn lane(self) -> u8 {
        match self {
            LaneRole::Top => LANE_TOP,
            LaneRole::Jungle | LaneRole::Mid => LANE_MID,
            LaneRole::Bot | LaneRole::Support => LANE_BOT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LaneRole::Top => "Top",
            LaneRole::Jungle => "Jungle",
            LaneRole::Mid => "Mid",
            LaneRole::Bot => "Bot",
            LaneRole::Support => "Support",
        }
    }
}

/// How a match participant is configured before the game starts.
#[derive(Clone, Debug)]
pub struct PlayerSetup {
    pub name: String,
    pub team: Team,
    pub loadout: Loadout,
    pub bot: bool,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub index: usize,
    pub name: String,
    pub team: Team,
    pub loadout: Loadout,
    pub entity: EntityId,
    pub bot: Option<bot::BotBrain>,
    pub role: LaneRole,
}

#[derive(Clone, Debug)]
pub enum Payload {
    /// Basic attack (or tower shot) with damage fixed at fire time.
    Attack {
        damage: f32,
        kind: DamageKind,
        crit: bool,
    },
    Skill {
        skill: SkillId,
        rank: u8,
        stats: Stats,
    },
}

#[derive(Clone, Debug)]
pub struct Projectile {
    pub id: u32,
    pub owner: EntityId,
    pub team: Team,
    pub pos: Vec2,
    pub dir: Vec2,
    pub speed: f32,
    /// Homing target.
    pub target: Option<EntityId>,
    /// Distance left for linear skillshots.
    pub remaining: f32,
    pub width: f32,
    pub pierce: bool,
    pub style: ProjectileStyle,
    pub color: u32,
    pub payload: Payload,
    pub hit: Vec<EntityId>,
    pub dead: bool,
}

#[derive(Clone, Debug)]
pub enum ZoneArea {
    Circle(f32),
    Line { dir: Vec2, length: f32, width: f32 },
}

#[derive(Clone, Debug)]
pub struct Zone {
    pub id: u32,
    pub owner: EntityId,
    pub team: Team,
    pub pos: Vec2,
    pub area: ZoneArea,
    pub kind: ZoneKind,
    pub timer: f32,
    pub total: f32,
    pub skill: SkillId,
    pub rank: u8,
    pub stats: Stats,
    pub dead: bool,
}

/// Who should receive an event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Audience {
    All,
    Team(Team),
    Player(usize),
    /// Whoever can see this position.
    At(Vec2),
}

#[derive(Clone, Debug)]
pub struct CampState {
    pub spawn: usize,
    pub members: Vec<EntityId>,
    pub respawn_at: Option<f32>,
    pub aggro: Option<EntityId>,
}

#[derive(Clone, Debug, Default)]
pub struct TeamState {
    pub score: TeamScore,
}

pub struct World {
    pub time: f32,
    pub tick: u32,
    pub map: &'static GameMap,
    pub entities: Vec<Entity>,
    index: HashMap<EntityId, usize>,
    next_id: u32,
    pub projectiles: Vec<Projectile>,
    pub zones: Vec<Zone>,
    pub players: Vec<Player>,
    pub teams: [TeamState; 2],
    pub camps: Vec<CampState>,
    pub events: Vec<(Audience, FxEvent)>,
    pub rng: Rng,
    pub winner: Option<Team>,
    next_wave: f32,
    wave: u32,
    first_blood: bool,
    pub vision: [vision::TeamVision; 2],
    /// Set when the vision grid changed this tick (snapshots carry the fog then).
    pub vision_dirty: bool,
}

impl World {
    pub fn new(setups: Vec<PlayerSetup>, seed: u64) -> World {
        let map = GameMap::get();
        let mut w = World {
            time: 0.0,
            tick: 0,
            map,
            entities: Vec::new(),
            index: HashMap::new(),
            next_id: 1,
            projectiles: Vec::new(),
            zones: Vec::new(),
            players: Vec::new(),
            teams: Default::default(),
            camps: Vec::new(),
            events: Vec::new(),
            rng: Rng::new(seed),
            winner: None,
            next_wave: FIRST_WAVE,
            wave: 0,
            first_blood: false,
            vision: [vision::TeamVision::new(), vision::TeamVision::new()],
            vision_dirty: true,
        };
        for s in &map.structures {
            w.spawn_structure(s.team, s.kind, s.pos);
        }
        let mut per_team = [0usize; 2];
        for setup in setups {
            let team = if setup.team == Team::Red { Team::Red } else { Team::Blue };
            let n = per_team[team.index()];
            per_team[team.index()] += 1;
            let role = LaneRole::ORDER[n % 5];
            let index = w.players.len();
            let loadout = setup.loadout.sanitized();
            w.players.push(Player {
                index,
                name: setup.name,
                team,
                loadout,
                entity: 0,
                bot: if setup.bot { Some(bot::BotBrain::new(role, index)) } else { None },
                role,
            });
            let id = w.spawn_hero(index, n);
            w.players[index].entity = id;
        }
        for (i, c) in map.camps.iter().enumerate() {
            w.camps.push(CampState { spawn: i, members: Vec::new(), respawn_at: Some(c.first_spawn), aggro: None });
        }
        stats_calc::update_all(&mut w);
        for e in &mut w.entities {
            e.hp = e.stats.max_hp;
            e.mana = e.stats.max_mana;
        }
        vision::update(&mut w);
        w.event(Audience::All, FxEvent::Announce { text: "Welcome to the Rift!".into(), team: None });
        w
    }

    // ------------------------------------------------------------ lookup

    pub fn idx(&self, id: EntityId) -> Option<usize> {
        self.index.get(&id).copied()
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.idx(id).map(|i| &self.entities[i])
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        match self.idx(id) {
            Some(i) => Some(&mut self.entities[i]),
            None => None,
        }
    }

    pub fn player_hero(&self, player: usize) -> Option<&Entity> {
        self.players.get(player).and_then(|p| self.get(p.entity))
    }

    pub fn event(&mut self, audience: Audience, ev: FxEvent) {
        self.events.push((audience, ev));
    }

    pub fn team_state(&self, team: Team) -> &TeamState {
        &self.teams[team.index().min(1)]
    }

    pub fn team_state_mut(&mut self, team: Team) -> &mut TeamState {
        &mut self.teams[team.index().min(1)]
    }

    /// Is entity `i` visible to `team`?
    pub fn visible_to(&self, team: Team, i: usize) -> bool {
        vision::entity_visible(self, team, i)
    }

    /// Indices of living entities whose centre is within `r` of `p`.
    pub fn units_near(&self, p: Vec2, r: f32) -> Vec<usize> {
        let r2 = r * r;
        (0..self.entities.len())
            .filter(|&i| {
                let e = &self.entities[i];
                e.alive && e.pos.distance_sq(p) <= r2
            })
            .collect()
    }

    // ------------------------------------------------------------ spawning

    fn alloc_id(&mut self) -> EntityId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub(crate) fn alloc_misc_id(&mut self) -> u32 {
        self.alloc_id()
    }

    fn push_entity(&mut self, e: Entity) -> usize {
        let i = self.entities.len();
        self.index.insert(e.id, i);
        self.entities.push(e);
        i
    }

    fn new_entity(&mut self, team: Team, kind: UnitKind, pos: Vec2, radius: f32, base: Stats) -> Entity {
        Entity {
            id: self.alloc_id(),
            team,
            kind,
            pos,
            vel: Vec2::ZERO,
            radius,
            facing: if team == Team::Red { std::f32::consts::PI } else { 0.0 },
            hp: base.max_hp,
            mana: base.max_mana,
            base,
            stats: base,
            alive: true,
            statuses: Vec::new(),
            attack_cd: 0.0,
            windup: None,
            ranged: false,
            projectile_speed: 0.0,
            attack_style: ProjectileStyle::Orb,
            attack_color: 0xffffff,
            order: Order::Idle,
            engage: None,
            chase: false,
            move_goal: None,
            path: Vec::new(),
            path_goal: pos,
            repath: 0.0,
            dash: None,
            revealed_until: 0.0,
            invulnerable: false,
            remove: false,
            stuck_time: 0.0,
        }
    }

    fn unit_stats(u: &UnitStats) -> Stats {
        Stats {
            max_hp: u.hp,
            max_mana: 0.0,
            ad: u.ad,
            base_ad: u.ad,
            ap: 0.0,
            armor: u.armor,
            mr: u.mr,
            attack_speed: u.attack_speed,
            range: u.range,
            move_speed: u.move_speed,
            crit: 0.0,
            lifesteal: 0.0,
            haste: 0.0,
            hp_regen: 0.0,
            mana_regen: 0.0,
        }
    }

    fn spawn_structure(&mut self, team: Team, kind: StructureKind, pos: Vec2) -> EntityId {
        let is_tower = matches!(kind, StructureKind::Tower { .. });
        let stats = Stats {
            max_hp: kind.max_hp(),
            ad: if is_tower { TOWER_AD } else { 0.0 },
            base_ad: if is_tower { TOWER_AD } else { 0.0 },
            armor: kind.armor(),
            mr: kind.armor(),
            attack_speed: TOWER_ATTACK_SPEED,
            range: TOWER_RANGE,
            ..Stats::default()
        };
        let st = StructureState { kind, streak: 0, respawn_at: None };
        let mut e = self.new_entity(team, UnitKind::Structure(st), pos, kind.radius(), stats);
        e.ranged = true;
        e.projectile_speed = crate::units::TOWER_PROJECTILE_SPEED;
        e.attack_style = ProjectileStyle::TowerShot;
        e.attack_color = if team == Team::Blue { 0x68b8ff } else { 0xff7868 };
        let id = e.id;
        self.push_entity(e);
        id
    }

    fn spawn_hero(&mut self, player: usize, n: usize) -> EntityId {
        let p = &self.players[player];
        let team = p.team;
        let loadout = p.loadout;
        let def = hero(loadout.hero);
        let hs = HeroState {
            player,
            hero: def.id,
            skills: [loadout.skills[0], loadout.skills[1], loadout.skills[2], def.ultimate],
            summoners: loadout.summoners,
            level: 1,
            xp: 0.0,
            gold: STARTING_GOLD,
            ranks: [0; 4],
            skill_points: 1,
            cooldowns: [0.0; 4],
            cooldown_max: [1.0; 4],
            summoner_cd: [0.0; 2],
            items: [None; 6],
            respawn: 0.0,
            recall: None,
            cast: None,
            kills: 0,
            deaths: 0,
            assists: 0,
            cs: 0,
            empower: 0.0,
            damagers: Vec::new(),
            aggression: None,
        };
        let pos = self.map.spawn_point(team, n);
        let mut e = self.new_entity(team, UnitKind::Hero(Box::new(hs)), pos, 11.0, Stats::default());
        e.ranged = def.base.ranged;
        e.projectile_speed = 380.0;
        use crate::heroes::Weapon;
        e.attack_style = match def.look.weapon {
            Weapon::Bow => ProjectileStyle::Arrow,
            Weapon::Rifle => ProjectileStyle::Bolt,
            _ => ProjectileStyle::Orb,
        };
        e.attack_color = def.look.trim;
        let id = e.id;
        self.push_entity(e);
        id
    }

    fn spawn_minion(&mut self, team: Team, lane: u8, kind: MinionKind, pos: Vec2) {
        let u = kind.stats();
        let mut base = Self::unit_stats(&u);
        // Minions grow stronger over time.
        let steps = (self.time / 90.0).floor();
        let (ghp, gad) = kind.growth();
        base.max_hp += ghp * steps;
        base.ad += gad * steps;
        base.base_ad = base.ad;
        let st = MinionState { kind, lane, waypoint: 0 };
        let mut e = self.new_entity(team, UnitKind::Minion(st), pos, u.radius, base);
        e.ranged = u.ranged;
        e.projectile_speed = u.projectile_speed;
        e.attack_style = if kind == MinionKind::Siege { ProjectileStyle::Cannon } else { ProjectileStyle::Orb };
        e.attack_color = if team == Team::Blue { 0x78c8ff } else { 0xff8878 };
        self.push_entity(e);
    }

    fn spawn_monster(&mut self, kind: MonsterKind, camp: usize, pos: Vec2) -> EntityId {
        let u = kind.stats();
        let mut base = Self::unit_stats(&u);
        // Mild scaling so camps stay relevant later on.
        let mins = self.time / 60.0;
        base.max_hp *= 1.0 + 0.04 * mins;
        base.ad *= 1.0 + 0.03 * mins;
        base.base_ad = base.ad;
        let st = MonsterState { kind, camp, home: pos, resetting: false };
        let mut e = self.new_entity(Team::Neutral, UnitKind::Monster(st), pos, u.radius, base);
        e.ranged = u.ranged;
        e.projectile_speed = u.projectile_speed;
        e.attack_style = ProjectileStyle::Orb;
        e.attack_color = match kind {
            MonsterKind::Dragon => 0xff8030,
            MonsterKind::Baron => 0xb050ff,
            _ => 0xd0e070,
        };
        e.facing = std::f32::consts::FRAC_PI_2;
        let id = e.id;
        self.push_entity(e);
        id
    }

    // ------------------------------------------------------------ main loop

    /// Advance the world by one tick. `commands` are (player index, command).
    pub fn step(&mut self, commands: &[(usize, Command)]) {
        self.events.clear();
        self.vision_dirty = false;
        if self.winner.is_some() {
            // The match clock stops; ticks keep counting so snapshots flow.
            self.tick += 1;
            return;
        }
        let mut cmds: Vec<(usize, Command)> = commands.to_vec();
        bot::think_all(self, &mut cmds);
        for (p, c) in cmds {
            self.apply_command(p, c);
        }

        self.spawn_waves();
        self.update_camps();
        self.update_respawns();
        self.passive_income();
        stats_calc::tick_statuses(self);
        stats_calc::update_all(self);
        abilities::update_casts(self);
        ai::think(self);
        combat::update_attacks(self);
        movement::update(self);
        combat::update_projectiles(self);
        abilities::update_zones(self);
        combat::update_fountains(self);
        self.cleanup();

        self.time += DT;
        self.tick += 1;
        if self.tick.is_multiple_of(3) {
            vision::update(self);
        }
    }

    fn cleanup(&mut self) {
        let before = self.entities.len();
        self.entities.retain(|e| !e.remove);
        if self.entities.len() != before {
            self.index.clear();
            for (i, e) in self.entities.iter().enumerate() {
                self.index.insert(e.id, i);
            }
        }
        self.projectiles.retain(|p| !p.dead);
        self.zones.retain(|z| !z.dead);
    }

    fn passive_income(&mut self) {
        if self.time < PASSIVE_GOLD_START {
            return;
        }
        for e in &mut self.entities {
            if let Some(h) = e.hero_mut() {
                h.gold += PASSIVE_GOLD * DT;
            }
        }
    }

    fn spawn_waves(&mut self) {
        if self.time < self.next_wave {
            return;
        }
        self.next_wave += WAVE_INTERVAL;
        self.wave += 1;
        if self.wave == 1 {
            self.event(Audience::All, FxEvent::Announce { text: "Minions have spawned!".into(), team: None });
        }
        for team in Team::PLAYABLE {
            for lane in [LANE_TOP, LANE_MID, LANE_BOT] {
                // Super minions when the enemy inhibitor of this lane is down.
                let enemy_inhib_down = self.entities.iter().any(|e| {
                    e.team == team.enemy()
                        && !e.alive
                        && matches!(e.structure(), Some(s) if s.kind == StructureKind::Inhibitor { lane })
                });
                let mut kinds = vec![MinionKind::Melee; 3];
                if enemy_inhib_down {
                    kinds.insert(0, MinionKind::Super);
                } else if self.wave.is_multiple_of(3) {
                    kinds.push(MinionKind::Siege);
                }
                kinds.extend([MinionKind::Caster; 3]);
                let path = &self.map.lanes[team.index()][lane as usize];
                let start = path[0];
                let dir = (path[1] - path[0]).normalize_or_zero();
                for (n, k) in kinds.into_iter().enumerate() {
                    let back = n as f32 * 18.0;
                    let side = if n % 2 == 0 { 6.0 } else { -6.0 };
                    let pos = self.map.nearest_walkable(start - dir * back + dir.perp() * side);
                    self.spawn_minion(team, lane, k, pos);
                }
            }
        }
    }

    fn update_camps(&mut self) {
        for c in 0..self.camps.len() {
            let Some(at) = self.camps[c].respawn_at else {
                continue;
            };
            if self.time < at {
                continue;
            }
            self.camps[c].respawn_at = None;
            self.camps[c].aggro = None;
            let spawn = &self.map.camps[self.camps[c].spawn];
            let mut members = Vec::new();
            for (kind, pos) in spawn.monsters.clone() {
                members.push(self.spawn_monster(kind, c, pos));
            }
            self.camps[c].members = members;
            let kind = spawn.monsters[0].0;
            if kind.is_epic() {
                self.event(
                    Audience::All,
                    FxEvent::Announce { text: format!("The {} has spawned!", kind.name()), team: None },
                );
            }
        }
    }

    fn update_respawns(&mut self) {
        let time = self.time;
        let mut respawned = Vec::new();
        for i in 0..self.entities.len() {
            let e = &mut self.entities[i];
            if e.alive {
                continue;
            }
            match &mut e.kind {
                UnitKind::Hero(h) => {
                    h.respawn -= DT;
                    if h.respawn <= 0.0 {
                        respawned.push(i);
                    }
                }
                UnitKind::Structure(s) => {
                    if let Some(at) = s.respawn_at {
                        if time >= at {
                            s.respawn_at = None;
                            e.alive = true;
                            e.hp = e.stats.max_hp;
                            let team = e.team;
                            self.event(
                                Audience::All,
                                FxEvent::Announce {
                                    text: format!("{} inhibitor has respawned", team.name()),
                                    team: Some(team),
                                },
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        for i in respawned {
            let team = self.entities[i].team;
            let n = self.entities[i].hero().map(|h| h.player).unwrap_or(0) % 5;
            let pos = self.map.spawn_point(team, n);
            let e = &mut self.entities[i];
            e.alive = true;
            e.pos = pos;
            e.hp = e.stats.max_hp;
            e.mana = e.stats.max_mana;
            e.order = Order::Idle;
            e.path.clear();
            e.move_goal = None;
            e.engage = None;
            e.statuses.clear();
            if let Some(h) = e.hero_mut() {
                h.respawn = 0.0;
                h.damagers.clear();
            }
        }
    }

    // ------------------------------------------------------------ commands

    pub fn apply_command(&mut self, player: usize, cmd: Command) {
        // Commands come from the network: drop non-finite coordinates and
        // clamp everything else onto the map.
        let clamp = |v: Vec2| -> Option<Vec2> {
            (v.x.is_finite() && v.y.is_finite())
                .then(|| Vec2::new(v.x.clamp(0.0, MAP_SIZE - 1.0), v.y.clamp(0.0, MAP_SIZE - 1.0)))
        };
        let cmd = match cmd {
            Command::Move(v) => Command::Move(match clamp(v) {
                Some(v) => v,
                None => return,
            }),
            Command::AttackMove(v) => Command::AttackMove(match clamp(v) {
                Some(v) => v,
                None => return,
            }),
            Command::Cast { slot, point, unit } => match clamp(point) {
                Some(point) => Command::Cast { slot, point, unit },
                None => return,
            },
            Command::Summoner { slot, point, unit } => match clamp(point) {
                Some(point) => Command::Summoner { slot, point, unit },
                None => return,
            },
            other => other,
        };
        let Some(p) = self.players.get(player) else {
            return;
        };
        let Some(i) = self.idx(p.entity) else { return };
        let alive = self.entities[i].alive;
        match cmd {
            Command::Buy(item) => abilities::buy(self, i, item),
            Command::Sell(slot) => abilities::sell(self, i, slot),
            Command::LevelUp(slot) => abilities::level_up(self, i, slot),
            _ if !alive => {}
            Command::UseItem(slot) => abilities::use_item(self, i, slot),
            Command::Move(target) => {
                let target = self.map.nearest_walkable(target);
                let e = &mut self.entities[i];
                cancel_recall(e);
                e.order = Order::Move(target);
                e.engage = None;
                e.windup = None;
                e.repath = 0.0;
            }
            Command::AttackMove(target) => {
                let target = self.map.nearest_walkable(target);
                let e = &mut self.entities[i];
                cancel_recall(e);
                e.order = Order::AttackMove(target);
                e.engage = None;
                e.repath = 0.0;
            }
            Command::Attack(target) => {
                let team = self.entities[i].team;
                let ok = self
                    .idx(target)
                    .map(|t| {
                        let te = &self.entities[t];
                        te.targetable() && te.team != team && self.visible_to(team, t)
                    })
                    .unwrap_or(false);
                if ok {
                    let e = &mut self.entities[i];
                    cancel_recall(e);
                    e.order = Order::Attack(target);
                }
            }
            Command::Stop => {
                let e = &mut self.entities[i];
                cancel_recall(e);
                e.order = Order::Idle;
                e.engage = None;
                e.windup = None;
                e.path.clear();
                e.move_goal = None;
            }
            Command::Cast { slot, point, unit } => {
                abilities::command_cast(self, i, slot, CastTarget { point, unit });
            }
            Command::Summoner { slot, point, unit } => {
                abilities::cast_summoner(self, i, slot, CastTarget { point, unit });
            }
            Command::Recall => {
                let baron = self.entities[i].has(|k| matches!(k, StatusKind::BaronBuff));
                let e = &mut self.entities[i];
                if e.dash.is_some() || e.stunned() {
                    return;
                }
                let time = if baron { RECALL_TIME / 2.0 } else { RECALL_TIME };
                if let Some(h) = e.hero_mut() {
                    if h.recall.is_none() && h.cast.is_none() {
                        h.recall = Some(time);
                    }
                }
                e.order = Order::Idle;
                e.engage = None;
                e.windup = None;
                e.path.clear();
                e.move_goal = None;
            }
        }
    }

    // ------------------------------------------------------------ rewards

    pub(crate) fn give_gold(&mut self, i: usize, amount: f32, show: bool) {
        let pos = self.entities[i].pos;
        let mut player = None;
        if let Some(h) = self.entities[i].hero_mut() {
            h.gold += amount;
            player = Some(h.player);
        }
        if let (Some(p), true) = (player, show) {
            self.event(Audience::Player(p), FxEvent::Gold { pos, amount });
        }
    }

    pub(crate) fn give_xp(&mut self, i: usize, amount: f32) {
        let mut leveled = None;
        if let Some(h) = self.entities[i].hero_mut() {
            if h.level >= MAX_LEVEL {
                return;
            }
            h.xp += amount;
            while h.level < MAX_LEVEL && h.xp >= xp_to_next(h.level) {
                h.xp -= xp_to_next(h.level);
                h.level += 1;
                h.skill_points += 1;
                leveled = Some(h.level);
            }
            if h.level >= MAX_LEVEL {
                h.xp = 0.0;
            }
        }
        if let Some(level) = leveled {
            let id = self.entities[i].id;
            let pos = self.entities[i].pos;
            self.event(Audience::At(pos), FxEvent::LevelUp { id, level });
        }
    }

    /// Lane of a hero's player (for bots and minion bookkeeping).
    pub fn player_role(&self, player: usize) -> LaneRole {
        self.players.get(player).map(|p| p.role).unwrap_or(LaneRole::Mid)
    }

    /// Spectator/debug snapshot of everything, or a per-player filtered view.
    pub fn snapshot_for(&self, player: Option<usize>) -> crate::protocol::Snapshot {
        snapshot::build(self, player)
    }

    pub fn team_of_player(&self, player: usize) -> Team {
        self.players.get(player).map(|p| p.team).unwrap_or(Team::Neutral)
    }
}

pub(crate) fn cancel_recall(e: &mut Entity) {
    if let Some(h) = e.hero_mut() {
        h.recall = None;
    }
}

#[cfg(test)]
mod tests;
