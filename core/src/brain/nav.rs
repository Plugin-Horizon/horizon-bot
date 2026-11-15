//! Getting around: A* over block positions for a player-sized body (walk, step up, drop down,
//! climb, swim), smoothing, steering a point into key presses, and the building moves (bridge a
//! gap, pillar up, dig soft cover) when walking can't get there.

use crate::math::{yaw_to, Vec3};
use std::cell::RefCell;
use crate::world::{BlockView, Fluid};
use std::collections::{BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

/// A fast non-cryptographic hasher (FxHash-style) for the search's position keys.
#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);
impl Hasher for FxHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_i32(&mut self, i: i32) {
        self.write_u64(i as u32 as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}
type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

pub type Pos = [i32; 3];

pub fn standable(w: &dyn BlockView, p: Pos) -> bool {
    let [x, y, z] = p;
    let feet = w.flags(x, y, z);
    let head = w.flags(x, y + 1, z);
    if !feet.passable || !head.passable || feet.harmful || head.harmful || feet.cobweb {
        return false;
    }
    if feet.fluid == Fluid::Water || feet.climbable {
        return true;
    }
    let below = w.flags(x, y - 1, z);
    if below.unloaded || below.fluid == Fluid::Lava || below.harmful {
        return false;
    }
    below.has_top || below.fluid == Fluid::Water
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

/// Per-search memo of `standable`: each cell is asked about by up to eight neighbours.
type Memo = FxMap<Pos, bool>;

fn stand(w: &dyn BlockView, memo: &mut Memo, p: Pos) -> bool {
    *memo.entry(p).or_insert_with(|| standable(w, p))
}

/// Sprint-jump simulations one search may run (each is a few dozen physics ticks): gaps are rare, a search
/// that meets many of them should not turn into a physics benchmark.
const GAP_SIMS_PER_SEARCH: u32 = 24;
/// The longest gap (blocks of missing floor) a planned sprint jump crosses.
const MAX_GAP: i32 = 3;

fn neighbours(w: &dyn BlockView, memo: &mut Memo, sims: &mut u32, p: Pos, out: &mut Vec<(Pos, f64)>) {
    out.clear();
    let [x, y, z] = p;
    let here = w.flags(x, y, z);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
        let diag = dx != 0 && dz != 0;
        let before = out.len();
        if diag {
            // No cutting corners through blocks.
            let a = w.flags(x + dx, y, z);
            let b = w.flags(x, y, z + dz);
            let a2 = w.flags(x + dx, y + 1, z);
            let b2 = w.flags(x, y + 1, z + dz);
            if !(a.passable && b.passable && a2.passable && b2.passable) {
                continue;
            }
        }
        let base = if diag { std::f64::consts::SQRT_2 } else { 1.0 };
        let flat = [x + dx, y, z + dz];
        if stand(w, memo, flat) {
            let water = w.flags(flat[0], flat[1], flat[2]).fluid == Fluid::Water;
            out.push((flat, base + if water { 2.0 } else { 0.0 }));
            continue;
        }
        // Step or jump up one (head room above us too).
        let up = [x + dx, y + 1, z + dz];
        if !diag && w.flags(x, y + 2, z).passable && stand(w, memo, up) {
            out.push((up, base + 0.6));
            continue;
        }
        // Drop down, up to three blocks (a planned drop, not a bridge).
        for d in 1..=3 {
            let down = [x + dx, y - d, z + dz];
            if !w.flags(x + dx, y - d + 1, z + dz).passable {
                break;
            }
            if stand(w, memo, down) {
                out.push((down, base + 0.3 * d as f64));
                break;
            }
        }
        // Nothing to walk, step or drop to that way: a gap a sprint jump may cross.
        if !diag && out.len() == before && *sims > 0 {
            if let Some(land) = gap_jump(w, memo, sims, p, dx, dz) {
                out.push((land.0, land.1));
            }
        }
    }
    // Ladders and water: straight up and down.
    if here.climbable || here.fluid == Fluid::Water {
        let up = [x, y + 1, z];
        if stand(w, memo, up) || w.flags(x, y + 1, z).climbable {
            out.push((up, 1.2));
        }
    }
    let down = [x, y - 1, z];
    let below = w.flags(x, y - 1, z);
    if (below.climbable || below.fluid == Fluid::Water) && stand(w, memo, down) {
        out.push((down, 1.0));
    }
}

/// Where a sprint jump from `p` along (dx, dz) lands across 1..=MAX_GAP blocks of missing floor, and what the
/// edge costs. The landing is verified with the real body physics (a run-up when there are cells to run on
/// behind `p`), so the search never plans a jump the bot's own physics would not make.
fn gap_jump(w: &dyn BlockView, memo: &mut Memo, sims: &mut u32, p: Pos, dx: i32, dz: i32) -> Option<(Pos, f64)> {
    let [x, y, z] = p;
    // Head room over the cells the jump passes (the jump arc peaks about 1.25 high).
    let mut clear = 0;
    for k in 1..=MAX_GAP + 1 {
        let (cx, cz) = (x + dx * k, z + dz * k);
        if !(w.flags(cx, y, cz).passable && w.flags(cx, y + 1, cz).passable && w.flags(cx, y + 2, cz).passable) {
            break;
        }
        clear = k;
    }
    for n in 1..=MAX_GAP {
        if clear < n + 1 {
            break;
        }
        let land = [x + dx * (n + 1), y, z + dz * (n + 1)];
        if !stand(w, memo, land) {
            continue;
        }
        // A run-up: three standable cells behind the take-off.
        let run = (1..=3).all(|k| stand(w, memo, [x - dx * k, y, z - dz * k]));
        if *sims == 0 {
            return None;
        }
        *sims -= 1;
        if jump_lands(w, p, dx, dz, n, run) {
            return Some((land, (n + 1) as f64 + 3.0));
        }
    }
    None
}

/// Plays a sprint jump from the middle of `p` along (dx, dz) and says whether it comes down on the far side of
/// an `n`-block gap at the same height (the body's far edge past the gap's end).
fn jump_lands(w: &dyn BlockView, p: Pos, dx: i32, dz: i32, n: i32, run: bool) -> bool {
    use crate::physics::{Body, Input};
    let dir = Vec3::new(dx as f64, 0.0, dz as f64);
    let start = Vec3::new(p[0] as f64 + 0.5, p[1] as f64, p[2] as f64 + 0.5);
    let mut b = Body::new(start);
    b.yaw = yaw_to(Vec3::ZERO, dir);
    b.on_ground = true;
    b.sprinting = true;
    if run {
        // Sprinting speed on the ground, about 0.28 blocks a tick.
        b.vel = dir * 0.28;
    }
    let input = Input { forward: 1.0, jump: true, sprint: true, ..Default::default() };
    let mut airborne = false;
    for _ in 0..30 {
        b.tick(&input, w);
        if !b.on_ground {
            airborne = true;
        } else if airborne {
            let moved = b.pos - start;
            let along = moved.x * dir.x + moved.z * dir.z;
            // On the floor again at the take-off height, with the body's front edge over the landing cell.
            return (b.pos.y - p[1] as f64).abs() < 0.05 && along >= n as f64 + 0.2;
        }
        if b.in_water || b.in_lava || b.pos.y < p[1] as f64 - 2.0 {
            return false;
        }
    }
    false
}

/// Whether the bot standing in `feet` should jump now for the planned step to `next`: a straight run of two
/// or more cells along one axis at the same height, with no floor in the very next cell (a gap edge).
pub fn gap_jump_due(w: &dyn BlockView, feet: Pos, next: Pos) -> bool {
    let (dx, dz) = (next[0] - feet[0], next[2] - feet[2]);
    if next[1] != feet[1] || (dx != 0 && dz != 0) || dx.abs().max(dz.abs()) < 2 {
        return false;
    }
    !standable(w, [feet[0] + dx.signum(), feet[1], feet[2] + dz.signum()])
}

/// Whether the bot at `here` has passed waypoint `a` on its way to `b` (it is beside it going on, or `b` is
/// nearer) and a straight walk to `b` is clear: `a` can be dropped instead of walked back to. Only for short
/// same-height legs; steps, drops and gaps keep their exact waypoints.
pub fn waypoint_passed(w: &dyn BlockView, here: Vec3, a: Pos, b: Pos) -> bool {
    if a[1] != b[1] || here.block()[1] != a[1] || (b[0] - a[0]).abs().max((b[2] - a[2]).abs()) > 4 {
        return false;
    }
    let (ca, cb) = ((a[0] as f64 + 0.5, a[2] as f64 + 0.5), (b[0] as f64 + 0.5, b[2] as f64 + 0.5));
    let r = (here.x - ca.0, here.z - ca.1);
    let dot = r.0 * (cb.0 - ca.0) + r.1 * (cb.1 - ca.1);
    let da = (r.0 * r.0 + r.1 * r.1).sqrt();
    let db = ((here.x - cb.0).powi(2) + (here.z - cb.1).powi(2)).sqrt();
    let passed = (dot > 0.0 && da < 2.0) || (db < da && da < 3.0);
    passed && straight_walkable(w, here.block(), b)
}

/// A route from `start` to within `reach` blocks of `goal` (feet positions), at most `budget`
/// nodes searched. Long detours are found: the heuristic is weighted only slightly.
pub fn find_path(w: &dyn BlockView, start: Pos, goal: Pos, reach: f64, budget: usize) -> Option<Vec<Pos>> {
    find_path_counted(w, start, goal, reach, budget).0
}

/// `find_path`, also returning how many nodes were expanded (what the shared search budget pays for).
pub fn find_path_counted(w: &dyn BlockView, start: Pos, goal: Pos, reach: f64, budget: usize) -> (Option<Vec<Pos>>, usize) {
    if dist(start, goal) > 160.0 {
        return (None, 0);
    }
    // The search's maps and heap are kept between calls (a search asks for hundreds of entries each time).
    let mut sc = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
    sc.open.clear();
    sc.g.clear();
    sc.from.clear();
    sc.memo.clear();
    let out = search(w, &mut sc, start, goal, reach, budget);
    // An unusually big search does not keep its memory.
    if sc.g.capacity() > 1 << 16 {
        sc = Scratch::default();
    }
    SCRATCH.with(|s| *s.borrow_mut() = sc);
    out
}

#[derive(Default)]
struct Scratch {
    open: BinaryHeap<Node>,
    g: FxMap<Pos, f64>,
    from: FxMap<Pos, Pos>,
    memo: Memo,
    nbrs: Vec<(Pos, f64)>,
}

thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::new(Scratch::default());
}

fn search(w: &dyn BlockView, sc: &mut Scratch, start: Pos, goal: Pos, reach: f64, budget: usize) -> (Option<Vec<Pos>>, usize) {
    let Scratch { open, g, from, memo, nbrs } = sc;
    g.insert(start, 0.0);
    open.push(Node { f: dist(start, goal), p: start });
    let mut sims = GAP_SIMS_PER_SEARCH;
    let mut best = (dist(start, goal), start);
    let mut searched = 0;
    while let Some(Node { p, f }) = open.pop() {
        let gp = g[&p];
        // An entry superseded by a cheaper route to the same cell: that one was expanded already.
        if p != start && f > gp + dist(p, goal) * 1.1 + 1e-6 {
            continue;
        }
        let d = dist(p, goal);
        if d <= reach {
            return (Some(rebuild(from, p)), searched);
        }
        if d < best.0 {
            best = (d, p);
        }
        searched += 1;
        if searched > budget {
            break;
        }
        neighbours(w, memo, &mut sims, p, nbrs);
        for &(n, c) in nbrs.iter() {
            let ng = gp + c;
            if g.get(&n).is_none_or(|&old| ng < old - 1e-9) {
                g.insert(n, ng);
                from.insert(n, p);
                open.push(Node { f: ng + dist(n, goal) * 1.1, p: n });
            }
        }
    }
    // Out of budget or no way: the route to the closest point reached, if it gets closer.
    let route = (best.1 != start && best.0 + 1.0 < dist(start, goal)).then(|| rebuild(from, best.1));
    (route, searched)
}

fn rebuild(from: &FxMap<Pos, Pos>, mut p: Pos) -> Vec<Pos> {
    let mut v = vec![p];
    while let Some(&q) = from.get(&p) {
        v.push(q);
        p = q;
    }
    v.reverse();
    v
}

const SMOOTH_LOOKAHEAD: usize = 16;

/// Drops the points a straight walk would pass anyway (same height, clear line of floor).
pub fn smooth(w: &dyn BlockView, path: &[Pos]) -> Vec<Pos> {
    if path.len() <= 2 {
        return path.to_vec();
    }
    // From each anchor the farthest point (within the look-ahead) a straight walk reaches: the far end is tried
    // first and, if it fails, a bisection finds the last good one. (Trying every point from the anchor up, each
    // a straight-walk check of growing length, cost several times the search itself on an open field.) The next
    // point after the anchor is always taken, as a walk of one step needs no check.
    let last = path.len() - 1;
    let mut out = vec![path[0]];
    let mut anchor = 0;
    while anchor < last {
        let far = (anchor + SMOOTH_LOOKAHEAD).min(last);
        let mut best = anchor + 1;
        if far > best {
            if straight_walkable(w, path[anchor], path[far]) {
                best = far;
            } else {
                let (mut lo, mut hi) = (best, far);
                while hi - lo > 1 {
                    let mid = (lo + hi) / 2;
                    if straight_walkable(w, path[anchor], path[mid]) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                best = lo;
            }
        }
        out.push(path[best]);
        anchor = best;
    }
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

    /// A run along z = 0: floor from x = -4 to 1, a gap of `gap` cells, then floor to the end.
    fn gap_world(gap: i32) -> (MapWorld, i32) {
        let mut w = MapWorld::default();
        for x in -4..=1 {
            w.set(x, 63, 0, "minecraft:stone");
        }
        let far = 2 + gap;
        for x in far..=far + 3 {
            w.set(x, 63, 0, "minecraft:stone");
        }
        (w, far + 3)
    }

    #[test]
    fn crosses_a_gap_with_a_sprint_jump() {
        let (w, end) = gap_world(2);
        let p = find_path(&w, [0, 64, 0], [end, 64, 0], 0.5, 2000).expect("a route");
        assert_eq!(*p.last().unwrap(), [end, 64, 0]);
        assert!(p.windows(2).any(|s| (s[1][0] - s[0][0]).abs() >= 3), "no jump edge in {p:?}");
        // The bot jumps at the edge cell, not before it.
        assert!(gap_jump_due(&w, [1, 64, 0], [4, 64, 0]));
        assert!(!gap_jump_due(&w, [0, 64, 0], [4, 64, 0]));
        assert!(!gap_jump_due(&w, [0, 64, 0], [1, 64, 0]));
    }

    #[test]
    fn a_gap_too_wide_is_not_planned() {
        let (w, end) = gap_world(7);
        let p = find_path(&w, [0, 64, 0], [end, 64, 0], 0.5, 2000);
        assert!(p.is_none_or(|p| *p.last().unwrap() != [end, 64, 0]));
    }

    #[test]
    fn a_passed_waypoint_is_dropped() {
        let w = MapWorld::flat(63);
        let (a, b) = ([1, 64, 0], [2, 64, 0]);
        // Beside the waypoint and going on: passed.
        assert!(waypoint_passed(&w, Vec3::new(1.8, 64.0, 0.5), a, b));
        // Short of it, the next one farther: not yet.
        assert!(!waypoint_passed(&w, Vec3::new(0.6, 64.0, 0.5), a, b));
        // A step up keeps its exact waypoint.
        assert!(!waypoint_passed(&w, Vec3::new(1.8, 64.0, 0.5), a, [2, 65, 0]));
        // A wall in the way of the straight walk: not passed.
        let mut walled = MapWorld::flat(63);
        walled.set(2, 64, 0, "minecraft:stone");
        walled.set(2, 65, 0, "minecraft:stone");
        assert!(!waypoint_passed(&walled, Vec3::new(1.8, 64.0, 0.5), a, b));
    }

    #[test]
    fn searches_reuse_their_scratch() {
        let w = MapWorld::flat(63);
        let first = find_path(&w, [0, 64, 0], [6, 64, 3], 0.5, 2000).unwrap();
        let second = find_path(&w, [0, 64, 0], [6, 64, 3], 0.5, 2000).unwrap();
        assert_eq!(first, second);
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
