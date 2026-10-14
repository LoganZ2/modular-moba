use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f32 {
        self.length_sq().sqrt()
    }

    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    pub fn normalize_or_zero(self) -> Vec2 {
        let l = self.length();
        if l > 1e-6 {
            self / l
        } else {
            Vec2::ZERO
        }
    }

    /// Normalized vector, or `fallback` when the vector is (almost) zero.
    pub fn normalize_or(self, fallback: Vec2) -> Vec2 {
        let l = self.length();
        if l > 1e-6 {
            self / l
        } else {
            fallback
        }
    }

    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }

    pub fn cross(self, o: Vec2) -> f32 {
        self.x * o.y - self.y * o.x
    }

    pub fn distance(self, o: Vec2) -> f32 {
        (self - o).length()
    }

    pub fn distance_sq(self, o: Vec2) -> f32 {
        (self - o).length_sq()
    }

    pub fn lerp(self, o: Vec2, t: f32) -> Vec2 {
        self + (o - self) * t
    }

    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    pub fn from_angle(a: f32) -> Vec2 {
        Vec2::new(a.cos(), a.sin())
    }

    pub fn rotate(self, a: f32) -> Vec2 {
        let (s, c) = a.sin_cos();
        Vec2::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }

    /// Move from `self` toward `target` by at most `max_dist`.
    pub fn move_towards(self, target: Vec2, max_dist: f32) -> Vec2 {
        let d = target - self;
        let l = d.length();
        if l <= max_dist || l < 1e-6 {
            target
        } else {
            self + d / l * max_dist
        }
    }

    pub fn clamp_length(self, max: f32) -> Vec2 {
        let l = self.length();
        if l > max && l > 1e-6 {
            self / l * max
        } else {
            self
        }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, s: f32) {
        self.x *= s;
        self.y *= s;
    }
}
impl Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, s: f32) -> Vec2 {
        Vec2::new(self.x / s, self.y / s)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

/// Closest point to `p` on the segment `a..b`.
pub fn closest_point_on_segment(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-9 {
        return a;
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}

pub fn dist_point_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    p.distance(closest_point_on_segment(p, a, b))
}

/// Smallest signed difference between two angles, in (-PI, PI].
pub fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = (a - b) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// Tiny deterministic PRNG (xorshift64*). Good enough for crits and jitter and
/// keeps the simulation reproducible from a seed.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as u32
        }
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }
}

/// Stateless integer hash, handy for procedural decoration.
pub fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h =
        (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_distance() {
        let d = dist_point_segment(Vec2::new(5.0, 5.0), Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0));
        assert!((d - 5.0).abs() < 1e-5);
        let d = dist_point_segment(Vec2::new(-3.0, 4.0), Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0));
        assert!((d - 5.0).abs() < 1e-5);
    }

    #[test]
    fn angle_diff_wraps() {
        let d = angle_diff(3.1, -3.1);
        assert!(d.abs() < 0.1);
    }

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let f = a.next_f32();
        assert!((0.0..1.0).contains(&f));
    }
}
