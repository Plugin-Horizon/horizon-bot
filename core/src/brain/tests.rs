//! Behaviour tests: a bot in a small test world, ticked as the server would tick it.

use super::squad::Squad;
use super::*;
use crate::items::Stack;
use crate::settings::Settings;
use crate::world::MapWorld;

pub fn settings_with(pairs: &[(&str, &str)]) -> Snapshot {
    let mut s = Settings::new();
    for (k, v) in pairs {
        s.set_global(k, v).unwrap();
    }
    s.snapshot(None, None)
}

pub fn bot_at(pos: Vec3) -> Bot {
    bot_with(pos, &[("ping-millis", "0"), ("reaction-ticks", "0"), ("spawn-grace", "0")])
}

pub fn bot_with(pos: Vec3, pairs: &[(&str, &str)]) -> Bot {
    Bot::new("Tester", 1, pos, 0.0, settings_with(pairs), 0)
}

pub fn me_with(items: &[(usize, &str, u32)]) -> SelfView {
    let mut inv = Inventory::new();
    for (slot, id, n) in items {
        inv.slots[*slot] = Some(Stack::new(id, *n));
    }
    SelfView {
        health: 20.0,
        food: 20,
        saturation: 5.0,
        inventory: inv,
        dimension: "minecraft:overworld".into(),
        ..Default::default()
    }
}

pub fn perceive<'a>(tick: u64, me: &'a SelfView, others: &'a [Seen], world: &'a MapWorld) -> Perception<'a> {
    Perception {
        tick,
        me,
        others: others.into(),
        ground: &[],
        events: &[],
        world,
    }
}

pub fn zombie(id: i32, pos: Vec3) -> Seen {
    Seen {
        id,
        uuid: 1000 + id as u128,
        name: format!("zombie{id}"),
        kind: "minecraft:zombie".into(),
        pos,
        health: 20.0,
        max_health: 20.0,
        hostile: true,
        width: 0.6,
        height: 1.95,
        eye_height: 1.74,
        on_ground: true,
        ..Default::default()
    }
}

pub fn player(id: i32, name: &str, pos: Vec3) -> Seen {
    Seen {
        id,
        uuid: 2000 + id as u128,
        name: name.into(),
        kind: "minecraft:player".into(),
        is_player: true,
        pos,
        health: 20.0,
        max_health: 20.0,
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        on_ground: true,
        ..Default::default()
    }
}

fn settle(bot: &mut Bot, w: &MapWorld) {
    for _ in 0..3 {
        bot.body.tick(&crate::physics::Input::default(), w);
    }
}

#[test]
fn walks_up_to_a_zombie_and_hits_it() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let me = me_with(&[(0, "diamond_sword", 1)]);
    let mut squad = Squad::default();
    let mut z = zombie(5, Vec3::new(0.5, 64.0, 10.5));
    let mut hit_at = None;
    for t in 0..200 {
        squad.begin_tick(t);
        let others = [z.clone()];
        let p = perceive(t, &me, &others, &w);
        let d = bot.tick(&p, &mut squad);
        if d.actions.contains(&Action::Attack(5)) {
            hit_at = Some(t);
            break;
        }
        z.health = 20.0;
    }
    let t = hit_at.expect("it attacked");
    assert!(t < 120, "took {t} ticks");
    assert!(bot.body.pos.distance(z.pos) < 4.0, "{:?}", bot.body.pos);
}

#[test]
fn a_player_behind_a_wall_is_not_noticed() {
    let mut w = MapWorld::flat(63);
    for x in -3..=3 {
        for y in 64..=67 {
            w.set(x, y, 5, "minecraft:stone");
        }
    }
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let me = me_with(&[(0, "iron_sword", 1)]);
    let mut squad = Squad::default();
    let pl = player(9, "Steve", Vec3::new(0.5, 64.0, 10.5));
    let others = [pl];
    let p = perceive(1, &me, &others, &w);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_none());
    // In the open it is.
    let w2 = MapWorld::flat(63);
    let p = perceive(10, &me, &others, &w2);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_some());
}

#[test]
fn sneaking_players_are_noticed_from_closer() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let me = me_with(&[]);
    let mut squad = Squad::default();
    let mut pl = player(9, "Steve", Vec3::new(0.5, 64.0, 20.5));
    pl.sneaking = true;
    let others = [pl.clone()];
    let p = perceive(1, &me, &others, &w);
    // 32 * 0.5 (sneaking) * 0.7 (no armour) = 11.2 < 20
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_none());
    pl.sneaking = false;
    pl.armor = 10.0;
    let others = [pl];
    let p = perceive(1, &me, &others, &w);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_some());
}

#[test]
fn it_sizes_up_a_player_before_attacking() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(Vec3::new(0.5, 64.0, 0.5), &[("reaction-ticks", "30"), ("spawn-grace", "0")]);
    settle(&mut bot, &w);
    let me = me_with(&[]);
    let mut squad = Squad::default();
    let others = [player(9, "Steve", Vec3::new(0.5, 64.0, 6.5))];
    let p = perceive(10, &me, &others, &w);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_none());
    let p = perceive(10 + 46, &me, &others, &w);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_some());
}

#[test]
fn spawn_grace_spares_players_but_not_zombies() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(Vec3::new(0.5, 64.0, 0.5), &[("reaction-ticks", "0")]);
    settle(&mut bot, &w);
    let me = me_with(&[]);
    let mut squad = Squad::default();
    let others = [player(9, "Steve", Vec3::new(0.5, 64.0, 4.5))];
    let p = perceive(20, &me, &others, &w);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_none());
    let others = [zombie(3, Vec3::new(0.5, 64.0, 4.5))];
    let p = perceive(20, &me, &others, &w);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_some());
}

#[test]
fn factions_at_peace_leave_each_other_alone_until_hit() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let me = me_with(&[]);
    let mut squad = Squad::default();
    squad.factions.create("bandits").unwrap();
    squad.factions.create("player").unwrap();
    squad.factions.peace("bandits", "player").unwrap();
    bot.faction = Some("bandits".into());
    let mut pl = player(9, "Steve", Vec3::new(0.5, 64.0, 4.5));
    pl.faction = Some("player".into());
    let others = [pl];
    let p = perceive(200, &me, &others, &w);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_none());
    let events = [Event::Hurt { by: 9, amount: 2.0, kind: "minecraft:player_attack".into() }];
    let p = Perception { events: &events, ..perceive(201, &me, &others, &w) };
    let _ = bot.tick(&p, &mut squad);
    let p = perceive(202, &me, &others, &w);
    assert!(combat::choose_target(&mut bot, &p, &mut squad).is_some(), "revenge");
}

#[test]
fn the_totem_goes_up_after_a_pause() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(Vec3::new(0.5, 64.0, 0.5), &[("mistake-chance", "0"), ("totem-delay-ticks", "10")]);
    settle(&mut bot, &w);
    let mut me = me_with(&[(4, "totem_of_undying", 1)]);
    me.health = 5.0;
    let mut squad = Squad::default();
    let mut when = None;
    for t in 0..40 {
        let p = perceive(t, &me, &[], &w);
        let d = bot.tick(&p, &mut squad);
        if d.actions.contains(&Action::SwapHands) {
            assert!(d.actions.contains(&Action::SelectSlot(4)));
            when = Some(t);
            break;
        }
    }
    let t = when.expect("totem swapped");
    assert!(t >= 10, "too fast: {t}");
}

#[test]
fn a_paused_bot_does_nothing() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    bot.orders.paused = true;
    let me = me_with(&[(0, "diamond_sword", 1)]);
    let mut squad = Squad::default();
    let others = [zombie(5, Vec3::new(0.5, 64.0, 2.0))];
    for t in 0..40 {
        let p = perceive(t, &me, &others, &w);
        let d = bot.tick(&p, &mut squad);
        assert!(d.actions.is_empty());
        assert_eq!(d.input, crate::physics::Input::default());
    }
}

#[test]
fn followers_keep_their_slot() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(Vec3::new(10.5, 64.0, 10.5), &[("auto-target", "false")]);
    settle(&mut bot, &w);
    bot.orders.follow = Some(Follow { target: "Steve".into(), shape: formation::Shape::Behind, size: 1, slot: 0, of: 1 });
    let me = me_with(&[]);
    let mut squad = Squad::default();
    let others = [player(9, "Steve", Vec3::new(0.5, 64.0, 0.5))];
    for t in 0..200 {
        let p = perceive(t, &me, &others, &w);
        let _ = bot.tick(&p, &mut squad);
    }
    let slot = formation::slot(formation::Shape::Behind, others[0].pos, 0.0, 0, 1, 1, 3.5, None);
    assert!(bot.body.pos.horizontal_distance(slot) < 2.0, "{:?} vs {slot:?}", bot.body.pos);
}

#[test]
fn eats_when_hungry_and_alone() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let mut me = me_with(&[(2, "cooked_beef", 8), (3, "bread", 8)]);
    me.food = 10;
    let mut squad = Squad::default();
    let p = perceive(1, &me, &[], &w);
    let d = bot.tick(&p, &mut squad);
    assert!(d.actions.contains(&Action::SelectSlot(2)), "{:?}", d.actions);
    assert!(d.actions.contains(&Action::Use(Hand::Main)));
    assert!(matches!(bot.busy, Some(Busy::Using { .. })));
}

#[test]
fn ping_delays_the_attack() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(Vec3::new(0.5, 64.0, 0.5), &[("ping-millis", "150"), ("reaction-ticks", "0"), ("crits", "false"), ("miss-chance", "0"), ("aim-hits", "false")]);
    settle(&mut bot, &w);
    bot.fight.since_swing = 100;
    let me = me_with(&[(0, "diamond_sword", 1)]);
    let mut squad = Squad::default();
    let others = [zombie(5, Vec3::new(0.5, 64.0, 2.5))];
    let mut decided = None;
    let mut sent = None;
    for t in 0..30 {
        let p = perceive(t, &me, &others, &w);
        let d = bot.tick(&p, &mut squad);
        if decided.is_none() && bot.outbox.iter().any(|(_, a)| *a == Action::Attack(5)) {
            decided = Some(t);
        }
        if d.actions.contains(&Action::Attack(5)) {
            sent = Some(t);
            break;
        }
    }
    let (a, b) = (decided.expect("decided"), sent.expect("sent"));
    assert_eq!(b - a, 3);
}

/// Runs a bot against fixed others for `ticks`, collecting every action and mode.
fn run(bot: &mut Bot, me: &SelfView, others: &[Seen], w: &MapWorld, ticks: u64) -> (Vec<Action>, Vec<&'static str>) {
    let mut squad = Squad::default();
    let (mut acts, mut modes) = (Vec::new(), Vec::new());
    for t in 0..ticks {
        squad.begin_tick(t);
        let p = perceive(t, me, others, w);
        let d = bot.tick(&p, &mut squad);
        acts.extend(d.actions);
        modes.push(d.mode);
    }
    (acts, modes)
}

#[test]
fn shoots_a_far_target_with_the_bow() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(Vec3::new(0.5, 64.0, 0.5), &[("ping-millis", "0"), ("reaction-ticks", "0"), ("spawn-grace", "0"), ("miss-chance", "0")]);
    settle(&mut bot, &w);
    let me = me_with(&[(0, "bow", 1), (1, "iron_sword", 1), (9, "arrow", 64)]);
    let others = [zombie(5, Vec3::new(0.5, 64.0, 16.5))];
    let (acts, modes) = run(&mut bot, &me, &others, &w, 80);
    assert!(modes.contains(&"ranged"), "{modes:?}");
    assert!(acts.contains(&Action::Use(Hand::Main)));
    assert!(acts.contains(&Action::Release), "loosed an arrow");
}

#[test]
fn takes_the_axe_to_a_raised_shield() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(
        Vec3::new(0.5, 64.0, 0.5),
        &[("ping-millis", "0"), ("reaction-ticks", "0"), ("spawn-grace", "0"), ("shield-disable-chance", "100"), ("damage-model", "false")],
    );
    settle(&mut bot, &w);
    let me = me_with(&[(0, "diamond_sword", 1), (1, "diamond_axe", 1)]);
    let mut pl = player(9, "Steve", Vec3::new(0.5, 64.0, 2.5));
    pl.blocking = true;
    pl.offhand = Some("minecraft:shield".into());
    bot.orders.attack = Some("Steve".into());
    let (acts, _) = run(&mut bot, &me, &[pl], &w, 60);
    assert!(acts.contains(&Action::SelectSlot(1)), "{acts:?}");
}

#[test]
fn guards_walk_back_to_their_post() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(Vec3::new(20.5, 64.0, 0.5), &[("auto-target", "false")]);
    settle(&mut bot, &w);
    bot.orders.guard = Some(Guard { post: Vec3::new(0.5, 64.0, 0.5), radius: 5.0, sector: 0.0 });
    let me = me_with(&[]);
    run(&mut bot, &me, &[], &w, 200);
    assert!(bot.body.pos.horizontal_distance(Vec3::new(0.5, 64.0, 0.5)) < 6.0, "{:?}", bot.body.pos);
}

#[test]
fn a_retreat_order_runs_away_and_comes_to_the_point() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    bot.orders.retreat = Some(Retreat { to: RetreatTo::Point(Vec3::new(30.5, 64.0, 0.5)) });
    let me = me_with(&[(0, "iron_sword", 1)]);
    let others = [zombie(5, Vec3::new(-3.0, 64.0, 0.5))];
    let (_, modes) = run(&mut bot, &me, &others, &w, 200);
    assert!(modes.contains(&"retreat-order"));
    assert!(bot.body.pos.x > 25.0, "{:?}", bot.body.pos);
}

#[test]
fn pearls_out_when_nearly_dead() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(
        Vec3::new(0.5, 64.0, 0.5),
        &[("ping-millis", "0"), ("reaction-ticks", "0"), ("spawn-grace", "0"), ("mistake-chance", "0"), ("auto-potion", "false")],
    );
    settle(&mut bot, &w);
    let mut me = me_with(&[(0, "iron_sword", 1), (2, "ender_pearl", 16)]);
    me.health = 3.0;
    bot.orders.attack = Some("Steve".into());
    let others = [player(9, "Steve", Vec3::new(0.5, 64.0, 2.5))];
    let (acts, _) = run(&mut bot, &me, &others, &w, 40);
    let i = acts.iter().position(|a| *a == Action::SelectSlot(2)).expect("pearl selected");
    assert!(acts[i..].contains(&Action::Use(Hand::Main)));
}

#[test]
fn hiding_with_a_potion_drinks_it_and_takes_the_armour_off() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    bot.orders.hide = true;
    let mut me = me_with(&[]);
    me.inventory.slots[3] = Some(Stack::new("potion", 1).with_potion("invisibility"));
    me.inventory.armor[2] = Some(Stack::new("iron_chestplate", 1));
    let (acts, _) = run(&mut bot, &me, &[], &w, 3);
    assert!(acts.contains(&Action::SelectSlot(3)));
    assert!(acts.contains(&Action::Unwear(crate::items::ArmorSlot::Chest)));
    assert!(bot.hidden_with_potion);
    // Without a potion it crouches.
    let mut b2 = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut b2, &w);
    b2.orders.hide = true;
    let mut squad = Squad::default();
    let me2 = me_with(&[]);
    let p = perceive(5, &me2, &[], &w);
    let d = b2.tick(&p, &mut squad);
    assert!(d.input.sneak);
}

#[test]
fn webs_go_under_a_close_enemy_and_the_squad_shares_the_clock() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(
        Vec3::new(0.5, 64.0, 0.5),
        &[("ping-millis", "0"), ("reaction-ticks", "0"), ("spawn-grace", "0"), ("cobwebs", "true"), ("cobweb-global-cooldown", "600")],
    );
    settle(&mut bot, &w);
    let me = me_with(&[(0, "iron_sword", 1), (4, "cobweb", 8)]);
    bot.orders.attack = Some("Steve".into());
    let others = [player(9, "Steve", Vec3::new(0.5, 64.0, 3.0))];
    let mut squad = Squad::default();
    let mut webbed = 0;
    for t in 0..200 {
        squad.begin_tick(t);
        let p = perceive(t, &me, &others, &w);
        let d = bot.tick(&p, &mut squad);
        if d.actions.iter().any(|a| matches!(a, Action::UseOn { pos, face: 1, .. } if *pos == [0, 63, 3])) {
            webbed += 1;
        }
    }
    assert_eq!(webbed, 1, "one web inside the squad-wide cooldown");
}

#[test]
fn a_tether_warns_once_then_turns_hostile() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(Vec3::new(10.5, 64.0, 0.5), &[("ping-millis", "0"), ("auto-target", "false")]);
    settle(&mut bot, &w);
    bot.orders.tether = Some(TetherRole { target: "Prisoner".into(), leader: "Boss".into(), radius: 5.0 });
    let me = me_with(&[]);
    let others = [player(2, "Prisoner", Vec3::new(12.5, 64.0, 0.5)), player(3, "Boss", Vec3::new(0.5, 64.0, 0.5))];
    let mut squad = Squad::default();
    let mut attacks = 0;
    for t in 0..200 {
        squad.begin_tick(t);
        let p = perceive(t, &me, &others, &w);
        let d = bot.tick(&p, &mut squad);
        attacks += d.actions.iter().filter(|a| matches!(a, Action::Attack(2))).count();
    }
    let st = &squad.tethers["prisoner"];
    assert!(st.warned_at.is_some());
    assert!(st.hostile, "still outside after 5 s");
    assert!(attacks >= 2, "the warning and then the real fight: {attacks}");
}

#[test]
fn followers_of_one_leader_take_different_slots() {
    let w = MapWorld::flat(63);
    let others = [player(9, "Steve", Vec3::new(0.5, 64.0, 0.5))];
    let me = me_with(&[]);
    let mut ends = Vec::new();
    for i in 0..4 {
        let mut b = bot_with(Vec3::new(5.5 + i as f64, 64.0, 8.5), &[("auto-target", "false"), ("loose-formations", "false")]);
        settle(&mut b, &w);
        b.orders.follow = Some(Follow { target: "Steve".into(), shape: formation::Shape::Line, size: 4, slot: i, of: 4 });
        run(&mut b, &me, &others, &w, 250);
        ends.push(b.body.pos);
    }
    for a in 0..ends.len() {
        for b in a + 1..ends.len() {
            assert!(ends[a].horizontal_distance(ends[b]) > 1.0, "{ends:?}");
        }
    }
}

#[test]
fn effects_are_amplifier_then_ticks() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let mut me = me_with(&[]);
    me.effects = vec![("speed".into(), 1, 600)];
    let mut squad = Squad::default();
    let _ = bot.tick(&perceive(1, &me, &[], &w), &mut squad);
    assert!((bot.body.speed_attr - 0.14).abs() < 1e-9, "{}", bot.body.speed_attr);
}

#[test]
fn a_retreating_bot_runs_away_from_the_first_tick() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let mut me = me_with(&[(0, "diamond_sword", 1)]);
    me.health = 3.0;
    let mut squad = Squad::default();
    let others = [zombie(5, Vec3::new(0.5, 64.0, 3.5))];
    let d = bot.tick(&perceive(1, &me, &others, &w), &mut squad);
    assert_eq!(d.mode, "retreat");
    assert!(bot.body.pos.z < 0.5 && bot.body.vel.z < 0.0, "{:?}", bot.body.pos);
}

#[test]
fn streaks_end_with_a_quiet_spell_and_falls_are_nobody() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let me = me_with(&[]);
    let hurt = |by| Event::Hurt { by, amount: 2.0, kind: String::new() };
    let events = [hurt(5), hurt(5), hurt(-1)];
    let p = Perception { events: &events, ..perceive(10, &me, &[], &w) };
    bot.take_events(&p);
    assert_eq!(bot.fight.taken_streak, 2);
    assert_eq!(bot.revenge.map(|r| r.0), Some(5));
    bot.take_events(&perceive(10 + STREAK_TICKS + 1, &me, &[], &w));
    assert_eq!(bot.fight.taken_streak, 0);
}

#[test]
fn it_keeps_its_target_between_scans() {
    let w = MapWorld::flat(63);
    let mut bot = bot_with(
        Vec3::new(0.5, 64.0, 0.5),
        &[("ping-millis", "0"), ("reaction-ticks", "0"), ("spawn-grace", "0"), ("target-scan-interval", "10")],
    );
    settle(&mut bot, &w);
    let me = me_with(&[]);
    let mut squad = Squad::default();
    let a = zombie(5, Vec3::new(0.5, 64.0, 6.5));
    let b = zombie(6, Vec3::new(1.5, 64.0, 2.5));
    let both = [a.clone(), b.clone()];
    bot.fight.target = Some(5);
    // The bot's own beat: (tick + uuid 1) % 10 == 0 is a scan.
    let id = |t, o: &[Seen], bot: &mut Bot, sq: &mut Squad| combat::choose_target(bot, &perceive(t, &me, o, &w), sq).map(|s| s.id);
    assert_eq!(id(10, &both, &mut bot, &mut squad), Some(5), "no scan: stays");
    assert_eq!(id(19, &both, &mut bot, &mut squad), Some(6), "scan: the closer one");
    bot.fight.target = Some(5);
    assert_eq!(id(10, &[b], &mut bot, &mut squad), Some(6), "its target is gone: no wait");
}

#[test]
fn the_swing_meter_survives_a_new_target() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let me = me_with(&[(0, "iron_sword", 1)]);
    let mut squad = Squad::default();
    bot.fight.target = Some(5);
    bot.fight.since_swing = 50;
    let t = zombie(6, Vec3::new(0.5, 64.0, 9.5));
    let mut d = Decision::default();
    combat::fight(&mut bot, &perceive(1, &me, &[t.clone()], &w), &mut d, &t, &mut squad);
    assert_eq!(bot.fight.target, Some(6));
    assert_eq!(bot.fight.since_swing, 50);
}

#[test]
fn it_sprints_in_from_afar() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    let me = me_with(&[(0, "iron_sword", 1)]);
    let mut squad = Squad::default();
    let others = [zombie(5, Vec3::new(0.5, 64.0, 25.5))];
    let d = bot.tick(&perceive(1, &me, &others, &w), &mut squad);
    assert!(d.input.sprint && d.input.forward > 0.0, "{:?}", d.input);
}

#[test]
fn aim_error_drifts_instead_of_shaking() {
    let mut bot = bot_with(Vec3::new(0.5, 64.0, 0.5), &[("aim", "0"), ("turn-speed", "170")]);
    let point = bot.body.eye() + Vec3::new(0.0, 0.0, 10.0);
    let xs: Vec<f64> = (0..3000)
        .map(|_| {
            let mut d = Decision::default();
            bot.look_at(&mut d, point);
            d.yaw as f64
        })
        .collect();
    let var = xs.iter().map(|x| x * x).sum::<f64>() / xs.len() as f64;
    let cov = xs.windows(2).map(|w| w[0] * w[1]).sum::<f64>() / (xs.len() - 1) as f64;
    assert!(var.sqrt() > 2.2 && var.sqrt() < 3.8, "sd {}", var.sqrt());
    assert!(cov / var > 0.6, "lag-1 correlation {}", cov / var);
}

#[test]
fn a_drawn_bow_is_let_go_when_the_target_is_gone() {
    let w = MapWorld::flat(63);
    let mut bot = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut bot, &w);
    bot.busy = Some(Busy::Drawing { since: 0 });
    let me = me_with(&[(0, "bow", 1), (9, "arrow", 8)]);
    let mut squad = Squad::default();
    let d = bot.tick(&perceive(1, &me, &[], &w), &mut squad);
    assert!(d.actions.contains(&Action::Release));
    assert!(bot.busy.is_none());
}

#[test]
fn the_bow_is_kept_a_little_inside_the_minimum_range() {
    let w = MapWorld::flat(63);
    let me = me_with(&[(0, "bow", 1), (1, "iron_sword", 1), (9, "arrow", 64)]);
    let others = [zombie(5, Vec3::new(0.5, 64.0, 6.5))];
    let mut drawing = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut drawing, &w);
    drawing.busy = Some(Busy::Drawing { since: 0 });
    let d = drawing.tick(&perceive(1, &me, &others, &w), &mut Squad::default());
    assert_eq!(d.mode, "ranged");
    let mut fresh = bot_at(Vec3::new(0.5, 64.0, 0.5));
    settle(&mut fresh, &w);
    let d = fresh.tick(&perceive(1, &me, &others, &w), &mut Squad::default());
    assert_ne!(d.mode, "ranged");
}

/// Micro-benchmark of the decision logic (ignored by default; run with
/// `cargo test --release bench_ -- --ignored --nocapture`): `bots` bots in a field with `mobs`
/// zombies and a few players, ticked the way the glue ticks them. Prints the time per server tick.
fn bench_run(bots: usize, mobs: usize, prefilter: bool, factions: bool) {
    use std::time::Instant;
    let w = MapWorld::flat(63);
    let mut rng = 12345u64;
    let mut next = move || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((rng >> 33) as f64) / ((1u64 << 31) as f64)
    };
    let mut brains: Vec<Bot> = (0..bots)
        .map(|i| {
            let mut b = Bot::new(
                &format!("Bot{i}"),
                10_000 + i as u128,
                Vec3::new(next() * 200.0, 64.0, next() * 200.0),
                0.0,
                settings_with(&[("ping-millis", "0"), ("reaction-ticks", "0"), ("spawn-grace", "0")]),
                0,
            );
            settle(&mut b, &w);
            b
        })
        .collect();
    let me = me_with(&[(0, "diamond_sword", 1)]);
    let mut seen: Vec<Seen> = brains
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let mut s = player(i as i32, &format!("Bot{i}"), b.body.pos);
            s.uuid = b.uuid;
            s.is_bot = true;
            s.held = Some("minecraft:diamond_sword".into());
            s
        })
        .collect();
    for j in 0..mobs {
        seen.push(zombie(5000 + j as i32, Vec3::new(next() * 200.0, 64.0, next() * 200.0)));
    }
    let mut squad = Squad::default();
    if factions {
        // Four factions, two wars: every bot has to weigh the relation to every player it sees.
        for f in ["red", "blue", "green", "gold"] {
            squad.factions.create(f).unwrap();
        }
        squad.factions.hostile("red", "blue").unwrap();
        squad.factions.hostile("green", "gold").unwrap();
        for i in 0..bots {
            let f = ["red", "blue", "green", "gold"][i % 4];
            squad.factions.join(f, &format!("Bot{i}")).unwrap();
            brains[i].faction = Some(f.to_owned());
            seen[i].faction = Some(f.to_owned());
        }
    }
    let mut idx: Vec<u32> = Vec::new();
    let ticks = 40u64;
    let mut clone_time = std::time::Duration::ZERO;
    let mut think_time = std::time::Duration::ZERO;
    let mut phys = std::time::Duration::ZERO;
    for t in 0..ticks {
        squad.begin_tick(t);
        for i in 0..bots {
            let c = Instant::now();
            let mut probe = brains[i].body.clone();
            probe.tick(&crate::physics::Input { forward: 1.0, sprint: true, ..Default::default() }, &w);
            phys += c.elapsed();
            let c = Instant::now();
            let me_pos = brains[i].body.pos;
            // As the glue does: every player and bot, the mobs within range, never itself; indices
            // into the one shared slice instead of a copy per bot.
            idx.clear();
            for (j, s) in seen.iter().enumerate() {
                if s.uuid == brains[i].uuid {
                    continue;
                }
                if !prefilter || s.is_player || (s.pos - me_pos).length() < 48.0 {
                    idx.push(j as u32);
                }
            }
            clone_time += c.elapsed();
            let c = Instant::now();
            let p = Perception { others: Others::indexed(&seen, &idx), ..perceive(t, &me, &[], &w) };
            let d = brains[i].tick(&p, &mut squad);
            std::hint::black_box(&d);
            think_time += c.elapsed();
        }
        for (i, b) in brains.iter().enumerate() {
            seen[i].pos = b.body.pos;
        }
    }
    let per = |d: std::time::Duration| d.as_secs_f64() * 1000.0 / ticks as f64;
    println!("  (core physics alone: {:.2} ms/tick)", per(phys));
    println!(
        "bench: {bots} bots, {mobs} mobs, factions={factions}, mob culling={prefilter}: perceive {:.2} ms/tick, think {:.2} ms/tick, total {:.2} ms/tick ({:.3} ms/bot)",
        per(clone_time),
        per(think_time),
        per(clone_time + think_time),
        per(clone_time + think_time) / bots as f64
    );
}

#[test]
#[ignore]
fn bench_300_bots_decision_logic() {
    bench_run(80, 850, false, false);
    bench_run(300, 900, false, false);
    bench_run(300, 900, true, false);
    bench_run(80, 850, true, true);
    bench_run(300, 900, true, true);
}
