//! The battlefield: a three-lane map modelled on Summoner's Rift.
//!
//! * Blue base bottom-left, red base top-right (screen coordinates: +y is down).
//! * Top / mid / bot lanes, a diagonal river with the Baron pit (top side) and
//!   the Dragon pit (bot side), four jungle quadrants with six camps each side.
//! * The layout is point-symmetric: every blue-side shape is mirrored through
//!   the map centre to build the red side, so both teams get the same map.
//!
//! The map is generated procedurally and deterministically on both client and
//! server, so it never travels over the network.

use crate::math::Vec2;
use crate::units::{MonsterKind, StructureKind};
use crate::Team;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::OnceLock;

pub const MAP_SIZE: f32 = 3200.0;
pub const TILE: f32 = 16.0;
pub const GRID: usize = (MAP_SIZE / TILE) as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Terrain {
    Wall,
    Grass,
    Lane,
    River,
    Brush,
    BaseBlue,
    BaseRed,
    Pit,
}

impl Terrain {
    pub fn walkable(self) -> bool {
        self != Terrain::Wall
    }
}

pub const LANE_TOP: u8 = 0;
pub const LANE_MID: u8 = 1;
pub const LANE_BOT: u8 = 2;
pub const LANE_NAMES: [&str; 3] = ["Top", "Mid", "Bot"];

/// Rotate a point 180 degrees around the map centre (blue side -> red side).
pub fn rot(p: Vec2) -> Vec2 {
    Vec2::new(MAP_SIZE - p.x, MAP_SIZE - p.y)
}

const fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

#[derive(Clone, Debug)]
pub struct StructureSpawn {
    pub team: Team,
    pub kind: StructureKind,
    pub pos: Vec2,
}

#[derive(Clone, Debug)]
pub struct CampSpawn {
    pub id: u8,
    pub pos: Vec2,
    pub monsters: Vec<(MonsterKind, Vec2)>,
    /// First spawn time (seconds).
    pub first_spawn: f32,
    /// Respawn delay after the camp is cleared.
    pub respawn: f32,
    /// Whose jungle the camp is in (Neutral for river objectives).
    pub side: Team,
}

pub struct GameMap {
    tiles: Vec<Terrain>,
    brush: Vec<u16>,
    /// `lanes[team][lane]`: waypoints from that team's base to the enemy nexus.
    pub lanes: [[Vec<Vec2>; 3]; 2],
    pub structures: Vec<StructureSpawn>,
    pub camps: Vec<CampSpawn>,
    pub fountains: [Vec2; 2],
    pub nexus: [Vec2; 2],
}

/// Radius of the fountain platform (healing, shop and the fountain laser).
pub const FOUNTAIN_RADIUS: f32 = 190.0;

// ------------------------------------------------------------------ layout

/// Top lane centre line. The bot lane is its rotation.
const TOP_LANE: [Vec2; 4] = [v(260.0, 2700.0), v(260.0, 430.0), v(430.0, 260.0), v(2700.0, 260.0)];
const MID_LANE: [Vec2; 2] = [v(520.0, 2680.0), v(2680.0, 520.0)];
/// Half of the river; the other half is its rotation.
const RIVER: [Vec2; 4] = [v(300.0, 300.0), v(700.0, 620.0), v(1150.0, 1100.0), v(1600.0, 1600.0)];
const BASE_CENTER: Vec2 = v(160.0, 3040.0);
const BASE_RADIUS: f32 = 640.0;
const BARON_PIT: Vec2 = v(1040.0, 745.0);
const BLUE_FOUNTAIN: Vec2 = v(110.0, 3090.0);
const BLUE_NEXUS: Vec2 = v(330.0, 2870.0);

const CAMP_GROMP: Vec2 = v(520.0, 1250.0);
const CAMP_BLUE: Vec2 = v(790.0, 1560.0);
const CAMP_WOLVES: Vec2 = v(830.0, 2010.0);
const CAMP_RAPTORS: Vec2 = v(1450.0, 2060.0);
const CAMP_RED: Vec2 = v(1700.0, 2430.0);
const CAMP_KRUGS: Vec2 = v(2020.0, 2660.0);

/// Jungle corridors on the blue side (half width 50).
const JUNGLE_PATHS: &[&[Vec2]] = &[
    &[v(260.0, 1330.0), CAMP_GROMP],
    &[CAMP_GROMP, CAMP_BLUE],
    &[CAMP_BLUE, v(1120.0, 1700.0), v(1300.0, 1900.0)],
    &[CAMP_BLUE, v(760.0, 1180.0), v(880.0, 860.0)],
    &[CAMP_BLUE, CAMP_WOLVES],
    &[CAMP_WOLVES, v(600.0, 2330.0), v(480.0, 2520.0)],
    &[CAMP_WOLVES, v(1120.0, 2120.0)],
    &[v(1300.0, 1900.0), CAMP_RAPTORS],
    &[CAMP_RAPTORS, CAMP_RED],
    &[CAMP_RED, CAMP_KRUGS],
    &[CAMP_RED, v(1300.0, 2620.0), v(900.0, 2700.0), v(700.0, 2760.0)],
    &[CAMP_KRUGS, v(2150.0, 2940.0)],
    &[CAMP_RED, v(1880.0, 2180.0), v(2060.0, 2110.0)],
    &[CAMP_RAPTORS, v(1720.0, 1760.0)],
    &[CAMP_KRUGS, v(2160.0, 2455.0)],
    &[CAMP_GROMP, v(540.0, 900.0), v(620.0, 600.0)],
];

/// Extra open jungle clearings (centre, radius).
const CLEARINGS: &[(Vec2, f32)] = &[
    (CAMP_GROMP, 85.0),
    (CAMP_BLUE, 90.0),
    (CAMP_WOLVES, 80.0),
    (CAMP_RAPTORS, 80.0),
    (CAMP_RED, 90.0),
    (CAMP_KRUGS, 85.0),
    (v(620.0, 1640.0), 60.0),
    (v(1180.0, 2350.0), 55.0),
    (v(1550.0, 2600.0), 50.0),
];

/// Brush patches (centre, radius) on the blue side.
const BRUSHES: &[(Vec2, f32)] = &[
    (v(330.0, 640.0), 40.0),
    (v(640.0, 330.0), 40.0),
    (v(190.0, 1500.0), 34.0),
    (v(1500.0, 190.0), 34.0),
    (v(1350.0, 1720.0), 34.0),
    (v(1720.0, 1350.0), 34.0),
    (v(840.0, 900.0), 40.0),
    (v(700.0, 1450.0), 34.0),
    (v(1580.0, 2330.0), 34.0),
    (v(2120.0, 2830.0), 45.0),
    (v(1180.0, 880.0), 38.0),
];

enum Shape {
    Circle(Vec2, f32),
    Path(Vec<Vec2>, f32),
}

impl Shape {
    fn rotated(&self) -> Shape {
        match self {
            Shape::Circle(c, r) => Shape::Circle(rot(*c), *r),
            Shape::Path(pts, hw) => Shape::Path(pts.iter().map(|p| rot(*p)).collect(), *hw),
        }
    }

    fn bbox(&self) -> (Vec2, Vec2) {
        match self {
            Shape::Circle(c, r) => (*c - Vec2::new(*r, *r), *c + Vec2::new(*r, *r)),
            Shape::Path(pts, hw) => {
                let mut lo = pts[0];
                let mut hi = pts[0];
                for p in pts {
                    lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                    hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
                }
                (lo - Vec2::new(*hw, *hw), hi + Vec2::new(*hw, *hw))
            }
        }
    }

    fn contains(&self, p: Vec2) -> bool {
        match self {
            Shape::Circle(c, r) => p.distance_sq(*c) <= r * r,
            Shape::Path(pts, hw) => pts.windows(2).any(|w| crate::math::dist_point_segment(p, w[0], w[1]) <= *hw),
        }
    }
}

fn tile_center(tx: usize, ty: usize) -> Vec2 {
    Vec2::new((tx as f32 + 0.5) * TILE, (ty as f32 + 0.5) * TILE)
}

impl GameMap {
    /// The (lazily generated, shared) map.
    pub fn get() -> &'static GameMap {
        static MAP: OnceLock<GameMap> = OnceLock::new();
        MAP.get_or_init(GameMap::generate)
    }

    fn generate() -> GameMap {
        let mut tiles = vec![Terrain::Wall; GRID * GRID];

        let paint = |tiles: &mut Vec<Terrain>, shape: &Shape, terrain: Terrain, only_walkable: bool| {
            let (lo, hi) = shape.bbox();
            let x0 = ((lo.x / TILE).floor().max(0.0)) as usize;
            let y0 = ((lo.y / TILE).floor().max(0.0)) as usize;
            let x1 = ((hi.x / TILE).ceil() as usize).min(GRID - 1);
            let y1 = ((hi.y / TILE).ceil() as usize).min(GRID - 1);
            for ty in y0..=y1 {
                for tx in x0..=x1 {
                    let i = ty * GRID + tx;
                    if only_walkable && !matches!(tiles[i], Terrain::Grass | Terrain::Lane | Terrain::River) {
                        continue;
                    }
                    if shape.contains(tile_center(tx, ty)) {
                        tiles[i] = terrain;
                    }
                }
            }
        };
        // Paint a blue-side shape and its red-side mirror image.
        let paint_both = |tiles: &mut Vec<Terrain>, shape: Shape, terrain: Terrain, only_walkable: bool| {
            paint(tiles, &shape, terrain, only_walkable);
            paint(tiles, &shape.rotated(), terrain, only_walkable);
        };

        // Jungle.
        for path in JUNGLE_PATHS {
            paint_both(&mut tiles, Shape::Path(path.to_vec(), 50.0), Terrain::Grass, false);
        }
        for (c, r) in CLEARINGS {
            paint_both(&mut tiles, Shape::Circle(*c, *r), Terrain::Grass, false);
        }
        // Lanes.
        paint_both(&mut tiles, Shape::Path(TOP_LANE.to_vec(), 80.0), Terrain::Lane, false);
        paint_both(&mut tiles, Shape::Path(MID_LANE.to_vec(), 85.0), Terrain::Lane, false);
        // River + pits.
        paint_both(&mut tiles, Shape::Path(RIVER.to_vec(), 75.0), Terrain::River, false);
        paint_both(&mut tiles, Shape::Circle(BARON_PIT, 100.0), Terrain::Pit, false);
        // Brush.
        for (c, r) in BRUSHES {
            paint_both(&mut tiles, Shape::Circle(*c, *r), Terrain::Brush, true);
        }
        // Bases.
        paint(&mut tiles, &Shape::Circle(BASE_CENTER, BASE_RADIUS), Terrain::BaseBlue, false);
        paint(&mut tiles, &Shape::Circle(rot(BASE_CENTER), BASE_RADIUS), Terrain::BaseRed, false);
        // Enforce exact point symmetry (guards against float rounding at
        // shape borders): the blue (bottom) half is authoritative.
        let n = GRID * GRID;
        for i in n / 2..n {
            tiles[n - 1 - i] = match tiles[i] {
                Terrain::BaseBlue => Terrain::BaseRed,
                Terrain::BaseRed => Terrain::BaseBlue,
                t => t,
            };
        }
        // Map border.
        for i in 0..GRID {
            for b in [0, 1, GRID - 2, GRID - 1] {
                tiles[i * GRID + b] = Terrain::Wall;
                tiles[b * GRID + i] = Terrain::Wall;
            }
        }

        let brush = label_brush(&tiles);
        let structures = build_structures();
        let camps = build_camps();

        let blue_paths: [Vec<Vec2>; 3] = [
            vec![v(260.0, 2700.0), v(260.0, 430.0), v(430.0, 260.0), v(2700.0, 260.0), rot(BLUE_NEXUS)],
            vec![v(520.0, 2680.0), v(2680.0, 520.0), rot(BLUE_NEXUS)],
            vec![v(500.0, 2940.0), v(2770.0, 2940.0), v(2940.0, 2770.0), v(2940.0, 500.0), rot(BLUE_NEXUS)],
        ];
        let rot_path = |p: &Vec<Vec2>| p.iter().map(|x| rot(*x)).collect::<Vec<_>>();
        // Red's top lane is the mirror image of blue's bot lane and vice versa.
        let red_paths = [rot_path(&blue_paths[2]), rot_path(&blue_paths[1]), rot_path(&blue_paths[0])];

        GameMap {
            tiles,
            brush,
            lanes: [blue_paths, red_paths],
            structures,
            camps,
            fountains: [BLUE_FOUNTAIN, rot(BLUE_FOUNTAIN)],
            nexus: [BLUE_NEXUS, rot(BLUE_NEXUS)],
        }
    }

    // -------------------------------------------------------------- queries

    pub fn tile_index(p: Vec2) -> Option<usize> {
        if p.x < 0.0 || p.y < 0.0 || p.x >= MAP_SIZE || p.y >= MAP_SIZE {
            return None;
        }
        Some((p.y / TILE) as usize * GRID + (p.x / TILE) as usize)
    }

    pub fn terrain_at_tile(&self, tx: usize, ty: usize) -> Terrain {
        if tx >= GRID || ty >= GRID {
            return Terrain::Wall;
        }
        self.tiles[ty * GRID + tx]
    }

    pub fn terrain(&self, p: Vec2) -> Terrain {
        match Self::tile_index(p) {
            Some(i) => self.tiles[i],
            None => Terrain::Wall,
        }
    }

    pub fn walkable(&self, p: Vec2) -> bool {
        self.terrain(p).walkable()
    }

    /// Brush region id at `p` (0 = not in brush).
    pub fn brush_at(&self, p: Vec2) -> u16 {
        match Self::tile_index(p) {
            Some(i) => self.brush[i],
            None => 0,
        }
    }

    pub fn fountain(&self, team: Team) -> Vec2 {
        self.fountains[team.index().min(1)]
    }

    /// Spawn point for the n-th hero of a team, spread around the fountain.
    pub fn spawn_point(&self, team: Team, n: usize) -> Vec2 {
        let f = self.fountain(team);
        let toward_center = (Vec2::new(MAP_SIZE / 2.0, MAP_SIZE / 2.0) - f).normalize_or_zero();
        let side = toward_center.perp();
        let offset = (n as f32 - 2.0) * 22.0;
        let p = f + toward_center * 60.0 + side * offset;
        self.nearest_walkable(p)
    }

    /// Is the straight segment a..b walkable for a body of radius `r`?
    pub fn line_clear(&self, a: Vec2, b: Vec2, r: f32) -> bool {
        let d = b - a;
        let len = d.length();
        let steps = (len / 6.0).ceil().max(1.0) as usize;
        let n = if len > 1e-6 { d.perp() / len } else { Vec2::ZERO };
        for i in 0..=steps {
            let p = a + d * (i as f32 / steps as f32);
            if !self.walkable(p) {
                return false;
            }
            if r > 0.0 && (!self.walkable(p + n * r) || !self.walkable(p - n * r)) {
                return false;
            }
        }
        true
    }

    /// Line of sight for vision: walls block, everything else doesn't.
    pub fn sight_clear(&self, a: Vec2, b: Vec2) -> bool {
        let d = b - a;
        let steps = (d.length() / 12.0).ceil().max(1.0) as usize;
        (1..steps).all(|i| self.walkable(a + d * (i as f32 / steps as f32)))
    }

    /// Closest walkable position (tile centre) to `p`, or `p` if walkable.
    pub fn nearest_walkable(&self, p: Vec2) -> Vec2 {
        if self.walkable(p) {
            return p;
        }
        let cx = (p.x / TILE).floor() as i32;
        let cy = (p.y / TILE).floor() as i32;
        for r in 1..40i32 {
            let mut best: Option<(f32, Vec2)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let (tx, ty) = (cx + dx, cy + dy);
                    if tx < 0 || ty < 0 || tx >= GRID as i32 || ty >= GRID as i32 {
                        continue;
                    }
                    if self.terrain_at_tile(tx as usize, ty as usize).walkable() {
                        let c = tile_center(tx as usize, ty as usize);
                        let d = c.distance_sq(p);
                        if best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, c));
                        }
                    }
                }
            }
            if let Some((_, c)) = best {
                return c;
            }
        }
        p
    }

    /// Move `pos` by `delta`, sliding along walls instead of entering them.
    pub fn slide_move(&self, pos: Vec2, delta: Vec2) -> Vec2 {
        let np = pos + delta;
        if self.walkable(np) {
            return np;
        }
        let nx = Vec2::new(pos.x + delta.x, pos.y);
        let ny = Vec2::new(pos.x, pos.y + delta.y);
        let (first, second) = if delta.x.abs() >= delta.y.abs() { (nx, ny) } else { (ny, nx) };
        if self.walkable(first) {
            return first;
        }
        if self.walkable(second) {
            return second;
        }
        pos
    }

    /// A* over the tile grid followed by string pulling. Returns waypoints
    /// (excluding `from`). `None` when unreachable.
    pub fn find_path(&self, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
        if self.line_clear(from, to, 5.0) {
            return Some(vec![to]);
        }
        let goal_pos = self.nearest_walkable(to);
        let start = Self::tile_index(self.nearest_walkable(from))?;
        let goal = Self::tile_index(goal_pos)?;
        if start == goal {
            return Some(vec![goal_pos]);
        }
        let raw = ASTAR.with(|s| s.borrow_mut().search(&self.tiles, start, goal))?;
        let mut pts: Vec<Vec2> = raw.iter().map(|&i| tile_center(i % GRID, i / GRID)).collect();
        if let Some(last) = pts.last_mut() {
            *last = goal_pos;
        }
        // String pulling.
        let mut out = Vec::new();
        let mut anchor = from;
        let mut i = 0;
        while i < pts.len() {
            let mut j = i;
            while j + 1 < pts.len() && self.line_clear(anchor, pts[j + 1], 5.0) {
                j += 1;
            }
            out.push(pts[j]);
            anchor = pts[j];
            i = j + 1;
        }
        Some(out)
    }

    /// Distance along a lane path of the projection of `p` (used to find
    /// the front-most minion of a wave).
    pub fn lane_progress(path: &[Vec2], p: Vec2) -> f32 {
        let mut best = (f32::MAX, 0.0);
        let mut acc = 0.0;
        for w in path.windows(2) {
            let q = crate::math::closest_point_on_segment(p, w[0], w[1]);
            let d = q.distance_sq(p);
            if d < best.0 {
                best = (d, acc + w[0].distance(q));
            }
            acc += w[0].distance(w[1]);
        }
        best.1
    }

    /// Point at a given distance along a path.
    pub fn point_along(path: &[Vec2], dist: f32) -> Vec2 {
        let mut left = dist.max(0.0);
        for w in path.windows(2) {
            let seg = w[0].distance(w[1]);
            if left <= seg {
                return w[0].move_towards(w[1], left);
            }
            left -= seg;
        }
        *path.last().unwrap_or(&Vec2::ZERO)
    }
}

fn label_brush(tiles: &[Terrain]) -> Vec<u16> {
    let mut ids = vec![0u16; tiles.len()];
    let mut next = 1u16;
    let mut stack = Vec::new();
    for start in 0..tiles.len() {
        if tiles[start] != Terrain::Brush || ids[start] != 0 {
            continue;
        }
        ids[start] = next;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % GRID, i / GRID);
            let mut visit = |nx: usize, ny: usize| {
                let j = ny * GRID + nx;
                if tiles[j] == Terrain::Brush && ids[j] == 0 {
                    ids[j] = next;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(x - 1, y);
            }
            if x + 1 < GRID {
                visit(x + 1, y);
            }
            if y > 0 {
                visit(x, y - 1);
            }
            if y + 1 < GRID {
                visit(x, y + 1);
            }
        }
        next += 1;
    }
    ids
}

fn build_structures() -> Vec<StructureSpawn> {
    use StructureKind::*;
    let blue: Vec<(StructureKind, Vec2)> = vec![
        (Tower { lane: LANE_TOP, tier: 1 }, v(260.0, 1220.0)),
        (Tower { lane: LANE_TOP, tier: 2 }, v(260.0, 1800.0)),
        (Tower { lane: LANE_TOP, tier: 3 }, v(260.0, 2320.0)),
        (Inhibitor { lane: LANE_TOP }, v(260.0, 2470.0)),
        (Tower { lane: LANE_MID, tier: 1 }, v(1250.0, 1950.0)),
        (Tower { lane: LANE_MID, tier: 2 }, v(960.0, 2240.0)),
        (Tower { lane: LANE_MID, tier: 3 }, v(720.0, 2480.0)),
        (Inhibitor { lane: LANE_MID }, v(600.0, 2600.0)),
        (Tower { lane: LANE_BOT, tier: 1 }, v(1980.0, 2940.0)),
        (Tower { lane: LANE_BOT, tier: 2 }, v(1400.0, 2940.0)),
        (Tower { lane: LANE_BOT, tier: 3 }, v(880.0, 2940.0)),
        (Inhibitor { lane: LANE_BOT }, v(730.0, 2940.0)),
        (Tower { lane: 3, tier: 4 }, v(380.0, 2760.0)),
        (Tower { lane: 3, tier: 4 }, v(440.0, 2820.0)),
        (Nexus, BLUE_NEXUS),
    ];
    let mut out = Vec::new();
    for (kind, pos) in &blue {
        out.push(StructureSpawn { team: Team::Blue, kind: *kind, pos: *pos });
    }
    for (kind, pos) in &blue {
        // Mirror: blue top <-> red bot.
        let kind = match *kind {
            Tower { lane, tier } if lane < 3 => Tower { lane: 2 - lane, tier },
            Inhibitor { lane } => Inhibitor { lane: 2 - lane },
            k => k,
        };
        out.push(StructureSpawn { team: Team::Red, kind, pos: rot(*pos) });
    }
    out
}

fn build_camps() -> Vec<CampSpawn> {
    use MonsterKind::*;
    let off = |c: Vec2, dx: f32, dy: f32| Vec2::new(c.x + dx, c.y + dy);
    let blue_side: Vec<Vec<(MonsterKind, Vec2)>> = vec![
        vec![(Gromp, CAMP_GROMP)],
        vec![(BlueSentinel, CAMP_BLUE)],
        vec![(BigWolf, CAMP_WOLVES), (Wolf, off(CAMP_WOLVES, -30.0, 28.0)), (Wolf, off(CAMP_WOLVES, 30.0, 28.0))],
        vec![
            (BigRaptor, CAMP_RAPTORS),
            (Raptor, off(CAMP_RAPTORS, -28.0, 22.0)),
            (Raptor, off(CAMP_RAPTORS, 28.0, 22.0)),
        ],
        vec![(RedBrambleback, CAMP_RED)],
        vec![(BigKrug, CAMP_KRUGS), (Krug, off(CAMP_KRUGS, 30.0, -20.0))],
    ];
    let mut camps = Vec::new();
    let mut id = 0u8;
    for (side, mirror) in [(Team::Blue, false), (Team::Red, true)] {
        for members in &blue_side {
            let m: Vec<(MonsterKind, Vec2)> =
                members.iter().map(|(k, p)| (*k, if mirror { rot(*p) } else { *p })).collect();
            let buff = matches!(m[0].0, BlueSentinel | RedBrambleback);
            camps.push(CampSpawn {
                id,
                pos: m[0].1,
                monsters: m,
                first_spawn: 45.0,
                respawn: if buff { 150.0 } else { 100.0 },
                side,
            });
            id += 1;
        }
    }
    camps.push(CampSpawn {
        id,
        pos: rot(BARON_PIT),
        monsters: vec![(Dragon, rot(BARON_PIT))],
        first_spawn: 150.0,
        respawn: 240.0,
        side: Team::Neutral,
    });
    id += 1;
    camps.push(CampSpawn {
        id,
        pos: BARON_PIT,
        monsters: vec![(Baron, BARON_PIT)],
        first_spawn: 480.0,
        respawn: 300.0,
        side: Team::Neutral,
    });
    camps
}

// ------------------------------------------------------------------ A*

thread_local! {
    static ASTAR: RefCell<AStarScratch> = RefCell::new(AStarScratch::new());
}

struct AStarScratch {
    g: Vec<u32>,
    parent: Vec<u32>,
    stamp: Vec<u32>,
    closed: Vec<u32>,
    generation: u32,
}

impl AStarScratch {
    fn new() -> Self {
        let n = GRID * GRID;
        AStarScratch { g: vec![0; n], parent: vec![0; n], stamp: vec![0; n], closed: vec![0; n], generation: 0 }
    }

    fn search(&mut self, tiles: &[Terrain], start: usize, goal: usize) -> Option<Vec<usize>> {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.closed.iter_mut().for_each(|s| *s = 0);
            self.generation = 1;
        }
        let gen = self.generation;
        let (gx, gy) = ((goal % GRID) as i32, (goal / GRID) as i32);
        let h = |i: usize| -> u32 {
            let dx = ((i % GRID) as i32 - gx).unsigned_abs();
            let dy = ((i / GRID) as i32 - gy).unsigned_abs();
            let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
            lo * 14 + (hi - lo) * 10
        };
        let mut open = BinaryHeap::new();
        self.g[start] = 0;
        self.stamp[start] = gen;
        self.parent[start] = start as u32;
        open.push(Reverse((h(start), start as u32)));
        let mut expanded = 0;
        while let Some(Reverse((_, cur))) = open.pop() {
            let cur = cur as usize;
            if self.closed[cur] == gen {
                continue;
            }
            self.closed[cur] = gen;
            if cur == goal {
                let mut out = vec![goal];
                let mut c = goal;
                while c != start {
                    c = self.parent[c] as usize;
                    out.push(c);
                }
                out.pop(); // drop start
                out.reverse();
                return Some(out);
            }
            expanded += 1;
            if expanded > 30_000 {
                return None;
            }
            let (cx, cy) = ((cur % GRID) as i32, (cur / GRID) as i32);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= GRID as i32 || ny >= GRID as i32 {
                    continue;
                }
                let ni = ny as usize * GRID + nx as usize;
                if !tiles[ni].walkable() || self.closed[ni] == gen {
                    continue;
                }
                let diagonal = dx != 0 && dy != 0;
                if diagonal {
                    // No corner cutting.
                    let a = cy as usize * GRID + nx as usize;
                    let b = ny as usize * GRID + cx as usize;
                    if !tiles[a].walkable() || !tiles[b].walkable() {
                        continue;
                    }
                }
                let ng = self.g[cur] + if diagonal { 14 } else { 10 };
                if self.stamp[ni] != gen || ng < self.g[ni] {
                    self.stamp[ni] = gen;
                    self.g[ni] = ng;
                    self.parent[ni] = cur as u32;
                    open.push(Reverse((ng + h(ni), ni as u32)));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_points_are_walkable() {
        let m = GameMap::get();
        for s in &m.structures {
            assert!(m.walkable(s.pos), "{:?} {:?} at {:?} is inside a wall", s.team, s.kind, s.pos);
        }
        for c in &m.camps {
            for (k, p) in &c.monsters {
                assert!(m.walkable(*p), "{k:?} at {p:?} is inside a wall");
            }
        }
        for f in m.fountains {
            assert!(m.walkable(f));
        }
        for team in 0..2 {
            for lane in &m.lanes[team] {
                for w in lane.windows(2) {
                    assert!(m.line_clear(w[0], w[1], 0.0), "lane segment {:?} -> {:?} blocked", w[0], w[1]);
                }
            }
        }
    }

    #[test]
    fn map_is_point_symmetric() {
        let m = GameMap::get();
        for ty in 0..GRID {
            for tx in 0..GRID {
                let a = m.terrain_at_tile(tx, ty);
                let b = m.terrain_at_tile(GRID - 1 - tx, GRID - 1 - ty);
                let a_norm = if a == Terrain::BaseRed { Terrain::BaseBlue } else { a };
                let b_norm = if b == Terrain::BaseRed { Terrain::BaseBlue } else { b };
                assert_eq!(a.walkable(), b.walkable(), "asymmetric walkability at {tx},{ty}");
                if a != Terrain::Pit && b != Terrain::Pit {
                    assert_eq!(a_norm, b_norm, "asymmetric terrain at {tx},{ty}");
                }
            }
        }
    }

    #[test]
    fn paths_between_bases_and_camps() {
        let m = GameMap::get();
        let blue = m.spawn_point(Team::Blue, 0);
        let red = m.spawn_point(Team::Red, 0);
        let path = m.find_path(blue, red).expect("bases connected");
        assert!(!path.is_empty());
        for c in &m.camps {
            assert!(m.find_path(blue, c.pos).is_some(), "camp {} unreachable", c.id);
        }
        // Waypoints must be walkable.
        for p in &path {
            assert!(m.walkable(*p));
        }
    }

    #[test]
    fn brush_exists() {
        let m = GameMap::get();
        let max = m.brush.iter().copied().max().unwrap_or(0);
        assert!(max >= 10, "expected many brush patches, got {max}");
    }

    #[test]
    fn lane_progress_monotonic() {
        let m = GameMap::get();
        let path = &m.lanes[0][1];
        let a = GameMap::lane_progress(path, Vec2::new(800.0, 2400.0));
        let b = GameMap::lane_progress(path, Vec2::new(1600.0, 1600.0));
        assert!(b > a);
        let p = GameMap::point_along(path, b);
        assert!(p.distance(Vec2::new(1600.0, 1600.0)) < 5.0);
    }
}
