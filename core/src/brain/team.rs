//! Team play and the remaining habits: peeling for a nearly dead ally, splash heals thrown at a
//! hurt ally, army roles (archers hang back, flankers come round), the panic-or-lock-in at three
//! hearts, combo answers, the damage model's weapon pick, fire and chorus escapes, Mending
//! bottles, and breaking an enemy's stash.

use super::squad::Squad;
use super::{Action, Bot, Decision, Hand, Perception, Seen};
use crate::factions::Relation;
use crate::items::{self, Stack, WeaponKind};
use crate::math::{pitch_to, yaw_to, Vec3};

/// An ally (same faction or allied) close by and nearly dead, and who is on them.
pub fn peel_target(bot: &Bot, p: &Perception, squad: &Squad) -> Option<Seen> {
    if !bot.settings.b("peel") {
        return None;
    }
    let my = bot.faction.as_deref()?;
    let ally = p.others.iter().filter(|s| {
        s.health > 0.0
            && s.health <= 6.0
            && s.pos.distance(bot.body.pos) < 12.0
            && matches!(squad.relation(Some(my), s.faction.as_deref()), Relation::Same | Relation::Allied)
    });
    for a in ally {
        // The enemy closest to the ally.
        let attacker = p
            .others
            .iter()
            .filter(|e| e.health > 0.0 && e.pos.distance(a.pos) < 4.0 && squad.relation(Some(my), e.faction.as_deref()) == Relation::War)
            .min_by(|x, y| x.pos.distance(a.pos).partial_cmp(&y.pos.distance(a.pos)).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(e) = attacker {
            return Some(e.clone());
        }
    }
    None
}

/// Splash healing thrown at a hurt ally close by.
pub fn heal_ally(bot: &mut Bot, p: &Perception, d: &mut Decision, squad: &Squad) -> bool {
    if !bot.settings.b("heal-allies") || !bot.ready("heal-ally", p.tick) {
        return false;
    }
    let Some(my) = bot.faction.clone() else { return false };
    let Some(slot) = p.me.inventory.find(true, |s| s.is("splash_potion") && items::is_healing_potion(s)) else { return false };
    // Keep one for itself.
    if p.me.inventory.count(|s| s.is("splash_potion") && items::is_healing_potion(s)) < 2 {
        return false;
    }
    let hurt = p.others.iter().find(|s| {
        s.health > 0.0
            && s.health <= 10.0
            && (2.0..6.0).contains(&s.pos.distance(bot.body.pos))
            && matches!(squad.relation(Some(&my), s.faction.as_deref()), Relation::Same | Relation::Allied)
    });
    let Some(a) = hurt else { return false };
    let at = a.pos + Vec3::new(0.0, 0.5, 0.0);
    d.actions.push(Action::SelectSlot(slot));
    d.yaw = yaw_to(bot.body.eye(), at);
    d.pitch = pitch_to(bot.body.eye(), at);
    d.actions.push(Action::Use(Hand::Main));
    bot.cool("heal-ally", p.tick, 100);
    bot.last_call = format!("healed {}", a.name);
    d.mode = "heal-ally";
    true
}

/// A bot's role in an army, from what it carries: an archer hangs back, a pearl carrier flanks,
/// a healer stays by the hurt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Fighter,
    Archer,
    Flanker,
    Healer,
}

pub fn role(bot: &Bot, p: &Perception) -> Role {
    if !bot.settings.b("roles") || bot.faction.is_none() {
        return Role::Fighter;
    }
    let inv = &p.me.inventory;
    let has = |f: &dyn Fn(&Stack) -> bool| inv.find(false, f).is_some();
    if has(&|s| s.is("bow") || s.is("crossbow")) && inv.has("arrow") && inv.best_weapon(false, None).is_none() {
        Role::Archer
    } else if inv.count(|s| s.is("splash_potion") && items::is_healing_potion(s)) >= 4 {
        Role::Healer
    } else if inv.count_id("ender_pearl") >= 8 {
        Role::Flanker
    } else {
        Role::Fighter
    }
}

/// Where a flanker comes in from: round the side of the target, away from the allies' line.
pub fn flank_point(bot: &Bot, t: &Seen) -> Vec3 {
    let side = if bot.personality.circle_left { 1.0 } else { -1.0 };
    let line = (t.pos - bot.body.pos).horizontal().normalize();
    let perp = Vec3::new(-line.z, 0.0, line.x) * side;
    t.pos + perp * 4.0
}

/// At three hearts: lock in (steadier) or panic (shaky aim, jumpy, strafing all over).
pub fn panic(bot: &mut Bot, p: &Perception, d: &mut Decision) {
    if !bot.settings.b("panic") || p.me.health > 6.0 {
        bot.ready_at.remove("panic-mode");
        return;
    }
    let mode = *bot.ready_at.entry("panic-mode").or_insert_with(|| 0);
    let mode = if mode == 0 {
        let lock_in = bot.rng.chance(bot.personality.nerve);
        let m = if lock_in { 1 } else { 2 };
        bot.ready_at.insert("panic-mode", m);
        bot.last_call = if lock_in { "locked in".into() } else { "panicking".into() };
        m
    } else {
        mode
    };
    if mode == 2 {
        d.yaw += bot.rng.gauss() as f32 * 6.0;
        d.pitch = (d.pitch + bot.rng.gauss() as f32 * 3.0).clamp(-90.0, 90.0);
        if bot.body.on_ground && bot.rng.chance(0.2) {
            d.input.jump = true;
        }
        if bot.rng.chance(0.15) {
            d.input.strafe = -d.input.strafe;
        }
    }
}

/// Being chained: answer with a crit, and pearl out once the exchange is clearly lost.
pub fn combo_answer(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen) -> bool {
    if !bot.settings.b("combo-answers") || bot.fight.taken_streak < 3 {
        return false;
    }
    let losing = (p.me.health as f64) < (t.health as f64) * 0.5;
    if losing && bot.settings.b("pearls") {
        let away = bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 12.0;
        if super::combat::pearl(bot, p, d, away, "pearl-out-of-combo") {
            bot.fight.taken_streak = 0;
            return true;
        }
    }
    if bot.body.on_ground {
        d.input.jump = true;
        d.input.sprint = false;
    }
    false
}

/// The damage model: the weapon that does the most through the target's armour, per second.
pub fn best_against(p: &Perception, t: &Seen, hotbar_only: bool) -> Option<usize> {
    let inv = &p.me.inventory;
    let end = if hotbar_only { 9 } else { 36 };
    (0..end)
        .filter(|&i| inv.slots[i].as_ref().is_some_and(|s| !matches!(items::weapon(&s.id).kind, WeaponKind::Hand | WeaponKind::Other)))
        .map(|i| {
            let s = inv.slots[i].as_ref().unwrap();
            let w = items::weapon(&s.id);
            let through = items::damage_after_armor(items::melee_damage(Some(s)), t.armor, 0.0, 0);
            (i, through * w.speed.min(1.6))
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
}

/// Burning with no fire resistance: water on its own feet, then the water back (`fire-escape`).
pub fn fire_escape(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    if !bot.settings.b("fire-escape") || !p.me.on_fire || p.me.has_effect("fire_resistance") || bot.body.in_water {
        return false;
    }
    let Some(slot) = p.me.inventory.find(true, |s| s.is("water_bucket")) else { return false };
    let feet = bot.body.pos.block();
    d.actions.push(Action::SelectSlot(slot));
    d.pitch = 90.0;
    d.actions.push(Action::UseOn { pos: [feet[0], feet[1] - 1, feet[2]], face: 1, hand: Hand::Main });
    bot.busy = Some(super::Busy::WaterOut { pos: feet, since: p.tick });
    d.mode = "fire-escape";
    true
}

/// Webbed and boxed in: a chorus fruit to teleport out (`chorus-escape`).
pub fn chorus_escape(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    if !bot.settings.b("chorus-escape") || !bot.body.in_web || p.me.inventory.has("water_bucket") {
        return false;
    }
    let Some(slot) = p.me.inventory.find(true, |s| s.is("chorus_fruit")) else { return false };
    d.actions.push(Action::SelectSlot(slot));
    d.actions.push(Action::Use(Hand::Main));
    bot.busy = Some(super::Busy::Using { until: p.tick + 32, what: "chorus" });
    d.mode = "chorus-escape";
    true
}

/// Worn gear with Mending and a bottle o' enchanting: thrown at its feet (`auto-repair-bottles`).
pub fn repair(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    let st = &bot.settings;
    if !st.b("auto-repair") || !bot.ready("repair", p.tick) {
        return false;
    }
    let inv = &p.me.inventory;
    let worn = inv.armor.iter().flatten().any(|s| s.enchant("mending") > 0 && s.durability_left() < 0.5);
    if !worn {
        return false;
    }
    // XP orbs on the ground first (walking over them repairs).
    if let Some(orb) = p.ground.iter().find(|g| g.item == "minecraft:experience_orb" && g.pos.distance(bot.body.pos) < 8.0) {
        let at = orb.pos;
        bot.walk_to(p, d, at, false);
        d.mode = "repair-orbs";
        return true;
    }
    if !st.b("auto-repair-bottles") {
        return false;
    }
    let Some(slot) = inv.find(true, |s| s.is("experience_bottle")) else { return false };
    d.actions.push(Action::SelectSlot(slot));
    d.pitch = 90.0;
    d.actions.push(Action::Use(Hand::Main));
    bot.cool("repair", p.tick, 6);
    d.mode = "repair";
    true
}

/// An enemy's placed shulker box or ender chest near the fight: broken to deny the restock.
pub fn break_stash(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen) -> bool {
    if !bot.settings.b("break-stashes") || !bot.ready("stash", p.tick) {
        return false;
    }
    let c = t.pos.block();
    for dx in -2..=2 {
        for dz in -2..=2 {
            for dy in -1..=1 {
                let pos = [c[0] + dx, c[1] + dy, c[2] + dz];
                let b = p.world.block(pos[0], pos[1], pos[2]);
                if b.name.ends_with("shulker_box") || b.name.ends_with("ender_chest") {
                    let at = Vec3::new(pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5);
                    if at.distance(bot.body.eye()) > 4.5 {
                        bot.walk_to(p, d, at, true);
                        d.mode = "to-stash";
                        return true;
                    }
                    if let Some(pk) = p.me.inventory.find(true, |s| s.short().ends_with("_pickaxe")) {
                        d.actions.push(Action::SelectSlot(pk));
                    }
                    d.yaw = yaw_to(bot.body.eye(), at);
                    d.pitch = pitch_to(bot.body.eye(), at);
                    d.actions.push(Action::StartDig { pos, face: 1 });
                    bot.busy = Some(super::Busy::Digging { pos, until: p.tick + 25 });
                    bot.cool("stash", p.tick, 60);
                    bot.last_call = "breaking their stash".into();
                    d.mode = "break-stash";
                    return true;
                }
            }
        }
    }
    bot.cool("stash", p.tick, 20);
    false
}

/// Sweep: drop sprint for vanilla's sweep with two or more enemies close (`sweep-hit`).
pub fn sweep_ready(bot: &Bot, p: &Perception, squad: &Squad) -> bool {
    if !bot.settings.b("sweep-hit") {
        return false;
    }
    let close = p
        .others
        .iter()
        .filter(|s| s.health > 0.0 && s.pos.distance(bot.body.pos) < 3.5)
        .filter(|s| !matches!(squad.relation(bot.faction.as_deref(), s.faction.as_deref()), Relation::Same | Relation::Allied))
        .count();
    // Ally care: no sweep into an ally.
    let ally_close = bot.settings.b("ally-care")
        && p.others.iter().any(|s| {
            s.pos.distance(bot.body.pos) < 3.5
                && matches!(squad.relation(bot.faction.as_deref(), s.faction.as_deref()), Relation::Same | Relation::Allied)
                && bot.faction.is_some()
        });
    close >= 2 && !ally_close
}

/// Ally care: an ally near the line from the bot's eyes to the target (no arrows through them).
pub fn ally_in_line(bot: &Bot, p: &Perception, squad: &Squad, to: Vec3) -> bool {
    if !bot.settings.b("ally-care") || bot.faction.is_none() {
        return false;
    }
    let from = bot.body.eye();
    let line = to - from;
    let len = line.length().max(1e-6);
    p.others.iter().any(|s| {
        // Close enough to matter at all (the line is under 1 block from them), before the relation.
        if s.pos.distance(from) > len + 2.5 || !matches!(squad.relation(bot.faction.as_deref(), s.faction.as_deref()), Relation::Same | Relation::Allied) {
            return false;
        }
        let rel = s.eye() - from;
        let t = (rel.dot(line) / (len * len)).clamp(0.0, 1.0);
        let closest = from + line * t;
        t > 0.05 && t < 0.95 && closest.distance(s.pos + Vec3::new(0.0, s.height * 0.5, 0.0)) < 1.0
    })
}

/// Ally care: a step away from an ally standing inside it.
pub fn separation(bot: &Bot, p: &Perception, squad: &Squad) -> Option<Vec3> {
    if !bot.settings.b("ally-care") {
        return None;
    }
    let crowd = p.others.iter().find(|s| {
        s.pos.horizontal_distance(bot.body.pos) < 0.7
            && (s.is_bot || matches!(squad.relation(bot.faction.as_deref(), s.faction.as_deref()), Relation::Same | Relation::Allied))
    })?;
    let away = (bot.body.pos - crowd.pos).horizontal();
    let away = if away.length() < 1e-3 { Vec3::new(1.0, 0.0, 0.0) } else { away.normalize() };
    Some(bot.body.pos + away)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::tests::{bot_at, me_with, perceive, player};

    #[test]
    fn the_damage_model_picks_through_armour() {
        let w = crate::world::MapWorld::flat(63);
        let me = me_with(&[(0, "diamond_sword", 1), (1, "netherite_axe", 1)]);
        let mut t = player(2, "Steve", Vec3::ZERO);
        let others = [t.clone()];
        let p = perceive(1, &me, &others, &w);
        // Bare skin: the faster sword does more a second; full diamond: the heavier axe does.
        assert_eq!(best_against(&p, &t, true), Some(0));
        t.armor = 20.0;
        assert_eq!(best_against(&p, &t, true), Some(1));
    }

    #[test]
    fn peels_for_a_low_ally() {
        let w = crate::world::MapWorld::flat(63);
        let mut bot = bot_at(Vec3::new(0.0, 64.0, 0.0));
        bot.faction = Some("red".into());
        let mut squad = Squad::default();
        squad.factions.create("red").unwrap();
        squad.factions.create("blue").unwrap();
        squad.factions.hostile("red", "blue").unwrap();
        let mut ally = player(2, "Ally", Vec3::new(5.0, 64.0, 0.0));
        ally.faction = Some("red".into());
        ally.health = 4.0;
        let mut enemy = player(3, "Enemy", Vec3::new(6.0, 64.0, 0.0));
        enemy.faction = Some("blue".into());
        let me = me_with(&[]);
        let others = [ally, enemy];
        let p = perceive(1, &me, &others, &w);
        assert_eq!(peel_target(&bot, &p, &squad).map(|s| s.id), Some(3));
    }
}
