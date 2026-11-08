//! Getting around: A* over block positions for a player-sized body (walk, step up, drop down,
//! climb, swim), smoothing, steering a point into key presses, and the building moves (bridge a
//! gap, pillar up, dig soft cover) when walking can't get there.

use crate::math::{yaw_to, Vec3};
use crate::world::{BlockView, Fluid};
use std::collections::{BinaryHeap, HashMap};

pub type Pos = [i32; 3];

pub fn standable(w: &dyn BlockView, p: Pos) -> bool {
    let [x, y, z] = p;
    let feet = w.block(x, y, z);
    let head = w.block(x, y + 1, z);
    if !feet.passable() || !head.passable() || feet.harmful || head.harmful || feet.cobweb {
        return false;
    }
    if feet.fluid == Fluid::Water || feet.climbable {
        return true;
    }
    let below = w.block(x, y - 1, z);
    if below.unloaded || below.fluid == Fluid::Lava || below.harmful {
        return false;
    }
    below.top().is_some() || below.fluid == Fluid::Water
}

#[derive(Copy, Clone, PartialEq)]
struct Node {
    f: f64,
    p: Pos,
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.f.partial_cmp(&self.f).unwrap_or(std::cmp::Ordering::Equal)
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

fn dist(a: Pos, b: Pos) -> f64 {
    let d = [(a[0] - b[0]) as f64, (a[1] - b[1]) as f64, (a[2] - b[2]) as f64];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn neighbours(w: &dyn BlockView, p: Pos, out: &mut Vec<(Pos, f64)>) {
    out.clear();
    let [x, y, z] = p;
    let here = w.block(x, y, z);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
        let diag = dx != 0 && dz != 0;
        if diag {
            // No cutting corners through blocks.
            let a = w.block(x + dx, y, z);
            let b = w.block(x, y, z + dz);
            let a2 = w.block(x + dx, y + 1, z);
            let b2 = w.block(x, y + 1, z + dz);
            if !(a.passable() && b.passable() && a2.passable() && b2.passable()) {
                continue;
            }
        }
        let base = if diag { std::f64::consts::SQRT_2 } else { 1.0 };
        let flat = [x + dx, y, z + dz];
        if standable(w, flat) {
            let water = w.block(flat[0], flat[1], flat[2]).fluid == Fluid::Water;
            out.push((flat, base + if water { 2.0 } else { 0.0 }));
            continue;
        }
        // Step or jump up one (head room above us too).
        let up = [x + dx, y + 1, z + dz];
        if !diag && w.block(x, y + 2, z).passable() && standable(w, up) {
            out.push((up, base + 0.6));
            continue;
        }
        // Drop down, up to three blocks (a planned drop, not a bridge).
        for d in 1..=3 {
            let down = [x + dx, y - d, z + dz];
            if !w.block(x + dx, y - d + 1, z + dz).passable() {
                break;
            }
            if standable(w, down) {
                out.push((down, base + 0.3 * d as f64));
                break;
            }
        }
    }
    // Ladders and water: straight up and down.
    if here.climbable || here.fluid == Fluid::Water {
        let up = [x, y + 1, z];
        if standable(w, up) || w.block(x, y + 1, z).climbable {
            out.push((up, 1.2));
        }
    }
    let down = [x, y - 1, z];
    let below = w.block(x, y - 1, z);
    if (below.climbable || below.fluid == Fluid::Water) && standable(w, down) {
        out.push((down, 1.0));
    }
}

/// A route from `start` to within `reach` blocks of `goal` (feet positions), at most `budget`
/// nodes searched. Long detours are found: the heuristic is weighted only slightly.
pub fn find_path(w: &dyn BlockView, start: Pos, goal: Pos, reach: f64, budget: usize) -> Option<Vec<Pos>> {
    if dist(start, goal) > 160.0 {
        return None;
    }
    let mut open = BinaryHeap::new();
    let mut g: HashMap<Pos, f64> = HashMap::new();
    let mut from: HashMap<Pos, Pos> = HashMap::new();
    g.insert(start, 0.0);
    open.push(Node { f: dist(start, goal), p: start });
    let mut nbrs = Vec::new();
    let mut best = (dist(start, goal), start);
    let mut searched = 0;
    while let Some(Node { p, .. }) = open.pop() {
        let d = dist(p, goal);
        if d <= reach {
            return Some(rebuild(&from, p));
        }
        if d < best.0 {
            best = (d, p);
        }
        searched += 1;
        if searched > budget {
            break;
        }
        let gp = g[&p];
        neighbours(w, p, &mut nbrs);
        for &(n, c) in &nbrs {
            let ng = gp + c;
            if g.get(&n).is_none_or(|&old| ng < old - 1e-9) {
                g.insert(n, ng);
                from.insert(n, p);
                open.push(Node { f: ng + dist(n, goal) * 1.1, p: n });
            }
        }
    }
    // Out of budget or no way: the route to the closest point reached, if it gets closer.
    (best.1 != start && best.0 + 1.0 < dist(start, goal)).then(|| rebuild(&from, best.1))
}

fn rebuild(from: &HashMap<Pos, Pos>, mut p: Pos) -> Vec<Pos> {
    let mut v = vec![p];
    while let Some(&q) = from.get(&p) {
        v.push(q);
        p = q;
    }
    v.reverse();
    v
}

/// Drops the points a straight walk would pass anyway (same height, clear line of floor).
pub fn smooth(w: &dyn BlockView, path: &[Pos]) -> Vec<Pos> {
    if path.len() <= 2 {
        return path.to_vec();
    }
    let mut out = vec![path[0]];
    let mut anchor = 0;
    for i in 2..path.len() {
        if !straight_walkable(w, path[anchor], path[i]) {
            out.push(path[i - 1]);
            anchor = i - 1;
        }
    }
    out.push(*path.last().unwrap());
    out
}

/// Whether a straight walk between two feet positions at the same height stays on floor.
pub fn straight_walkable(w: &dyn BlockView, a: Pos, b: Pos) -> bool {
    if a[1] != b[1] {
        return false;
    }
    let (ax, az) = (a[0] as f64 + 0.5, a[2] as f64 + 0.5);
    let (bx, bz) = (b[0] as f64 + 0.5, b[2] as f64 + 0.5);
    let len = ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt();
    let steps = (len * 3.0).ceil() as i32;
    // Samples a third of a block apart mostly land in the same cells: each cell is checked once.
    let mut seen: [Pos; 8] = [[i32::MIN; 3]; 8];
    let mut at = 0;
    for s in 0..=steps {
        let t = s as f64 / steps.max(1) as f64;
        for (ox, oz) in [(-0.3, -0.3), (0.3, 0.3), (-0.3, 0.3), (0.3, -0.3)] {
            let p = [(ax + (bx - ax) * t + ox).floor() as i32, a[1], (az + (bz - az) * t + oz).floor() as i32];
            if seen.contains(&p) {
                continue;
            }
            if !standable(w, p) {
                return false;
            }
            seen[at] = p;
            at = (at + 1) % seen.len();
        }
    }
    true
}

/// The key presses that move a body looking along `look_yaw` toward `dir` (world, horizontal):
/// the closest of the eight key combinations, as a keyboard gives.
pub fn keys_toward(look_yaw: f32, dir: Vec3) -> (f64, f64) {
    if dir.horizontal_length() < 1e-6 {
        return (0.0, 0.0);
    }
    let want = yaw_to(Vec3::ZERO, dir);
    let rel = crate::math::wrap_degrees(want - look_yaw);
    // 0 = forward, +90 = to the bot's right... vanilla: strafe +1 is left (toward +yaw-90).
    let sector = ((rel + 22.5).rem_euclid(360.0) / 45.0).floor() as i32 % 8;
    match sector {
        0 => (1.0, 0.0),
        1 => (1.0, -1.0),
        2 => (0.0, -1.0),
        3 => (-1.0, -1.0),
        4 => (-1.0, 0.0),
        5 => (-1.0, 1.0),
        6 => (0.0, 1.0),
        _ => (1.0, 1.0),
    }
}

/// What building does when the route is blocked.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildStep {
    /// Put a block here (against `against`, its face toward us).
    Place { pos: Pos, against: Pos },
    /// Jump and put a block under the feet.
    Pillar { pos: Pos },
    /// Dig this soft block.
    Dig { pos: Pos },
}

/// The block a bot would bridge with to step from `feet` toward `toward` (horizontal unit step).
pub fn bridge_step(w: &dyn BlockView, feet: Pos, dx: i32, dz: i32) -> Option<BuildStep> {
    let next = [feet[0] + dx, feet[1], feet[2] + dz];
    let below = [next[0], next[1] - 1, next[2]];
    let b = w.block(below[0], below[1], below[2]);
    if b.top().is_some() || !b.passable() || b.fluid == Fluid::Lava {
        return None;
    }
    if !(w.block(next[0], next[1], next[2]).passable() && w.block(next[0], next[1] + 1, next[2]).passable()) {
        return None;
    }
    // Placed against the block we stand on.
    Some(BuildStep::Place { pos: below, against: [feet[0], feet[1] - 1, feet[2]] })
}

/// Soft cover directly in the way (feet or head height), if any.
pub fn dig_step(w: &dyn BlockView, feet: Pos, dx: i32, dz: i32, extra: &[String]) -> Option<BuildStep> {
    for dy in [1, 0] {
        let p = [feet[0] + dx, feet[1] + dy, feet[2] + dz];
        let b = w.block(p[0], p[1], p[2]);
        if !b.passable() && !b.protected && (b.soft || extra.iter().any(|e| e.as_str() == &*b.name)) {
            return Some(BuildStep::Dig { pos: p });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::MapWorld;

    #[test]
    fn walks_round_a_wall() {
        let mut w = MapWorld::flat(63);
        for x in -5..=5 {
            for y in 64..=66 {
                w.set(x, y, 3, "minecraft:stone");
            }
        }
        let p = find_path(&w, [0, 64, 0], [0, 64, 6], 0.5, 4000).expect("a way round");
        assert_eq!(*p.last().unwrap(), [0, 64, 6]);
        assert!(p.iter().all(|q| q[2] != 3 || q[0].abs() > 5));
        let s = smooth(&w, &p);
        assert!(s.len() < p.len());
    }

    #[test]
    fn steps_up_and_drops_down() {
        let mut w = MapWorld::flat(63);
        w.set(2, 64, 0, "minecraft:stone");
        let p = find_path(&w, [0, 64, 0], [2, 65, 0], 0.5, 500).unwrap();
        assert_eq!(*p.last().unwrap(), [2, 65, 0]);
        let back = find_path(&w, [2, 65, 0], [4, 64, 0], 0.5, 500).unwrap();
        assert_eq!(*back.last().unwrap(), [4, 64, 0]);
    }

    #[test]
    fn keys_for_directions() {
        // Looking south (+Z), going south: forward.
        assert_eq!(keys_toward(0.0, Vec3::new(0.0, 0.0, 1.0)), (1.0, 0.0));
        // Going north: back.
        assert_eq!(keys_toward(0.0, Vec3::new(0.0, 0.0, -1.0)), (-1.0, 0.0));
        // Going east (+X) while looking south: east is to the left of south in Minecraft
        // (yaw -90 is east), so strafe left (+1)? The look yaw is 0, east is yaw -90: rel -90.
        let (f, s) = keys_toward(0.0, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(f, 0.0);
        // Check with the physics: strafe s moves toward +X.
        let r = 0.0f64.to_radians();
        let vx = s * r.cos() - f * r.sin();
        assert!(vx > 0.0, "{s}");
    }

    #[test]
    fn bridges_gaps_and_digs_soft_cover() {
        let mut w = MapWorld::default();
        w.set(0, 63, 0, "minecraft:stone");
        assert_eq!(
            bridge_step(&w, [0, 64, 0], 1, 0),
            Some(BuildStep::Place { pos: [1, 63, 0], against: [0, 63, 0] })
        );
        let mut w = MapWorld::flat(63);
        w.set(1, 64, 0, "minecraft:dirt");
        assert_eq!(dig_step(&w, [0, 64, 0], 1, 0, &[]), Some(BuildStep::Dig { pos: [1, 64, 0] }));
        w.set(1, 64, 0, "minecraft:stone");
        assert_eq!(dig_step(&w, [0, 64, 0], 1, 0, &[]), None);
        w.set(1, 64, 0, "minecraft:oak_planks");
        assert_eq!(dig_step(&w, [0, 64, 0], 1, 0, &["minecraft:oak_planks".into()]), None);
    }
}
