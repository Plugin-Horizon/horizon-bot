//! Formations: where follower `i` of `n` stands around whoever they follow. The first seven turn
//! with the leader; square, circle, hexagon and grid are lined up with the world.

use crate::math::{Rng, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Behind,
    Column,
    Line,
    Rows,
    Triangle,
    Wall,
    OpenCircle,
    Square,
    Circle,
    Hexagon,
    Grid,
}

impl Shape {
    pub fn parse(s: &str) -> Option<Shape> {
        Some(match s {
            "behind" => Shape::Behind,
            "column" => Shape::Column,
            "line" => Shape::Line,
            "rows" => Shape::Rows,
            "triangle" => Shape::Triangle,
            "wall" => Shape::Wall,
            "open_circle" => Shape::OpenCircle,
            "square" => Shape::Square,
            "circle" => Shape::Circle,
            "hexagon" => Shape::Hexagon,
            "grid" => Shape::Grid,
            _ => return None,
        })
    }
    pub fn turns_with_leader(self) -> bool {
        !matches!(self, Shape::Square | Shape::Circle | Shape::Hexagon | Shape::Grid)
    }
}

/// The slot offset (right, forward) in blocks for follower `i` of `n`, before turning.
fn local(shape: Shape, i: usize, n: usize, size: usize, gap: f64) -> (f64, f64) {
    let size = size.max(1);
    let i_f = i as f64;
    match shape {
        Shape::Behind => {
            // Rings behind: 3 in the first, a new ring every few bots.
            let ring = i / 3;
            let k = (i % 3) as f64 - 1.0;
            (k * gap * 0.8, -gap * (1.0 + ring as f64))
        }
        Shape::Column => (0.0, -gap * (1.0 + i_f)),
        Shape::Line => {
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            (side * gap * (1.0 + (i / 2) as f64), 0.0)
        }
        Shape::Rows => {
            let row = i / size;
            let col = (i % size) as f64 - (size as f64 - 1.0) / 2.0;
            (col * gap, -gap * (1.0 + row as f64))
        }
        Shape::Triangle => {
            // Row r holds r+2 bots, the leader at the point.
            let (mut r, mut start) = (0usize, 0usize);
            while start + r + 2 <= i {
                start += r + 2;
                r += 1;
            }
            let in_row = r + 2;
            let col = (i - start) as f64 - (in_row as f64 - 1.0) / 2.0;
            (col * gap, -gap * (1.0 + r as f64))
        }
        Shape::Wall => {
            let row = i / size.max(4);
            let w = size.max(4);
            let col = (i % w) as f64 - (w as f64 - 1.0) / 2.0;
            (col * gap * 0.7, gap * (1.5 - row as f64))
        }
        Shape::OpenCircle => {
            // A ring that leaves the front 90 degrees clear.
            let span = 270f64.to_radians();
            let a = std::f64::consts::FRAC_PI_4 * 3.0 * 0.0 + 135f64.to_radians() + span * (i_f + 0.5) / n.max(1) as f64;
            let r = gap * (1.0 + n as f64 / 8.0);
            (a.cos() * r, a.sin() * r)
        }
        Shape::Circle | Shape::Hexagon => {
            let corners = if shape == Shape::Hexagon { 6.0 } else { n.max(1) as f64 };
            let r = gap * (1.0 + n as f64 / 8.0);
            if shape == Shape::Hexagon {
                // Bots spread along the six sides.
                let per_side = (n as f64 / 6.0).ceil().max(1.0);
                let side = (i_f / per_side).floor();
                let t = (i_f % per_side) / per_side;
                let a0 = side * std::f64::consts::TAU / corners;
                let a1 = (side + 1.0) * std::f64::consts::TAU / corners;
                let (x0, z0) = (a0.cos() * r, a0.sin() * r);
                let (x1, z1) = (a1.cos() * r, a1.sin() * r);
                (x0 + (x1 - x0) * t, z0 + (z1 - z0) * t)
            } else {
                let a = std::f64::consts::TAU * i_f / corners;
                (a.cos() * r, a.sin() * r)
            }
        }
        Shape::Square => {
            let side = ((n as f64).sqrt().ceil() as usize).max(1);
            let (row, col) = (i / side, i % side);
            let half = (side as f64 - 1.0) / 2.0;
            ((col as f64 - half) * gap, (row as f64 - half) * gap)
        }
        Shape::Grid => {
            let side = size.max(2);
            let (row, col) = (i / side, i % side);
            let half = (side as f64 - 1.0) / 2.0;
            ((col as f64 - half) * gap, -(row as f64 + 1.0) * gap)
        }
    }
}

/// Where follower `i` of `n` should stand. `wobble` (loose formations) moves each slot a little,
/// steadily per bot.
pub fn slot(shape: Shape, leader: Vec3, leader_yaw: f32, i: usize, n: usize, size: usize, gap: f64, wobble: Option<(&mut Rng, u64)>) -> Vec3 {
    let (mut right, mut fwd) = local(shape, i, n, size, gap);
    if let Some((rng, t)) = wobble {
        let phase = (t as f64) / 40.0 + rng.unit() * 0.0 + i as f64 * 1.7;
        right += phase.sin() * 0.35;
        fwd += (phase * 0.7).cos() * 0.35;
    }
    let yaw = if shape.turns_with_leader() { leader_yaw as f64 } else { 0.0 };
    let r = yaw.to_radians();
    // Forward is the leader's look direction; right is 90 degrees clockwise of it.
    let f = Vec3::new(-r.sin(), 0.0, r.cos());
    let rt = Vec3::new(-r.cos(), 0.0, -r.sin());
    leader + f * fwd + rt * right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behind_is_behind_and_slots_differ() {
        let l = Vec3::new(0.0, 64.0, 0.0);
        let s0 = slot(Shape::Behind, l, 0.0, 0, 6, 1, 3.5, None);
        assert!(s0.z < 0.0, "{s0:?}");
        let mut all = Vec::new();
        for i in 0..12 {
            for shape in [Shape::Rows, Shape::Circle, Shape::Triangle, Shape::Square, Shape::Hexagon, Shape::Wall] {
                all.push((shape, slot(shape, l, 0.0, i, 12, 4, 3.0, None)));
            }
        }
        for shape in [Shape::Rows, Shape::Circle, Shape::Triangle, Shape::Square, Shape::Hexagon] {
            let pts: Vec<Vec3> = all.iter().filter(|(s, _)| *s == shape).map(|(_, p)| *p).collect();
            for a in 0..pts.len() {
                for b in a + 1..pts.len() {
                    assert!(pts[a].distance(pts[b]) > 0.5, "{shape:?} slots {a} and {b} overlap");
                }
            }
        }
    }

    #[test]
    fn turning_formations_turn() {
        let l = Vec3::ZERO;
        // Leader faces west (yaw 90): behind is east (+X).
        let s = slot(Shape::Column, l, 90.0, 0, 3, 1, 2.0, None);
        assert!(s.x > 1.9, "{s:?}");
        // A world-aligned square does not turn.
        let a = slot(Shape::Square, l, 0.0, 1, 4, 2, 2.0, None);
        let b = slot(Shape::Square, l, 90.0, 1, 4, 2, 2.0, None);
        assert!(a.distance(b) < 1e-9);
    }
}
