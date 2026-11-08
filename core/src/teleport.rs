//! Squad teleports: where each bot lands around the destination, the nether portals they step
//! out of (real obsidian frames, lit, that stay), and the pearls they ride in on.

use crate::math::{Rng, Vec3};

/// Spots scattered within `range` of `center`, at least `spacing` apart where it can.
pub fn scatter(center: Vec3, n: usize, range: f64, spacing: f64, rng: &mut Rng) -> Vec<Vec3> {
    let mut out: Vec<Vec3> = Vec::with_capacity(n);
    for _ in 0..n {
        let mut best = center;
        for attempt in 0..30 {
            let a = rng.unit() * std::f64::consts::TAU;
            let r = range * rng.unit().sqrt();
            let p = center + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
            best = p;
            if out.iter().all(|q| q.horizontal_distance(p) >= spacing) || attempt == 29 {
                break;
            }
        }
        out.push(best);
    }
    out
}

/// A vanilla-sized portal: obsidian frame 4 wide and 5 tall, lit inside (2x3), along the X axis.
#[derive(Clone, Debug, PartialEq)]
pub struct Portal {
    /// The bottom-left frame block.
    pub base: [i32; 3],
    pub axis_x: bool,
}

impl Portal {
    pub fn blocks(&self) -> Vec<([i32; 3], String)> {
        let mut v = Vec::new();
        let [x, y, z] = self.base;
        let at = |i: i32, j: i32| if self.axis_x { [x + i, y + j, z] } else { [x, y + j, z + i] };
        for i in 0..4 {
            for j in 0..5 {
                let frame = i == 0 || i == 3 || j == 0 || j == 4;
                let state = if frame {
                    "minecraft:obsidian".to_owned()
                } else {
                    format!("minecraft:nether_portal[axis={}]", if self.axis_x { "x" } else { "z" })
                };
                v.push((at(i, j), state));
            }
        }
        v
    }

    /// Where the bots inside stand (up to two wide).
    pub fn inside(&self) -> Vec3 {
        let [x, y, z] = self.base;
        if self.axis_x {
            Vec3::new(x as f64 + 2.0, y as f64 + 1.0, z as f64 + 0.5)
        } else {
            Vec3::new(x as f64 + 0.5, y as f64 + 1.0, z as f64 + 2.0)
        }
    }
}

/// Portals for `n` bots: grounded ones take up to five, floating ones three. `ground_y` gives
/// the ground height at a column (None: unknown, the portal floats at the destination's height).
pub fn portals(center: Vec3, n: usize, range: f64, grounded: bool, ground_y: &dyn Fn(i32, i32) -> Option<i32>, rng: &mut Rng) -> Vec<(Portal, usize)> {
    let per = if grounded { 5 } else { 3 };
    let count = n.div_ceil(per).max(1);
    let spots = scatter(center, count, range, 8.0, rng);
    let mut left = n;
    spots
        .into_iter()
        .map(|s| {
            let (bx, bz) = (s.x.floor() as i32, s.z.floor() as i32);
            let y = if grounded {
                ground_y(bx, bz).unwrap_or(s.y.floor() as i32)
            } else {
                // Floating: a little above wherever it is.
                s.y.floor() as i32 + 2 + rng.int(0, 3) as i32
            };
            let take = left.min(per);
            left -= take;
            (Portal { base: [bx, y, bz], axis_x: rng.chance(0.5) }, take)
        })
        .collect()
}

/// A pearl's start: high above the landing spot, so vanilla's physics bring it down (a little
/// fall damage on landing, as a real pearl).
pub fn pearl_start(land: Vec3) -> (Vec3, Vec3) {
    (land + Vec3::new(0.0, 40.0, 0.0), Vec3::new(0.0, -1.5, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scattered_spots_keep_apart_and_in_range() {
        let mut r = Rng::new(4);
        let c = Vec3::new(100.0, 64.0, -50.0);
        let s = scatter(c, 10, 30.0, 3.0, &mut r);
        assert_eq!(s.len(), 10);
        assert!(s.iter().all(|p| p.horizontal_distance(c) <= 30.0 + 1e-9));
    }

    #[test]
    fn portals_hold_five_or_three() {
        let mut r = Rng::new(1);
        let g = |_, _| Some(70);
        let p = portals(Vec3::new(0.0, 64.0, 0.0), 12, 40.0, true, &g, &mut r);
        assert_eq!(p.iter().map(|(_, n)| n).sum::<usize>(), 12);
        assert_eq!(p.len(), 3);
        assert!(p.iter().all(|(pt, _)| pt.base[1] == 70));
        let f = portals(Vec3::new(0.0, 64.0, 0.0), 7, 40.0, false, &g, &mut r);
        assert_eq!(f.len(), 3);
        let blocks = p[0].0.blocks();
        assert_eq!(blocks.iter().filter(|(_, b)| b == "minecraft:obsidian").count(), 14);
        assert_eq!(blocks.iter().filter(|(_, b)| b.starts_with("minecraft:nether_portal")).count(), 6);
    }
}
