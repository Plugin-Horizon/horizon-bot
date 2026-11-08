//! Vectors, boxes, angles and a small deterministic random source.

use std::ops::{Add, AddAssign, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
    pub fn from_array(a: [f64; 3]) -> Self {
        Vec3::new(a[0], a[1], a[2])
    }
    pub fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }
    pub fn length(self) -> f64 {
        self.length_sq().sqrt()
    }
    pub fn horizontal(self) -> Vec3 {
        Vec3::new(self.x, 0.0, self.z)
    }
    pub fn horizontal_length(self) -> f64 {
        (self.x * self.x + self.z * self.z).sqrt()
    }
    pub fn normalize(self) -> Vec3 {
        let l = self.length();
        if l < 1e-9 {
            Vec3::ZERO
        } else {
            self * (1.0 / l)
        }
    }
    pub fn distance(self, o: Vec3) -> f64 {
        (self - o).length()
    }
    pub fn distance_sq(self, o: Vec3) -> f64 {
        (self - o).length_sq()
    }
    pub fn horizontal_distance(self, o: Vec3) -> f64 {
        (self - o).horizontal_length()
    }
    pub fn with_y(self, y: f64) -> Vec3 {
        Vec3::new(self.x, y, self.z)
    }
    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self + (o - self) * t
    }
    pub fn block(self) -> [i32; 3] {
        [self.x.floor() as i32, self.y.floor() as i32, self.z.floor() as i32]
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, k: f64) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Aabb { min, max }
    }
    /// A player-sized box standing at `feet` (0.6 wide, `height` tall).
    pub fn player(feet: Vec3, height: f64) -> Self {
        Aabb::new(
            Vec3::new(feet.x - 0.3, feet.y, feet.z - 0.3),
            Vec3::new(feet.x + 0.3, feet.y + height, feet.z + 0.3),
        )
    }
    pub fn block(x: i32, y: i32, z: i32, top: f64) -> Self {
        Aabb::new(
            Vec3::new(x as f64, y as f64, z as f64),
            Vec3::new(x as f64 + 1.0, y as f64 + top, z as f64 + 1.0),
        )
    }
    pub fn offset(self, d: Vec3) -> Self {
        Aabb::new(self.min + d, self.max + d)
    }
    pub fn expand(self, d: Vec3) -> Self {
        let mut a = self;
        if d.x < 0.0 { a.min.x += d.x } else { a.max.x += d.x }
        if d.y < 0.0 { a.min.y += d.y } else { a.max.y += d.y }
        if d.z < 0.0 { a.min.z += d.z } else { a.max.z += d.z }
        a
    }
    pub fn inflate(self, r: f64) -> Self {
        Aabb::new(self.min - Vec3::new(r, r, r), self.max + Vec3::new(r, r, r))
    }
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min.x < o.max.x
            && self.max.x > o.min.x
            && self.min.y < o.max.y
            && self.max.y > o.min.y
            && self.min.z < o.max.z
            && self.max.z > o.min.z
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    /// The point of the box nearest to `p` (vanilla measures reach to it).
    pub fn closest_point(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            p.x.clamp(self.min.x, self.max.x),
            p.y.clamp(self.min.y, self.max.y),
            p.z.clamp(self.min.z, self.max.z),
        )
    }
    pub fn distance_to(&self, p: Vec3) -> f64 {
        self.closest_point(p).distance(p)
    }
    /// How far `self` may move along `axis` by `d` before touching `o` (vanilla's clip).
    pub fn clip(&self, o: &Aabb, axis: usize, d: f64) -> f64 {
        let (a0, a1, b0, b1) = match axis {
            0 => (self.min.y, self.max.y, self.min.z, self.max.z),
            1 => (self.min.x, self.max.x, self.min.z, self.max.z),
            _ => (self.min.x, self.max.x, self.min.y, self.max.y),
        };
        let (o0, o1, p0, p1) = match axis {
            0 => (o.min.y, o.max.y, o.min.z, o.max.z),
            1 => (o.min.x, o.max.x, o.min.z, o.max.z),
            _ => (o.min.x, o.max.x, o.min.y, o.max.y),
        };
        if !(a1 > o0 + 1e-7 && a0 < o1 - 1e-7 && b1 > p0 + 1e-7 && b0 < p1 - 1e-7) {
            return d;
        }
        let (smin, smax, omin, omax) = match axis {
            0 => (self.min.x, self.max.x, o.min.x, o.max.x),
            1 => (self.min.y, self.max.y, o.min.y, o.max.y),
            _ => (self.min.z, self.max.z, o.min.z, o.max.z),
        };
        if d > 0.0 && smax <= omin + 1e-7 {
            d.min(omin - smax)
        } else if d < 0.0 && smin >= omax - 1e-7 {
            d.max(omax - smin)
        } else {
            d
        }
    }
    /// Where the segment `from`..`to` enters the box, as a fraction of the way, if it does.
    pub fn ray(&self, from: Vec3, to: Vec3) -> Option<f64> {
        let d = to - from;
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for (o, dd, lo, hi) in [
            (from.x, d.x, self.min.x, self.max.x),
            (from.y, d.y, self.min.y, self.max.y),
            (from.z, d.z, self.min.z, self.max.z),
        ] {
            if dd.abs() < 1e-12 {
                if o < lo || o > hi {
                    return None;
                }
            } else {
                let (mut a, mut b) = ((lo - o) / dd, (hi - o) / dd);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some(t0)
    }
}

/// Vanilla's yaw: 0 looks to +Z (south), 90 to -X (west).
pub fn yaw_to(from: Vec3, to: Vec3) -> f32 {
    let d = to - from;
    (f64::atan2(-d.x, d.z).to_degrees()) as f32
}

pub fn pitch_to(from: Vec3, to: Vec3) -> f32 {
    let d = to - from;
    let h = d.horizontal_length();
    (-(f64::atan2(d.y, h).to_degrees())) as f32
}

/// The unit vector a player looks along.
pub fn look_vector(yaw: f32, pitch: f32) -> Vec3 {
    let (y, p) = ((yaw as f64).to_radians(), (pitch as f64).to_radians());
    Vec3::new(-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos())
}

/// The signed shortest turn from `a` to `b`, in degrees (-180..180].
pub fn wrap_degrees(d: f32) -> f32 {
    let mut d = d % 360.0;
    if d > 180.0 {
        d -= 360.0;
    }
    if d <= -180.0 {
        d += 360.0;
    }
    d
}

/// Turns `from` toward `to` by at most `max` degrees.
pub fn approach_angle(from: f32, to: f32, max: f32) -> f32 {
    let delta = wrap_degrees(to - from);
    from + delta.clamp(-max, max)
}

/// FNV-1a of a name, lower-cased: a bot's stable seed.
pub fn name_hash(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.to_ascii_lowercase().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// splitmix64: small, fast, good enough for gameplay dice.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// 0..1
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// -1..1
    pub fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    /// lo..=hi
    pub fn int(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % ((hi - lo + 1) as u64)) as i64
    }
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[(self.next_u64() % items.len() as u64) as usize])
        }
    }
    /// An approximately normal value (mean 0, sd 1).
    pub fn gauss(&mut self) -> f64 {
        (0..4).map(|_| self.unit()).sum::<f64>() * 3f64.sqrt() - 2.0 * 3f64.sqrt()
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
    /// A weighted pick: index of the chosen weight.
    pub fn weighted(&mut self, weights: &[f64]) -> Option<usize> {
        let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
        if total <= 0.0 {
            return None;
        }
        let mut r = self.unit() * total;
        for (i, w) in weights.iter().enumerate() {
            if *w <= 0.0 {
                continue;
            }
            if r < *w {
                return Some(i);
            }
            r -= w;
        }
        weights.iter().rposition(|w| *w > 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_matches_vanilla_axes() {
        let o = Vec3::ZERO;
        assert!((yaw_to(o, Vec3::new(0.0, 0.0, 1.0)) - 0.0).abs() < 1e-4);
        assert!((yaw_to(o, Vec3::new(-1.0, 0.0, 0.0)) - 90.0).abs() < 1e-4);
        assert!((yaw_to(o, Vec3::new(1.0, 0.0, 0.0)) + 90.0).abs() < 1e-4);
        let v = look_vector(90.0, 0.0);
        assert!((v.x + 1.0).abs() < 1e-9 && v.z.abs() < 1e-9);
        assert!((pitch_to(o, Vec3::new(1.0, 1.0, 0.0)) + 45.0).abs() < 1e-4);
    }

    #[test]
    fn turning_is_rate_limited_and_takes_the_short_way() {
        assert_eq!(approach_angle(170.0, -170.0, 5.0), 175.0);
        assert_eq!(approach_angle(0.0, 90.0, 30.0), 30.0);
        assert_eq!(approach_angle(0.0, 10.0, 30.0), 10.0);
    }

    #[test]
    fn clip_stops_at_a_wall() {
        let me = Aabb::player(Vec3::new(0.5, 0.0, 0.5), 1.8);
        let wall = Aabb::block(1, 0, 0, 1.0);
        let d = me.clip(&wall, 0, 2.0);
        assert!((d - 0.2).abs() < 1e-9, "{d}");
        assert_eq!(me.clip(&wall, 0, -2.0), -2.0);
    }

    #[test]
    fn ray_enters_box() {
        let b = Aabb::block(2, 0, 0, 1.0);
        let t = b.ray(Vec3::new(0.0, 0.5, 0.5), Vec3::new(4.0, 0.5, 0.5)).unwrap();
        assert!((t - 0.5).abs() < 1e-9);
        assert!(b.ray(Vec3::new(0.0, 2.5, 0.5), Vec3::new(4.0, 2.5, 0.5)).is_none());
    }

    #[test]
    fn rng_is_deterministic_and_weighted_picks_respect_zero() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..10 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut r = Rng::new(1);
        for _ in 0..100 {
            assert_ne!(r.weighted(&[1.0, 0.0, 3.0]), Some(1));
        }
        assert_eq!(r.weighted(&[0.0, 0.0]), None);
    }
}
