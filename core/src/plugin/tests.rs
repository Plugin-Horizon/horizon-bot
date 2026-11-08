use super::*;

fn plugin() -> Plugin {
    let mut files = Files::default();
    files.texts.insert(
        "kits.yml".into(),
        "kits:\n  iron:\n    contents:\n    - ==: org.bukkit.inventory.ItemStack\n      id: minecraft:iron_sword\n      count: 1\n    armour: []\n    offhand: null\n".into(),
    );
    files.texts.insert("scripts.yml".into(), "scripts:\n  raid:\n  - horizonbot spawn 3\n  - horizonbot faction join {1} latest\n  - wait 2\n  - horizonbot faction hostile {1} player\n".into());
    let mut p = Plugin::new(crate::config::DEFAULT_TOML, &files).unwrap();
    p.config.skins_random = RandomSkins::Off;
    p
}

fn op() -> Who {
    Who::Player(PlayerInfo {
        uuid: 77,
        name: "Admin".into(),
        world: "minecraft:overworld".into(),
        pos: Vec3::new(0.5, 64.0, 0.5),
        yaw: 0.0,
        level: 4,
        entity_id: 1,
        game_mode: 0,
    })
}

fn spawned(e: &[Effect]) -> Vec<String> {
    e.iter()
        .filter_map(|x| if let Effect::SpawnBot { name, .. } = x { Some(name.clone()) } else { None })
        .collect()
}

fn told(e: &[Effect]) -> String {
    e.iter()
        .filter_map(|x| if let Effect::Tell { text, .. } = x { Some(text.clone()) } else { None })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn spawns_with_offline_uuids_and_names_latest() {
    let mut p = plugin();
    let e = p.command(&op(), "spawn 5");
    let names = spawned(&e);
    assert_eq!(names.len(), 5);
    assert_eq!(p.latest, names);
    for x in &e {
        if let Effect::SpawnBot { name, uuid, .. } = x {
            assert_eq!(*uuid, crate::uuid::offline(name));
        }
    }
    let e = p.command(&op(), "spawn Marco none");
    assert_eq!(spawned(&e), vec!["Marco"]);
    assert!(told(&p.command(&op(), "spawn Marco")).contains("taken"));
    assert_eq!(p.bots.len(), 6);
}

#[test]
fn players_without_permission_get_only_stats() {
    let mut p = plugin();
    let pleb = Who::Player(PlayerInfo { level: 0, ..match op() { Who::Player(x) => x, _ => unreachable!() } });
    assert!(told(&p.command(&pleb, "spawn")).contains("not allowed"));
    assert!(!told(&p.command(&pleb, "stats")).contains("not allowed"));
}

#[test]
fn factions_kits_and_selectors() {
    let mut p = plugin();
    p.command(&op(), "spawn 4");
    p.command(&op(), "faction create bandits");
    let t = told(&p.command(&op(), "faction join bandits latest"));
    assert!(t.contains("4 joined"), "{t}");
    assert!(p.bots.values().all(|l| l.bot.faction.as_deref() == Some("bandits")));
    let e = p.command(&op(), "kit givefaction iron bandits");
    assert_eq!(e.iter().filter(|x| matches!(x, Effect::GiveKit { .. })).count(), 4);
    assert!(p.bots.values().all(|l| l.kit.as_deref() == Some("iron")));
    let e = p.command(&op(), "faction set bandits skill 90");
    assert!(e.iter().any(|x| matches!(x, Effect::Save { file, .. } if file == "factions.yml")));
    // The faction's value, each bot within the skill spread (12) of it.
    assert_eq!(p.settings.get("skill", Some("bandits"), None), settings::Value::Int(90));
    assert!(p.bots.values().all(|l| (78..=100).contains(&l.bot.settings.i("skill"))));
    let t = told(&p.command(&op(), "retreat random[limit=2]"));
    assert!(t.contains("2 bots"), "{t}");
    assert_eq!(p.bots.values().filter(|l| l.bot.orders.retreat.is_some()).count(), 2);
    // pause takes a name, all or a faction only.
    assert!(told(&p.command(&op(), "pause random[limit=2]")).contains("No bots"));
}

#[test]
fn set_writes_the_config_keeping_it_valid() {
    let mut p = plugin();
    let e = p.command(&op(), "set skill 77");
    let text = e.iter().find_map(|x| if let Effect::SaveConfig(t) = x { Some(t.clone()) } else { None }).unwrap();
    let c = crate::config::Config::parse(&text).unwrap();
    assert_eq!(c.base.get("skill"), Some(&settings::Value::Int(77)));
    assert!(told(&p.command(&op(), "set skill 700")).contains("0 to 100"));
}

#[test]
fn scripts_run_over_ticks_with_latest() {
    let mut p = plugin();
    p.command(&op(), "faction create raiders");
    p.command(&op(), "faction create player");
    p.command(&op(), "script run raid raiders");
    let mut all = Vec::new();
    for t in 1..10 {
        let players = vec![match op() { Who::Player(x) => x, _ => unreachable!() }];
        all.extend(p.begin_tick(t, players));
    }
    assert_eq!(spawned(&all).len(), 3);
    assert_eq!(p.bots.values().filter(|l| l.bot.faction.as_deref() == Some("raiders")).count(), 3);
    assert_eq!(p.squad.factions.relation(Some("raiders"), Some("player")), crate::factions::Relation::War);
}

#[test]
fn companions_obey_their_owner_in_chat() {
    let mut p = plugin();
    let owner = PlayerInfo { uuid: 5, name: "Steve".into(), world: "w".into(), pos: Vec3::new(10.0, 64.0, 10.0), yaw: 0.0, level: 0, entity_id: 9, game_mode: 0 };
    p.player_joined(owner.clone());
    p.command(&op(), "spawn Fido none");
    p.command(&op(), "owner set Fido Steve");
    let (cancel, replies, _) = p.chat(5, "Steve", "bot follow");
    assert!(cancel);
    assert!(replies[0].contains("Fido"));
    assert!(p.bots["fido"].bot.orders.follow.is_some());
    let (cancel, _, _) = p.chat(5, "Steve", "nice weather");
    assert!(!cancel);
    let (_, _, fx) = p.chat(5, "Steve", "bot summon");
    assert!(fx.iter().any(|e| matches!(e, Effect::Teleport { .. })));
    let (_, r, fx) = p.chat(5, "Steve", "bot summon");
    assert!(fx.is_empty() && r[0].contains("Too soon"), "{r:?}");
    let fx = p.player_left(5, "Steve");
    assert!(fx.iter().any(|e| matches!(e, Effect::Teleport { .. })));
    assert!(p.bots["fido"].bot.orders.paused);
}

#[test]
fn stats_and_bot_deaths() {
    let mut p = plugin();
    p.command(&op(), "spawn Marco none");
    p.bot_joined("Marco", 50, Vec3::ZERO);
    p.player_joined(PlayerInfo { uuid: 5, name: "Steve".into(), world: "w".into(), pos: Vec3::ZERO, yaw: 0.0, level: 0, entity_id: 9, game_mode: 0 });
    p.damaged(9, 50, 6.0);
    let fx = p.died(9, 50);
    assert!(fx.is_empty());
    assert_eq!(p.stats.find("marco").unwrap().kills, 1);
    let fx = p.died(50, 9);
    assert!(fx.iter().any(|e| matches!(e, Effect::RemoveBot { .. })));
    assert!(!p.is_bot("Marco"));
    assert_eq!(p.stats.find("steve").unwrap().kills, 1);
}

#[test]
fn leaders_call_their_faction() {
    let mut p = plugin();
    p.command(&op(), "spawn Boss none");
    p.bot_joined("Boss", 50, Vec3::ZERO);
    p.player_joined(PlayerInfo { uuid: 5, name: "Steve".into(), world: "w".into(), pos: Vec3::ZERO, yaw: 0.0, level: 0, entity_id: 9, game_mode: 0 });
    p.command(&op(), "faction create bandits");
    p.command(&op(), "faction leader bandits Boss");
    p.damaged(50, 9, 3.0);
    assert!(p.squad.factions.called_on("bandits", "Steve"));
}

#[test]
fn relog_brings_bots_back() {
    let mut p = plugin();
    p.command(&op(), "spawn Marco none");
    let fx = p.stopping();
    let text = fx.iter().find_map(|e| if let Effect::Save { file, text } = e { (file == "bots.yml").then(|| text.clone()) } else { None }).unwrap();
    let mut files = Files::default();
    files.texts.insert("bots.yml".into(), text);
    let mut q = Plugin::new(crate::config::DEFAULT_TOML, &files).unwrap();
    q.config.skins_random = RandomSkins::Off;
    let fx = q.begin_tick(1, Vec::new());
    assert_eq!(spawned(&fx), vec!["Marco"]);
}

#[test]
fn ghosts_record_and_replay() {
    let mut p = plugin();
    p.ghost_command(&op(), "record lap");
    for i in 0..5 {
        p.record(77, Vec3::new(i as f64, 64.0, 0.0), 0.0, 0.0, false);
    }
    let fx = p.ghost_command(&op(), "stoprecord");
    assert!(fx.iter().any(|e| matches!(e, Effect::Save { file, .. } if file == "ghost-recordings/lap.txt")));
    let fx = p.ghost_command(&op(), "spawn Wanderer lap skin:none Boo|Hoo");
    assert!(fx.iter().any(|e| matches!(e, Effect::SpawnGhost { .. })));
    let fx = p.begin_tick(2, Vec::new());
    assert!(fx.iter().any(|e| matches!(e, Effect::MoveGhost { .. })));
}

#[test]
fn every_command_parses_and_runs() {
    let mut p = plugin();
    p.player_joined(PlayerInfo { uuid: 5, name: "Steve".into(), world: "w".into(), pos: Vec3::ZERO, yaw: 0.0, level: 0, entity_id: 9, game_mode: 0 });
    for line in [
        "spawn 3", "spawn Marco none", "list", "restock all instant", "restock all", "attack Marco Steve", "stop Marco",
        "retreat all to Steve", "retreat stop all", "hide latest", "hide stop latest", "tether Steve Marco latest 15",
        "tether info Steve", "tether stop Steve", "follow latest Steve circle", "follow stop all", "guard all 12 0 64 0",
        "guard stop", "pause all", "pause", "resume all", "tp all Steve", "tp pearl all 10 70 10 20", "tp portal ground all Steve 40",
        "path create walls", "path add walls", "path list", "path list walls", "path assign all walls",
        "path command walls 1 add say {bot}", "path command walls 1 list", "path stop all", "path delete walls",
        "set pearls false", "settings list combat", "settings search pearl", "difficulty", "difficulty preset hard",
        "difficulty aim 60", "difficulty bot Marco preset easy", "difficulty faction player clear", "playstyle list",
        "playstyle aggressive latest", "playstyle clear latest", "randomplaystyle all", "randomplaystylechances",
        "randomplaystylechances safe 40", "randomplaystylechances reset", "techniques", "techniques mace-swap off",
        "techniques bot Marco stun-slam-variant easy", "faction create red", "faction create blue", "faction join red latest",
        "faction hostile red blue", "faction peace red blue", "faction ally red blue", "faction unally red blue",
        "faction leader red Marco", "faction follow red Steve rows 4", "faction follow red stop", "faction goto red 10 64 10 wall",
        "faction goto red status", "faction goto red stop", "faction formation red circle", "faction skin red Notch",
        "faction skin list", "faction kit red set iron 30", "faction kit red list", "faction kit red give",
        "faction kit red clear", "faction info red", "faction list", "faction set red miss-chance 45", "faction settings red",
        "overfaction create north", "of add north red", "of add north blue", "of hostile north player", "of info north",
        "of list", "of remove blue", "of follow north Steve", "of tp north Steve", "of disband north", "kit list",
        "kit give iron latest", "kit rename iron iron2", "kit delete iron2", "preset", "preset sweaty", "preset save mine",
        "preset off", "preset delete mine", "script create s", "script add s horizonbot list", "script show s",
        "script remove s 1", "script list", "script delete s", "owner set Marco Steve", "owner list Steve", "owner help",
        "owner clear Marco", "diagnose Marco", "diagnose Steve", "debugview all", "debugview all", "stats", "stats Steve",
        "stats top", "stats top kd", "stats reset all", "debug Marco", "debug Marco invulnerable false", "givebow Marco",
        "drop Marco diamond 3", "resetattr Steve", "clearplayerdata", "faction disband blue", "remove Marco", "removeall",
    ] {
        let fx = p.command(&op(), line);
        let t = told(&fx);
        assert!(!t.contains("Unknown command") && !t.starts_with("Missing"), "{line}: {t}");
    }
}

/// The whole loop without an engine: two bots ordered to fight, perceptions built from their
/// bodies each tick, their attacks taken as damage; one of them must win.
#[test]
fn two_bots_fight_to_the_end() {
    use crate::brain::{Action, Perception, SelfView, Seen};
    let mut p = plugin();
    p.command(&op(), "set ping-millis 0");
    p.command(&op(), "set reaction-ticks 0");
    // Without healing items a low bot would back off for ever.
    p.command(&op(), "set retreat false");
    p.command(&op(), "spawn Red none");
    p.command(&op(), "spawn Blue none");
    let w = crate::world::MapWorld::flat(63);
    let start = [("Red", 10, Vec3::new(0.5, 64.0, 0.5)), ("Blue", 11, Vec3::new(6.5, 64.0, 0.5))];
    for (n, id, at) in start {
        p.bot_joined(n, id, at);
    }
    p.command(&op(), "fight Red Blue");
    let mut health = std::collections::BTreeMap::from([(10, 20.0f32), (11, 20.0f32)]);
    let mut winner = None;
    for tick in 1..1200 {
        p.begin_tick(tick, Vec::new());
        let bodies: Vec<(String, i32, Vec3, f32)> = p
            .bots
            .values()
            .map(|l| (l.bot.name.clone(), l.bot.entity_id, l.bot.body.pos, l.bot.body.yaw))
            .collect();
        let mut hits = Vec::new();
        for (name, id, _, _) in &bodies {
            let others: Vec<Seen> = bodies
                .iter()
                .filter(|(_, oid, _, _)| oid != id)
                .map(|(n, oid, pos, yaw)| Seen {
                    id: *oid,
                    uuid: crate::uuid::offline(n),
                    name: n.clone(),
                    kind: "minecraft:player".into(),
                    is_player: true,
                    is_bot: true,
                    pos: *pos,
                    yaw: *yaw,
                    health: health[oid],
                    max_health: 20.0,
                    width: 0.6,
                    height: 1.8,
                    eye_height: 1.62,
                    on_ground: true,
                    ..Default::default()
                })
                .collect();
            let mut me = SelfView { health: health[id], food: 20, saturation: 5.0, ..Default::default() };
            me.inventory = crate::brain::inventory::Inventory::new();
            me.inventory.slots[0] = Some(crate::items::Stack::new("iron_sword", 1));
            let perception = Perception { tick, me: &me, others: (&others).into(), ground: &[], events: &[], world: &w };
            if let Some(d) = p.think(name, &perception) {
                for a in d.actions {
                    if let Action::Attack(target) = a {
                        hits.push((*id, target));
                    }
                }
            }
        }
        for (by, target) in hits {
            // Whoever fell earlier in this very tick lands no blow (no double knock-out).
            if health[&by] <= 0.0 {
                continue;
            }
            p.damaged(target, by, 4.0);
            let h = health.get_mut(&target).unwrap();
            *h -= 4.0;
            if *h <= 0.0 {
                p.died(target, by);
                winner = Some(by);
            }
        }
        if winner.is_some() {
            break;
        }
    }
    let w = winner.unwrap_or_else(|| {
        let modes: Vec<String> = p.bots.values().map(|l| format!("{} {} {:?} {}", l.bot.name, l.bot.last_mode, l.bot.body.pos, l.bot.last_call)).collect();
        panic!("nobody won: health {health:?}, {modes:?}")
    });
    assert_eq!(p.bots.len(), 1);
    assert!(p.bots.values().all(|l| l.bot.entity_id == w));
    assert_eq!(p.stats.records.values().map(|r| r.kills).sum::<u64>(), 1);
}

#[test]
fn the_kit_editor_adds_copies_and_takes_slots_out() {
    let mut p = plugin();
    p.command(&op(), "kit edit fresh");
    assert!(p.kit_edits.contains_key(&77));
    let sword = crate::kits::KitItem::new("diamond_sword", 1);
    let helmet = crate::kits::KitItem::new("iron_helmet", 1);
    let shield = crate::kits::KitItem::new("shield", 1);
    p.kit_editor_change(77, None, Some(sword), false, false);
    p.kit_editor_change(77, None, Some(helmet), false, false);
    p.kit_editor_change(77, None, Some(shield), false, false);
    let k = &p.kits.kits["fresh"];
    assert_eq!(k.storage()[0].as_ref().unwrap().id, "minecraft:diamond_sword");
    assert_eq!(k.armour_slots()[3].as_ref().unwrap().id, "minecraft:iron_helmet");
    assert_eq!(k.offhand_item().unwrap().id, "minecraft:shield");
    // Menu slot 27 is hotbar slot 0: taken out.
    p.kit_editor_change(77, Some(27), None, false, false);
    assert!(p.kits.kits["fresh"].storage()[0].is_none());
    // The ender chest page.
    p.kit_editor_change(77, None, None, true, false);
    p.kit_editor_change(77, None, Some(crate::kits::KitItem::new("golden_apple", 8)), false, false);
    assert_eq!(p.kits.kits["fresh"].enderchest.iter().flatten().count(), 1);
    let (_, ender, slots) = p.kit_editor_slots(77).unwrap();
    assert!(ender && slots[0].is_some());
    p.menu_closed(77);
    assert!(p.kit_editor_slots(77).is_none());
}

#[test]
fn max_bots_caps_spawns() {
    let mut p = plugin();
    p.config.max_bots = 7;
    assert_eq!(spawned(&p.command(&op(), "spawn 5")).len(), 5);
    let e = p.command(&op(), "spawn 5");
    assert_eq!(spawned(&e).len(), 2);
    assert!(told(&e).contains("Bot limit"), "{}", told(&e));
    let e = p.command(&op(), "spawn Extra");
    assert!(spawned(&e).is_empty());
    assert!(told(&e).contains("Bot limit reached"));
    assert_eq!(p.bots.len(), 7);
}

#[test]
fn horizon_section_is_read() {
    let c = crate::config::Config::parse("[horizon]\nmax-bots = 12\nbot-view-distance = 1\n").unwrap();
    assert_eq!((c.max_bots, c.bot_view_distance), (12, 2));
    assert_eq!(crate::config::Config::parse(crate::config::DEFAULT_TOML).unwrap().max_bots, 1000);
    assert_eq!(crate::config::Config::parse(crate::config::DEFAULT_TOML).unwrap().joins_per_tick, 5);
    assert_eq!(crate::config::Config::parse("[horizon]\njoins-per-tick = 9\n").unwrap().joins_per_tick, 9);
}
