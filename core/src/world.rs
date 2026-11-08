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
    Boxes(Vec<[f64; 6]>),
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
                for b in boxes {
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
        b.shape = Shape::Boxes(vec![
            [0.0, slab_y[0], 0.0, 1.0, slab_y[1], 1.0],
            [x0, step_y[0], z0, x1, step_y[1], z1],
        ]);
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
    fn line_of_sight(&self, from: crate::math::Vec3, to: crate::math::Vec3) -> bool {
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
            let blk = self.block(b[0], b[1], b[2]);
            if blk.unloaded {
                return false;
            }
            boxes.clear();
            blk.boxes(b[0], b[1], b[2], &mut boxes);
            // Leaves and glass-like blocks still block here: a bot is not an x-ray.
            if boxes.iter().any(|bx| {
                bx.min.x <= p.x && p.x <= bx.max.x && bx.min.y <= p.y && p.y <= bx.max.y && bx.min.z <= p.z && p.z <= bx.max.z
            }) {
                return false;
            }
        }
        true
    }
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
    fn line_of_sight_is_blocked_by_a_wall() {
        let mut w = MapWorld::flat(63);
        use crate::math::Vec3;
        let a = Vec3::new(0.5, 65.6, 0.5);
        let b = Vec3::new(5.5, 65.6, 0.5);
        assert!(w.line_of_sight(a, b));
        w.set(3, 65, 0, "minecraft:stone");
        assert!(!w.line_of_sight(a, b));
    }
}
