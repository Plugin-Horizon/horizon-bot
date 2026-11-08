//! Regearing as on an SMP: away from the fight, the ender chest (or a carried shulker box) down
//! beside it, opened, what it is short of taken out, closed, mined back up. One member of a
//! faction at a time (`regear-turns`). `restock instant` fills it at once instead.

use super::squad::Squad;
use super::{Action, Bot, Decision, Hand, Perception, Regear, Seen};
use crate::math::{pitch_to, yaw_to, Vec3};

fn shortfall(bot: &Bot, p: &Perception) -> Vec<(String, u32)> {
    let Some(t) = &bot.restock_targets else { return Vec::new() };
    crate::kits::shortfall(t, &p.me.inventory.restock_counts())
}

/// Worth a regear: out of totems, pearls or gapples, or short of a quarter of the kit.
fn worth_it(short: &[(String, u32)], targets: Option<&std::collections::BTreeMap<String, u32>>) -> bool {
    let key = |k: &str| short.iter().any(|(s, _)| s.ends_with(k));
    if key("totem_of_undying") || key("ender_pearl") || key("golden_apple") {
        return true;
    }
    let total: u32 = targets.map_or(0, |t| t.values().sum());
    let missing: u32 = short.iter().map(|(_, n)| n).sum();
    total > 0 && missing * 4 >= total
}

pub fn regear(bot: &mut Bot, p: &Perception, d: &mut Decision, threat: Option<&Seen>, squad: &mut Squad) -> bool {
    let st = bot.settings.clone();
    let tick = p.tick;
    let inv = &p.me.inventory;
    let skill = st.knob("technique");
    let away = threat.is_none_or(|t| t.pos.distance(bot.body.pos) > st.f("regear-distance") || !p.world.line_of_sight(bot.body.eye(), t.eye()));
    match bot.regear.clone() {
        Regear::Idle => {
            let ender = st.b("enderchest-regear") && inv.has("ender_chest");
            let shulker = st.b("shulker-regear") && inv.find(false, |s| s.short().ends_with("shulker_box")).is_some();
            if !(ender || shulker) {
                return false;
            }
            let short = shortfall(bot, p);
            if short.is_empty() || !worth_it(&short, bot.restock_targets.as_ref()) {
                return false;
            }
            if let Some(f) = &bot.faction {
                if st.b("regear-turns") && squad.regearing.contains(f) {
                    return false;
                }
                squad.regearing.insert(f.clone());
            }
            bot.regear = Regear::Leaving { since: tick };
            d.mode = "regear";
            true
        }
        Regear::Leaving { since } => {
            d.mode = "regear-leave";
            if !away && tick < since + 200 {
                if let Some(t) = threat {
                    let to = bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 12.0;
                    if !super::combat::pearl(bot, p, d, to, "pearl-to-regear") {
                        d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, to), 20.0);
                        bot.walk_to(p, d, to, true);
                    }
                }
                return true;
            }
            if !away {
                release(bot, squad);
                return false;
            }
            // Down beside it: the block next to its feet, on solid ground.
            let feet = bot.body.pos.block();
            let ender = st.b("enderchest-regear") && inv.has("ender_chest");
            let item = if ender {
                inv.find(true, |s| s.is("ender_chest"))
            } else {
                inv.find(true, |s| s.short().ends_with("shulker_box"))
            };
            let Some(slot) = item else {
                // In the bag only: up to the hotbar first.
                let from = if ender { inv.find(false, |s| s.is("ender_chest")) } else { inv.find(false, |s| s.short().ends_with("shulker_box")) };
                if let Some(f) = from {
                    d.actions.push(Action::ToHotbar { slot: f, hotbar: inv.hotbar_slot_for_swap(&[]) });
                    return true;
                }
                release(bot, squad);
                return false;
            };
            for (dx, dz) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                let spot = [feet[0] + dx, feet[1], feet[2] + dz];
                let below = [spot[0], spot[1] - 1, spot[2]];
                if p.world.block(spot[0], spot[1], spot[2]).passable() && p.world.block(below[0], below[1], below[2]).top().is_some() {
                    d.actions.push(Action::SelectSlot(slot));
                    let at = Vec3::new(below[0] as f64 + 0.5, below[1] as f64 + 1.0, below[2] as f64 + 0.5);
                    d.yaw = yaw_to(bot.body.eye(), at);
                    d.pitch = pitch_to(bot.body.eye(), at);
                    d.actions.push(Action::UseOn { pos: below, face: 1, hand: Hand::Main });
                    d.actions.push(Action::Swing(Hand::Main));
                    bot.regear = Regear::Placed { pos: spot, since: tick, ender };
                    d.mode = "regear-place";
                    return true;
                }
            }
            release(bot, squad);
            false
        }
        Regear::Placed { pos, since, ender } => {
            d.mode = "regear-open";
            if tick < since + 4 {
                return true;
            }
            let at = Vec3::new(pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5);
            d.yaw = yaw_to(bot.body.eye(), at);
            d.pitch = pitch_to(bot.body.eye(), at);
            d.actions.push(Action::UseOn { pos, face: 1, hand: Hand::Main });
            let wants = shortfall(bot, p);
            // One stack at a time, faster with skill.
            let per = (8.0 - 5.0 * skill).round().max(2.0) as u64;
            let until = tick + 6 + per * wants.len() as u64;
            d.actions.push(Action::TakeFrom { pos, ender, wants });
            bot.regear = Regear::Taking { pos, until, ender };
            true
        }
        Regear::Taking { pos, until, .. } => {
            d.mode = "regear-take";
            if tick < until {
                // Caught halfway: it fights for its stuff (the chest stays and is mined later).
                return !threat.is_some_and(|t| t.pos.distance(bot.body.pos) < 5.0);
            }
            d.actions.push(Action::CloseContainer);
            // Mined with its best pickaxe over the real break time.
            let pick = inv.find(true, |s| s.short().ends_with("_pickaxe"));
            if let Some(pk) = pick {
                d.actions.push(Action::SelectSlot(pk));
            }
            let speed = pick
                .and_then(|i| inv.slots[i].as_ref())
                .map_or(1.0, |s| match s.short() {
                    "netherite_pickaxe" => 9.0,
                    "diamond_pickaxe" => 8.0,
                    "iron_pickaxe" => 6.0,
                    "stone_pickaxe" => 4.0,
                    _ => 2.0,
                } + s.enchant("efficiency") as f64 * 2.0);
            // Ender chest: hardness 22.5; shulker 2.
            let hardness = if p.world.block(pos[0], pos[1], pos[2]).name.ends_with("ender_chest") { 22.5 } else { 2.0 };
            let ticks = ((hardness * 1.5 * 20.0) / speed / if pick.is_some() { 1.0 } else { 3.33 }).ceil() as u64;
            d.actions.push(Action::StartDig { pos, face: 1 });
            bot.regear = Regear::Mining { pos, until: tick + ticks.max(1) };
            true
        }
        Regear::Mining { pos, until } => {
            d.mode = "regear-mine";
            let at = Vec3::new(pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5);
            d.yaw = yaw_to(bot.body.eye(), at);
            d.pitch = pitch_to(bot.body.eye(), at);
            if tick >= until {
                d.actions.push(Action::StopDig { pos, face: 1 });
                release(bot, squad);
                bot.last_call = "regeared".into();
            } else if tick % 4 == 0 {
                d.actions.push(Action::Swing(Hand::Main));
            }
            true
        }
    }
}

fn release(bot: &mut Bot, squad: &mut Squad) {
    if let Some(f) = &bot.faction {
        squad.regearing.remove(f);
    }
    bot.regear = Regear::Idle;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::tests::{bot_at, me_with, perceive};

    #[test]
    fn regears_alone_from_its_ender_chest() {
        let w = crate::world::MapWorld::flat(63);
        let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
        let mut targets = std::collections::BTreeMap::new();
        targets.insert("minecraft:totem_of_undying".to_owned(), 4u32);
        bot.restock_targets = Some(targets);
        let me = me_with(&[(0, "ender_chest", 1), (1, "diamond_pickaxe", 1)]);
        let mut squad = Squad::default();
        let mut steps = Vec::new();
        for t in 0..400 {
            let p = perceive(t, &me, &[], &w);
            let mut d = Decision::default();
            if regear(&mut bot, &p, &mut d, None, &mut squad) {
                steps.push(d.mode);
                if d.actions.iter().any(|a| matches!(a, Action::StopDig { .. })) {
                    break;
                }
            }
        }
        for want in ["regear-place", "regear-open", "regear-take", "regear-mine"] {
            assert!(steps.contains(&want), "{want} missing from {steps:?}");
        }
        assert_eq!(bot.regear, Regear::Idle);
    }
}
