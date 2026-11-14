//! Picking a fight and fighting it: detection and target choice, the opening walk-in, spacing
//! and strafing, jump crits, shields and axes, the mace (wind-charge launch and smash), bows with
//! lead prediction, pearls, cobwebs, thrown potions; and the orders that move a bot: retreat,
//! tether, march, guard, follow, patrol, wander.

use super::formation;
use super::squad::Squad;
use super::{Action, Bot, Busy, Decision, Hand, MarchTo, Perception, RetreatTo, Seen};
use crate::factions::Relation;
use crate::items::{self, WeaponKind};
use crate::math::{look_vector, pitch_to, wrap_degrees, yaw_to, Vec3};

/// Whether the bot may hit this one at all (relations, game mode, friendly fire).
fn hittable(bot: &Bot, s: &Seen, squad: &Squad) -> bool {
    if s.health <= 0.0 || s.uuid == bot.uuid {
        return false;
    }
    if s.is_player && s.invulnerable_mode() && !bot.settings.b("attack-invincible") {
        return false;
    }
    if bot.orders.owner.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(&s.name)) {
        return false;
    }
    let rel = squad.relation(bot.faction.as_deref(), s.faction.as_deref());
    !(matches!(rel, Relation::Same | Relation::Allied) && !bot.settings.b("friendly-fire"))
}

/// The notice range against `s`: sneaking, invisibility and armour change it for players.
fn notice_range(bot: &Bot, s: &Seen) -> f64 {
    let st = &bot.settings;
    let mut r = st.f("max-target-distance");
    if s.is_player {
        if s.invisible {
            r *= st.f("detection-invisible");
        }
        if s.sneaking {
            r *= st.f("detection-sneaking");
        }
        r *= if s.armor > 0.0 { st.f("detection-armoured") } else { st.f("detection-unarmoured") };
    }
    r
}

/// What the rules that need no block read say about noticing `s`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Notice {
    Yes,
    No,
    /// In range and in the field of view: the line of sight decides.
    IfSeen,
}

fn notice_cheaply(bot: &Bot, s: &Seen) -> Notice {
    let st = &bot.settings;
    let d = s.pos.distance(bot.body.pos);
    if d > st.f("max-target-distance") {
        return Notice::No;
    }
    if !st.b("detection") {
        return Notice::Yes;
    }
    if d <= st.f("detection-touch-radius") || bot.known.contains_key(&s.id) {
        return Notice::Yes;
    }
    if d > notice_range(bot, s) {
        return Notice::No;
    }
    let fov = st.f("fov");
    if fov < 360.0 {
        let to = yaw_to(bot.body.pos, s.pos);
        if wrap_degrees(to - bot.body.yaw).abs() as f64 > fov / 2.0 {
            return Notice::No;
        }
    }
    Notice::IfSeen
}

/// Detection: in sight (field of view, line of sight, range), touching distance, or known.
pub fn can_notice(bot: &Bot, p: &Perception, s: &Seen) -> bool {
    match notice_cheaply(bot, s) {
        Notice::Yes => true,
        Notice::No => false,
        Notice::IfSeen => sees(bot, p, s),
    }
}

/// Line of sight from the bot's eyes to another's, reused for up to 4 ticks while neither eye
/// moved more than half a block.
pub fn sees(bot: &Bot, p: &Perception, s: &Seen) -> bool {
    let (from, to) = (bot.body.eye(), s.eye());
    if let Some(&(t, seen, f, o)) = bot.sight_cache.borrow().get(&s.id) {
        if p.tick < t + 4 && f.distance_sq(from) < 0.25 && o.distance_sq(to) < 0.25 {
            return seen;
        }
    }
    let t_sight = std::time::Instant::now();
    let seen = p.world.line_of_sight(from, to);
    super::prof_add(super::P_SIGHT, t_sight.elapsed().as_nanos() as u64);
    let mut cache = bot.sight_cache.borrow_mut();
    if cache.len() > 64 {
        cache.retain(|_, e| p.tick < e.0 + 4);
    }
    cache.insert(s.id, (p.tick, seen, from, to));
    seen
}

/// Picks the target for this tick, or none. Orders, faction calls and revenge come first and
/// need no detection beyond knowing the target is there.
pub fn choose_target(bot: &mut Bot, p: &Perception, squad: &mut Squad) -> Option<Seen> {
    let tick = p.tick;
    if bot.orders.stopped {
        return None;
    }
    // An explicit order.
    if let Some(name) = bot.orders.attack.clone() {
        if let Some(s) = p.by_name(&name).filter(|s| s.health > 0.0).cloned() {
            bot.known.insert(s.id, tick);
            return Some(s);
        }
    }
    // A tether turned hostile.
    if let Some(t) = &bot.orders.tether {
        if squad.tethers.get(&t.target.to_ascii_lowercase()).is_some_and(|s| s.hostile) {
            if let Some(s) = p.by_name(&t.target).cloned() {
                return Some(s);
            }
        }
    }
    let my_faction = bot.faction.clone();
    // Peel: whoever is on a nearly dead ally.
    if let Some(e) = super::team::peel_target(bot, p, squad) {
        if hittable(bot, &e, squad) {
            bot.known.insert(e.id, tick);
            bot.last_call = "peeling".into();
            return Some(e);
        }
    }
    // The leader's call to arms.
    // (`called_on` builds a key per look: nothing to look for while no call is active.)
    if let Some(f) = my_faction.as_ref().filter(|_| !squad.factions.calls.is_empty()) {
        let called: Option<Seen> = p
            .others
            .iter()
            .filter(|s| squad.factions.called_on(f, &s.name) && hittable(bot, s, squad))
            .min_by(|a, b| dist_cmp(bot, a, b))
            .cloned();
        if let Some(s) = called {
            bot.known.insert(s.id, tick);
            return Some(s);
        }
    }
    // Revenge (companions always hit back, unless at their owner or told to stop).
    let revenge_on = bot.settings.b("revenge") || bot.orders.owner.is_some();
    if revenge_on {
        if let Some((id, _)) = bot.revenge {
            if let Some(s) = p.get(id).filter(|s| hittable(bot, s, squad)).cloned() {
                if bot.orders.retreat.is_none() {
                    return Some(s);
                }
            }
        }
    }
    if !bot.settings.b("auto-target") {
        return None;
    }
    // The target is looked at again every `target-scan-interval` ticks (each bot on its own
    // beat); in between it stays on the one it has, unless that one is gone.
    let fighting = bot.fight.target;
    let interval = bot.settings.i("target-scan-interval").max(1) as u64;
    let only = fighting.filter(|_| (tick + bot.uuid as u64) % interval != 0);
    let mut best = best_candidate(bot, p, squad, only);
    if best.is_none() && only.is_some() {
        best = best_candidate(bot, p, squad, None);
    }
    let pick = best.map(|(_, _, s)| s)?;
    // Reaction ripple: a new target reaches each bot of an army after its own moment.
    if bot.settings.b("reaction-ripple") && bot.faction.is_some() && bot.fight.target != Some(pick.id) {
        let delay = 1 + (bot.personality.hop_phase * 8.0) as u64 + (pick.pos.distance(bot.body.pos) / 8.0) as u64;
        match bot.ripple {
            Some((id, at)) if id == pick.id => {
                if tick < at {
                    return None;
                }
                bot.ripple = None;
            }
            _ => {
                bot.ripple = Some((pick.id, tick + delay));
                return None;
            }
        }
    }
    bot.known.insert(pick.id, tick);
    Some(pick)
}

/// How many candidates a bot weighs fully in one scan: the nearest ones the cheap rules let through.
pub const TARGET_CANDIDATES: usize = 5;

/// The best target by priority (war, player, mob) and then score, among `only` when it is set.
fn best_candidate(bot: &mut Bot, p: &Perception, squad: &Squad, only: Option<i32>) -> Option<(i32, f64, Seen)> {
    let tick = p.tick;
    let my_faction = bot.faction.clone();
    let st = &bot.settings;
    let grace = tick < bot.spawned_tick + st.i("spawn-grace").max(0) as u64 * 20;
    let fighting = bot.fight.target;
    let pile_cap = st.i("pile-cap").max(0) as u32;
    let prefer_low = st.b("priority-hp-target");
    let max_distance = st.f("max-target-distance");
    // Read once: with hundreds of bots this loop runs for every pair, and a setting is a map lookup.
    let (bot_vs_bot, target_players, target_passive) = (st.b("bot-vs-bot"), st.b("target-players"), st.b("target-passive-mobs"));
    let focus = my_faction.as_ref().and_then(|f| squad.focus.get(f)).copied().filter(|_| st.b("focus-fire"));
    let here = bot.body.pos;
    // First the rules that cost nothing, over everyone: the `TARGET_CANDIDATES` best by priority and
    // distance are kept. Only those get the line of sight, the pile cap and the full score.
    let mut near: [Option<(i32, f64, &Seen, Notice)>; TARGET_CANDIDATES] = [None; TARGET_CANDIDATES];
    for s in p.others {
        if only.is_some_and(|id| s.id != id) {
            continue;
        }
        let dist = s.pos.distance(here);
        if dist > max_distance {
            continue;
        }
        // A bot among bots it may not fight: nothing else to ask (the common case in a crowd).
        if s.is_bot && !bot_vs_bot && (my_faction.is_none() || s.faction.is_none()) {
            continue;
        }
        if !hittable(bot, s, squad) || s.passive && !target_passive && !s.is_player && !s.hostile {
            continue;
        }
        let rel = squad.relation(my_faction.as_deref(), s.faction.as_deref());
        let war = rel == Relation::War;
        if rel == Relation::Peace {
            continue;
        }
        if !war {
            if s.is_bot && !bot_vs_bot {
                continue;
            }
            if s.is_player && !s.is_bot && (!target_players || grace) {
                continue;
            }
            if !s.is_player && !s.hostile && !(s.passive && target_passive) {
                continue;
            }
        }
        if bot.orders.hide && dist > 6.0 {
            continue;
        }
        if let Some(g) = &bot.orders.guard {
            if s.pos.distance(g.post) > g.radius + 2.0 {
                continue;
            }
        }
        if pile_cap > 0 && Some(s.id) != fighting && squad.pile(s.id) >= pile_cap {
            continue;
        }
        let notice = notice_cheaply(bot, s);
        if notice == Notice::No {
            continue;
        }
        // A hostile mob at arm's length is as urgent as a player.
        let prio = if war { 0 } else if s.is_player || (s.hostile && dist < 4.0) { 1 } else { 2 };
        // The one it fights and the faction's focus stay in the running from further away (their score bonus).
        let key = dist - if Some(s.id) == fighting { 3.0 } else { 0.0 } - if Some(s.id) == focus { 4.0 } else { 0.0 };
        // A free slot, else the worst one kept if this one is better.
        let mut slot = 0;
        for (i, n) in near.iter().enumerate() {
            match (n, &near[slot]) {
                (None, _) => {
                    slot = i;
                    break;
                }
                (Some(o), Some(w)) if o.0 > w.0 || (o.0 == w.0 && o.1 > w.1) => slot = i,
                _ => {}
            }
        }
        if near[slot].as_ref().is_none_or(|w| prio < w.0 || (prio == w.0 && key < w.1)) {
            near[slot] = Some((prio, key, s, notice));
        }
    }
    let mut best: Option<(i32, f64, Seen)> = None;
    for (prio, _, s, notice) in near.into_iter().flatten() {
        if notice == Notice::IfSeen && !sees(bot, p, s) {
            continue;
        }
        // Sizing up a player it noticed itself.
        if s.is_player && fighting.is_none() {
            let rt = bot.settings.i("reaction-ticks").max(0) as u64;
            let extra = (rt as f64 * 0.5 * bot.rng.unit()) as u64;
            let entry = bot.noticed.entry(s.id).or_insert((tick, tick + rt + extra));
            if tick < entry.1 {
                bot.known.insert(s.id, tick);
                continue;
            }
        }
        let mut score = s.pos.distance(here);
        if prefer_low {
            score += (s.health as f64 / s.max_health.max(1.0) as f64) * 6.0;
        }
        if Some(s.id) == fighting {
            score -= 3.0;
        }
        if Some(s.id) == focus {
            score -= 4.0;
        }
        if best.as_ref().is_none_or(|(bp, bs, _)| prio < *bp || (prio == *bp && score < *bs)) {
            best = Some((prio, score, s.clone()));
        }
    }
    best
}

fn dist_cmp(bot: &Bot, a: &Seen, b: &Seen) -> std::cmp::Ordering {
    a.pos
        .distance(bot.body.pos)
        .partial_cmp(&b.pos.distance(bot.body.pos))
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// Ticks a full swing takes with what the bot holds.
fn swing_ticks(bot: &Bot, held: Option<&items::Stack>) -> f64 {
    let base = bot.settings.f("attack-cooldown");
    let w = items::weapon(held.map_or("air", |s| s.id.as_str()));
    // attack-cooldown is the sword's swing in ticks (vanilla's full charge is 12.5): other weapons scale with their own speed.
    let mut t = base * (w.cooldown_ticks() / 12.5);
    if bot.settings.b("cooldown-trick") {
        // Vanilla needs the meter above 90% for a full hit: strength = (ticks + 0.5) / full, so swinging at
        // 92% of the configured ticks (whole ticks, rounded up) never comes before that.
        t *= 0.92;
    }
    t.max(1.0)
}

fn select(d: &mut Decision, inv: &super::inventory::Inventory, slot: usize) {
    if inv.selected != slot && !d.actions.iter().any(|a| matches!(a, Action::SelectSlot(_))) {
        d.actions.push(Action::SelectSlot(slot));
    }
}

/// The selected slot after this tick's actions.
fn selected_after(d: &Decision, inv: &super::inventory::Inventory) -> usize {
    d.actions
        .iter()
        .rev()
        .find_map(|a| if let Action::SelectSlot(s) = a { Some(*s) } else { None })
        .unwrap_or(inv.selected)
}

pub fn fight(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, squad: &mut Squad) {
    let tick = p.tick;
    let st = bot.settings.clone();
    squad.engage(t.id);
    if bot.fight.target != Some(t.id) {
        bot.fight = super::FightMemory {
            target: Some(t.id),
            target_name: t.name.clone(),
            strafe_sign: if bot.personality.circle_left { 1.0 } else { -1.0 },
            // The swing meter belongs to the weapon, not to the target.
            since_swing: bot.fight.since_swing,
            ..Default::default()
        };
    }
    d.mode = "fight";
    let inv = &p.me.inventory;
    let seen_at = bot.perceived(t);
    let aim_h = st.f("aim-height") / 100.0;
    let aim_point = seen_at + Vec3::new(0.0, t.height * aim_h, 0.0);
    let dist = bot.reach_to(t);
    let flat = bot.body.pos.horizontal_distance(t.pos);
    // A totem popped: gone from the hands with their health at the bottom.
    if bot.fight.last_totem && !t.totem_shown && t.health <= 2.0 {
        bot.fight.popped_at = Some(tick);
        bot.last_call = format!("{} popped a totem", t.name);
    }
    bot.fight.last_totem = t.totem_shown;
    // Read the opponent: how often they block (an axe kept ready), how often they jump-crit.
    if st.b("read-opponent") {
        let r = &mut bot.fight;
        r.reads_block = r.reads_block * 0.98 + if t.blocking { 0.02 } else { 0.0 };
        r.reads_crits = r.reads_crits * 0.98 + if t.vel.y > 0.3 && dist < 4.0 { 0.02 } else { 0.0 };
    }
    if st.b("hotbar-shield-tracking") && (t.offhand.as_deref().is_some_and(|o| o.ends_with("shield")) || bot.fight.reads_block > 0.3) {
        let inv = &p.me.inventory;
        if inv.best_weapon(true, Some(WeaponKind::Axe)).is_none() {
            if let Some(axe) = inv.best_weapon(false, Some(WeaponKind::Axe)) {
                if bot.ready("axe-up", tick) {
                    d.actions.push(Action::ToHotbar { slot: axe, hotbar: inv.hotbar_slot_for_swap(&[inv.selected]) });
                    bot.cool("axe-up", tick, 100);
                }
            }
        }
    }
    let gap_growing = dist > bot.fight.last_dist + 0.05;
    bot.fight.growing_gap = if gap_growing { bot.fight.growing_gap + 1 } else { 0 };
    bot.fight.last_dist = dist;

    // Morale: a side that is breaking runs.
    if let Some(f) = bot.faction.clone() {
        if st.b("morale") {
            let leader_near = squad
                .factions
                .leader_of(&f)
                .and_then(|l| p.by_name(l))
                .is_some_and(|l| l.pos.distance(bot.body.pos) < 16.0);
            if squad.breaking(&f, bot.personality.bravery, leader_near) {
                d.mode = "breaking";
                let away = bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 12.0;
                bot.flee_to(p, d, away, true);
                return;
            }
        }
    }

    // Low and losing: back off (heal() drinks and eats; this keeps the distance).
    let temper = super::personality::Temper::of(bot.playstyle, race(p, t));
    // Running only helps with a way to recover (or when clearly losing); otherwise it fights on.
    if st.b("retreat")
        && (p.me.health as f64) <= st.f("retreat-health") + temper.heal_shift
        && !temper.heal_in_place
        && (can_recover(p) || race(p, t) < -0.3)
    {
        d.mode = "retreat";
        let away = bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 8.0;
        bot.flee_to(p, d, away, true);
        // Pearl out when nearly dead.
        if st.b("pearls") && (p.me.health as f64) <= st.f("pearl-escape-health") {
            pearl(bot, p, d, away + (away - t.pos).horizontal().normalize() * 6.0, "pearl-escape");
        }
        return;
    }

    bot.look_at(d, aim_point);

    let role = super::team::role(bot, p);
    // Ranged when far (an archer from closer in, and it backs off to shoot).
    let ranged_min = if role == super::team::Role::Archer { st.f("ranged-min-range") * 0.5 } else { st.f("ranged-min-range") };
    // With the bow already drawn it keeps shooting a little inside the minimum (no flicker
    // between bow and sword right on the line).
    let drawing = matches!(bot.busy, Some(Busy::Drawing { .. }));
    let ranged_from = if drawing { ranged_min - 1.5 } else { ranged_min };
    if st.b("ranged") && flat >= ranged_from && ranged(bot, p, d, t, flat, squad) {
        return;
    }
    if matches!(bot.busy, Some(Busy::Drawing { .. })) {
        // Too close for the bow now: put it away.
        d.actions.push(Action::Release);
        bot.busy = None;
    }

    // Chase a runner with a pearl.
    if st.b("pearls") && bot.fight.growing_gap > 10 && flat > st.f("pearl-chase-distance") {
        let lead = t.pos + t.vel * 10.0;
        if pearl(bot, p, d, lead, "pearl-chase") {
            d.mode = "pearl-chase";
            return;
        }
    }

    // Splash potions at them from a little way off.
    if st.b("throw-potions") && (3.0..7.0).contains(&flat) && bot.ready("throw-potion", tick) {
        if let Some(slot) = inv.find(st.b("hotbar-only"), items::is_offensive_splash) {
            if slot < 9 {
                select(d, inv, slot);
                d.pitch = pitch_to(bot.body.eye(), t.pos).min(10.0) - 15.0;
                d.actions.push(Action::Use(Hand::Main));
                bot.cool("throw-potion", tick, 120);
                d.mode = "throw-potion";
                return;
            }
        }
    }

    // Cobwebs on them (rate-limited per bot and squad-wide).
    if st.b("cobwebs") && flat <= st.f("cobweb-range") && bot.ready("cobweb", tick) && tick >= squad.web_ready_at {
        if let Some(slot) = inv.find(true, |s| s.is("cobweb")) {
            if sees(bot, p, t) {
                // Where: under them; where they are about to step (web prediction); their head
                // against a wall (head web); where a knockback hit lands them (hit web).
                let mut at = t.pos.block();
                if st.b("web-prediction") && t.vel.horizontal_length() > 0.1 {
                    at = (t.pos + t.vel * 6.0).block();
                }
                if st.b("hit-web") && bot.fight.landed_streak > 0 && bot.rng.chance(0.5) {
                    let push = (t.pos - bot.body.pos).horizontal().normalize() * 1.5;
                    at = (t.pos + push).block();
                }
                if st.b("head-web") {
                    let push = (t.pos - bot.body.pos).horizontal().normalize();
                    let behind = (t.pos + push).block();
                    if !p.world.block(behind[0], behind[1] + 1, behind[2]).passable() {
                        at = [at[0], at[1] + 1, at[2]];
                    }
                }
                let below = [at[0], at[1] - 1, at[2]];
                if p.world.block(at[0], at[1], at[2]).passable() && p.world.block(below[0], below[1], below[2]).top().is_some() {
                    select(d, inv, slot);
                    let face_point = Vec3::new(below[0] as f64 + 0.5, below[1] as f64 + 1.0, below[2] as f64 + 0.5);
                    d.yaw = yaw_to(bot.body.eye(), face_point);
                    d.pitch = pitch_to(bot.body.eye(), face_point);
                    d.actions.push(Action::UseOn { pos: below, face: 1, hand: Hand::Main });
                    bot.cool("cobweb", tick, st.i("cobweb-cooldown").max(0) as u64);
                    squad.web_ready_at = tick + st.i("cobweb-global-cooldown").max(0) as u64;
                    bot.last_call = "webbed them".into();
                    d.mode = "cobweb";
                    return;
                }
            }
        }
    }

    if super::techniques::try_all(bot, p, d, t, dist, flat, squad) || super::team::break_stash(bot, p, d, t) {
        return;
    }
    if super::team::combo_answer(bot, p, d, t) {
        return;
    }
    let sweep = super::team::sweep_ready(bot, p, squad);
    let flank = (role == super::team::Role::Flanker && flat > 6.0).then(|| super::team::flank_point(bot, t));
    // Ally care: not standing inside each other.
    let flank = super::team::separation(bot, p, squad).or(flank);
    melee(bot, p, d, t, dist, flat, aim_point, sweep, flank);
    super::team::panic(bot, p, d);
}

/// Something to heal or escape with: a healing potion, a golden apple, a totem or a pearl off cooldown.
fn can_recover(p: &Perception) -> bool {
    let inv = &p.me.inventory;
    inv.count(|s| items::is_healing_potion(s) || s.is("golden_apple") || s.is("enchanted_golden_apple") || s.is("totem_of_undying")) > 0
        || (inv.has("ender_pearl") && p.me.cooldown("ender_pearl") == 0)
}

/// The health race: positive when the bot is ahead.
fn race(p: &Perception, t: &Seen) -> f64 {
    let mine = p.me.health as f64 / 20.0;
    let theirs = t.health as f64 / t.max_health.max(1.0) as f64;
    (mine - theirs).clamp(-1.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn melee(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, dist: f64, flat: f64, aim_point: Vec3, sweep: bool, flank: Option<Vec3>) {
    let tick = p.tick;
    let st = bot.settings.clone();
    let inv = &p.me.inventory;
    let hotbar_only = st.b("hotbar-only");
    let reach = st.f("melee-range");

    // Weapon choice: an axe for a raised shield, the mace for its plays, else the best weapon.
    let mut want = if st.b("damage-model") {
        super::team::best_against(p, t, hotbar_only).or_else(|| inv.best_weapon(hotbar_only, None))
    } else {
        inv.best_weapon(hotbar_only, None)
    };
    let mut mace_play = false;
    if t.blocking && st.b("shield-disable") {
        if tick >= bot.fight.shield_down_until {
            // React to the raised shield after a moment, not every time.
            let key = "shield-read";
            if bot.ready(key, tick) {
                let react = (bot.reaction_ticks() as f64 * (1.5 - st.knob("shield-disable-speed"))) as u64;
                bot.cool(key, tick, react.max(2));
                if bot.rng.chance(st.pct("shield-disable-chance")) {
                    bot.fight.axe_until = tick + 30;
                    bot.last_call = "axe for the shield".into();
                }
            }
        }
        if tick < bot.fight.axe_until {
            if let Some(axe) = inv.best_weapon(hotbar_only, Some(WeaponKind::Axe)) {
                want = Some(axe);
            }
        }
    }
    let has_mace = inv.find(hotbar_only, |s| s.is("mace"));
    if st.b("mace") && has_mace.is_some() {
        if tick >= bot.fight.mace_mode_until {
            bot.fight.mace_mode = !bot.rng.chance(st.pct("special-weapon-melee-chance"));
            bot.fight.mace_mode_until = tick + 60 + bot.rng.int(0, 60) as u64;
        }
        mace_play = bot.fight.mace_mode;
    }
    let falling_smash = !bot.body.on_ground && bot.body.vel.y < 0.0 && bot.body.fall_distance > 1.5;
    if mace_play && falling_smash {
        want = has_mace;
    } else if mace_play && st.b("mace-swap") && !falling_smash {
        // Between smashes: the ordinary weapon.
        want = inv.best_weapon(hotbar_only, Some(WeaponKind::Sword)).or(want);
    }
    if let Some(w) = want {
        if w < 9 {
            select(d, inv, w);
        } else if bot.ready("hotbar-refill", tick) {
            // Out of the bag: opens the inventory, standing still a moment.
            let to = inv.hotbar_slot_for_swap(&[]);
            d.actions.push(Action::ToHotbar { slot: w, hotbar: to });
            bot.cool("hotbar-refill", tick, 10);
            d.input = Default::default();
            return;
        }
    }

    // The mace: a wind charge at its own feet to go up, the smash on the way down.
    if mace_play && st.b("windcharge-mace") && bot.body.on_ground && flat < 4.5 && bot.ready("wind-launch", tick) && p.me.cooldown("wind_charge") == 0 {
        if let Some(wc) = inv.find(true, |s| s.is("wind_charge")) {
            if bot.rng.chance(0.5 + 0.5 * st.knob("technique")) {
                select(d, inv, wc);
                d.pitch = if st.b("wc-crouch-boost") { 89.0 } else { 80.0 };
                d.input.jump = true;
                d.actions.push(Action::Use(Hand::Main));
                bot.cool("wind-launch", tick, 40);
                bot.last_call = "launching for a smash".into();
                d.mode = "mace-launch";
                return;
            }
        }
    }

    // Spacing: keep the standoff, circle at the strafe angle, step out while recharging.
    let standoff = st.f("melee-standoff") + bot.personality.range_bias;
    let held_after = inv.slots.get(selected_after(d, inv)).and_then(Option::as_ref);
    // A different item in the hand restarts the swing meter (vanilla); the same item with other components does not.
    let held_id = held_after.map(|s| s.id.clone());
    if bot.last_held.get_or_insert_with(|| inv.held().map(|s| s.id.clone()).unwrap_or_default()).as_str() != held_id.as_deref().unwrap_or("") {
        bot.fight.since_swing = 0;
        bot.last_held = Some(held_id.unwrap_or_default());
    }
    let cd = swing_ticks(bot, held_after);
    let charged = bot.fight.since_swing as f64 >= cd;
    let mut toward = t.pos;
    let opening = st.b("opening") && !bot.fight.opened && !mace_play;
    if opening {
        // Within sprint-in range the arcing is over: close enough to trade blows.
        if flat < 4.0 || bot.fight.growing_gap > 10 {
            bot.fight.opened = true;
        } else if flat < 10.0 {
            // A point beside them that drifts, arcing in.
            let side = (tick as f64 / 25.0 + bot.personality.hop_phase * 6.0).sin() * 2.5;
            let perp = Vec3::new(-(t.pos.z - bot.body.pos.z), 0.0, t.pos.x - bot.body.pos.x).normalize();
            toward = t.pos + perp * side;
            d.mode = "opening";
        }
    }
    // Sprint in from afar; only the last arcing steps of the opening are walked in.
    let arcing = opening && flat < 10.0;
    let mut sprint = if arcing { flat < 4.0 } else { flat > standoff + 1.0 };
    let strafe_deg = st.f("strafe") * bot.personality.strafe_width;
    if tick >= bot.fight.strafe_switch {
        if bot.rng.chance(0.4) {
            bot.fight.strafe_sign = -bot.fight.strafe_sign;
        }
        bot.fight.strafe_switch = tick + 15 + bot.rng.int(0, 30) as u64;
    }
    // A vanilla crit needs a fall without sprinting.
    let airborne_crit = st.b("crits") && !bot.body.on_ground && !mace_play;
    let mut goal = flank.unwrap_or(toward);
    if flat <= standoff + 1.5 {
        // Circle: a point on the ring around them, `strafe` degrees off the line.
        let ang = (yaw_to(t.pos, bot.body.pos) as f64 + strafe_deg * bot.fight.strafe_sign).to_radians();
        let ring = if !charged && st.b("spacing") { (reach + 0.4).max(standoff) } else { standoff };
        goal = t.pos + Vec3::new(-ang.sin(), 0.0, ang.cos()) * ring;
        sprint = false;
        // A comboer lands its charged hit sprinting (the extra knockback); the ring is for the recharge.
        if bot.personality.comboer && charged && dist <= reach + 0.5 {
            goal = t.pos;
            sprint = true;
        }
    }
    // Edge play: come round so the next hit knocks them toward the drop.
    if flat < 6.0 {
        if let Some(edge) = super::techniques::edge_goal(bot, p, t) {
            goal = edge;
            d.mode = "edge-play";
        }
    }
    if tick < bot.fight.punish_until {
        // They whiffed: step straight in for the free hit.
        goal = t.pos;
        sprint = !airborne_crit;
        bot.last_call = "punishing the whiff".into();
    }
    // Tempo: push after landing two, reset after taking two.
    if st.b("tempo") {
        if bot.fight.taken_streak >= 2 && super::personality::Temper::of(bot.playstyle, 0.0).resets {
            // Steps off for a moment; the streak ends by itself (`STREAK_TICKS`) or when a
            // hit of its own lands.
            goal = bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 3.0;
            d.mode = "reset";
        } else if bot.fight.landed_streak >= 2 {
            sprint = !airborne_crit;
        }
    }
    // Defensive bots never chase past a few blocks.
    let temper = super::personality::Temper::of(bot.playstyle, race(p, t));
    if temper.chase < 0.5 && bot.fight.growing_gap > 5 && flat > 6.0 {
        goal = bot.body.pos;
    }
    let _ = &goal;
    bot.walk_to(p, d, goal, sprint);
    if flat > 3.0 && bot.body.pos.y + 2.0 < t.pos.y {
        let extra = Vec::new();
        bot.build_if_blocked(p, d, t.pos, &extra);
    }

    // Jump crits: a hop timed so the swing lands falling with a full meter.
    let crit_ready_soon = bot.fight.since_swing as f64 + 6.0 >= cd;
    let mut hopping = false;
    if st.b("crits") && bot.body.on_ground && dist <= reach + 1.2 && crit_ready_soon && !mace_play && !bot.personality.comboer {
        d.input.jump = true;
        d.input.sprint = false;
        hopping = true;
    }

    // The swing.
    let max_reach = reach + if falling_smash && mace_play { st.f("mace-range-bonus") } else { 0.0 };
    if dist > max_reach + 0.3 {
        // Weak bots sometimes click at air.
        if bot.personality.spam_clicker && dist < 5.0 && bot.rng.chance(0.08) {
            d.actions.push(Action::Swing(Hand::Main));
            bot.fight.since_swing = 0;
        }
        return;
    }
    if !charged || hopping {
        // Just hopped for a crit: the swing comes on the way down.
        return;
    }
    // Hit select: no swing into a raised shield without an axe.
    let axe_out = held_after.is_some_and(|s| items::weapon(&s.id).kind == WeaponKind::Axe);
    if st.b("hit-select") && t.blocking && !axe_out && tick >= bot.fight.shield_down_until {
        return;
    }
    // Crit timing: wait for the fall when a crit is on (within the knob's slack).
    if st.b("crits") && !bot.body.on_ground && bot.body.vel.y > 0.0 {
        let slack = (1.0 - st.knob("crit-timing")) * 0.25;
        if !bot.rng.chance(slack) {
            return;
        }
    }
    bot.fight.since_swing = 0;
    if airborne_crit && bot.body.vel.y <= 0.0 && bot.body.fall_distance > 0.0 && !bot.body.in_water && !bot.body.on_climbable {
        d.input.sprint = false;
    }
    if sweep {
        // Vanilla sweeps only without sprinting.
        d.input.sprint = false;
    }
    let crosshair = !st.b("aim-hits") || bot.crosshair_on(d, t, max_reach);
    let miss = bot.rng.chance(st.pct("miss-chance"));
    if crosshair && !miss {
        bot.send_late(tick, Action::Attack(t.id));
        bot.send_late(tick, Action::Swing(Hand::Main));
        if axe_out && t.blocking {
            bot.fight.shield_down_until = tick + 100;
            bot.last_call = "their shield is down: all out".into();
        }
        if falling_smash && mace_play {
            bot.last_call = format!("smash from {:.0} blocks", bot.body.fall_distance);
            // Jump reset: straight back up for the next one.
            if st.b("jump-reset") || st.b("wc-reset") {
                bot.ready_at.insert("wind-launch", tick + 1);
            }
        }
        // A sprint reset after the hit (w-tap / d-tap).
        if st.b("d-tap") {
            d.input.sprint = false;
        }
    } else {
        bot.send_late(tick, Action::Swing(Hand::Main));
        bot.last_call = "whiffed".into();
    }
    let _ = aim_point;
}

/// Bows and crossbows: draw, hold, loose with lead; volleys with rests; hold the optimal range.
fn ranged(bot: &mut Bot, p: &Perception, d: &mut Decision, t: &Seen, flat: f64, squad: &Squad) -> bool {
    let tick = p.tick;
    let st = bot.settings.clone();
    let inv = &p.me.inventory;
    let arrows = inv.count(|s| s.is("arrow") || s.is("spectral_arrow") || s.is("tipped_arrow"));
    let crossbow = inv.find(true, |s| s.is("crossbow"));
    let bow = inv.find(true, |s| s.is("bow"));
    let Some(slot) = crossbow.or(bow) else { return false };
    super::techniques::mace_aware_arrows(bot, p, d, t);
    if arrows == 0 && !(crossbow.is_some() && st.b("crossbow-ammo") && inv.has("firework_rocket")) {
        return false;
    }
    if !bot.ready("volley-rest", tick) {
        // Resting: reposition instead of shooting.
        return false;
    }
    d.mode = "ranged";
    select(d, inv, slot);
    let optimal = st.f("ranged-optimal-range");
    let goal = if flat < optimal - 2.0 {
        bot.body.pos + (bot.body.pos - t.pos).horizontal().normalize() * 3.0
    } else if flat > optimal + 4.0 {
        t.pos
    } else {
        // Sidestep while drawing.
        let ang = (yaw_to(t.pos, bot.body.pos) as f64 + 30.0 * bot.fight.strafe_sign).to_radians();
        t.pos + Vec3::new(-ang.sin(), 0.0, ang.cos()) * optimal
    };
    bot.walk_to(p, d, goal, false);
    // Aim: lead the target by its movement and the arrow's drop.
    let eye = bot.body.eye();
    let mut aim = t.pos + Vec3::new(0.0, t.height * 0.6, 0.0);
    if st.b("arrow-prediction") {
        let flight = (aim.distance(eye) / 3.0).ceil();
        aim = aim + t.vel * flight;
    }
    let pitch = arrow_pitch(eye, aim, 3.0, 0.05, 0.99).unwrap_or_else(|| pitch_to(eye, aim));
    let max = st.f("turn-speed") as f32;
    d.yaw = crate::math::approach_angle(d.yaw, yaw_to(eye, aim), max);
    let noise = ((1.0 - st.knob("aim")) * 3.0) as f32;
    d.pitch = pitch + bot.rng.gauss() as f32 * noise;
    let blocked_by_ally = super::team::ally_in_line(bot, p, squad, aim);
    match bot.busy {
        Some(Busy::Drawing { since }) if blocked_by_ally => {
            // Hold the draw until the line is clear.
            let _ = since;
        }
        Some(Busy::Drawing { since }) => {
            let need = st.i("bow-min-draw-ticks").max(1) as u64 + if crossbow.is_some() { 7 } else { 0 };
            if tick >= since + need {
                let miss = bot.rng.chance(st.pct("miss-chance"));
                if miss {
                    d.pitch += 4.0 * bot.rng.signed() as f32;
                }
                d.actions.push(Action::Release);
                bot.busy = None;
                let fired = bot.ready_at.get("volley").copied().unwrap_or(0) + 1;
                bot.ready_at.insert("volley", fired);
                let volley = bot.rng.int(1, st.i("ranged-volley-size").max(1)) as u64;
                if fired >= volley {
                    bot.ready_at.insert("volley", 0);
                    bot.cool("volley-rest", tick, st.i("ranged-rest-ticks").max(0) as u64);
                } else {
                    let gap = st.i("ranged-shot-gap").max(0) as f64 * (1.0 + 0.5 * bot.rng.unit());
                    bot.cool("shot", tick, gap as u64);
                }
            }
        }
        _ => {
            if bot.ready("shot", tick) {
                d.actions.push(Action::Use(Hand::Main));
                bot.busy = Some(Busy::Drawing { since: tick });
            }
        }
    }
    true
}

/// The launch pitch that lands a projectile at `to` (speed, gravity, drag per tick), the flatter
/// of the two solutions, by stepping the flight.
pub fn arrow_pitch(from: Vec3, to: Vec3, speed: f64, gravity: f64, drag: f64) -> Option<f32> {
    let h = (to - from).horizontal_length();
    let dy = to.y - from.y;
    let mut best: Option<(f64, f32)> = None;
    let mut pitch = -60.0f32;
    while pitch <= 60.0 {
        let r = (pitch as f64).to_radians();
        let (mut vx, mut vy) = (speed * r.cos(), -speed * r.sin());
        let (mut x, mut y) = (0.0, 0.0);
        for _ in 0..200 {
            x += vx;
            y += vy;
            vx *= drag;
            vy = vy * drag - gravity;
            if x >= h {
                let err = (y - dy).abs();
                if best.is_none_or(|(e, _)| err < e) {
                    best = Some((err, pitch));
                }
                break;
            }
        }
        pitch += 0.5;
    }
    best.filter(|(e, _)| *e < 1.5).map(|(_, p)| p)
}

/// An ender pearl toward a point (escape or chase). False when it can't throw one now.
pub fn pearl(bot: &mut Bot, p: &Perception, d: &mut Decision, to: Vec3, why: &'static str) -> bool {
    let tick = p.tick;
    if !bot.ready("pearl", tick) || p.me.cooldown("ender_pearl") > 0 {
        return false;
    }
    let inv = &p.me.inventory;
    let Some(slot) = inv.find(true, |s| s.is("ender_pearl")) else { return false };
    if !bot.clutch_roll("pearl", tick) {
        return false;
    }
    let eye = bot.body.eye();
    let Some(pitch) = arrow_pitch(eye, to, 1.5, 0.03, 0.99) else { return false };
    select(d, inv, slot);
    d.yaw = yaw_to(eye, to);
    d.pitch = pitch + ((1.0 - bot.settings.knob("aim")) * 4.0 * bot.rng.signed()) as f32;
    d.actions.push(Action::Use(Hand::Main));
    bot.cool("pearl", tick, bot.settings.i("pearl-cooldown").max(20) as u64);
    bot.last_call = why.replace('-', " ");
    true
}

pub fn retreat(bot: &mut Bot, p: &Perception, d: &mut Decision, r: &super::Retreat, target: Option<&Seen>) -> bool {
    d.mode = "retreat-order";
    let goal = match &r.to {
        RetreatTo::Scatter(dir) => bot.body.pos + *dir * 10.0,
        RetreatTo::Point(at) => *at,
        RetreatTo::Player(name) => match p.by_name(name) {
            Some(s) => s.pos,
            None => bot.body.pos,
        },
    };
    // It hits back only at whoever just hit it or is right on top of it.
    if let Some(t) = target {
        let just_hit = bot.revenge.is_some_and(|(id, until)| id == t.id && until > p.tick + 560);
        if just_hit || bot.reach_to(t) < 1.5 {
            bot.look_at(d, t.eye());
            if bot.fight.since_swing as f64 >= swing_ticks(bot, p.me.inventory.held()) && bot.reach_to(t) <= 3.0 {
                bot.fight.since_swing = 0;
                bot.send_late(p.tick, Action::Attack(t.id));
                bot.send_late(p.tick, Action::Swing(Hand::Main));
            }
        }
    }
    if goal.horizontal_distance(bot.body.pos) > 2.0 {
        let look = yaw_to(bot.body.pos, goal);
        d.yaw = crate::math::approach_angle(d.yaw, look, bot.settings.f("turn-speed") as f32);
        bot.walk_to(p, d, goal, true);
    }
    true
}

/// Escort duty: follow the target, keep it within the radius of the leader. Straying earns one
/// weak warning hit that shoves it back; straying on (5 s) turns the escort hostile until stopped.
pub fn tether(bot: &mut Bot, p: &Perception, d: &mut Decision, squad: &mut Squad) -> bool {
    let Some(role) = bot.orders.tether.clone() else { return false };
    let (Some(target), Some(leader)) = (p.by_name(&role.target).cloned(), p.by_name(&role.leader).cloned()) else {
        return false;
    };
    let tick = p.tick;
    d.mode = "tether";
    let key = role.target.to_ascii_lowercase();
    let state = squad.tethers.entry(key).or_default();
    if state.hostile {
        // choose_target picks it up; nothing to escort any more.
        return false;
    }
    let outside = target.pos.distance(leader.pos) > role.radius;
    if !outside {
        state.warned_at = None;
        let slot = target.pos + (leader.pos - target.pos).horizontal().normalize() * -2.5;
        bot.look_at(d, target.eye());
        if slot.distance(bot.body.pos) > 1.5 {
            bot.walk_to(p, d, slot, slot.distance(bot.body.pos) > 4.0);
        }
        return true;
    }
    match state.warned_at {
        None => {
            // Get on the far side and give one weak hit toward the leader.
            let far_side = target.pos + (target.pos - leader.pos).horizontal().normalize() * 1.8;
            bot.look_at(d, target.eye());
            bot.walk_to(p, d, far_side, true);
            let behind = (target.pos - leader.pos).horizontal().dot((bot.body.pos - target.pos).horizontal()) > 0.0;
            if bot.reach_to(&target) <= 3.0 && behind {
                // A hit on a barely charged meter: little damage, full knockback direction.
                bot.send_late(tick, Action::Attack(target.id));
                bot.send_late(tick, Action::Swing(Hand::Main));
                bot.fight.since_swing = 0;
                state.warned_at = Some(tick);
                bot.last_call = format!("warned {}", target.name);
            }
        }
        Some(at) if tick > at + 100 => {
            state.hostile = true;
            bot.last_call = format!("{} strayed: escort hostile", target.name);
        }
        Some(_) => {
            bot.look_at(d, target.eye());
            bot.walk_to(p, d, target.pos, true);
        }
    }
    true
}

/// March, guard, follow, patrol: true when one of them moved the bot.
pub fn positional_orders(bot: &mut Bot, p: &Perception, d: &mut Decision) -> bool {
    let spacing = bot.settings.f("follow-distance");
    let tick = p.tick;
    if let Some(m) = bot.orders.march.clone() {
        let (center, yaw) = match &m.to {
            MarchTo::Point(at) => (*at, yaw_to(bot.body.pos, *at)),
            MarchTo::Player(n) => match p.by_name(n) {
                Some(s) => (s.pos, s.yaw),
                None => return false,
            },
        };
        let wobble = bot.settings.b("loose-formations");
        let slot = formation::slot(m.shape, center, yaw, m.slot, m.of, m.size, spacing, wobble.then_some((&mut bot.rng, tick)));
        d.mode = if m.arrived { "holding" } else { "marching" };
        let far = slot.horizontal_distance(bot.body.pos);
        if far > 1.0 {
            d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, slot), bot.settings.f("turn-speed") as f32);
            bot.walk_to(p, d, slot, far > 4.0);
        } else if !m.arrived {
            if let Some(march) = bot.orders.march.as_mut() {
                march.arrived = true;
                if let Some(t) = march.then_attack.take() {
                    bot.orders.attack = Some(t);
                }
            }
        }
        return true;
    }
    if let Some(g) = bot.orders.guard.clone() {
        let post = g.post;
        let away = bot.body.pos.distance(post);
        d.mode = "guarding";
        if away > g.radius {
            d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, post), bot.settings.f("turn-speed") as f32);
            bot.walk_to(p, d, post, away > g.radius + 6.0);
        } else {
            // Its own part of the perimeter, walked slowly.
            let a = (g.sector + tick as f64 / 1200.0) * std::f64::consts::TAU;
            let spot = post + Vec3::new(a.cos(), 0.0, a.sin()) * (g.radius * 0.6);
            if spot.distance(bot.body.pos) > 1.5 {
                d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, spot), 15.0);
                bot.walk_to(p, d, spot, false);
            }
        }
        return true;
    }
    if let Some(f) = bot.orders.follow.clone() {
        let Some(leader) = p.by_name(&f.target).cloned() else { return false };
        let slot = formation::slot(f.shape, leader.pos, leader.yaw, f.slot, f.of, f.size, spacing, bot.settings.b("loose-formations").then_some((&mut bot.rng, tick)));
        let far = slot.horizontal_distance(bot.body.pos);
        d.mode = "following";
        if far > 0.8 {
            let leader_moving = leader.vel.horizontal_length() > 0.05;
            d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, slot), bot.settings.f("turn-speed") as f32);
            // Behind: sprint back; close and leader walking: full speed; leader still: ease in.
            bot.walk_to(p, d, slot, far > 3.0 || leader.sprinting);
            if far > 6.0 && bot.body.on_ground && bot.rng.chance(0.2) {
                // (never undoes the jump walk_to wants for a step in the way)
                d.input.jump = true;
            }
            if !leader_moving && far < 2.0 {
                // Easing in: half speed, every tick (not a coin flip per tick).
                d.input.forward *= 0.5;
                d.input.strafe *= 0.5;
            }
        } else {
            let look = yaw_to(bot.body.pos, leader.pos + Vec3::new(-(leader.yaw as f64).to_radians().sin(), 0.0, (leader.yaw as f64).to_radians().cos()) * 8.0);
            d.yaw = crate::math::approach_angle(d.yaw, look, 10.0);
        }
        return true;
    }
    if let Some(point) = bot.orders.patrol_point {
        // The plugin owns the route: it hands the current point and advances on arrival.
        if walk_patrol(bot, p, d, point) {
            bot.patrol_arrived = true;
        }
        return true;
    }
    false
}

/// Patrol toward the current waypoint (the plugin owns the route). True when it arrived.
pub fn walk_patrol(bot: &mut Bot, p: &Perception, d: &mut Decision, point: Vec3) -> bool {
    d.mode = "patrol";
    let far = point.horizontal_distance(bot.body.pos);
    if far < 1.2 && (point.y - bot.body.pos.y).abs() < 2.0 {
        return true;
    }
    d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, point), bot.settings.f("turn-speed") as f32);
    bot.walk_to(p, d, point, false);
    false
}

/// Nothing to do: wander, and fidget like a person standing about.
pub fn idle(bot: &mut Bot, p: &Perception, d: &mut Decision) {
    let tick = p.tick;
    if bot.orders.hide {
        d.mode = "hiding";
        if !bot.hidden_with_potion {
            d.input.sneak = true;
        }
        return;
    }
    if bot.settings.b("wander") {
        let need_new = bot.wander_goal.is_none_or(|g| g.horizontal_distance(bot.body.pos) < 1.5) || bot.stuck_ticks > 40;
        if need_new && bot.rng.chance(0.02) {
            let a = bot.rng.unit() * std::f64::consts::TAU;
            let r = 4.0 + bot.rng.unit() * 8.0;
            bot.wander_goal = Some(bot.body.pos + Vec3::new(a.cos() * r, 0.0, a.sin() * r));
        }
        if let Some(g) = bot.wander_goal {
            if g.horizontal_distance(bot.body.pos) >= 1.5 {
                d.mode = "wander";
                d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.pos, g), 12.0);
                bot.walk_to(p, d, g, false);
                if bot.path_failed {
                    // No way there: pick another spot next time.
                    bot.wander_goal = None;
                }
                return;
            }
        }
    }
    if bot.settings.b("idle-fidget") {
        // Glance at someone near, look round, flick the hotbar, hop, tap crouch.
        if bot.rng.chance(0.03) {
            if let Some(s) = p.others.iter().filter(|s| s.is_player && s.pos.distance(bot.body.pos) < 10.0).min_by(|a, b| dist_cmp(bot, a, b)) {
                let at = s.eye();
                d.yaw = crate::math::approach_angle(d.yaw, yaw_to(bot.body.eye(), at), 20.0);
                d.pitch = pitch_to(bot.body.eye(), at);
            }
            // (no random turning on the spot when nobody is near)
        }
        if bot.rng.chance(0.004) {
            let s = bot.rng.int(0, 8) as usize;
            d.actions.push(Action::SelectSlot(s));
        }
        if bot.rng.chance(0.003) && bot.body.on_ground {
            d.input.jump = true;
        }
        if bot.rng.chance(0.004) {
            d.input.sneak = true;
        }
    }
    let _ = look_vector;
    let _ = tick;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cooldown_trick_still_swings_above_ninety_percent() {
        let bot = crate::brain::tests::bot_at(Vec3::ZERO);
        let sword = items::Stack::new("diamond_sword", 1);
        let with = swing_ticks(&bot, Some(&sword));
        let full = bot.settings.f("attack-cooldown");
        assert!(with >= 0.9 * full && with < full, "{with} vs {full}");
    }

    #[test]
    fn the_default_swing_is_never_before_vanillas_ninety_percent() {
        let bot = crate::brain::tests::bot_at(Vec3::ZERO);
        for id in ["diamond_sword", "diamond_axe", "diamond_pickaxe", "mace", "trident", "air", "diamond_shovel"] {
            let stack = items::Stack::new(id, 1);
            let full = items::weapon(id).cooldown_ticks();
            // The swing happens on the first whole tick at or past `swing_ticks`.
            let tick = swing_ticks(&bot, Some(&stack)).ceil();
            assert!((tick + 0.5) / full > 0.9, "{id}: swings at tick {tick} of {full}");
        }
    }

    #[test]
    fn arrow_pitch_hits_a_level_target() {
        let from = Vec3::new(0.0, 65.6, 0.0);
        let to = Vec3::new(20.0, 65.6, 0.0);
        let p = arrow_pitch(from, to, 3.0, 0.05, 0.99).unwrap();
        // Up a little (negative pitch looks up).
        assert!(p < 0.0 && p > -10.0, "{p}");
    }
}
