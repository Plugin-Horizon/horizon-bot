//! The special moves, each behind its own switch: attribute swaps (breach swap, mace stun, spear
//! lunge), the spear's reach jab, wind charges against a launch or a turtle, the fishing rod,
//! the trident, the elytra mace, cart PvP, lava and blocking-off plays, and edge play.
//!
//! Attribute swapping (a slot change and an attack in the same tick taking the left item's
//! enchantments with the new item's attributes) works only where the server orders things as
//! vanilla does; on Horizon it has to be tried (STATUS.md).

use super::squad::Squad;
use super::{Action, Bot, Decision, ElytraPhase, Hand, Perception, Seen};
use crate::items::{self, WeaponKind};
use crate::math::{pitch_to, yaw_to, Vec3};

fn hotbar(bot: &Bot, p: &Perception, f: impl Fn(&items::Stack) -> bool) -> Option<usize> {
    let _ = bot;
    p.me.inventory.find(true, f)
}

/// Tries the techniques that take the whole tick. True when one did.
pub fn try_all(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, dist: f64, flat: f64, squad: &mut Squad) -> bool {
    let _ = squad;
    wc_cancel(bot, p, d, t, flat)
        || pearl_launch(bot, p, d, t, flat)
        || shield_drain(bot, p, d, t, flat)
        || elytra_mace(bot, p, d, t, flat)
        || attribute_swaps(bot, p, d, t, dist)
        || spear_moves(bot, p, d, t, dist, flat)
        || spear_reach(bot, p, d, t, dist)
        || fishing_rod(bot, p, d, t, flat)
        || trident(bot, p, d, t, flat)
        || cart(bot, p, d, t, flat)
        || lava(bot, p, d, t, flat)
}

/// A wind charge into an opponent going up for a smash (never twice in 15 s).
fn wc_cancel(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64) -> bool {
    let st = &bot.settings;
    if !st.b("wc-cancel") || !t.holds("mace") || t.vel.y < 0.4 || flat > 8.0 || !bot.ready("wc-cancel", p.tick) {
        return false;
    }
    if !bot.rng.chance(st.pct("wc-cancel-chance")) {
        bot.cool("wc-cancel", p.tick, 20);
        return false;
    }
    throw_at(bot, p, d, t, "wind_charge", "wc-cancel", 300)
}

/// Mace pearl launch: a pearl thrown nearly straight up, a wind charge popped on it mid-flight,
/// the teleport far higher than a self-launch, the smash on the way down.
fn pearl_launch(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64) -> bool {
    let st = &bot.settings;
    if !st.b("mace-pearl-launch") || !st.b("mace") || !p.me.inventory.has("mace") {
        return false;
    }
    let tick = p.tick;
    let inv = &p.me.inventory;
    if let Some(&thrown) = bot.ready_at.get("pearl-launch") {
        // The pearl is about four blocks up after 6 ticks: the charge goes there.
        if tick >= thrown + 6 {
            bot.ready_at.remove("pearl-launch");
            if let Some(wc) = inv.find(true, |s| s.is("wind_charge")) {
                let at = bot.body.eye() + Vec3::new(0.0, 4.5, 0.0);
                d.actions.push(Action::SelectSlot(wc));
                d.yaw = yaw_to(bot.body.eye(), at);
                d.pitch = -89.0;
                d.actions.push(Action::Use(Hand::Main));
                bot.last_call = "pearl launch".into();
                d.mode = "pearl-launch";
                return true;
            }
        }
        return false;
    }
    if !(1.5..5.0).contains(&flat) || !bot.body.on_ground || !bot.ready("pearl-launch-cd", tick) || p.me.cooldown("ender_pearl") > 0 {
        return false;
    }
    let (Some(pearl), true) = (inv.find(true, |s| s.is("ender_pearl")), inv.has("wind_charge")) else { return false };
    if !bot.rng.chance(0.3 + 0.5 * st.knob("technique")) {
        bot.cool("pearl-launch-cd", tick, 60);
        return false;
    }
    let _ = t;
    d.actions.push(Action::SelectSlot(pearl));
    d.pitch = -88.0;
    d.actions.push(Action::Use(Hand::Main));
    bot.ready_at.insert("pearl-launch", tick);
    bot.cool("pearl-launch-cd", tick, 200);
    d.mode = "pearl-launch";
    true
}

/// Against a mace carrier: Slow Falling arrows first (they take the height away).
pub fn mace_aware_arrows(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen) {
    if t.holds("mace") {
        bot.ready_at.insert("saw-mace", p.tick + 100);
    }
    if bot.ready_at.get("saw-mace").is_none_or(|until| p.tick > *until) {
        return;
    }
    let inv = &p.me.inventory;
    let first_arrow = (0..inv.slots.len()).find(|&i| inv.slots[i].as_ref().is_some_and(|s| s.short().ends_with("arrow")));
    let slow = (0..inv.slots.len()).find(|&i| {
        inv.slots[i].as_ref().is_some_and(|s| s.is("tipped_arrow") && s.potion.as_deref().is_some_and(|p| p.ends_with("slow_falling")))
    });
    if let (Some(f), Some(sl)) = (first_arrow, slow) {
        if f != sl && bot.ready("arrow-swap", p.tick) {
            d.actions.push(Action::SwapSlots(f, sl));
            bot.cool("arrow-swap", p.tick, 40);
        }
    }
}

/// A wind charge into someone turtling behind a shield with no axe to answer.
fn shield_drain(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64) -> bool {
    if !bot.settings.b("shield-drain") || !t.blocking || flat > 6.0 {
        return false;
    }
    if p.me.inventory.best_weapon(true, Some(WeaponKind::Axe)).is_some() {
        return false;
    }
    throw_at(bot, p, d, t, "wind_charge", "shield-drain", 60)
}

fn throw_at(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, item: &str, why: &'static str, cooldown: u64) -> bool {
    if p.me.cooldown(item) > 0 {
        return false;
    }
    let Some(slot) = hotbar(bot, p, |s| s.is(item)) else { return false };
    let eye = bot.body.eye();
    let aim = t.pos + Vec3::new(0.0, t.height * 0.5, 0.0) + t.vel * 3.0;
    d.actions.push(Action::SelectSlot(slot));
    d.yaw = yaw_to(eye, aim);
    d.pitch = pitch_to(eye, aim);
    d.actions.push(Action::Use(Hand::Main));
    bot.cool(why, p.tick, cooldown);
    bot.last_call = why.replace('-', " ");
    d.mode = why;
    true
}

/// Breach swap and mace stun (master switch `attribute-swap`), rate-limited by `swap-cooldown`.
/// The enchanted item is held one tick; the next tick selects the other item and swings, so the
/// attack takes the left item's enchantments with the new item's speed.
fn attribute_swaps(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, dist: f64) -> bool {
    let st = bot.settings.clone();
    if !st.b("attribute-swap") || dist > st.f("melee-range") || !bot.ready("swap", p.tick) {
        return false;
    }
    let inv = &p.me.inventory;
    let mace = inv.find(true, |s| s.is("mace"));
    let sword = inv.best_weapon(true, Some(WeaponKind::Sword));
    let axe = inv.best_weapon(true, Some(WeaponKind::Axe));
    let technique = st.knob("technique");
    // Mace stun: axe the raised shield, the mace in the same tick.
    if st.b("mace-stun") && t.blocking && !st.b("ground-backstab") {
        if let (Some(a), Some(m)) = (axe, mace) {
            if bot.rng.chance(st.pct("stun-slam-chance")) && bot.rng.chance(0.4 + 0.6 * technique) {
                let variant = st.s("stun-slam-variant");
                d.actions.push(Action::SelectSlot(a));
                bot.send_late(p.tick, Action::Attack(t.id));
                bot.send_late(p.tick, Action::Swing(Hand::Main));
                match variant.as_str() {
                    "easy" => bot.ready_at.insert("stun-mace-at", p.tick + 2).map_or((), |_| ()),
                    "optimal" if bot.body.fall_distance < 1.5 => {}
                    _ => {
                        bot.send_late(p.tick, Action::SelectSlot(m));
                        bot.send_late(p.tick, Action::Attack(t.id));
                    }
                }
                bot.fight.shield_down_until = p.tick + 100;
                bot.fight.since_swing = 0;
                bot.cool("swap", p.tick, st.i("swap-cooldown").max(0) as u64);
                bot.last_call = "mace stun".into();
                d.mode = "mace-stun";
                return true;
            }
        }
    }
    // Breach swap: the mace's Breach/Density onto the sword's swing.
    if st.b("breach-swap") {
        if let (Some(m), Some(s)) = (mace, sword) {
            let breach = inv.slots[m].as_ref().is_some_and(|x| x.enchant("breach") > 0 || x.enchant("density") > 0);
            if breach && bot.fight.since_swing as f64 >= st.f("attack-cooldown") * 0.6 {
                if inv.selected != m {
                    // Hold the mace for a tick first.
                    d.actions.push(Action::SelectSlot(m));
                    return true;
                }
                d.actions.push(Action::SelectSlot(s));
                bot.send_late(p.tick, Action::Attack(t.id));
                bot.send_late(p.tick, Action::Swing(Hand::Main));
                bot.fight.since_swing = 0;
                bot.cool("swap", p.tick, st.i("swap-cooldown").max(0) as u64);
                bot.last_call = "breach swap".into();
                if st.b("detap-breach") {
                    bot.ready_at.insert("detap-at", p.tick + 10);
                }
                d.mode = "breach-swap";
                return true;
            }
        }
    }
    // Spear lunge: jabs chained past the cooldown to close in (Lunge does the dash).
    if st.b("spear-lunge") && dist > 2.5 {
        if let Some(sp) = inv.find(true, |s| items::weapon(&s.id).kind == WeaponKind::Spear && s.enchant("lunge") > 0) {
            d.actions.push(Action::SelectSlot(sp));
            bot.send_late(p.tick, Action::Swing(Hand::Main));
            if dist <= 4.5 {
                bot.send_late(p.tick, Action::Attack(t.id));
            }
            bot.cool("swap", p.tick, (st.i("swap-cooldown").max(0) / 3) as u64);
            d.mode = "spear-lunge";
            return true;
        }
    }
    false
}

/// The spear family: the stun slam with the spear (axe, then the long jab), the de-tap finishers
/// (a second hit the tick their hit protection ends), the dash at the top of a launch, and the
/// backstab round a raised shield.
fn spear_moves(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, dist: f64, flat: f64) -> bool {
    let st = bot.settings.clone();
    let tick = p.tick;
    let inv = &p.me.inventory;
    let spear = inv.find(true, |s| items::weapon(&s.id).kind == WeaponKind::Spear);
    let mace = inv.find(true, |s| s.is("mace"));
    // A finisher queued for the tick their protection ends.
    if let Some(&at) = bot.ready_at.get("detap-at") {
        if tick >= at {
            bot.ready_at.remove("detap-at");
            let slot = if st.b("detap-spear-mace") { mace } else { inv.find(true, |s| s.is("mace")) };
            if let Some(sl) = slot.or(spear) {
                if dist <= st.f("melee-range") + 1.5 {
                    d.actions.push(Action::SelectSlot(sl));
                    bot.send_late(tick, Action::Attack(t.id));
                    bot.send_late(tick, Action::Swing(Hand::Main));
                    bot.last_call = "de-tap".into();
                    d.mode = "detap";
                    return true;
                }
            }
        }
    }
    // Double pop: a smash that popped a totem, the spear's jab straight after for the next.
    if st.b("spear-double-pop") && bot.fight.popped_at.is_some_and(|at| tick <= at + 2) && dist <= 4.5 {
        if let Some(sp) = spear {
            bot.fight.popped_at = None;
            d.actions.push(Action::SelectSlot(sp));
            bot.send_late(tick, Action::Attack(t.id));
            bot.send_late(tick, Action::Swing(Hand::Main));
            bot.last_call = "double pop".into();
            d.mode = "double-pop";
            return true;
        }
    }
    // Spear stun slam: the axe on the shield, the spear's jab after it.
    if st.b("spear-stun-slam") && t.blocking && dist <= 4.5 && bot.ready("swap", tick) {
        if let (Some(axe), Some(sp)) = (inv.best_weapon(true, Some(WeaponKind::Axe)), spear) {
            d.actions.push(Action::SelectSlot(axe));
            bot.send_late(tick, Action::Attack(t.id));
            bot.send_late(tick, Action::SelectSlot(sp));
            bot.send_late(tick, Action::Attack(t.id));
            bot.fight.shield_down_until = tick + 100;
            bot.cool("swap", tick, st.i("swap-cooldown").max(0) as u64);
            bot.last_call = "spear stun slam".into();
            d.mode = "spear-stun-slam";
            return true;
        }
    }
    // De-tap on a low opponent: the jab now, the mace as their protection ends (10 ticks).
    if st.b("detap-spear-mace") && t.health <= 8.0 && dist <= 4.5 && mace.is_some() && bot.ready("detap", tick) {
        if let Some(sp) = spear {
            d.actions.push(Action::SelectSlot(sp));
            bot.send_late(tick, Action::Attack(t.id));
            bot.send_late(tick, Action::Swing(Hand::Main));
            bot.ready_at.insert("detap-at", tick + 10);
            bot.cool("detap", tick, 60);
            d.mode = "detap";
            return true;
        }
    }
    // The dash: at the top of a launch, a wind charge and the spear on the same tick.
    if (st.b("spear-dash") || st.b("spear-macing") || st.b("dash-chain")) && !bot.body.on_ground && bot.body.vel.y.abs() < 0.08 && (3.0..12.0).contains(&flat) {
        if let (Some(sp), Some(wc)) = (spear, inv.find(true, |s| s.is("wind_charge"))) {
            if bot.ready("dash", tick) && p.me.cooldown("wind_charge") == 0 {
                let eye = bot.body.eye();
                d.yaw = yaw_to(eye, t.pos);
                d.actions.push(Action::SelectSlot(wc));
                d.pitch = 60.0;
                d.actions.push(Action::Use(Hand::Main));
                d.actions.push(Action::SelectSlot(sp));
                d.actions.push(Action::Swing(Hand::Main));
                bot.cool("dash", tick, if st.b("dash-chain") { 15 } else { 60 });
                bot.last_call = "spear dash".into();
                d.mode = "spear-dash";
                return true;
            }
        }
    }
    // Ground backstab: past the shield, turn, mace their back.
    if st.b("ground-backstab") && t.blocking && mace.is_some() && flat < 5.0 {
        let r = (t.yaw as f64).to_radians();
        let behind = t.pos - Vec3::new(-r.sin(), 0.0, r.cos()) * 1.8;
        if behind.horizontal_distance(bot.body.pos) > 0.8 {
            bot.walk_to(p, d, behind, true);
            d.mode = "backstab";
            return false;
        }
        if let Some(m) = mace {
            d.actions.push(Action::SelectSlot(m));
            bot.send_late(tick, Action::Attack(t.id));
            bot.send_late(tick, Action::Swing(Hand::Main));
            bot.fight.since_swing = 0;
            d.mode = "backstab";
            return true;
        }
    }
    false
}

/// Just past sword reach: the spear's 4.5-block jab instead of a step in.
fn spear_reach(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, dist: f64) -> bool {
    let st = &bot.settings;
    if !st.b("spear-reach-swap") || !(st.f("melee-range")..4.5).contains(&dist) {
        return false;
    }
    let Some(sp) = p.me.inventory.find(true, |s| items::weapon(&s.id).kind == WeaponKind::Spear) else { return false };
    let cd = 20.0 / items::weapon("minecraft:iron_spear").speed;
    if (bot.fight.since_swing as f64) < cd {
        return false;
    }
    d.actions.push(Action::SelectSlot(sp));
    bot.send_late(p.tick, Action::Attack(t.id));
    bot.send_late(p.tick, Action::Swing(Hand::Main));
    bot.fight.since_swing = 0;
    d.mode = "spear-jab";
    true
}

/// The rod: hook them from a little way off for the pull and the free knockback.
fn fishing_rod(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64) -> bool {
    if !bot.settings.b("fishing-rod") || !(3.5..8.0).contains(&flat) {
        return false;
    }
    let Some(slot) = hotbar(bot, p, |s| s.is("fishing_rod")) else { return false };
    let tick = p.tick;
    if let Some(&cast) = bot.ready_at.get("rod-out") {
        if tick >= cast + 6 {
            // Reel in.
            d.actions.push(Action::SelectSlot(slot));
            d.actions.push(Action::Use(Hand::Main));
            bot.ready_at.remove("rod-out");
            bot.cool("rod", tick, 50);
            return true;
        }
        return false;
    }
    if !bot.ready("rod", tick) {
        return false;
    }
    let eye = bot.body.eye();
    d.actions.push(Action::SelectSlot(slot));
    d.yaw = yaw_to(eye, t.eye());
    d.pitch = super::combat::arrow_pitch(eye, t.pos + Vec3::new(0.0, 1.0, 0.0), 1.2, 0.04, 0.92).unwrap_or(-10.0);
    d.actions.push(Action::Use(Hand::Main));
    bot.ready_at.insert("rod-out", tick);
    d.mode = "rod";
    true
}

/// The trident thrown from range (no bow, or out of arrows).
fn trident(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64) -> bool {
    if !bot.settings.b("trident") || !(6.0..20.0).contains(&flat) {
        return false;
    }
    let Some(slot) = hotbar(bot, p, |s| s.is("trident") && s.enchant("riptide") == 0) else { return false };
    let tick = p.tick;
    let eye = bot.body.eye();
    let aim = t.pos + Vec3::new(0.0, t.height * 0.6, 0.0) + t.vel * (flat / 2.5);
    d.actions.push(Action::SelectSlot(slot));
    d.yaw = yaw_to(eye, aim);
    d.pitch = super::combat::arrow_pitch(eye, aim, 2.5, 0.05, 0.99).unwrap_or_else(|| pitch_to(eye, aim));
    match bot.ready_at.get("trident-draw").copied() {
        Some(since) if tick >= since + 11 => {
            d.actions.push(Action::Release);
            bot.ready_at.remove("trident-draw");
            bot.cool("trident", tick, 40);
        }
        Some(_) => {}
        None => {
            if !bot.ready("trident", tick) {
                return false;
            }
            d.actions.push(Action::Use(Hand::Main));
            bot.ready_at.insert("trident-draw", tick);
        }
    }
    d.mode = "trident";
    true
}

/// The elytra mace: elytra on, rockets up to `elytra-mace-height` above them, a dive at where
/// they are heading, the glide dropped for the swing; then up again (two passes a flight).
fn elytra_mace(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64) -> bool {
    let st = bot.settings.clone();
    if !st.b("elytra-mace") {
        return false;
    }
    let inv = &p.me.inventory;
    let has_elytra = inv.armor[2].as_ref().is_some_and(|s| s.is("elytra")) || inv.has("elytra");
    let mace = inv.find(true, |s| s.is("mace"));
    let rockets = inv.find(true, |s| s.is("firework_rocket"));
    if !has_elytra || mace.is_none() {
        bot.elytra = ElytraPhase::Ground;
        return false;
    }
    let tick = p.tick;
    let height = st.f("elytra-mace-height");
    let wearing = inv.armor[2].as_ref().is_some_and(|s| s.is("elytra"));
    match bot.elytra {
        ElytraPhase::Ground => {
            if rockets.is_none() || flat > 40.0 || !bot.ready("elytra-rest", tick) || !bot.body.on_ground {
                return false;
            }
            if !wearing {
                if let Some(slot) = inv.find(false, |s| s.is("elytra")) {
                    d.actions.push(Action::Wear(slot));
                }
                return true;
            }
            d.input.jump = true;
            bot.elytra = ElytraPhase::TakeOff { since: tick };
            d.mode = "elytra-takeoff";
            true
        }
        ElytraPhase::TakeOff { since } => {
            if !bot.body.on_ground && bot.body.vel.y < 0.1 {
                d.actions.push(Action::Glide);
                bot.body.gliding = true;
                if let Some(r) = rockets {
                    d.actions.push(Action::SelectSlot(r));
                    d.pitch = -70.0;
                    d.actions.push(Action::Use(Hand::Main));
                    bot.body.rocket_boost();
                }
                bot.elytra = ElytraPhase::Climb { passes: 0 };
            } else if tick > since + 20 {
                bot.elytra = ElytraPhase::Ground;
            } else {
                d.input.jump = true;
            }
            d.mode = "elytra-takeoff";
            true
        }
        ElytraPhase::Climb { passes } => {
            d.mode = "elytra-climb";
            let above = bot.body.pos.y - t.pos.y;
            if above >= height {
                bot.elytra = ElytraPhase::Dive { passes };
                return true;
            }
            d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, t.pos), 8.0);
            d.pitch = -50.0;
            if bot.ready("rocket", tick) {
                if let Some(r) = rockets {
                    d.actions.push(Action::SelectSlot(r));
                    d.actions.push(Action::Use(Hand::Main));
                    bot.body.rocket_boost();
                    bot.cool("rocket", tick, 25);
                } else {
                    bot.elytra = ElytraPhase::Dive { passes };
                }
            }
            true
        }
        ElytraPhase::Dive { passes } => {
            d.mode = "elytra-dive";
            let lead = t.pos + t.vel * 8.0;
            let eye = bot.body.eye();
            d.yaw = crate::math::approach_angle(d.yaw, yaw_to(eye, lead), 12.0);
            d.pitch = pitch_to(eye, lead).max(40.0);
            let reach = st.f("melee-range") + st.f("mace-range-bonus");
            if bot.reach_to(t) <= reach + 0.5 && bot.body.pos.y > t.pos.y {
                // The glide drops for the swing (a smash needs a fall), the mace comes out.
                bot.body.gliding = false;
                if let Some(m) = mace {
                    d.actions.push(Action::SelectSlot(m));
                }
                bot.send_late(tick, Action::Attack(t.id));
                bot.send_late(tick, Action::Swing(Hand::Main));
                bot.last_call = format!("e-mace pass from {:.0}", bot.body.fall_distance.max(bot.body.pos.y - t.pos.y));
                let passes = passes + 1;
                let chain = st.b("elytra-chain") && inv.slots[mace.unwrap()].as_ref().is_some_and(|s| s.enchant("wind_burst") > 0);
                if passes >= 2 && !chain {
                    bot.elytra = ElytraPhase::Ground;
                    let rest = 180 + bot.rng.int(0, 100) as u64;
                    bot.cool("elytra-rest", tick, rest);
                } else {
                    bot.elytra = ElytraPhase::Climb { passes };
                }
                return true;
            }
            if bot.body.on_ground {
                bot.elytra = ElytraPhase::Ground;
                bot.body.gliding = false;
            }
            true
        }
    }
}

/// Cart PvP: a Flame arrow, then a rail and a TNT minecart in its path, only when the numbers
/// work (its own expected blast damage under `cart-max-self-damage`).
fn cart(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64) -> bool {
    let st = bot.settings.clone();
    if !st.b("cart-pvp") || !st.b("building") || !(3.0..8.0).contains(&flat) || !bot.ready("cart", p.tick) {
        return false;
    }
    let inv = &p.me.inventory;
    let (Some(bow), Some(rail), Some(cart)) = (
        inv.find(true, |s| s.is("bow") && s.enchant("flame") > 0),
        inv.find(true, |s| s.is("rail") || s.is("powered_rail") || s.is("activator_rail")),
        inv.find(true, |s| s.is("tnt_minecart")),
    ) else {
        return false;
    };
    if !inv.has("arrow") || bot.rng.chance(st.pct("cart-skip")) {
        bot.cool("cart", p.tick, 20);
        return false;
    }
    // Its own blast: TNT power 4 at `flat` blocks (vanilla's falloff, before armour).
    let self_damage = tnt_damage(flat);
    let (armor, tough, prot) = inv.armor_value();
    let after = items::damage_after_armor(self_damage, armor, tough, prot);
    let totem = inv.offhand_is("totem_of_undying");
    if after > st.f("cart-max-self-damage") || (after * 1.3 >= p.me.health as f64 && !totem) {
        return false;
    }
    let tick = p.tick;
    let at = t.pos.block();
    let below = [at[0], at[1] - 1, at[2]];
    match bot.ready_at.get("cart-step").copied().unwrap_or(0) {
        0 => {
            d.actions.push(Action::SelectSlot(rail));
            let face_point = Vec3::new(below[0] as f64 + 0.5, below[1] as f64 + 1.0, below[2] as f64 + 0.5);
            d.yaw = yaw_to(bot.body.eye(), face_point);
            d.pitch = pitch_to(bot.body.eye(), face_point);
            d.actions.push(Action::UseOn { pos: below, face: 1, hand: Hand::Main });
            bot.ready_at.insert("cart-step", 1);
        }
        1 => {
            d.actions.push(Action::SelectSlot(cart));
            d.actions.push(Action::UseOn { pos: at, face: 1, hand: Hand::Main });
            bot.ready_at.insert("cart-step", 2);
        }
        _ => {
            d.actions.push(Action::SelectSlot(bow));
            match bot.ready_at.get("cart-draw").copied() {
                None => {
                    d.actions.push(Action::Use(Hand::Main));
                    bot.ready_at.insert("cart-draw", tick);
                }
                Some(since) if tick >= since + 12 => {
                    d.pitch = pitch_to(bot.body.eye(), t.pos);
                    d.actions.push(Action::Release);
                    bot.ready_at.remove("cart-draw");
                    bot.ready_at.remove("cart-step");
                    bot.cool("cart", tick, st.i("cart-cooldown").max(0) as u64);
                    bot.last_call = "cart".into();
                }
                Some(_) => {}
            }
        }
    }
    d.mode = "cart";
    true
}

/// Vanilla's explosion damage at a distance for TNT (power 4), no blocks in the way.
pub fn tnt_damage(distance: f64) -> f64 {
    let size = 8.0;
    let impact = (1.0 - distance / size).max(0.0);
    ((impact * impact + impact) / 2.0 * 7.0 * size + 1.0).floor()
}

/// UHC: lava at their feet, taken straight back.
fn lava(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64) -> bool {
    if !bot.settings.b("lava-play") || !(1.5..4.0).contains(&flat) {
        return false;
    }
    let tick = p.tick;
    if let Some(&(placed_at, ref pos)) = bot.lava_out.as_ref() {
        if tick >= placed_at + 8 {
            if let Some(b) = hotbar(bot, p, |s| s.is("bucket")) {
                let pos = *pos;
                d.actions.push(Action::SelectSlot(b));
                d.actions.push(Action::UseOn { pos, face: 1, hand: Hand::Main });
            }
            bot.lava_out = None;
            bot.cool("lava", tick, 100);
            return true;
        }
        return false;
    }
    if !bot.ready("lava", tick) {
        return false;
    }
    let Some(slot) = hotbar(bot, p, |s| s.is("lava_bucket")) else { return false };
    let at = t.pos.block();
    let below = [at[0], at[1] - 1, at[2]];
    if p.world.block(below[0], below[1], below[2]).top().is_none() {
        return false;
    }
    d.actions.push(Action::SelectSlot(slot));
    let face_point = Vec3::new(below[0] as f64 + 0.5, below[1] as f64 + 1.0, below[2] as f64 + 0.5);
    d.yaw = yaw_to(bot.body.eye(), face_point);
    d.pitch = pitch_to(bot.body.eye(), face_point);
    d.actions.push(Action::UseOn { pos: below, face: 1, hand: Hand::Main });
    bot.lava_out = Some((tick, at));
    bot.last_call = "lava".into();
    d.mode = "lava";
    true
}

/// While healing: a couple of blocks between it and them.
pub fn block_off(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen) -> bool {
    if !bot.settings.b("block-off") || !bot.settings.b("building") || !bot.ready("block-off", p.tick) {
        return false;
    }
    let Some(slot) = hotbar(bot, p, |s| items::is_building_block(&s.id)) else { return false };
    let dir = (t.pos - bot.body.pos).horizontal().normalize();
    let spot = (bot.body.pos + dir * 1.5).block();
    let below = [spot[0], spot[1] - 1, spot[2]];
    if !p.world.block(spot[0], spot[1], spot[2]).passable() || p.world.block(below[0], below[1], below[2]).top().is_none() {
        return false;
    }
    d.actions.push(Action::SelectSlot(slot));
    d.actions.push(Action::UseOn { pos: below, face: 1, hand: Hand::Main });
    bot.cool("block-off", p.tick, 40);
    d.mode = "block-off";
    true
}

/// Edge play: the point to circle to so that a hit knocks them toward a drop, lava or the void.
pub fn edge_goal(bot: &Bot, p: &Perception, t: &Seen) -> Option<Vec3> {
    if !bot.settings.b("edge-play") {
        return None;
    }
    let feet = t.pos.block();
    let mut best: Option<(f64, Vec3)> = None;
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        // How far one would fall two blocks past them in this direction.
        let x = feet[0] + dx * 2;
        let z = feet[2] + dz * 2;
        let mut drop = 0;
        for dy in 1..=12 {
            let b = p.world.block(x, feet[1] - dy, z);
            if b.unloaded {
                break;
            }
            if b.fluid == crate::world::Fluid::Lava {
                drop = 20;
                break;
            }
            if b.top().is_some() {
                break;
            }
            drop = dy;
        }
        if drop >= 4 {
            // Stand on the other side of them.
            let spot = t.pos - Vec3::new(dx as f64, 0.0, dz as f64) * 2.2;
            if best.as_ref().is_none_or(|(d, _)| drop as f64 > *d) {
                best = Some((drop as f64, spot));
            }
        }
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tnt_falloff() {
        assert_eq!(tnt_damage(0.0), 57.0);
        assert_eq!(tnt_damage(8.0), 1.0);
        assert!(tnt_damage(4.0) < tnt_damage(2.0));
    }

    #[test]
    fn edge_play_finds_the_drop() {
        use crate::brain::tests::{bot_with, me_with, perceive, player};
        let mut w = crate::world::MapWorld::default();
        for x in -5..=1 {
            for z in -5..=5 {
                w.set(x, 63, z, "minecraft:stone");
            }
        }
        let bot = bot_with(Vec3::new(-3.5, 64.0, 0.5), &[]);
        let me = me_with(&[]);
        let others = [player(2, "Steve", Vec3::new(0.5, 64.0, 0.5))];
        let p = perceive(1, &me, &others, &w);
        let g = edge_goal(&bot, &p, &others[0]).expect("an edge");
        // The drop is to +X: the bot goes to the -X side.
        assert!(g.x < 0.5, "{g:?}");
    }
}
