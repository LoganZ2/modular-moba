//! Writes a PPM preview of the generated map: `cargo run -p moba-core --example map_preview -- out.ppm`
use moba_core::map::{GameMap, Terrain, GRID};
use moba_core::units::StructureKind;
use moba_core::Team;
use std::io::Write;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "map.ppm".to_string());
    let m = GameMap::get();
    let scale = 4usize;
    let w = GRID * scale;
    let mut px = vec![[0u8; 3]; w * w];
    for ty in 0..GRID {
        for tx in 0..GRID {
            let c = match m.terrain_at_tile(tx, ty) {
                Terrain::Wall => [30, 60, 35],
                Terrain::Grass => [80, 140, 70],
                Terrain::Lane => [170, 150, 100],
                Terrain::River => [60, 110, 190],
                Terrain::Brush => [40, 180, 60],
                Terrain::BaseBlue => [120, 130, 170],
                Terrain::BaseRed => [170, 120, 120],
                Terrain::Pit => [90, 80, 120],
            };
            for dy in 0..scale {
                for dx in 0..scale {
                    px[(ty * scale + dy) * w + tx * scale + dx] = c;
                }
            }
        }
    }
    let mut dot = |p: moba_core::Vec2, r: i32, c: [u8; 3]| {
        let cx = (p.x / 16.0 * scale as f32) as i32;
        let cy = (p.y / 16.0 * scale as f32) as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx + dx, cy + dy);
                if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < w {
                    px[y as usize * w + x as usize] = c;
                }
            }
        }
    };
    for s in &m.structures {
        let c = if s.team == Team::Blue { [40, 80, 255] } else { [255, 40, 40] };
        let r = match s.kind {
            StructureKind::Nexus => 8,
            StructureKind::Inhibitor { .. } => 5,
            _ => 4,
        };
        dot(s.pos, r, c);
    }
    for c in &m.camps {
        for (_, p) in &c.monsters {
            dot(*p, 3, [255, 220, 0]);
        }
    }
    for f in m.fountains {
        dot(f, 6, [255, 255, 255]);
    }
    for team in 0..2 {
        for lane in &m.lanes[team] {
            for p in lane {
                dot(*p, 2, [0, 0, 0]);
            }
        }
    }
    let mut f = std::fs::File::create(&out).unwrap();
    write!(f, "P6\n{w} {w}\n255\n").unwrap();
    for p in px {
        f.write_all(&p).unwrap();
    }
    println!("wrote {out} ({w}x{w})");
}
