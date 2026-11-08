//! A bot moves as a player's client moves it: it presses keys and the client's physics turn them
//! into a position, which is what the server is told. This is that physics, after vanilla's
//! `LivingEntity.travel` for a player: walking with block friction, sprinting, jumping (with the
//! sprint-jump boost), gravity and drag, collisions with step-up, water, lava, cobwebs, ladders
//! and sneaking at edges. Knockback arrives from the server as a new velocity.

use crate::math::{Aabb, Vec3};
use crate::world::{BlockView, Fluid};

/// The keys a player holds this tick, as `ServerboundPlayerInputPacket` carries them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// -1..1 (back..forward)
    pub forward: f64,
    /// -1..1 (right..left, vanilla's sign)
    pub strafe: f64,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

impl Input {
    /// The packet's flags: forward 1, backward 2, left 4, right 8, jump 16, shift 32, sprint 64.
    pub fn flags(&self) -> u8 {
        let mut f = 0;
        if self.forward > 0.05 {
            f |= 1;
        }
        if self.forward < -0.05 {
            f |= 2;
        }
        if self.strafe > 0.05 {
            f |= 4;
        }
        if self.strafe < -0.05 {
            f |= 8;
        }
        if self.jump {
            f |= 16;
        }
        if self.sneak {
            f |= 32;
        }
        if self.sprint {
            f |= 64;
        }
        f
    }
}

pub const GRAVITY: f64 = 0.08;
pub const DRAG: f64 = 0.98;
pub const JUMP: f64 = 0.42;
pub const STEP: f64 = 0.6;
pub const WALK_SPEED: f64 = 0.1;
pub const STANDING_HEIGHT: f64 = 1.8;
pub const SNEAKING_HEIGHT: f64 = 1.5;
pub const EYE_STANDING: f64 = 1.62;
pub const EYE_SNEAKING: f64 = 1.27;

#[derive(Clone, Debug)]
pub struct Body {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub fall_distance: f64,
    pub in_water: bool,
    pub in_lava: bool,
    pub in_web: bool,
    pub on_climbable: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    /// Ticks until another jump (vanilla's `noJumpDelay`).
    pub jump_delay: u32,
    /// Movement speed attribute (0.1 base; Speed, Slowness change it).
    pub speed_attr: f64,
    /// Jump Boost amplifier + 1 (0 = none).
    pub jump_boost: u32,
    pub slow_falling: bool,
    pub levitation: u32,
    /// Gliding on an elytra.
    pub gliding: bool,
}

impl Body {
    pub fn new(pos: Vec3) -> Self {
        Body {
            pos,
            vel: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            horizontal_collision: false,
            fall_distance: 0.0,
            in_water: false,
            in_lava: false,
            in_web: false,
            on_climbable: false,
            sneaking: false,
            sprinting: false,
            jump_delay: 0,
            speed_attr: WALK_SPEED,
            jump_boost: 0,
            slow_falling: false,
            levitation: 0,
            gliding: false,
        }
    }

    pub fn height(&self) -> f64 {
        if self.sneaking {
            SNEAKING_HEIGHT
        } else {
            STANDING_HEIGHT
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::new(0.0, if self.sneaking { EYE_SNEAKING } else { EYE_STANDING }, 0.0)
    }

    pub fn aabb(&self) -> Aabb {
        Aabb::player(self.pos, self.height())
    }

    /// One client tick.
    pub fn tick(&mut self, input: &Input, world: &dyn BlockView) {
        self.update_contacts(world);
        self.sneaking = input.sneak && !self.gliding;
        // Sprinting needs forward input, no sneaking and food (the caller checks food).
        self.sprinting = input.sprint && input.forward > 0.5 && !self.sneaking && !self.in_web;
        if self.jump_delay > 0 {
            self.jump_delay -= 1;
        }
        let (mut fwd, mut strafe) = (input.forward * 0.98, input.strafe * 0.98);
        if self.sneaking {
            fwd *= 0.3;
            strafe *= 0.3;
        }

        if input.jump {
            if self.in_water || self.in_lava {
                self.vel.y += 0.04;
            } else if self.on_ground && self.jump_delay == 0 {
                self.jump();
                self.jump_delay = 10;
            }
        } else {
            self.jump_delay = 0;
        }

        if self.gliding {
            self.glide();
        } else if self.in_water || self.in_lava {
            // Water and lava take the same push (vanilla's 0.02).
            let speed = 0.02;
            self.move_relative(fwd, strafe, speed);
            self.do_move(world);
            let damp = if self.in_water { 0.8 } else { 0.5 };
            self.vel = Vec3::new(self.vel.x * damp, self.vel.y * 0.8, self.vel.z * damp);
            self.vel.y -= if self.in_water { 0.005 } else { 0.02 };
            if self.horizontal_collision && input.jump {
                self.vel.y = 0.3;
            }
        } else {
            let below = self.pos.with_y(self.pos.y - 0.5000001).block();
            let blk = world.block(below[0], below[1], below[2]);
            let slip = if self.on_ground { blk.friction } else { 1.0 };
            let friction = slip * 0.91;
            let speed = if self.on_ground {
                let mut s = self.speed_attr;
                if self.sprinting {
                    s *= 1.3;
                }
                s * (0.216_000_02 / (slip * slip * slip))
            } else if self.sprinting {
                0.025_999_999
            } else {
                0.02
            };
            self.move_relative(fwd, strafe, speed);
            if self.on_climbable {
                self.vel.x = self.vel.x.clamp(-0.15, 0.15);
                self.vel.z = self.vel.z.clamp(-0.15, 0.15);
                self.fall_distance = 0.0;
                self.vel.y = self.vel.y.max(-0.15);
                if self.sneaking && self.vel.y < 0.0 {
                    self.vel.y = 0.0;
                }
            }
            if self.sneaking && self.on_ground {
                self.back_off_edges(world);
            }
            self.do_move(world);
            if self.on_climbable && (self.horizontal_collision || input.jump) {
                self.vel.y = 0.2;
            }
            let gravity = if self.slow_falling && self.vel.y <= 0.0 { 0.01 } else { GRAVITY };
            if self.levitation > 0 {
                self.vel.y += (0.05 * self.levitation as f64 - self.vel.y) * 0.2;
            } else {
                self.vel.y -= gravity;
            }
            self.vel.y *= DRAG;
            self.vel.x *= friction;
            self.vel.z *= friction;
            let f = blk.speed_factor;
            if f < 1.0 && self.on_ground {
                self.vel.x *= f;
                self.vel.z *= f;
            }
        }
        if self.in_web {
            self.vel = Vec3::new(self.vel.x * 0.25, self.vel.y * 0.05, self.vel.z * 0.25);
            self.fall_distance = 0.0;
        }
        for v in [&mut self.vel.x, &mut self.vel.y, &mut self.vel.z] {
            if v.abs() < 0.003 {
                *v = 0.0;
            }
        }
    }

    fn jump(&mut self) {
        self.vel.y = JUMP + 0.1 * self.jump_boost as f64;
        if self.sprinting {
            let r = (self.yaw as f64).to_radians();
            self.vel.x -= r.sin() * 0.2;
            self.vel.z += r.cos() * 0.2;
        }
        self.on_ground = false;
    }

    fn glide(&mut self) {
        // Vanilla's elytra flight, condensed.
        let look = crate::math::look_vector(self.yaw, self.pitch);
        let pitch = (self.pitch as f64).to_radians();
        let h = look.horizontal_length();
        let speed = self.vel.horizontal_length();
        let lift = pitch.cos();
        let lift = lift * lift * (look.length().min(1.0));
        self.vel.y += -0.08 + lift * 0.06;
        if self.vel.y < 0.0 && h > 0.0 {
            let k = self.vel.y * -0.1 * lift;
            self.vel.y += k;
            self.vel.x += look.x * k / h;
            self.vel.z += look.z * k / h;
        }
        if pitch < 0.0 && h > 0.0 {
            let k = speed * (-pitch.sin()) * 0.04;
            self.vel.y += k * 3.2;
            self.vel.x -= look.x * k / h;
            self.vel.z -= look.z * k / h;
        }
        if h > 0.0 {
            self.vel.x += (look.x / h * speed - self.vel.x) * 0.1;
            self.vel.z += (look.z / h * speed - self.vel.z) * 0.1;
        }
        self.vel = Vec3::new(self.vel.x * 0.99, self.vel.y * 0.98, self.vel.z * 0.99);
        self.fall_distance = 0.0;
    }

    /// A firework rocket's boost while gliding.
    pub fn rocket_boost(&mut self) {
        let look = crate::math::look_vector(self.yaw, self.pitch);
        self.vel += Vec3::new(
            look.x * 0.1 + (look.x * 1.5 - self.vel.x) * 0.5,
            look.y * 0.1 + (look.y * 1.5 - self.vel.y) * 0.5,
            look.z * 0.1 + (look.z * 1.5 - self.vel.z) * 0.5,
        );
    }

    fn move_relative(&mut self, fwd: f64, strafe: f64, speed: f64) {
        let len2 = fwd * fwd + strafe * strafe;
        if len2 < 1e-7 {
            return;
        }
        let k = if len2 > 1.0 { 1.0 / len2.sqrt() } else { 1.0 };
        let (f, s) = (fwd * k * speed, strafe * k * speed);
        let r = (self.yaw as f64).to_radians();
        let (sin, cos) = (r.sin(), r.cos());
        self.vel.x += s * cos - f * sin;
        self.vel.z += f * cos + s * sin;
    }

    fn back_off_edges(&mut self, world: &dyn BlockView) {
        // Vanilla's maybeBackOffFromEdge: shrink the move until there is ground under it.
        let has_ground = |dx: f64, dz: f64| {
            let b = self.aabb().offset(Vec3::new(dx, -STEP, dz));
            let mut boxes = Vec::new();
            world.collision_boxes(&b, &mut boxes);
            boxes.iter().any(|bx| bx.intersects(&b))
        };
        let step = 0.05;
        let (mut dx, mut dz) = (self.vel.x, self.vel.z);
        while dx != 0.0 && !has_ground(dx, 0.0) {
            dx = if dx.abs() < step { 0.0 } else { dx - step * dx.signum() };
        }
        while dz != 0.0 && !has_ground(0.0, dz) {
            dz = if dz.abs() < step { 0.0 } else { dz - step * dz.signum() };
        }
        while dx != 0.0 && dz != 0.0 && !has_ground(dx, dz) {
            dx = if dx.abs() < step { 0.0 } else { dx - step * dx.signum() };
            dz = if dz.abs() < step { 0.0 } else { dz - step * dz.signum() };
        }
        self.vel.x = dx;
        self.vel.z = dz;
    }

    fn collide(&self, world: &dyn BlockView, motion: Vec3) -> Vec3 {
        let bb = self.aabb();
        let mut boxes = Vec::new();
        world.collision_boxes(&bb.expand(motion).inflate(1e-7), &mut boxes);
        let mut out = motion;
        let mut b = bb;
        // Y, then the larger horizontal axis first (vanilla's order).
        let mut dy = out.y;
        for o in &boxes {
            dy = b.clip(o, 1, dy);
        }
        b = b.offset(Vec3::new(0.0, dy, 0.0));
        out.y = dy;
        let x_first = out.x.abs() >= out.z.abs();
        for axis in if x_first { [0usize, 2] } else { [2, 0] } {
            let mut d = if axis == 0 { out.x } else { out.z };
            for o in &boxes {
                d = b.clip(o, axis, d);
            }
            let off = if axis == 0 { Vec3::new(d, 0.0, 0.0) } else { Vec3::new(0.0, 0.0, d) };
            b = b.offset(off);
            if axis == 0 {
                out.x = d;
            } else {
                out.z = d;
            }
        }
        out
    }

    fn do_move(&mut self, world: &dyn BlockView) {
        let wanted = self.vel;
        let mut got = self.collide(world, wanted);
        let blocked_h = (got.x - wanted.x).abs() > 1e-7 || (got.z - wanted.z).abs() > 1e-7;
        let landing = wanted.y < 0.0 && (got.y - wanted.y).abs() > 1e-7;
        if blocked_h && (self.on_ground || landing) {
            // Step up: try the move from STEP higher, keep it if it got further.
            let saved = self.pos;
            let up = self.collide(world, Vec3::new(0.0, STEP, 0.0));
            self.pos += up;
            let mut stepped = self.collide(world, Vec3::new(wanted.x, 0.0, wanted.z));
            self.pos += Vec3::new(stepped.x, 0.0, stepped.z);
            let down = self.collide(world, Vec3::new(0.0, -STEP - up.y.max(0.0), 0.0));
            self.pos = saved;
            stepped.y = up.y + down.y;
            if stepped.horizontal_length() > got.horizontal_length() + 1e-7 {
                got = stepped;
            }
        }
        self.pos += got;
        self.horizontal_collision = (got.x - wanted.x).abs() > 1e-7 || (got.z - wanted.z).abs() > 1e-7;
        let vertical_collision = (got.y - wanted.y).abs() > 1e-7;
        self.on_ground = vertical_collision && wanted.y < 0.0;
        if (got.x - wanted.x).abs() > 1e-7 {
            self.vel.x = 0.0;
        }
        if (got.z - wanted.z).abs() > 1e-7 {
            self.vel.z = 0.0;
        }
        if vertical_collision {
            self.vel.y = 0.0;
        }
        if self.on_ground {
            self.fall_distance = 0.0;
        } else if got.y < 0.0 {
            self.fall_distance -= got.y;
        }
    }

    fn update_contacts(&mut self, world: &dyn BlockView) {
        let bb = self.aabb().inflate(-0.001);
        let (mut water, mut lava, mut web) = (false, false, false);
        for x in bb.min.x.floor() as i32..=bb.max.x.floor() as i32 {
            for y in bb.min.y.floor() as i32..=bb.max.y.floor() as i32 {
                for z in bb.min.z.floor() as i32..=bb.max.z.floor() as i32 {
                    let b = world.block(x, y, z);
                    match b.fluid {
                        Fluid::Water => water = true,
                        Fluid::Lava => lava = true,
                        Fluid::None => {}
                    }
                    web |= b.cobweb;
                }
            }
        }
        let feet = self.pos.block();
        self.on_climbable = world.block(feet[0], feet[1], feet[2]).climbable;
        self.in_water = water;
        self.in_lava = lava;
        self.in_web = web;
        if water {
            self.fall_distance = 0.0;
        }
    }

    /// Where the body would be after `ticks` with no input (for fall and landing prediction).
    pub fn predict_fall(&self, world: &dyn BlockView, ticks: u32) -> (Vec3, Option<u32>) {
        let mut b = self.clone();
        for t in 0..ticks {
            b.tick(&Input::default(), world);
            if b.on_ground || b.in_water {
                return (b.pos, Some(t + 1));
            }
        }
        (b.pos, None)
    }

    /// The fall damage landing now would do (vanilla: distance - 3, less with safe blocks).
    pub fn fall_damage(&self) -> f64 {
        (self.fall_distance - 3.0).max(0.0).ceil()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::MapWorld;

    fn standing() -> (Body, MapWorld) {
        let w = MapWorld::flat(63);
        let mut b = Body::new(Vec3::new(0.5, 64.0, 0.5));
        for _ in 0..3 {
            b.tick(&Input::default(), &w);
        }
        assert!(b.on_ground);
        (b, w)
    }

    #[test]
    fn stands_on_the_floor() {
        let (mut b, w) = standing();
        for _ in 0..20 {
            b.tick(&Input::default(), &w);
        }
        assert!(b.on_ground);
        assert!((b.pos.y - 64.0).abs() < 1e-6, "{}", b.pos.y);
    }

    #[test]
    fn sprinting_reaches_vanillas_top_speed() {
        let (mut b, w) = standing();
        let i = Input { forward: 1.0, sprint: true, ..Default::default() };
        for _ in 0..40 {
            b.tick(&i, &w);
        }
        let start = b.pos;
        b.tick(&i, &w);
        // Vanilla sprinting on the ground: about 5.6 blocks a second.
        let per_tick = b.pos.distance(start);
        assert!(per_tick > 0.25 && per_tick < 0.32, "{per_tick}");
        // Yaw 0 walks to +Z.
        assert!(b.pos.z > start.z);
    }

    #[test]
    fn a_jump_peaks_at_vanillas_height() {
        let (mut b, w) = standing();
        b.tick(&Input { jump: true, ..Default::default() }, &w);
        let mut top = b.pos.y;
        for _ in 0..20 {
            b.tick(&Input::default(), &w);
            top = top.max(b.pos.y);
        }
        assert!((top - 64.0 - 1.2522).abs() < 0.02, "{top}");
        assert!(b.on_ground);
    }

    #[test]
    fn walls_stop_and_steps_are_climbed() {
        let (mut b, mut w) = standing();
        for y in 64..=66 {
            w.set(0, y, 3, "minecraft:stone");
        }
        let i = Input { forward: 1.0, ..Default::default() };
        for _ in 0..60 {
            b.tick(&i, &w);
        }
        assert!(b.pos.z <= 2.7 + 1e-6, "{}", b.pos.z);
        assert!(b.horizontal_collision);
        let (mut b, mut w) = standing();
        w.set(0, 64, 3, "minecraft:oak_slab[type=bottom]");
        for _ in 0..60 {
            b.tick(&i, &w);
        }
        assert!(b.pos.z > 3.5, "stepped onto the slab: {:?}", b.pos);
    }

    #[test]
    fn stairs_are_walked_up_without_a_jump() {
        let (mut b, mut w) = standing();
        w.set(0, 64, 3, "minecraft:oak_stairs[facing=south,half=bottom,shape=straight,waterlogged=false]");
        w.set(0, 64, 4, "minecraft:stone");
        let i = Input { forward: 1.0, ..Default::default() };
        let mut on_top = false;
        for _ in 0..60 {
            b.tick(&i, &w);
            // Standing on the block behind the stair: up a whole block without jumping.
            on_top |= b.on_ground && (b.pos.y - 65.0).abs() < 1e-6 && b.pos.z > 4.0 && b.pos.z < 5.0;
            assert!(!i.jump);
        }
        assert!(on_top, "never stood on the block behind the stair: {:?}", b.pos);
    }

    #[test]
    fn falling_counts_distance_and_cobwebs_stop_it() {
        let w = MapWorld::flat(40);
        let mut b = Body::new(Vec3::new(0.5, 60.0, 0.5));
        let mut max_fall: f64 = 0.0;
        for _ in 0..100 {
            b.tick(&Input::default(), &w);
            max_fall = max_fall.max(b.fall_distance);
            if b.on_ground {
                break;
            }
        }
        assert!(b.on_ground);
        assert!(max_fall > 18.0, "{max_fall}");
        let mut w = MapWorld::flat(40);
        w.set(0, 50, 0, "minecraft:cobweb");
        let mut b = Body::new(Vec3::new(0.5, 52.0, 0.5));
        for _ in 0..30 {
            b.tick(&Input::default(), &w);
        }
        assert!(b.pos.y > 49.0, "held by the web: {}", b.pos.y);
    }

    #[test]
    fn sneaking_does_not_walk_off_an_edge() {
        let mut w = MapWorld::default();
        w.set(0, 63, 0, "minecraft:stone");
        let mut b = Body::new(Vec3::new(0.5, 64.0, 0.5));
        b.tick(&Input::default(), &w);
        let i = Input { forward: 1.0, sneak: true, ..Default::default() };
        for _ in 0..60 {
            b.tick(&i, &w);
        }
        assert!(b.on_ground && b.pos.y > 63.9, "{:?}", b.pos);
    }

    #[test]
    fn input_flags_match_the_packet() {
        let i = Input { forward: 1.0, strafe: -1.0, jump: true, sneak: false, sprint: true };
        assert_eq!(i.flags(), 1 | 8 | 16 | 64);
    }
}
