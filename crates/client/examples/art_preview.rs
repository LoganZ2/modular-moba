//! Renders terrain crops and a sprite sheet to PPM files for eyeballing the art.
//! `cargo run -p moba-client --example art_preview -- <out_dir>`
use moba_client::art::{self, Frame, Pix};
use moba_client::terrain;
use moba_core::heroes::HEROES;
use moba_core::map::GameMap;
use moba_core::units::{MinionKind, MonsterKind};
use moba_core::Team;
use std::io::Write;

fn save(path: &str, w: usize, h: usize, rgba: &[u8]) {
    let mut f = std::fs::File::create(path).unwrap();
    write!(f, "P6\n{w} {h}\n255\n").unwrap();
    for p in rgba.chunks(4) {
        let a = p[3] as u32;
        let bg = 40u32;
        let mixc = |c: u8| ((c as u32 * a + bg * (255 - a)) / 255) as u8;
        f.write_all(&[mixc(p[0]), mixc(p[1]), mixc(p[2])]).unwrap();
    }
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let map = GameMap::get();
    // A 2x2 chunk crop around the blue base / bot lane, and mid.
    for (name, cx, cy) in [("base", 0, 6), ("mid", 3, 3), ("baron", 2, 1)] {
        let mut img = vec![0u8; 800 * 800 * 4];
        for oy in 0..2 {
            for ox in 0..2 {
                let c = terrain::chunk_rgba(map, cx + ox, cy + oy);
                for y in 0..400 {
                    let dst = ((oy * 400 + y) * 800 + ox * 400) * 4;
                    img[dst..dst + 1600].copy_from_slice(&c[y * 1600..(y + 1) * 1600]);
                }
            }
        }
        save(&format!("{out}/terrain_{name}.ppm"), 800, 800, &img);
    }
    // Sprite sheet.
    let mut sprites: Vec<Pix> = Vec::new();
    for h in HEROES {
        for f in [Frame::Idle, Frame::Walk, Frame::Attack] {
            sprites.push(art::hero(&h.look, f));
        }
    }
    for t in [Team::Blue, Team::Red] {
        for k in [MinionKind::Melee, MinionKind::Caster, MinionKind::Siege, MinionKind::Super] {
            sprites.push(art::minion(k, t, Frame::Idle));
        }
        sprites.push(art::tower(t, true));
        sprites.push(art::inhibitor(t, true));
        sprites.push(art::nexus(t, true));
    }
    for k in [
        MonsterKind::Gromp,
        MonsterKind::BigWolf,
        MonsterKind::Wolf,
        MonsterKind::BigRaptor,
        MonsterKind::Raptor,
        MonsterKind::BigKrug,
        MonsterKind::Krug,
        MonsterKind::BlueSentinel,
        MonsterKind::RedBrambleback,
        MonsterKind::Dragon,
        MonsterKind::Baron,
    ] {
        sprites.push(art::monster(k, Frame::Idle));
    }
    for s in moba_core::skills::SKILLS {
        sprites.push(art::skill_icon(s));
    }
    for i in moba_core::items::ITEMS {
        sprites.push(art::item_icon(i));
    }
    let cols = 12;
    let cell = 60;
    let rows = sprites.len().div_ceil(cols);
    let (w, h) = (cols * cell, rows * cell);
    let mut img = vec![0u8; w * h * 4];
    for (i, s) in sprites.iter().enumerate() {
        let ox = (i % cols) * cell + 2;
        let oy = (i / cols) * cell + 2;
        let rgba = s.rgba();
        for y in 0..s.h.min(cell - 2) {
            for x in 0..s.w.min(cell - 2) {
                let si = (y * s.w + x) * 4;
                let di = ((oy + y) * w + ox + x) * 4;
                img[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
            }
        }
    }
    save(&format!("{out}/sprites.ppm"), w, h, &img);
    println!("ok");
}
