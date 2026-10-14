//! Procedural pixel-art terrain. The map is rasterised lazily in 400x400
//! chunks (1 pixel = 1 map unit). Tile edges are smoothed by bilinear
//! interpolation of the 16-unit tile grid, then textured with hash noise.

use moba_core::map::{GameMap, Terrain, FOUNTAIN_RADIUS, GRID, MAP_SIZE, TILE};
use moba_core::math::hash2;

use crate::art::{mix, shade};

pub const CHUNK: usize = 400;
pub const CHUNKS: usize = (MAP_SIZE as usize).div_ceil(CHUNK);

fn tile(map: &GameMap, tx: i32, ty: i32) -> Terrain {
    let tx = tx.clamp(0, GRID as i32 - 1) as usize;
    let ty = ty.clamp(0, GRID as i32 - 1) as usize;
    map.terrain_at_tile(tx, ty)
}

struct Sample {
    terrain: Terrain,
    /// Interpolated "wallness" (0..1).
    wall: f32,
    /// Interpolated weight of the chosen terrain.
    weight: f32,
}

fn sample(map: &GameMap, x: i32, y: i32) -> Sample {
    let fx = x as f32 / TILE - 0.5;
    let fy = y as f32 / TILE - 0.5;
    let i0 = fx.floor() as i32;
    let j0 = fy.floor() as i32;
    let tx = fx - i0 as f32;
    let ty = fy - j0 as f32;
    let corners = [
        (tile(map, i0, j0), (1.0 - tx) * (1.0 - ty)),
        (tile(map, i0 + 1, j0), tx * (1.0 - ty)),
        (tile(map, i0, j0 + 1), (1.0 - tx) * ty),
        (tile(map, i0 + 1, j0 + 1), tx * ty),
    ];
    let first = corners[0].0;
    if corners.iter().all(|c| c.0 == first) {
        return Sample { terrain: first, wall: if first == Terrain::Wall { 1.0 } else { 0.0 }, weight: 1.0 };
    }
    let wall: f32 = corners.iter().filter(|c| c.0 == Terrain::Wall).map(|c| c.1).sum();
    let jitter = ((hash2(x >> 2, y >> 2, 7) & 0xff) as f32 / 255.0 - 0.5) * 0.14;
    if wall > 0.5 + jitter {
        return Sample { terrain: Terrain::Wall, wall, weight: wall };
    }
    let mut best = (Terrain::Grass, -1.0f32);
    for (t, _) in corners.iter() {
        if *t == Terrain::Wall {
            continue;
        }
        let wsum: f32 = corners.iter().filter(|c| c.0 == *t).map(|c| c.1).sum();
        if wsum > best.1 {
            best = (*t, wsum);
        }
    }
    Sample { terrain: best.0, wall, weight: best.1 / (1.0 - wall).max(0.01) }
}

fn pick(h: u32, palette: &[u32]) -> u32 {
    palette[(h as usize) % palette.len()]
}

fn trees(x: i32, y: i32) -> u32 {
    const CELL: i32 = 14;
    let cx = x.div_euclid(CELL);
    let cy = y.div_euclid(CELL);
    // (tree centre y, normalised distance^2, dx, dy, radius); the front-most
    // (lowest on screen) covering tree wins, like painting back to front.
    let mut best: Option<(f32, f32, f32, f32, f32)> = None;
    for oy in -1..=1 {
        for ox in -1..=1 {
            let (gx, gy) = (cx + ox, cy + oy);
            let h = hash2(gx, gy, 31);
            let px = gx as f32 * CELL as f32 + (h & 0xf) as f32 * CELL as f32 / 16.0;
            let py = gy as f32 * CELL as f32 + ((h >> 4) & 0xf) as f32 * CELL as f32 / 16.0;
            let r = 7.0 + ((h >> 8) & 0x7) as f32 * 0.6;
            let dx = x as f32 - px;
            let dy = y as f32 - py;
            let q = (dx * dx + dy * dy) / (r * r);
            if q <= 1.0 && best.is_none_or(|b| py > b.0) {
                best = Some((py, q, dx, dy, r));
            }
        }
    }
    match best {
        Some((_, q, dx, dy, r)) => {
            let light = -(dx + dy) / r;
            if q > 0.8 {
                0x1a3a1f
            } else if light > 0.55 {
                0x4a8a40
            } else if light > 0.1 {
                0x356e36
            } else if light > -0.5 {
                0x2a5a2e
            } else {
                0x214a26
            }
        }
        None => {
            if hash2(x, y, 3).is_multiple_of(11) {
                0x183420
            } else {
                0x12261a
            }
        }
    }
}

fn pixel(map: &GameMap, x: i32, y: i32) -> u32 {
    let s = sample(map, x, y);
    let h = hash2(x, y, 1);
    let coarse = hash2(x >> 3, y >> 3, 2);
    let mut c = match s.terrain {
        Terrain::Wall => {
            let c = trees(x, y);
            if s.wall < 0.6 {
                shade(c, 0.55)
            } else {
                c
            }
        }
        Terrain::Grass => {
            let mut c = pick(coarse, &[0x4f8f3a, 0x4a8836, 0x56963f, 0x4f8f3a]);
            if h.is_multiple_of(13) {
                c = shade(c, 0.85);
            } else if h.is_multiple_of(89) {
                c = 0x78b452;
            } else if h.is_multiple_of(1499) {
                c = if h & 1 == 0 { 0xf0e070 } else { 0xe080a8 };
            }
            c
        }
        Terrain::Lane => {
            let mut c = pick(coarse, &[0xb8a070, 0xb09868, 0xbea676]);
            if h.is_multiple_of(41) {
                c = 0x8a7a58;
            } else if h.is_multiple_of(17) {
                c = shade(c, 1.07);
            }
            // Dither into grass at the lane borders.
            if s.weight < 0.62 && h.is_multiple_of(3) {
                c = 0x6a9a48;
            }
            c
        }
        Terrain::River => {
            let wave = ((x as f32 * 0.21 + (y as f32 * 0.09).sin() * 3.0 + y as f32 * 0.07) as i32).rem_euclid(19);
            let mut c = if wave == 0 { 0x6aa4e8 } else { pick(coarse, &[0x3a74c0, 0x3870ba, 0x3e7ac6]) };
            if s.weight < 0.6 {
                c = 0x9cc8f0;
            }
            c
        }
        Terrain::Brush => {
            let col = hash2(x, 0, 5);
            let stripe = (y + (col % 7) as i32).rem_euclid(6);
            if x % 2 == 0 && stripe < 4 {
                if stripe == 0 {
                    0x7ac860
                } else {
                    0x48a048
                }
            } else {
                0x2c7a34
            }
        }
        Terrain::BaseBlue | Terrain::BaseRed => {
            let (tile_c, mortar) =
                if s.terrain == Terrain::BaseBlue { (0x8a96b4, 0x5a6278) } else { (0xb49494, 0x785a5a) };
            let row = y.div_euclid(12);
            let bx = (x + if row % 2 == 0 { 0 } else { 6 }).rem_euclid(12);
            let by = y.rem_euclid(12);
            if bx == 0 || by == 0 {
                mortar
            } else {
                let t = hash2(x.div_euclid(12), row, 9);
                shade(tile_c, 0.92 + (t % 16) as f32 / 100.0)
            }
        }
        Terrain::Pit => {
            let t = hash2(x >> 3, y >> 3, 11);
            if (x + y).rem_euclid(8) == 0 {
                0x3a3448
            } else {
                shade(0x5a5068, 0.9 + (t % 20) as f32 / 100.0)
            }
        }
    };
    // Shadow cast by walls onto walkable ground.
    if s.terrain != Terrain::Wall && s.wall > 0.22 {
        c = shade(c, 1.0 - (s.wall - 0.2) * 0.9);
    }
    // Fountain platforms.
    for (i, f) in map.fountains.iter().enumerate() {
        let d = ((x as f32 - f.x).powi(2) + (y as f32 - f.y).powi(2)).sqrt();
        if d < FOUNTAIN_RADIUS {
            let team = if i == 0 { 0x58a0ff } else { 0xff6858 };
            if d > FOUNTAIN_RADIUS - 3.0 {
                c = team;
            } else if d < 26.0 {
                c = if d > 22.0 { 0xe8e0c0 } else { mix(team, 0xffffff, 0.35 + ((d * 0.6) as i32 % 2) as f32 * 0.2) };
            } else if (d as i32) % 24 == 0 {
                c = shade(c, 0.85);
            }
        }
    }
    c
}

/// RGBA pixels of chunk (cx, cy).
pub fn chunk_rgba(map: &GameMap, cx: usize, cy: usize) -> Vec<u8> {
    let mut out = vec![0u8; CHUNK * CHUNK * 4];
    let ox = (cx * CHUNK) as i32;
    let oy = (cy * CHUNK) as i32;
    for y in 0..CHUNK as i32 {
        for x in 0..CHUNK as i32 {
            let c = pixel(map, ox + x, oy + y);
            let i = (y as usize * CHUNK + x as usize) * 4;
            out[i] = (c >> 16) as u8;
            out[i + 1] = (c >> 8) as u8;
            out[i + 2] = c as u8;
            out[i + 3] = 255;
        }
    }
    out
}

pub fn minimap_color(t: Terrain) -> u32 {
    match t {
        Terrain::Wall => 0x1c3a22,
        Terrain::Grass => 0x3f7a34,
        Terrain::Lane => 0x9a8a62,
        Terrain::River => 0x3466a8,
        Terrain::Brush => 0x2e6a30,
        Terrain::BaseBlue => 0x5a6a90,
        Terrain::BaseRed => 0x905a5a,
        Terrain::Pit => 0x504860,
    }
}

/// Square minimap image of `size` x `size` pixels.
pub fn minimap_rgba(map: &GameMap, size: usize) -> Vec<u8> {
    let mut out = vec![0u8; size * size * 4];
    let k = MAP_SIZE / size as f32;
    for y in 0..size {
        for x in 0..size {
            let t = map.terrain(moba_core::Vec2::new((x as f32 + 0.5) * k, (y as f32 + 0.5) * k));
            let c = minimap_color(t);
            let i = (y * size + x) * 4;
            out[i] = (c >> 16) as u8;
            out[i + 1] = (c >> 8) as u8;
            out[i + 2] = c as u8;
            out[i + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_renders() {
        let map = GameMap::get();
        let px = chunk_rgba(map, 0, 7);
        assert_eq!(px.len(), CHUNK * CHUNK * 4);
        assert!(px.chunks(4).all(|p| p[3] == 255));
        let mm = minimap_rgba(map, 64);
        assert_eq!(mm.len(), 64 * 64 * 4);
    }
}
