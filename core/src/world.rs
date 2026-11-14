//! What a bot needs to know about blocks: a coarse shape for collision and pathfinding, fluids,
//! climbables, cobwebs and friction, read from a block state string
//! (`minecraft:oak_slab[type=bottom,waterlogged=false]`).

use crate::math::Aabb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fluid {
    None,
    Water,
    Lava,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Empty,
    Full,
    /// A box from the bottom up to this height (slabs, carpets, snow layers, soul sand).
    Bottom(f64),
    /// A box from this height to the top (top slabs).
    Top(f64),
    /// Fences and walls: 1.5 high, narrow, but a player can't step over them.
    Post,
    /// Several boxes, in block-local coordinates (min x, y, z, max x, y, z): stairs.
    /// Shared, so cloning a stair block copies no boxes.
    Boxes(std::sync::Arc<[[f64; 6]]>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// Shared: a block is cloned on every look (line of sight, paths, physics), so it must not copy a string.
    pub name: std::sync::Arc<str>,
    pub shape: Shape,
    pub fluid: Fluid,
    pub climbable: bool,
    pub cobweb: bool,
    pub powder_snow: bool,
    /// Block slipperiness: 0.6 by default, ice 0.98, slime 0.8.
    pub friction: f64,
    /// Soul sand, honey: walking speed factor.
    pub speed_factor: f64,
    /// Soft natural cover a bot may dig through (dirt, sand, leaves...).
    pub soft: bool,
    /// Hurts to stand in (fire, magma top, cactus, sweet berries, lava).
    pub harmful: bool,
    /// A door, trapdoor or gate that is shut.
    pub closed_door: bool,
    /// Holds items or was clearly put there by a player (never broken by a bot).
    pub protected: bool,
    /// Not loaded (a bot never walks into the unknown).
    pub unloaded: bool,
}

impl Block {
    pub fn air() -> Self {
        Block {
            name: "minecraft:air".into(),
            shape: Shape::Empty,
            fluid: Fluid::None,
            climbable: false,
            cobweb: false,
            powder_snow: false,
            friction: 0.6,
            speed_factor: 1.0,
            soft: false,
            harmful: false,
            closed_door: false,
            protected: false,
            unloaded: false,
        }
    }
    pub fn unloaded() -> Self {
        Block {
            name: "unloaded".into(),
            shape: Shape::Full,
            unloaded: true,
            ..Block::air()
        }
    }
    pub fn solid(name: &str) -> Self {
        Block {
            name: name.into(),
            shape: Shape::Full,
            ..Block::air()
        }
    }

    /// The collision boxes at a block position.
    pub fn boxes(&self, x: i32, y: i32, z: i32, out: &mut Vec<Aabb>) {
        match self.shape {
            Shape::Empty => {}
            Shape::Full => out.push(Aabb::block(x, y, z, 1.0)),
            Shape::Bottom(h) => out.push(Aabb::block(x, y, z, h)),
            Shape::Top(h) => {
                let mut b = Aabb::block(x, y, z, 1.0);
                b.min.y = y as f64 + h;
                out.push(b);
            }
            Shape::Post => {
                let mut b = Aabb::block(x, y, z, 1.5);
                b.min.x += 0.375;
                b.max.x -= 0.375;
                b.min.z += 0.375;
                b.max.z -= 0.375;
                out.push(b);
            }
            Shape::Boxes(ref boxes) => {
                let o = crate::math::Vec3::new(x as f64, y as f64, z as f64);
                for b in boxes.iter() {
                    out.push(Aabb::new(o + crate::math::Vec3::new(b[0], b[1], b[2]), o + crate::math::Vec3::new(b[3], b[4], b[5])));
                }
            }
        }
    }

    /// Height of the surface a player stands on, above the block's own y (None: can't stand).
    pub fn top(&self) -> Option<f64> {
        match self.shape {
            Shape::Empty => None,
            Shape::Full | Shape::Top(_) => Some(1.0),
            Shape::Bottom(h) => (h > 0.0).then_some(h),
            Shape::Post => Some(1.5),
            Shape::Boxes(ref b) => b.iter().map(|x| x[4]).fold(None, |m: Option<f64>, h| Some(m.map_or(h, |m| m.max(h)))),
        }
    }

    /// Whether a player's body can be in this block.
    pub fn passable(&self) -> bool {
        !self.unloaded
            && match self.shape {
                Shape::Empty => true,
                Shape::Bottom(h) => h <= 0.1,
                _ => false,
            }
    }
}

fn prop<'a>(state: &'a str, key: &str) -> Option<&'a str> {
    let props = state.split_once('[')?.1.strip_suffix(']')?;
    props
        .split(',')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

fn has_any(name: &str, parts: &[&str]) -> bool {
    parts.iter().any(|p| name.contains(p))
}

/// Reads a block state string. Unknown blocks are full cubes (the safe guess for walking).
pub fn classify(state: &str) -> Block {
    let name = state.split('[').next().unwrap_or(state);
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    let mut b = Block {
        name: name.into(),
        ..Block::air()
    };
    if prop(state, "waterlogged") == Some("true") {
        b.fluid = Fluid::Water;
    }
    match short {
        "air" | "cave_air" | "void_air" | "light" | "structure_void" => return b,
        "water" | "bubble_column" => {
            b.fluid = Fluid::Water;
            return b;
        }
        "lava" => {
            b.fluid = Fluid::Lava;
            b.harmful = true;
            return b;
        }
        "cobweb" => {
            b.cobweb = true;
            b.soft = true;
            return b;
        }
        "powder_snow" => {
            b.powder_snow = true;
            return b;
        }
        "ladder" | "vine" | "scaffolding" | "twisting_vines" | "twisting_vines_plant" | "weeping_vines"
        | "weeping_vines_plant" | "cave_vines" | "cave_vines_plant" => {
            b.climbable = true;
            b.soft = short.contains("vine");
            if short == "scaffolding" {
                b.shape = Shape::Bottom(0.0);
            }
            return b;
        }
        "fire" | "soul_fire" => {
            b.harmful = true;
            return b;
        }
        "sweet_berry_bush" => {
            b.harmful = true;
            b.soft = true;
            return b;
        }
        "snow" => {
            let layers: f64 = prop(state, "layers").and_then(|l| l.parse().ok()).unwrap_or(1.0);
            b.shape = Shape::Bottom((layers - 1.0) * 0.125);
            b.soft = true;
            return b;
        }
        "soul_sand" | "mud" => {
            b.shape = Shape::Bottom(0.875);
            b.speed_factor = if short == "soul_sand" { 0.4 } else { 1.0 };
            b.soft = true;
            return b;
        }
        "honey_block" => {
            b.shape = Shape::Bottom(0.9375);
            b.speed_factor = 0.4;
            return b;
        }
        "slime_block" => {
            b.shape = Shape::Full;
            b.friction = 0.8;
            return b;
        }
        "ice" | "packed_ice" | "frosted_ice" => {
            b.shape = Shape::Full;
            b.friction = 0.98;
            return b;
        }
        "blue_ice" => {
            b.shape = Shape::Full;
            b.friction = 0.989;
            return b;
        }
        "magma_block" => {
            b.shape = Shape::Full;
            b.harmful = true;
            return b;
        }
        "cactus" => {
            b.shape = Shape::Full;
            b.harmful = true;
            return b;
        }
        "farmland" | "dirt_path" => {
            b.shape = Shape::Bottom(0.9375);
            b.soft = true;
            return b;
        }
        _ => {}
    }
    // Plants, flowers, grass, signs, buttons, rails, torches, pressure plates: no collision.
    if has_any(
        short,
        &[
            "sapling", "flower", "tulip", "orchid", "allium", "bluet", "daisy", "poppy", "dandelion",
            "cornflower", "lily_of_the_valley", "wither_rose", "torch", "button", "rail", "pressure_plate",
            "redstone_wire", "tripwire", "lever", "_sign", "banner", "fern", "dead_bush", "seagrass",
            "kelp", "coral_fan", "mushroom", "fungus", "roots", "sprouts", "wheat", "carrots",
            "potatoes", "beetroots", "nether_wart", "sugar_cane", "bamboo_sapling", "pitcher_crop",
            "torchflower", "pink_petals", "leaf_litter", "wildflowers", "bush", "glow_lichen",
            "sculk_vein", "moss_carpet_", "hanging_roots", "spore_blossom", "frogspawn", "firefly",
            "cactus_flower", "dry_",
        ],
    ) || matches!(short, "short_grass" | "tall_grass" | "grass" | "large_fern" | "lily_pad" | "pale_hanging_moss")
    {
        b.soft = !has_any(short, &["sign", "banner", "rail", "button", "lever", "torch", "pressure_plate", "redstone"]);
        b.protected = !b.soft;
        return b;
    }
    if short.ends_with("_carpet") || short == "moss_carpet" {
        b.shape = Shape::Bottom(0.0625);
        b.protected = true;
        return b;
    }
    if short.ends_with("_slab") {
        b.shape = match prop(state, "type") {
            Some("top") => Shape::Top(0.5),
            Some("double") => Shape::Full,
            _ => Shape::Bottom(0.5),
        };
        b.protected = true;
        return b;
    }
    if short.ends_with("_fence") || short.ends_with("_wall") || short.ends_with("_fence_gate") {
        let open = prop(state, "open") == Some("true");
        b.shape = if short.ends_with("_fence_gate") && open { Shape::Empty } else { Shape::Post };
        b.closed_door = short.ends_with("_fence_gate") && !open;
        b.protected = true;
        return b;
    }
    if short.ends_with("_door") {
        let open = prop(state, "open") == Some("true");
        // An open door is thin: treat it as passable for planning.
        b.shape = if open { Shape::Empty } else { Shape::Full };
        b.closed_door = !open;
        b.protected = true;
        return b;
    }
    if short.ends_with("_trapdoor") {
        let open = prop(state, "open") == Some("true");
        b.shape = if open {
            Shape::Empty
        } else if prop(state, "half") == Some("top") {
            Shape::Top(0.8125)
        } else {
            Shape::Bottom(0.1875)
        };
        b.protected = true;
        return b;
    }
    if short.ends_with("_stairs") {
        // A half slab and the step's tall half on its facing side (straight stairs; corners as
        // straight ones).
        let top = prop(state, "half") == Some("top");
        let (slab, step) = if top { ([0.0, 0.5], [0.0, 0.5]) } else { ([0.0, 0.5], [0.5, 1.0]) };
        let (slab_y, step_y) = if top { ([0.5, 1.0], step) } else { (slab, step) };
        let (x0, x1, z0, z1) = match prop(state, "facing") {
            Some("south") => (0.0, 1.0, 0.5, 1.0),
            Some("west") => (0.0, 0.5, 0.0, 1.0),
            Some("east") => (0.5, 1.0, 0.0, 1.0),
            _ => (0.0, 1.0, 0.0, 0.5),
        };
        b.shape = Shape::Boxes(std::sync::Arc::from(vec![
            [0.0, slab_y[0], 0.0, 1.0, slab_y[1], 1.0],
            [x0, step_y[0], z0, x1, step_y[1], z1],
        ]));
        b.protected = true;
        return b;
    }
    if short.ends_with("_leaves") {
        b.shape = Shape::Full;
        b.soft = true;
        return b;
    }
    if has_any(short, &["chest", "barrel", "shulker_box", "furnace", "hopper", "dispenser", "dropper", "smoker", "crafter", "brewing_stand", "lectern", "decorated_pot", "chiseled_bookshelf", "beacon", "spawner", "vault", "trial_spawner"])
    {
        b.shape = if short.contains("chest") { Shape::Bottom(0.875) } else { Shape::Full };
        b.protected = true;
        return b;
    }
    b.shape = Shape::Full;
    b.soft = matches!(
        short,
        "dirt" | "coarse_dirt" | "rooted_dirt" | "grass_block" | "podzol" | "mycelium" | "sand"
            | "red_sand" | "gravel" | "clay" | "soul_soil" | "snow_block" | "moss_block" | "pale_moss_block"
            | "muddy_mangrove_roots" | "suspicious_sand" | "suspicious_gravel"
    );
    b.protected = !b.soft
        && (has_any(short, &["planks", "brick", "glass", "wool", "concrete", "terracotta", "polished", "smooth", "cut_", "chiseled", "_block", "lantern", "bed", "table", "anvil", "bell"])
            || short.starts_with("stripped_"));
    b
}

/// A Paper world name (`world`, `world_nether`, `world_the_end`) as a dimension id; ids pass.
pub fn dimension_id(world: &str) -> String {
    if world.contains(':') {
        return world.to_owned();
    }
    if world.ends_with("_nether") {
        "minecraft:the_nether".into()
    } else if world.ends_with("_the_end") {
        "minecraft:the_end".into()
    } else {
        "minecraft:overworld".into()
    }
}

/// The world as a bot reads it. Implemented by the glue over the server's level, and by tests.
pub trait BlockView {
    fn block(&self, x: i32, y: i32, z: i32) -> Block;

    fn collision_boxes(&self, area: &Aabb, out: &mut Vec<Aabb>) {
        let (x0, y0, z0) = (area.min.x.floor() as i32, (area.min.y.floor() as i32) - 1, area.min.z.floor() as i32);
        let (x1, y1, z1) = (area.max.x.floor() as i32, area.max.y.floor() as i32, area.max.z.floor() as i32);
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    self.block(x, y, z).boxes(x, y, z, out);
                }
            }
        }
    }

    /// Whether the straight line between two points passes no solid block (eyes to eyes).
    /// A voxel walk (Amanatides-Woo): every cell on the line is read once and the line is tested
    /// exactly against its boxes. Unloaded cells block; leaves and glass block too (a bot is not an x-ray).
    fn line_of_sight(&self, from: crate::math::Vec3, to: crate::math::Vec3) -> bool {
        let d = to - from;
        if d.length() < 1e-6 {
            return true;
        }
        let f = [from.x, from.y, from.z];
        let dv = [d.x, d.y, d.z];
        let mut cell = from.block();
        let end = to.block();
        let mut step = [0i32; 3];
        let mut t_max = [f64::INFINITY; 3];
        let mut t_delta = [f64::INFINITY; 3];
        for i in 0..3 {
            if dv[i] > 0.0 {
                step[i] = 1;
                t_delta[i] = 1.0 / dv[i];
                t_max[i] = ((cell[i] + 1) as f64 - f[i]) / dv[i];
            } else if dv[i] < 0.0 {
                step[i] = -1;
                t_delta[i] = -1.0 / dv[i];
                t_max[i] = (cell[i] as f64 - f[i]) / dv[i];
            }
        }
        let mut left = (0..3).map(|i| (end[i] - cell[i]).abs()).sum::<i32>() + 1;
        let mut boxes: Vec<Aabb> = Vec::new();
        loop {
            let blk = self.block(cell[0], cell[1], cell[2]);
            if blk.unloaded {
                return false;
            }
            match blk.shape {
                Shape::Empty => {}
                // The walk may visit a cell the line only touches at an edge (an exact tie): the box test decides.
                Shape::Full => {
                    if segment_hits(&Aabb::block(cell[0], cell[1], cell[2], 1.0), f, dv) {
                        return false;
                    }
                }
                _ => {
                    boxes.clear();
                    blk.boxes(cell[0], cell[1], cell[2], &mut boxes);
                    if boxes.iter().any(|b| segment_hits(b, f, dv)) {
                        return false;
                    }
                }
            }
            left -= 1;
            if left <= 0 || cell == end {
                return true;
            }
            let axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };
            if t_max[axis] > 1.0 {
                return true;
            }
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
        }
    }
}

/// Whether the segment `from + t * d`, `t` in 0..=1, meets the box (slab test; flat boxes never block).
fn segment_hits(b: &Aabb, from: [f64; 3], d: [f64; 3]) -> bool {
    let min = [b.min.x, b.min.y, b.min.z];
    let max = [b.max.x, b.max.y, b.max.z];
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for i in 0..3 {
        if max[i] <= min[i] {
            return false;
        }
        if d[i].abs() < 1e-12 {
            if from[i] < min[i] || from[i] > max[i] {
                return false;
            }
        } else {
            let (mut a, mut c) = ((min[i] - from[i]) / d[i], (max[i] - from[i]) / d[i]);
            if a > c {
                std::mem::swap(&mut a, &mut c);
            }
            t0 = t0.max(a);
            t1 = t1.min(c);
            // Touching an edge or a corner only (t0 == t1) is not a hit: sight passes between two blocks.
            if t0 >= t1 {
                return false;
            }
        }
    }
    true
}

/// A sparse test world: blocks by position, air elsewhere.
#[derive(Default, Clone)]
pub struct MapWorld {
    pub blocks: std::collections::HashMap<(i32, i32, i32), Block>,
    /// Everything below this y is stone (a floor), when set.
    pub floor: Option<i32>,
}

impl MapWorld {
    pub fn flat(floor_y: i32) -> Self {
        MapWorld {
            blocks: Default::default(),
            floor: Some(floor_y),
        }
    }
    pub fn set(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.blocks.insert((x, y, z), classify(state));
    }
}

impl BlockView for MapWorld {
    fn block(&self, x: i32, y: i32, z: i32) -> Block {
        if let Some(b) = self.blocks.get(&(x, y, z)) {
            return b.clone();
        }
        match self.floor {
            Some(f) if y <= f => Block::solid("minecraft:stone"),
            _ => Block::air(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_states() {
        assert_eq!(classify("minecraft:stone").shape, Shape::Full);
        assert!(classify("minecraft:air").passable());
        assert_eq!(classify("minecraft:oak_slab[type=bottom,waterlogged=false]").shape, Shape::Bottom(0.5));
        assert_eq!(classify("minecraft:oak_slab[type=top,waterlogged=true]").fluid, Fluid::Water);
        assert!(classify("minecraft:cobweb").cobweb);
        assert!(classify("minecraft:ladder[facing=north]").climbable);
        assert!(classify("minecraft:dirt").soft);
        assert!(!classify("minecraft:stone").soft);
        assert!(classify("minecraft:chest[facing=north]").protected);
        assert!(classify("minecraft:oak_planks").protected);
        assert!(classify("minecraft:short_grass").passable());
        assert_eq!(classify("minecraft:oak_fence").shape, Shape::Post);
        assert!(classify("minecraft:oak_door[open=false]").closed_door);
        assert!(classify("minecraft:lava[level=0]").harmful);
        assert_eq!(classify("minecraft:snow[layers=1]").shape, Shape::Bottom(0.0));
        let st = classify("minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]");
        let mut boxes = Vec::new();
        st.boxes(0, 0, 0, &mut boxes);
        assert_eq!(boxes.len(), 2);
        assert_eq!(st.top(), Some(1.0));
        assert!(boxes[1].max.z <= 0.5 && boxes[1].min.y == 0.5);
    }

    #[test]
    fn line_of_sight_passes_between_two_blocks_touching_at_an_edge() {
        use crate::math::Vec3;
        let mut w = MapWorld::flat(63);
        w.set(1, 64, 0, "minecraft:stone");
        w.set(0, 64, 1, "minecraft:stone");
        // Exactly through the shared vertical edge at x = 1, z = 1.
        assert!(w.line_of_sight(Vec3::new(0.5, 64.5, 0.5), Vec3::new(1.5, 64.5, 1.5)));
        w.set(1, 64, 1, "minecraft:stone");
        assert!(!w.line_of_sight(Vec3::new(0.5, 64.5, 0.5), Vec3::new(1.5, 64.5, 1.5)));
    }

    #[test]
    fn line_of_sight_is_blocked_by_a_wall() {
        let mut w = MapWorld::flat(63);
        use crate::math::Vec3;
        let a = Vec3::new(0.5, 65.6, 0.5);
        let b = Vec3::new(5.5, 65.6, 0.5);
        assert!(w.line_of_sight(a, b));
        w.set(3, 65, 0, "minecraft:stone");
        assert!(!w.line_of_sight(a, b));
    }

    /// The previous implementation: four samples per block, a box test at each.
    fn old_los(w: &MapWorld, from: crate::math::Vec3, to: crate::math::Vec3) -> bool {
        let d = to - from;
        let len = d.length();
        if len < 1e-6 {
            return true;
        }
        let steps = (len * 4.0).ceil() as usize;
        let mut last = [i32::MIN; 3];
        let mut boxes = Vec::new();
        for i in 1..steps {
            let p = from + d * (i as f64 / steps as f64);
            let b = p.block();
            if b == last {
                continue;
            }
            last = b;
            let blk = w.block(b[0], b[1], b[2]);
            if blk.unloaded {
                return false;
            }
            boxes.clear();
            blk.boxes(b[0], b[1], b[2], &mut boxes);
            if boxes
                .iter()
                .any(|bx| bx.min.x <= p.x && p.x <= bx.max.x && bx.min.y <= p.y && p.y <= bx.max.y && bx.min.z <= p.z && p.z <= bx.max.z)
            {
                return false;
            }
        }
        true
    }

    /// A dense reference: 200 points per block, every point tested (no per-cell shortcut).
    fn dense_los(w: &MapWorld, from: crate::math::Vec3, to: crate::math::Vec3) -> bool {
        let d = to - from;
        let steps = (d.length() * 200.0).ceil() as usize;
        let mut boxes = Vec::new();
        for i in 1..steps {
            let p = from + d * (i as f64 / steps as f64);
            let b = p.block();
            let blk = w.block(b[0], b[1], b[2]);
            boxes.clear();
            blk.boxes(b[0], b[1], b[2], &mut boxes);
            if boxes
                .iter()
                .any(|bx| bx.min.x <= p.x && p.x <= bx.max.x && bx.min.y <= p.y && p.y <= bx.max.y && bx.min.z <= p.z && p.z <= bx.max.z)
            {
                return false;
            }
        }
        true
    }

    #[test]
    fn line_of_sight_slabs_stairs_and_diagonals() {
        use crate::math::Vec3;
        let mut w = MapWorld::flat(63);
        // A bottom slab: a line over it passes, a line through it does not.
        w.set(3, 64, 0, "minecraft:oak_slab[type=bottom]");
        assert!(w.line_of_sight(Vec3::new(0.5, 64.8, 0.5), Vec3::new(6.5, 64.8, 0.5)));
        assert!(!w.line_of_sight(Vec3::new(0.5, 64.3, 0.5), Vec3::new(6.5, 64.3, 0.5)));
        // A top slab is the other way round.
        w.set(3, 64, 4, "minecraft:oak_slab[type=top]");
        assert!(w.line_of_sight(Vec3::new(0.5, 64.3, 4.5), Vec3::new(6.5, 64.3, 4.5)));
        assert!(!w.line_of_sight(Vec3::new(0.5, 64.8, 4.5), Vec3::new(6.5, 64.8, 4.5)));
        // A stair facing north: tall half on the north side (z < 0.5).
        w.set(3, 64, 8, "minecraft:oak_stairs[facing=north,half=bottom]");
        assert!(w.line_of_sight(Vec3::new(0.5, 64.8, 8.8), Vec3::new(6.5, 64.8, 8.8)));
        assert!(!w.line_of_sight(Vec3::new(0.5, 64.8, 8.2), Vec3::new(6.5, 64.8, 8.2)));
        // A diagonal line through a pillar, and one that misses it.
        w.set(5, 65, 5, "minecraft:stone");
        assert!(!w.line_of_sight(Vec3::new(2.5, 65.5, 2.5), Vec3::new(8.5, 65.5, 8.5)));
        assert!(w.line_of_sight(Vec3::new(2.5, 65.5, 3.7), Vec3::new(8.5, 65.5, 9.7)));
        // Leaves and glass block, a fence post blocks, an open gate does not.
        w.set(10, 65, 10, "minecraft:oak_leaves");
        w.set(10, 65, 12, "minecraft:glass");
        assert!(!w.line_of_sight(Vec3::new(8.5, 65.5, 10.5), Vec3::new(12.5, 65.5, 10.5)));
        assert!(!w.line_of_sight(Vec3::new(8.5, 65.5, 12.5), Vec3::new(12.5, 65.5, 12.5)));
        // Same point, and straight up and down.
        assert!(w.line_of_sight(Vec3::new(1.5, 65.5, 1.5), Vec3::new(1.5, 65.5, 1.5)));
        assert!(w.line_of_sight(Vec3::new(1.5, 65.5, 1.5), Vec3::new(1.5, 70.5, 1.5)));
        assert!(!w.line_of_sight(Vec3::new(1.5, 70.5, 1.5), Vec3::new(1.5, 60.5, 1.5)));
    }

    #[test]
    fn line_of_sight_unloaded_blocks() {
        use crate::math::Vec3;
        struct Hole(MapWorld);
        impl BlockView for Hole {
            fn block(&self, x: i32, y: i32, z: i32) -> Block {
                if x == 3 {
                    Block::unloaded()
                } else {
                    self.0.block(x, y, z)
                }
            }
        }
        let w = Hole(MapWorld::flat(63));
        assert!(!w.line_of_sight(Vec3::new(0.5, 65.5, 0.5), Vec3::new(6.5, 65.5, 0.5)));
        assert!(w.line_of_sight(Vec3::new(0.5, 65.5, 0.5), Vec3::new(2.5, 65.5, 0.5)));
    }

    #[test]
    fn line_of_sight_agrees_with_a_dense_reference_on_random_lines() {
        use crate::math::Vec3;
        let mut w = MapWorld::flat(63);
        let mut rng = 12345u64;
        let mut next = move || {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((rng >> 33) as f64) / ((1u64 << 31) as f64)
        };
        for _ in 0..120 {
            let (x, y, z) = ((next() * 20.0) as i32, 64 + (next() * 3.0) as i32, (next() * 20.0) as i32);
            let st =
                ["minecraft:stone", "minecraft:oak_slab[type=top]", "minecraft:oak_stairs[facing=east,half=bottom]", "minecraft:oak_fence"]
                    [(next() * 4.0) as usize];
            w.set(x, y, z, st);
        }
        let (mut differ, mut blocked) = (0, 0);
        for _ in 0..4000 {
            let a = Vec3::new(next() * 20.0, 64.0 + next() * 3.0, next() * 20.0);
            let b = Vec3::new(next() * 20.0, 64.0 + next() * 3.0, next() * 20.0);
            let (n, o) = (w.line_of_sight(a, b), old_los(&w, a, b));
            blocked += !n as i32;
            // Against the dense reference; the old sampling tested one point per cell and missed a lot.
            differ += (n != dense_los(&w, a, b)) as i32;
            // The old one could only miss thin things (a sample between two cells), never invent a block.
            assert!(!(n && !o), "new sees through what the old one hit: {a:?} -> {b:?}");
        }
        assert!(blocked > 300, "test world too empty: {blocked}");
        assert!(differ < 60, "{differ} of 4000 differ from the dense reference");
    }

    #[test]
    #[ignore]
    fn bench_line_of_sight() {
        use crate::math::Vec3;
        let mut w = MapWorld::flat(63);
        for x in (4..60).step_by(7) {
            w.set(x, 65, 5, "minecraft:stone");
            w.set(x, 64, 5, "minecraft:oak_stairs[facing=north,half=bottom]");
        }
        let (a, b) = (Vec3::new(0.5, 65.6, 0.5), Vec3::new(40.5, 65.6, 10.5));
        for (name, f) in [("new", 0), ("old", 1)] {
            let t = std::time::Instant::now();
            let mut n = 0u32;
            for i in 0..1_000_000u32 {
                let o = (i % 7) as f64 * 0.01;
                let (a, b) = (Vec3::new(a.x + o, a.y, a.z), Vec3::new(b.x, b.y, b.z + o));
                n += if f == 0 { w.line_of_sight(a, b) } else { old_los(&w, a, b) } as u32;
            }
            println!("bench_line_of_sight {name}: {:?} per 1M calls ({n} seen)", t.elapsed());
        }
    }
}
