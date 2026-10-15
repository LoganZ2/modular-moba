use super::*;
use crate::heroes::{HeroId, HEROES};
use crate::protocol::{decode, encode, ServerMsg};
use crate::skills::SkillId;
use crate::summoners::SummonerId;
use crate::TICK_RATE;

fn bots_match(seed: u64) -> World {
    let mut setups = Vec::new();
    for (n, h) in HEROES.iter().enumerate() {
        setups.push(PlayerSetup {
            name: format!("Bot{n}"),
            team: if n < 5 { Team::Blue } else { Team::Red },
            loadout: Loadout::suggested(h.id),
            bot: true,
        });
    }
    World::new(setups, seed)
}

fn run(w: &mut World, seconds: f32) {
    let ticks = (seconds * TICK_RATE as f32) as u32;
    for _ in 0..ticks {
        w.step(&[]);
        if w.winner.is_some() {
            break;
        }
    }
}

#[test]
fn world_spawns_everything() {
    let w = bots_match(1);
    let heroes = w.entities.iter().filter(|e| e.is_hero()).count();
    let structures = w.entities.iter().filter(|e| e.is_structure()).count();
    assert_eq!(heroes, 10);
    assert_eq!(structures, 30);
    for e in &w.entities {
        assert!(e.hp > 0.0, "{:?} spawned with no hp", e.kind);
    }
}

#[test]
fn minions_spawn_and_fight() {
    let mut w = bots_match(2);
    run(&mut w, 100.0);
    let minions = w.entities.iter().filter(|e| e.is_minion()).count();
    assert!(minions > 20, "expected minion waves, got {minions}");
    // Somebody earned gold beyond passive income / got XP.
    let leveled = w.entities.iter().filter_map(|e| e.hero()).filter(|h| h.level >= 2).count();
    assert!(leveled > 0, "no hero gained a level after the first waves");
}

#[test]
fn long_bot_game_is_stable_and_progresses() {
    let mut w = bots_match(3);
    run(&mut w, 60.0 * 12.0);
    let max_level = w.entities.iter().filter_map(|e| e.hero()).map(|h| h.level).max().unwrap();
    assert!(max_level >= 6, "heroes should reach level 6 by 12 minutes (max {max_level})");
    let bought = w.entities.iter().filter_map(|e| e.hero()).filter(|h| h.items.iter().flatten().count() >= 2).count();
    assert!(bought >= 5, "bots should buy items ({bought})");
    let cs: u16 = w.entities.iter().filter_map(|e| e.hero()).map(|h| h.cs).sum();
    assert!(cs > 100, "bots should last hit minions (total cs {cs})");
    for e in &w.entities {
        assert!(e.pos.x.is_finite() && e.pos.y.is_finite());
        if e.alive && !e.is_structure() {
            assert!(w.map.walkable(e.pos) || e.dash.is_some(), "{:?} stuck in a wall at {:?}", e.kind, e.pos);
        }
    }
    // Snapshots for every player encode / decode.
    for p in 0..w.players.len() {
        let snap = w.snapshot_for(Some(p));
        let bytes = encode(&ServerMsg::Snapshot(Box::new(snap)));
        assert!(decode::<ServerMsg>(&bytes).is_some());
        assert!(bytes.len() < 64 * 1024, "snapshot too large: {}", bytes.len());
    }
}

#[test]
fn fog_hides_enemies_far_away() {
    let w = bots_match(4);
    let snap = w.snapshot_for(Some(0));
    // At spawn, red heroes are in their own base, far outside blue vision.
    let red_heroes = snap
        .entities
        .iter()
        .filter(|e| e.team == Team::Red && matches!(e.unit, crate::protocol::UnitView::Hero { .. }))
        .count();
    assert_eq!(red_heroes, 0);
    // Structures are always visible.
    assert!(snap
        .entities
        .iter()
        .any(|e| e.team == Team::Red && matches!(e.unit, crate::protocol::UnitView::Structure(_))));
}

fn duel_world() -> (World, usize, usize) {
    // A human-controlled Ironclad vs a passive red Lumen, placed mid lane.
    let setups = vec![
        PlayerSetup {
            name: "Me".into(),
            team: Team::Blue,
            loadout: Loadout {
                hero: HeroId(0),
                skills: [SkillId(1), SkillId(2), SkillId(7)],
                summoners: [SummonerId::Flash, SummonerId::Ignite],
            },
            bot: false,
        },
        PlayerSetup { name: "Dummy".into(), team: Team::Red, loadout: Loadout::suggested(HeroId(8)), bot: false },
    ];
    let mut w = World::new(setups, 9);
    let a = w.idx(w.players[0].entity).unwrap();
    let b = w.idx(w.players[1].entity).unwrap();
    w.entities[a].pos = Vec2::new(1500.0, 1700.0);
    w.entities[b].pos = Vec2::new(1600.0, 1600.0);
    w.step(&[]);
    w.step(&[]);
    w.step(&[]);
    (w, a, b)
}

#[test]
fn skill_ranks_follow_rules() {
    let (mut w, a, _) = duel_world();
    // Level 1: one point. R can't be learned yet.
    w.step(&[(0, Command::LevelUp(3))]);
    assert_eq!(w.entities[a].hero().unwrap().ranks[3], 0);
    w.step(&[(0, Command::LevelUp(0))]);
    assert_eq!(w.entities[a].hero().unwrap().ranks[0], 1);
    // No points left.
    w.step(&[(0, Command::LevelUp(1))]);
    assert_eq!(w.entities[a].hero().unwrap().ranks[1], 0);
    // Give enough XP for level 6 and learn R.
    w.give_xp(a, 3000.0);
    assert!(w.entities[a].hero().unwrap().level >= 6);
    w.step(&[(0, Command::LevelUp(3))]);
    assert_eq!(w.entities[a].hero().unwrap().ranks[3], 1);
}

#[test]
fn shield_bash_stuns_and_damages() {
    let (mut w, a, b) = duel_world();
    w.step(&[(0, Command::LevelUp(0))]);
    let hp_before = w.entities[b].hp;
    let target = w.entities[b].pos;
    w.step(&[(0, Command::Cast { slot: 0, point: target, unit: None })]);
    let mut stunned = false;
    for _ in 0..30 {
        w.step(&[]);
        stunned |= w.entities[b].stunned();
    }
    assert!(stunned, "shield bash should stun");
    assert!(w.entities[b].hp < hp_before, "shield bash should damage");
    assert!(w.entities[a].hero().unwrap().cooldowns[0] > 0.0);
}

#[test]
fn cannot_cast_without_mana_or_rank() {
    let (mut w, a, _) = duel_world();
    let p = w.entities[a].pos;
    w.step(&[(0, Command::Cast { slot: 1, point: p, unit: None })]);
    assert_eq!(w.entities[a].hero().unwrap().cooldowns[1], 0.0, "unranked skill must not cast");
    w.step(&[(0, Command::LevelUp(1))]);
    w.entities[a].mana = 0.0;
    w.step(&[(0, Command::Cast { slot: 1, point: p, unit: None })]);
    assert_eq!(w.entities[a].hero().unwrap().cooldowns[1], 0.0, "no mana, no cast");
}

#[test]
fn hook_pulls_target() {
    let (mut w, a, b) = duel_world();
    w.step(&[(0, Command::LevelUp(2))]);
    let start_gap = w.entities[a].pos.distance(w.entities[b].pos);
    let target = w.entities[b].pos;
    w.step(&[(0, Command::Cast { slot: 2, point: target, unit: None })]);
    for _ in 0..40 {
        w.step(&[]);
    }
    let gap = w.entities[a].pos.distance(w.entities[b].pos);
    assert!(gap < start_gap * 0.5, "hook should pull: {start_gap} -> {gap}");
}

#[test]
fn killing_a_hero_gives_gold_and_score() {
    let (mut w, a, b) = duel_world();
    let gold_before = w.entities[a].hero().unwrap().gold;
    w.entities[b].hp = 5.0;
    let target = w.entities[b].id;
    w.step(&[(0, Command::Attack(target))]);
    for _ in 0..90 {
        w.step(&[]);
        if !w.entities[b].alive {
            break;
        }
    }
    assert!(!w.entities[b].alive, "dummy should die");
    let h = w.entities[a].hero().unwrap();
    assert_eq!(h.kills, 1);
    assert!(h.gold >= gold_before + 300.0);
    assert_eq!(w.teams[0].score.kills, 1);
    assert!(w.entities[b].hero().unwrap().respawn > 0.0);
}

#[test]
fn shop_only_at_fountain() {
    let (mut w, a, _) = duel_world();
    w.step(&[(0, Command::Buy(crate::items::ItemId(1)))]);
    assert!(w.entities[a].hero().unwrap().items.iter().all(|s| s.is_none()), "can't shop in lane");
    let f = w.map.fountain(Team::Blue);
    w.entities[a].pos = f;
    w.step(&[(0, Command::Buy(crate::items::ItemId(1)))]);
    let h = w.entities[a].hero().unwrap();
    assert_eq!(h.items[0], Some((crate::items::ItemId(1), 1)));
    assert!((h.gold - (STARTING_GOLD - 350.0)).abs() < 5.0);
    let ad = w.entities[a].stats.ad;
    w.step(&[]);
    assert!(w.entities[a].stats.ad >= ad);
}

#[test]
fn towers_are_protected_in_order() {
    let mut w = bots_match(5);
    w.step(&[]);
    let inner = w
        .entities
        .iter()
        .find(|e| {
            e.team == Team::Red
                && matches!(e.structure(), Some(s) if s.kind == StructureKind::Tower { lane: 1, tier: 2 })
        })
        .unwrap();
    assert!(inner.invulnerable, "inner tower is protected while the outer one stands");
    let outer = w
        .entities
        .iter()
        .find(|e| {
            e.team == Team::Red
                && matches!(e.structure(), Some(s) if s.kind == StructureKind::Tower { lane: 1, tier: 1 })
        })
        .unwrap();
    assert!(!outer.invulnerable);
}

#[test]
fn recall_teleports_home() {
    let (mut w, a, b) = duel_world();
    w.entities[b].pos = Vec2::new(2900.0, 300.0);
    w.step(&[(0, Command::Recall)]);
    run(&mut w, RECALL_TIME + 0.5);
    let f = w.map.fountain(Team::Blue);
    assert!(w.entities[a].pos.distance(f) < 200.0, "recall should bring us home");
}

#[test]
fn move_command_reaches_destination_through_jungle() {
    let (mut w, a, _) = duel_world();
    let dest = w.map.camps[0].pos; // blue gromp, behind walls
    w.step(&[(0, Command::Move(dest))]);
    run(&mut w, 25.0);
    assert!(w.entities[a].pos.distance(dest) < 40.0, "ended at {:?}, wanted {:?}", w.entities[a].pos, dest);
}

#[test]
fn deterministic_given_seed() {
    let mut a = bots_match(42);
    let mut b = bots_match(42);
    run(&mut a, 90.0);
    run(&mut b, 90.0);
    let pa: Vec<Vec2> = a.entities.iter().map(|e| e.pos).collect();
    let pb: Vec<Vec2> = b.entities.iter().map(|e| e.pos).collect();
    assert_eq!(pa, pb);
}

#[test]
fn every_hero_and_skill_can_be_cast() {
    // Each hero equips every eligible skill at least once across rotations;
    // casting at a nearby dummy must never panic.
    for h in HEROES {
        let skills = crate::loadout::eligible_skills(h);
        for chunk in skills.chunks(3) {
            if chunk.len() < 3 {
                continue;
            }
            let loadout = Loadout {
                hero: h.id,
                skills: [chunk[0].id, chunk[1].id, chunk[2].id],
                summoners: [SummonerId::Flash, SummonerId::Heal],
            };
            assert_eq!(loadout.validate(), Ok(()));
            let setups = vec![
                PlayerSetup { name: "A".into(), team: Team::Blue, loadout, bot: false },
                PlayerSetup { name: "B".into(), team: Team::Red, loadout: Loadout::suggested(HeroId(0)), bot: false },
            ];
            let mut w = World::new(setups, 1);
            let a = w.idx(w.players[0].entity).unwrap();
            let b = w.idx(w.players[1].entity).unwrap();
            w.entities[a].pos = Vec2::new(1500.0, 1700.0);
            w.entities[b].pos = Vec2::new(1560.0, 1640.0);
            w.give_xp(a, 20000.0);
            run(&mut w, 0.2);
            for slot in 0..4u8 {
                w.step(&[(0, Command::LevelUp(slot))]);
            }
            let bid = w.entities[b].id;
            for slot in 0..4u8 {
                let tp = w.entities[b].pos;
                w.step(&[(0, Command::Cast { slot, point: tp, unit: Some(bid) })]);
                run(&mut w, 1.5);
            }
            for sum in 0..2u8 {
                let tp = w.entities[b].pos;
                w.step(&[(0, Command::Summoner { slot: sum, point: tp, unit: Some(bid) })]);
            }
            run(&mut w, 1.0);
        }
    }
}

#[test]
fn hostile_commands_are_harmless() {
    let (mut w, a, _) = duel_world();
    let bad = [f32::NAN, f32::INFINITY, -1e30, 1e30];
    for &x in &bad {
        for &y in &bad {
            let v = Vec2::new(x, y);
            w.step(&[
                (0, Command::Move(v)),
                (0, Command::AttackMove(v)),
                (0, Command::Cast { slot: 0, point: v, unit: Some(999_999) }),
                (0, Command::Summoner { slot: 0, point: v, unit: None }),
                (0, Command::LevelUp(200)),
                (0, Command::Sell(200)),
                (0, Command::UseItem(200)),
                (0, Command::Buy(crate::items::ItemId(250))),
                (7, Command::Stop),
            ]);
        }
    }
    run(&mut w, 2.0);
    let p = w.entities[a].pos;
    assert!(p.x.is_finite() && p.y.is_finite());
    assert!(w.map.walkable(p));
}

#[test]
fn cooldowns_tick_while_dead() {
    let (mut w, a, _) = duel_world();
    w.step(&[(0, Command::LevelUp(0))]);
    let p = w.entities[a].pos + Vec2::new(50.0, 0.0);
    // Flash first: it can't be used while Shield Bash's dash is in progress.
    w.step(&[(0, Command::Summoner { slot: 0, point: p, unit: None })]);
    w.step(&[(0, Command::Cast { slot: 0, point: p, unit: None })]);
    let (q, flash) = {
        let h = w.entities[a].hero().unwrap();
        (h.cooldowns[0], h.summoner_cd[0])
    };
    assert!(q > 5.0 && flash > 100.0);
    combat::kill(&mut w, a, None);
    run(&mut w, 5.0);
    let h = w.entities[a].hero().unwrap();
    assert!(h.cooldowns[0] < q - 4.5, "Q cooldown froze while dead");
    assert!(h.summoner_cd[0] < flash - 4.5, "Flash cooldown froze while dead");
}

#[test]
fn taunt_blocks_recall_and_casting() {
    let (mut w, a, b) = duel_world();
    w.step(&[(1, Command::LevelUp(0))]);
    let taunter = w.entities[a].id;
    w.entities[b].add_status(StatusKind::Taunt(taunter), 2.0);
    let p = w.entities[a].pos;
    w.step(&[(1, Command::Recall), (1, Command::Cast { slot: 0, point: p, unit: None })]);
    let h = w.entities[b].hero().unwrap();
    assert!(h.recall.is_none(), "taunted heroes can't recall");
    assert_eq!(h.cooldowns[0], 0.0, "taunted heroes can't cast");
    assert_eq!(w.entities[b].engage, Some(taunter));
}
