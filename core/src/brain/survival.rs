//! Staying alive: the water-bucket clutch, washing out a cobweb, totems (with a human pause),
//! healing (potions, gapples, buying room first), the shield, food, draining an enemy's water,
//! buff potions before a fight, best armour, and picking the good stuff out of a kill's drop.

use super::{Action, Bot, Busy, Decision, Event, Hand, Perception, Seen};
use crate::items;
use crate::math::{pitch_to, yaw_to, Vec3};
use crate::world::Fluid;

fn select(d: &mut Decision, cur: usize, slot: usize) {
    if cur != slot {
        d.actions.push(Action::SelectSlot(slot));
    }
}

impl Bot {
    /// Remember an item use of the bot's own. The glue never fills `SelfView::cooldowns`, so the
    /// brain tracks the ones it cares about: ender pearl 20 ticks, wind charge 10 ticks, and
    /// "shield" (disabled by an axe) 100 ticks. Must be called by whoever emits the throw/use.
    pub(crate) fn note_used(&mut self, item: &str, now: u64) {
        let (key, ticks) = match item.strip_prefix("minecraft:").unwrap_or(item) {
            "ender_pearl" => ("used:ender_pearl", 20),
            "wind_charge" => ("used:wind_charge", 10),
            "shield" => ("used:shield", 100),
            _ => return,
        };
        self.cool(key, now, ticks);
    }

    /// Ticks left on an item's cooldown: the larger of the engine's value and the tracked one.
    pub(crate) fn cooldown_left(&self, p: &Perception, item: &str) -> u32 {
        let key = match item.strip_prefix("minecraft:").unwrap_or(item) {
            "ender_pearl" => "used:ender_pearl",
            "wind_charge" => "used:wind_charge",
            "shield" => "used:shield",
            _ => return p.me.cooldown(item),
        };
        let tracked = self.ready_at.get(key).map_or(0, |t| t.saturating_sub(p.tick).min(u32::MAX as u64) as u32);
        p.me.cooldown(item).max(tracked)
    }
}

/// The MLG: falling far enough to get hurt, a bucket of water onto the landing spot inside the
/// window; picked back up once down. A slime block, cobweb, powder snow or hay without water.
pub fn clutch(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    let st = bot.settings.clone();
    if !st.b("water-clutch") || bot.body.on_ground || bot.body.vel.y >= -0.3 || bot.body.in_water {
        return false;
    }
    // A smash fall is the attack; no clutch on it.
    if bot.fight.mace_mode && bot.fight.target.is_some() {
        return false;
    }
    // Nothing usable in the hotbar: skip the (expensive) fall prediction.
    {
        let inv = &p.me.inventory;
        let usable = inv.find(true, |s| s.is("water_bucket")).is_some()
            || (st.b("block-clutch") && inv.find(true, |s| items::is_clutch_block(&s.id)).is_some())
            || (st.b("wc-self-reset") && bot.cooldown_left(p, "wind_charge") == 0 && inv.find(true, |s| s.is("wind_charge")).is_some());
        if !usable {
            return false;
        }
    }
    let (landing, ticks) = bot.body.predict_fall(p.world, 80);
    let Some(ticks) = ticks else { return false };
    let total_fall = bot.body.fall_distance + (bot.body.pos.y - landing.y);
    if total_fall < st.f("water-clutch-min-fall").max(3.5) {
        return false;
    }
    let above = bot.body.pos.y - landing.y;
    // The window widens with speed: near terminal velocity a tick covers several blocks.
    let window = st.f("water-clutch-window") + (-bot.body.vel.y) * 1.5;
    if above > window || ticks > 12 {
        return false;
    }
    let tick = p.tick;
    let inv = &p.me.inventory;
    let water = inv.find(true, |s| s.is("water_bucket"));
    let block = if water.is_none() && st.b("block-clutch") {
        inv.find(true, |s| items::is_clutch_block(&s.id))
    } else {
        None
    };
    let Some(slot) = water.or(block) else {
        // No water, no block: a wind charge under itself resets the fall (`wc-self-reset`).
        if st.b("wc-self-reset") && above < 4.0 && bot.cooldown_left(p, "wind_charge") == 0 {
            if let Some(wc) = p.me.inventory.find(true, |s| s.is("wind_charge")) {
                select(d, p.me.inventory.selected, wc);
                d.pitch = 90.0;
                d.actions.push(Action::Use(Hand::Main));
                bot.last_call = "wind charge under itself".into();
                d.mode = "wc-self-reset";
                bot.note_used("wind_charge", tick);
                return true;
            }
        }
        return false;
    };
    if !bot.ready("clutch-roll", tick) {
        return false;
    }
    bot.cool("clutch-roll", tick, 3);
    // Clutch skill: success rate (clutch knob), fumbleable.
    if !bot.rng.chance(st.knob("clutch")) {
        bot.last_call = "missed the clutch".into();
        bot.cool("clutch-roll", tick, 40);
        return false;
    }
    let ground = [landing.x.floor() as i32, (landing.y - 0.5).floor() as i32, landing.z.floor() as i32];
    select(d, inv.selected, slot);
    let face_point = Vec3::new(ground[0] as f64 + 0.5, ground[1] as f64 + 1.0, ground[2] as f64 + 0.5);
    d.yaw = yaw_to(bot.body.eye(), face_point);
    d.pitch = pitch_to(bot.body.eye(), face_point).max(60.0);
    d.actions.push(Action::UseOn { pos: ground, face: 1, hand: Hand::Main });
    if water.is_some() && st.b("water-clutch-pickup") {
        bot.busy = Some(Busy::WaterOut { pos: [ground[0], ground[1] + 1, ground[2]], since: tick });
    }
    bot.last_call = "clutch".into();
    d.mode = "clutch";
    true
}

/// Caught in a web with a water bucket: pour it (after noticing), maybe fumble; else cut it
/// with shears or a sword (`web-cut`).
pub fn web_escape(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    if !bot.body.in_web {
        bot.ready_at.remove("web-noticed");
        return false;
    }
    let tick = p.tick;
    let st = bot.settings.clone();
    if !bot.ready_at.contains_key("web-noticed") {
        let delay = st.i("web-escape-delay").max(0) as f64 * (1.0 + 0.5 * bot.rng.unit());
        bot.ready_at.insert("web-noticed", tick + delay as u64);
        return false;
    }
    if !bot.ready("web-noticed", tick) || !bot.ready("web-fumble", tick) {
        return false;
    }
    let inv = &p.me.inventory;
    let feet = bot.body.pos.block();
    if let Some(slot) = inv.find(true, |s| s.is("water_bucket")) {
        if bot.rng.chance(st.pct("web-escape-fumble-chance")) {
            let wait = 10 + bot.rng.int(0, 20) as u64;
            bot.cool("web-fumble", tick, wait);
            bot.last_call = "fumbled the web escape".into();
            return false;
        }
        select(d, inv.selected, slot);
        d.pitch = 90.0;
        d.actions.push(Action::UseOn { pos: [feet[0], feet[1] - 1, feet[2]], face: 1, hand: Hand::Main });
        bot.busy = Some(Busy::WaterOut { pos: feet, since: tick });
        d.mode = "web-escape";
        return true;
    }
    if st.b("web-cut") {
        let tool = inv.find(true, |s| s.is("shears")).or_else(|| inv.best_weapon(true, Some(items::WeaponKind::Sword)));
        if let Some(slot) = tool {
            // The web at its feet or head.
            let web = [feet, [feet[0], feet[1] + 1, feet[2]]]
                .into_iter()
                .find(|b| p.world.block(b[0], b[1], b[2]).cobweb);
            if let Some(pos) = web {
                select(d, inv.selected, slot);
                d.pitch = if pos[1] > feet[1] { -10.0 } else { 70.0 };
                d.actions.push(Action::StartDig { pos, face: 1 });
                bot.busy = Some(Busy::Digging { pos, until: tick + 3 });
                d.mode = "web-cut";
                return true;
            }
        }
    }
    false
}

/// Totems: none held permanently; at `totem-health` one goes to the off hand after a short,
/// human pause, unless the panic roll loses it. `offhand: totem` keeps one there always.
pub fn totem(bot: &mut Bot, p: &Perception, d: &mut Decision) {
    let st = bot.settings.clone();
    if !st.b("auto-totem") {
        return;
    }
    let tick = p.tick;
    let inv = &p.me.inventory;
    if inv.offhand_is("totem_of_undying") {
        bot.totem_at = None;
        return;
    }
    let mode = st.s("offhand");
    let low = (p.me.health as f64) <= st.f("totem-health");
    let hearts_low = (p.me.health as f64) / 2.0 <= st.f("offhand-totem-hearts");
    let mace_threat = st.b("mace-totem") && bot.fight.mace_totem == Some(true);
    let want = match mode.as_str() {
        "totem" => true,
        "shield" => hearts_low,
        _ => low || mace_threat,
    };
    if !want {
        bot.totem_at = None;
        return;
    }
    let Some(slot) = inv.find(false, |s| s.is("totem_of_undying")) else { return };
    match bot.totem_at {
        None => {
            let delay = st.i("totem-delay-ticks").max(0) as u64 + bot.rng.int(0, 4) as u64;
            bot.totem_at = Some(tick + delay);
        }
        Some(at) if tick >= at => {
            if !bot.clutch_roll("totem", tick) {
                bot.totem_at = Some(tick + 4);
                return;
            }
            if slot < 9 {
                select(d, inv.selected, slot);
                d.actions.push(Action::SwapHands);
            } else {
                d.actions.push(Action::ToOffhand(slot));
            }
            bot.totem_at = None;
            bot.last_call = "totem up".into();
        }
        _ => {}
    }
}

/// Keeps an eat/drink/draw going, and picks the clutch water back up. True while the hands are
/// busy and nothing else should be clicked.
pub fn continue_busy(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    let tick = p.tick;
    match bot.busy.clone() {
        Some(Busy::Using { until, what }) => {
            if tick >= until {
                bot.busy = None;
                return false;
            }
            d.mode = what;
            // Back away slowly from the nearest threat while eating.
            if let Some(t) = bot.fight.target.and_then(|id| p.get(id)).cloned() {
                let away = bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 4.0;
                bot.look_at(d, t.eye());
                bot.walk_to(p, d, away, false);
            }
            true
        }
        Some(Busy::WaterOut { pos, since }) => {
            if !bot.body.on_ground && !bot.body.in_water && tick < since + 40 {
                return false;
            }
            let inv = &p.me.inventory;
            let b = p.world.block(pos[0], pos[1], pos[2]);
            if b.fluid != Fluid::Water || tick > since + 60 {
                bot.busy = None;
                return false;
            }
            if let Some(slot) = inv.find(true, |s| s.is("bucket")) {
                select(d, inv.selected, slot);
                let at = Vec3::new(pos[0] as f64 + 0.5, pos[1] as f64 + 0.2, pos[2] as f64 + 0.5);
                d.yaw = yaw_to(bot.body.eye(), at);
                d.pitch = pitch_to(bot.body.eye(), at);
                d.actions.push(Action::UseOn { pos, face: 1, hand: Hand::Main });
                d.mode = "water-pickup";
            }
            bot.busy = None;
            true
        }
        Some(Busy::Digging { pos, until }) => {
            if tick >= until {
                d.actions.push(Action::StopDig { pos, face: 1 });
                bot.busy = None;
            }
            d.mode = "digging";
            true
        }
        Some(Busy::Blocking { since }) => {
            // Hurt by someone holding an axe while blocking: the shield is disabled for 5 s.
            for e in p.events {
                if let Event::Hurt { by, .. } = e {
                    if *by != 0 && p.others.iter().any(|o| o.id == *by && o.holds("axe")) {
                        bot.note_used("shield", tick);
                    }
                }
            }
            // Lowered by heal() or fight when it is time; a shield is held at most 2 s.
            if tick > since + 40 {
                d.actions.push(Action::Release);
                bot.busy = None;
            }
            false
        }
        Some(Busy::Drawing { .. }) | None => false,
    }
}

/// Every `check-interval` ticks: the best armour carried goes on.
pub fn auto_equip(bot: &mut Bot, p: &Perception, d: &mut Decision) {
    let st = &bot.settings;
    if !st.b("auto-equip") || p.tick % st.i("check-interval").max(1) as u64 != 0 || bot.hidden_with_potion {
        return;
    }
    if let Some((_, slot)) = p.me.inventory.armor_upgrades().first() {
        d.actions.push(Action::Wear(*slot));
    }
}

fn start_use(bot: &mut Bot, d: &mut Decision, tick: u64, hand: Hand, ticks: u64, what: &'static str) {
    d.actions.push(Action::Use(hand));
    bot.busy = Some(Busy::Using { until: tick + ticks, what });
    d.mode = what;
}

/// Healing when low: splash healing at its feet, a healing potion, a gapple; with someone on
/// top of it (advanced healing) it buys room first. Also the shield when hurt and pressed.
pub fn heal(bot: &mut Bot, p: &Perception, d: &mut Decision, threat: Option<&Seen>) -> bool {
    let st = bot.settings.clone();
    let tick = p.tick;
    let inv = &p.me.inventory;
    let temper = super::personality::Temper::of(bot.playstyle, 0.0);
    let hp = p.me.health as f64;
    let close = threat.map(|t| bot.reach_to(t)).unwrap_or(99.0);

    // The shield against a mace coming down on it, or when hurt and pressed.
    if st.b("auto-shield") && inv.offhand_is("shield") && bot.cooldown_left(p, "shield") == 0 {
        let smash = threat.is_some_and(|t| t.holds("mace") && t.vel.y < -0.4 && t.pos.y > bot.body.pos.y + 2.0 && close < 5.0);
        let pressed = hp <= st.f("shield-health") && close < 3.5 && threat.is_some_and(|t| !t.blocking);
        // A jump-crit windup read from how they fight: the shield goes up early.
        let windup = st.b("shield-read-windup") && close < 3.5 && bot.fight.reads_crits > 0.2 && threat.is_some_and(|t| t.vel.y > 0.3);
        let mut raise = false;
        if smash && bot.ready("mace-defence", tick) {
            bot.cool("mace-defence", tick, 20);
            let r = bot.rng.unit() * 100.0;
            if bot.fight.mace_totem != Some(true) && r < st.f("mace-defence-shield") {
                raise = true;
            } else if r < st.f("mace-defence-shield") + st.f("mace-defence-flee") {
                // Get out from under it.
                if let Some(t) = threat {
                    let away = bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 3.0;
                    bot.walk_to(p, d, away, true);
                }
            }
        } else if pressed || windup {
            // A short reaction before the shield comes up (a quarter of the reaction time, at
            // least one tick), as a player needs to see the threat first.
            match bot.ready_at.get("shield-react").copied() {
                None => {
                    let wait = (bot.reaction_ticks() / 4).max(1);
                    bot.cool("shield-react", tick, wait);
                }
                Some(at) if tick >= at && bot.rng.chance(0.4 + temper.block) => raise = true,
                _ => {}
            }
        } else {
            bot.ready_at.remove("shield-react");
        }
        if raise && !matches!(bot.busy, Some(Busy::Blocking { .. })) && bot.clutch_roll("shield", tick) {
            d.actions.push(Action::Use(Hand::Off));
            bot.busy = Some(Busy::Blocking { since: tick });
            bot.ready_at.remove("shield-react");
            d.mode = "blocking";
            return false;
        }
    }

    if !st.b("auto-potion") || hp > st.f("potion-health") + temper.heal_shift {
        return false;
    }
    let gap = (st.f("potion-gap") * 20.0) as u64;
    if !bot.ready("heal", tick) {
        return false;
    }
    // Advanced healing: buy the gap first with someone on top of it.
    if st.s("healing") == "advanced" && close < 3.0 && !temper.heal_in_place {
        if let Some(t) = threat {
            if bot.fight.since_swing >= 10 && bot.reach_to(t) <= 3.0 {
                bot.look_at(d, t.eye());
                bot.send_late(tick, Action::Attack(t.id));
                bot.send_late(tick, Action::Swing(Hand::Main));
                bot.fight.since_swing = 0;
                d.mode = "buying-room";
                return true;
            }
            let away = bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 10.0;
            if super::combat::pearl(bot, p, d, away, "pearl-to-heal") {
                return true;
            }
        }
    }
    if !bot.clutch_roll("heal", tick) {
        return false;
    }
    if let Some(t) = threat {
        if close < 5.0 && super::techniques::block_off(bot, p, d, t) {
            return true;
        }
    }
    let show = st.b("show-items");
    // Splash healing at its own feet: instant.
    if let Some(slot) = inv.find(true, |s| s.is("splash_potion") && items::is_healing_potion(s)) {
        select(d, inv.selected, slot);
        d.pitch = 90.0;
        d.actions.push(Action::Use(Hand::Main));
        bot.cool("heal", tick, gap.max(10));
        d.mode = "splash-heal";
        return true;
    }
    if let Some(slot) = inv.find(true, |s| s.is("potion") && items::is_healing_potion(s)) {
        select(d, inv.selected, slot);
        if show && inv.selected != slot {
            // Up in its hand a moment before it is used.
            bot.cool("heal", tick, 3);
            return true;
        }
        start_use(bot, d, tick, Hand::Main, 32, "drinking");
        bot.cool("heal", tick, gap + 32);
        return true;
    }
    // A plain gapple before an enchanted one.
    let gapple = inv
        .find(true, |s| s.is("golden_apple"))
        .or_else(|| inv.find(true, |s| s.is("enchanted_golden_apple")));
    if let Some(slot) = gapple {
        select(d, inv.selected, slot);
        start_use(bot, d, tick, Hand::Main, 32, "eating-gapple");
        bot.cool("heal", tick, gap + 32);
        return true;
    }
    false
}

/// Eating when hungry (best food first), never past being fed; at three bars it stops whatever
/// it is doing (`hunger-stop`).
pub fn eat(bot: &mut Bot, p: &Perception, d: &mut Decision, threat: Option<&Seen>) -> bool {
    let st = &bot.settings;
    if !st.b("auto-food") || p.me.food as i64 >= st.i("food-threshold") || p.me.food >= 20 {
        return false;
    }
    let busy_fight = threat.is_some_and(|t| t.pos.distance(bot.body.pos) < 6.0);
    if busy_fight && !(st.b("hunger-stop") && p.me.food <= 6) {
        return false;
    }
    let inv = &p.me.inventory;
    let best = (0..9)
        .filter(|&i| inv.slots[i].as_ref().is_some_and(|s| items::is_plain_food(&s.id)))
        .max_by(|&a, &b| {
            let fa = items::food(&inv.slots[a].as_ref().unwrap().id).map_or(0.0, |(n, s)| n as f64 + s);
            let fb = items::food(&inv.slots[b].as_ref().unwrap().id).map_or(0.0, |(n, s)| n as f64 + s);
            fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
        });
    let Some(slot) = best else { return false };
    let tick = p.tick;
    select(d, inv.selected, slot);
    start_use(bot, d, tick, Hand::Main, 32, "eating");
    true
}

/// Scoops up water an enemy placed (their MLG or safe spot), after a beatable delay.
pub fn drain(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    let st = bot.settings.clone();
    if !st.b("drain") {
        return false;
    }
    let tick = p.tick;
    for e in p.events {
        if let Event::WaterPlaced { pos, by } = e {
            if Some(*by) == bot.fight.target && bot.drain_at.is_none() {
                bot.drain_at = Some((tick + st.i("drain-delay").max(0) as u64, *pos));
            }
        }
    }
    let Some((at, pos)) = bot.drain_at else { return false };
    if tick < at {
        return false;
    }
    bot.drain_at = None;
    if p.world.block(pos[0], pos[1], pos[2]).fluid != Fluid::Water || !bot.rng.chance(st.pct("web-drain-chance")) {
        return false;
    }
    let center = Vec3::new(pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5);
    if center.distance(bot.body.eye()) > 4.5 {
        return false;
    }
    let inv = &p.me.inventory;
    let Some(slot) = inv.find(true, |s| s.is("bucket")) else { return false };
    select(d, inv.selected, slot);
    d.yaw = yaw_to(bot.body.eye(), center);
    d.pitch = pitch_to(bot.body.eye(), center);
    d.actions.push(Action::UseOn { pos, face: 1, hand: Hand::Main });
    bot.last_call = "drained their water".into();
    d.mode = "drain";
    true
}

/// Strength, speed and fire resistance before engaging, only with room to spare.
pub fn buff(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    let st = &bot.settings;
    if !st.b("buff-potions") {
        return false;
    }
    // Someone it knows of, not too close: a fight is coming.
    let coming = bot
        .known
        .keys()
        .filter_map(|id| p.get(*id))
        .any(|s| (6.0..24.0).contains(&s.pos.distance(bot.body.pos)) && s.is_player);
    if !coming {
        return false;
    }
    let inv = &p.me.inventory;
    let slot = (0..9).find(|&i| {
        inv.slots[i].as_ref().is_some_and(|s| {
            items::buff_effect(s).is_some_and(|e| e != "invisibility" && e != "slow_falling" && !p.me.has_effect(e))
        })
    });
    let Some(slot) = slot else { return false };
    let tick = p.tick;
    if !bot.ready("buff", tick) {
        return false;
    }
    bot.cool("buff", tick, 100);
    select(d, inv.selected, slot);
    start_use(bot, d, tick, Hand::Main, 32, "buffing");
    true
}

/// `hide`: with an invisibility potion it uses it and takes its armour off (it would show
/// floating); without one it crouches (done in `finish`).
pub fn hide(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    if !bot.orders.hide || bot.hidden_with_potion || p.me.has_effect("invisibility") {
        return false;
    }
    let inv = &p.me.inventory;
    let Some(slot) = inv.find(true, items::is_invisibility) else { return false };
    let tick = p.tick;
    select(d, inv.selected, slot);
    if inv.slots[slot].as_ref().is_some_and(|s| s.is("splash_potion") || s.is("lingering_potion")) {
        d.pitch = 90.0;
        d.actions.push(Action::Use(Hand::Main));
    } else {
        start_use(bot, d, tick, Hand::Main, 32, "drinking-invisibility");
    }
    for (i, a) in inv.armor.iter().enumerate() {
        if a.is_some() {
            let slot = [items::ArmorSlot::Feet, items::ArmorSlot::Legs, items::ArmorSlot::Chest, items::ArmorSlot::Head][i];
            d.actions.push(Action::Unwear(slot));
        }
    }
    bot.hidden_with_potion = true;
    d.mode = "hiding";
    true
}

/// After a kill: some crouch over the body; most pick totems, pearls, gapples and potions out of
/// the drop (walking over them picks them up).
pub fn after_fight(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    if !bot.settings.b("after-fight") {
        return false;
    }
    let good = |i: &str| {
        let s = i.strip_prefix("minecraft:").unwrap_or(i);
        matches!(s, "totem_of_undying" | "ender_pearl" | "golden_apple" | "enchanted_golden_apple" | "splash_potion" | "potion" | "experience_bottle")
    };
    let near = p
        .ground
        .iter()
        .filter(|g| good(&g.item) && g.pos.distance(bot.body.pos) < 8.0)
        .min_by(|a, b| a.pos.distance(bot.body.pos).partial_cmp(&b.pos.distance(bot.body.pos)).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(g) = near {
        d.mode = "looting";
        let at = g.pos;
        d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, at), 20.0);
        bot.walk_to(p, d, at, false);
        if at.distance(bot.body.pos) < 1.5 && bot.rng.chance(0.3) {
            d.input.sneak = bot.personality.rush < -0.3;
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_clutch_fires_inside_the_window() {
        use crate::brain::tests::{bot_at, perceive};
        let mut w = crate::world::MapWorld::flat(40);
        w.floor = Some(40);
        let mut bot = bot_at(Vec3::new(0.5, 70.0, 0.5));
        bot.body.vel.y = -1.0;
        bot.body.fall_distance = 20.0;
        let mut me = crate::brain::tests::me_with(&[(0, "water_bucket", 1)]);
        me.health = 20.0;
        let mut squad = crate::brain::squad::Squad::default();
        let mut fired = None;
        for t in 0..40 {
            let p = perceive(t, &me, &[], &w);
            let dcs = bot.tick(&p, &mut squad);
            if dcs.actions.iter().any(|a| matches!(a, Action::UseOn { face: 1, .. })) {
                fired = Some(bot.body.pos.y);
                break;
            }
            me.health = 20.0;
        }
        let y = fired.expect("poured");
        assert!(y - 41.0 < 9.0, "too early: {y}");
    }

    #[test]
    fn clutch_skips_the_fall_prediction_without_items() {
        use crate::brain::tests::{bot_at, me_with, perceive};
        let mut w = crate::world::MapWorld::flat(40);
        w.floor = Some(40);
        let mut bot = bot_at(Vec3::new(0.5, 70.0, 0.5));
        bot.body.vel.y = -1.0;
        bot.body.fall_distance = 20.0;
        let me = me_with(&[(0, "stone_sword", 1)]);
        let p = perceive(1, &me, &[], &w);
        let mut d = Decision::default();
        assert!(!clutch(&mut bot, &p, &mut d));
        assert!(d.actions.is_empty());
    }

    #[test]
    fn tracked_cooldowns_merge_with_the_engines() {
        use crate::brain::tests::{bot_at, me_with, perceive};
        let w = crate::world::MapWorld::flat(40);
        let mut bot = bot_at(Vec3::new(0.5, 70.0, 0.5));
        let mut me = me_with(&[]);
        bot.note_used("minecraft:ender_pearl", 100);
        bot.note_used("wind_charge", 100);
        bot.note_used("shield", 100);
        bot.note_used("apple", 100);
        let p = perceive(105, &me, &[], &w);
        assert_eq!(bot.cooldown_left(&p, "ender_pearl"), 15);
        assert_eq!(bot.cooldown_left(&p, "wind_charge"), 5);
        assert_eq!(bot.cooldown_left(&p, "shield"), 95);
        let p = perceive(125, &me, &[], &w);
        assert_eq!(bot.cooldown_left(&p, "ender_pearl"), 0);
        assert_eq!(bot.cooldown_left(&p, "wind_charge"), 0);
        // An engine value larger than the tracked one wins.
        me.cooldowns.insert(crate::items::full("ender_pearl"), 7);
        let p = perceive(125, &me, &[], &w);
        assert_eq!(bot.cooldown_left(&p, "ender_pearl"), 7);
    }

    #[test]
    fn an_axe_hit_while_blocking_disables_the_shield() {
        use crate::brain::tests::{bot_at, me_with, perceive, player};
        let w = crate::world::MapWorld::flat(40);
        let mut bot = bot_at(Vec3::new(0.5, 70.0, 0.5));
        bot.busy = Some(Busy::Blocking { since: 50 });
        let me = me_with(&[]);
        let mut foe = player(2, "Foe", Vec3::new(1.5, 70.0, 0.5));
        foe.held = Some("minecraft:diamond_axe".into());
        let others = [foe];
        let events = [Event::Hurt { by: 2, amount: 9.0, kind: "player".into() }];
        let mut p = perceive(60, &me, &others, &w);
        p.events = &events;
        let mut d = Decision::default();
        continue_busy(&mut bot, &p, &mut d);
        assert_eq!(bot.cooldown_left(&p, "shield"), 100);
    }
}
