//! Headless 5v5 bot match, printing a summary every minute.
//! `cargo run --release -p moba-core --example bot_match -- [seed] [minutes]`
use moba_core::heroes::HEROES;
use moba_core::loadout::Loadout;
use moba_core::sim::{PlayerSetup, World};
use moba_core::{Team, TICK_RATE};

fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7);
    let minutes: f32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(40.0);
    // `rot` rotates the hero list so different heroes get different roles.
    let rot: usize = std::env::args().find_map(|a| a.strip_prefix("rot=").and_then(|v| v.parse().ok())).unwrap_or(0);
    let mut heroes: Vec<_> = HEROES.iter().collect();
    heroes.rotate_left(rot % HEROES.len());
    let setups = heroes
        .into_iter()
        .enumerate()
        .map(|(n, h)| PlayerSetup {
            name: h.name.to_string(),
            team: if (n % 2 == 0) != std::env::args().any(|a| a == "swap") { Team::Blue } else { Team::Red },
            loadout: Loadout::suggested(h.id),
            bot: true,
        })
        .collect();
    let mut w = World::new(setups, seed);
    let start = std::time::Instant::now();
    let total = (minutes * 60.0 * TICK_RATE as f32) as u32;
    let mut death_causes: std::collections::BTreeMap<(String, String), u32> = Default::default();
    for t in 0..total {
        w.step(&[]);
        for (_, ev) in &w.events {
            if let moba_core::protocol::FxEvent::Kill { killer, victim, .. } = ev {
                let name = w.players[*victim as usize].name.clone();
                let cause = match killer {
                    moba_core::protocol::Killer::Player(_) => "champion".to_string(),
                    k => format!("{k:?}"),
                };
                *death_causes.entry((name, cause)).or_default() += 1;
            }
        }
        if t % (TICK_RATE * 60) == 0 || w.winner.is_some() {
            let s = &w.teams;
            println!(
                "{:>5.1}m  kills {:>2}-{:<2} towers {}-{} inhib {}-{} drag {}-{} baron {}-{}  ents {}",
                w.time / 60.0,
                s[0].score.kills,
                s[1].score.kills,
                s[0].score.towers,
                s[1].score.towers,
                s[0].score.inhibitors,
                s[1].score.inhibitors,
                s[0].score.dragons,
                s[1].score.dragons,
                s[0].score.barons,
                s[1].score.barons,
                w.entities.len()
            );
            for p in &w.players {
                let e = w.get(p.entity).unwrap();
                let h = e.hero().unwrap();
                print!(
                    "   {:<8} {:?} L{:<2} {}/{}/{} cs{:<3} g{:<5.0} items{}",
                    p.name,
                    p.role,
                    h.level,
                    h.kills,
                    h.deaths,
                    h.assists,
                    h.cs,
                    h.gold,
                    h.items.iter().flatten().count()
                );
                println!();
            }
        }
        if let Some(win) = w.winner {
            println!("WINNER: {win:?} at {:.1} min", w.time / 60.0);
            break;
        }
    }
    if std::env::args().any(|a| a == "causes") {
        for ((who, cause), n) in &death_causes {
            println!("   death: {who:<9} by {cause:<14} x{n}");
        }
    }
    let el = start.elapsed().as_secs_f32();
    println!(
        "simulated {:.1} min in {:.2}s ({:.0}x realtime, {:.2} ms/tick)",
        w.time / 60.0,
        el,
        w.time / el,
        el * 1000.0 / w.tick as f32
    );
}
