# What Horizon lacks for HorizonBot

Every place the mod reaches into Horizon's internals or does without, against `a589a3dd9`. Each line is a hook,
helper or field that would replace a workaround; together they are meant as one upstream change.

## Players with no client (the core of the mod)

- [ ] `native_fake_player_join(name: &str, uuid: u128, properties: &[Property], at: Option<(dimension, [f64; 3], yaw)>) -> Option<u32>`,
  `native_fake_player_send(conn: u32, packet: ServerboundPlayPacket)`, `native_fake_player_take_sent(conn: u32) -> Vec<ClientboundPlayPacket>`,
  `native_fake_player_leave(conn: u32, quiet: bool)` — needed for every bot and ghost — today: the injected module
  `horizon_client::horizonbot` builds a `lan::Joined` with `PlayerSender::new(test channel)`, sends it on
  `hub.lan.shared.events` with `shared.next_id()`, pushes `NetEvent::Packet`/`NetEvent::Left`, and decodes the bodies
  the channel gets (`Out`). Works only while `Joined`, `Out`, `NetEvent`, `Shared.events` and `PlayerSender::new` stay
  public and shaped as now.
- [ ] A quiet join: `Joined.quiet: bool` (or `Guest.quiet_join`), checked where the join is announced — needed for "bots
  never broadcast a join message" — today: not possible (the leave is quiet through `Guest.quiet_leave`).
- [x] The spawn point of a joining player — done with the engine call `native_expect_fake_player_at(name, dimension, pos,
  yaw)`: the bot appears where it was spawned, no hop (the teleport after placement stays only as a fallback).
- [ ] `Shared::admit_virtual(uuid, name)`: into the online list without the login gate — needed for the server list's
  player count and duplicate-login handling to include bots — today: skipped (`try_admit` would apply the whitelist
  and the player cap to bots).
- [ ] An anticheat exemption: `native_anticheat_exempt(uuid: u128, exempt: bool)` or a hook
  `native_anticheat_flag(player: u128, check: &str, cancel: &mut bool)` — needed because a bot's moves come from its own
  client physics — today: raw HEAD injection on `horizon_client::lan::anticheat::Hub::flag_to` (not in the hook
  catalog), answering `false` for bots.
- [x] `native_simulate_player_move(uuid, NativePlayerInput) -> Option<NativeSimulatedMove>` (Horizon branch
  `hooks-bots`, pending API `player_bots`): the bots' moves are played by the engine's player movement
  (`helpers/glue.rs` `engine_move`), the core's physics stays the fallback where it can't play them (riding, spectator).
  The brain still plans with the core's model in between ticks.
- [ ] `native_collision_boxes(dimension, aabb) -> Vec<[f64; 6]>` (exists on branch `hooks-bots`, not wired yet) — needed for
  the bots' physics and pathfinding to collide with real block shapes — today: block-state names are classified into a
  few shapes (`core/src/world.rs`: full, bottom/top slab, post, open door...).
- [ ] A signed-less chat line under a player's name: `native_chat_as(uuid: u128, text: &str)` (disguised chat) —
  needed for ghosts' lines and bots that talk — today: a system broadcast `<name> text`.
- [ ] `native_set_player_invulnerable(uuid: u128, on: bool)` — needed for `/horizonbot debug <bot> invulnerable` and for
  ghosts that can't be hurt — today: Resistance 255 (blocks damage, not knockback or targeting).
- [ ] `native_player_untargetable(uuid: u128, on: bool)` (no collisions, no hits, not in the tab list) — needed for
  ghosts — today: not possible; ghosts are ordinary players with Resistance.

## Reading players and mobs

- [ ] `NativeServerPlayer` with `sneaking`, `sprinting`, `gliding`, `using_item`, `blocking`, `held: Option<String>`,
  `offhand: Option<String>`, `armor_points: f32`, `velocity: [f64; 3]` — needed for every bot's read of a player —
  today: read from `Guest` fields directly (`sneaking`, `sprinting`, `fall_flying`, `living_flags & 1` as "using",
  `inventory[selected]`, position minus `previous_position` as velocity).
- [ ] `native_player_item_cooldowns(uuid: u128) -> Vec<(String, u32)>` — needed so a bot doesn't try a pearl or wind
  charge on cooldown — today: not read; the brain keeps its own timers.
- [ ] `NativeMob` with `velocity`, `on_ground`, `width`, `height`, `held` — needed for aiming at and dodging mobs —
  today: velocity 0, sizes from a table of names.
- [ ] `native_mobs(dimension)` from `native_server_tick` (any loaded level, read-only) — the bots of all levels could
  think in one place — today: each level's bots think in `level_tick_pre`, where its level is in place.

## Events the bots react to

- [ ] `native_totem_used(entity: i32)` — needed for "goes all in when you have no totem showing", stats, arena —
  today: not heard.
- [ ] `native_item_use_started(entity: i32, item: &str)` / `..._finished` (eating, drinking, a pearl thrown, a bow
  drawn) — needed for reading the opponent (follow pearls, punish eating) — today: not heard.
- [ ] `native_fluid_placed(player: u128, dimension: &str, pos: [i32; 3], fluid: &str)` — needed for `drain` (scoop up
  an enemy's water) — today: not heard; drain never fires.
- [ ] `native_block_placed(player, dimension, pos, before, after)` and `native_block_broken(player, dimension, pos,
  state)` — needed for ghost recordings with their block changes and for counting a bot's building budget against the
  server's answer — today: ghosts record movement only.
- [ ] `native_living_damaged(entity, causing, amount_after_armor)`: after armour and after every mod had its say —
  needed for exact stats (damage dealt) and the arena report — today: HEAD of `living_incoming_damage`, before armour
  and before other mods may cancel.
- [ ] `native_swing(entity: i32, hit: bool)` (a swing that hit nothing) — needed for whiff punishes and melee accuracy
  of real players in `stats.yml` — today: not heard.

## Commands and permissions

- [ ] A console command hook: `native_console_command(line: &str, ran: &mut bool)` — needed for `/horizonbot` from the
  server console — today: not possible; the console can only run scripts' lines through the mod.
- [ ] `native_run_command_as(uuid: u128, line: &str) -> bool` with that player's permission — needed for script lines
  that are not HorizonBot's to run as whoever started the script — today: `execute as <name> at <name> run ...` from the
  console (console permission).
- [ ] Permission nodes (`horizonbot.admin`, `horizonbot.stats`) — needed for permission plugins to grant them — today:
  operator level 2.

## Items, menus, displays

- [ ] `native_item_from_text(id: &str, count: u32, components: &[(String, String)]) -> Option<ItemStack>` and
  `native_item_components_text(stack: &ItemStack) -> Vec<(String, String)>` — needed for kits in Paper's
  `kits.yml` layout to round-trip every component — today: kits are handed out with `/item replace` (the server
  parses the components), and saved with enchantments, potion and damage only.
- [ ] Menus that move items: `native_menu_take(uuid, slot) -> Option<ItemStack>` / `native_menu_put(uuid, slot,
  stack)` with the cursor (server menus show items and hear clicks, `native_open_menu` / `native_menu_clicked`, but no
  click moves anything) — needed for a kit editor that works like an inventory (drag stacks in and out) — today:
  a click on one of the editor's own stacks adds a copy, a click on a kit slot takes it out.
- [ ] A live, editable view of another player's inventory and ender chest:
  `native_open_player_inventory(viewer: u128, target: u128, ender: bool)` — needed for `/horizonbot view` — today: the
  contents listed in chat.
- [ ] A line over a player's head: `native_player_name_line(uuid: u128, line: Option<Text>)` (a text display riding the
  player, or a below-name score) — needed for `/horizonbot debugview` — today: operators' action bars.
- [ ] A placed container's slots from a server tick with the hub at hand (`native_container_slots` /
  `native_extract_item` outside the level's own pass) — needed for regearing from a shulker box — today: the box is
  placed, opened and mined back, nothing taken.

## Elsewhere

- [ ] A skin lookup helper (`native_profile_lookup(name) -> Option<(uuid, value, signature)>`, cached like the
  server's user cache) — needed for skins by account name — today: the mod calls the Mojang API itself with `ureq`
  (one lookup a second for the pool).
- [ ] World names: the data files name worlds as Bukkit-style servers do (`world`, `world_nether`, `world_the_end`), Horizon
  by dimension id; a `native_dimension_of_world(name) -> Option<String>` (and back) — needed for `bots.yml`,
  `paths.yml` and selectors' `world=` with more than the three vanilla levels — today: mapped by name suffix.
- [ ] Boats driven by a player with no client (`MoveVehicle`, `PaddleBoat` against the boat physics) — needed for bots
  crossing water by boat — today: not started.

## Notes on defaults and texts

- `stats.line` placeholders: `{0}` is an empty rank prefix (`""`, or `"N. "` in a top list), `{1}` the name, `{5}` accuracy
  printed as a percent, `{6}`/`{7}` landed hits and swings, `{8}` bow hits (`core/src/plugin.rs`).
- `stats.top`: `{0}` is the count, `{1}` the sort name (`kills`, `K/D`, `accuracy`, `damage`). `stats.reset`: `{0}` is the name typed
  (`all` included). Companion status reply: `"{bot}: {state}"`; `state-following` and `state-marching` take the ordering player's name.
- Permission nodes: `horizonbot.admin` and `horizonbot.stats` are the intended names; today operator level 2 and open-to-all.
- `plugin.language` is the only key under `[plugin]`; there is no metrics or update check.

## Command completion (SWARM2)

- [ ] `names:<table>` (spawn from a name table) is typed as a word: the `names:` prefix can't be a literal in a usage,
      so the client gets no completion for it.
- [ ] `skin:<name>` in ghost commands and `random[limit=..,world=..]` filters are free text (`text`/`word`), not
      completed.
- [ ] Coordinates (`x y z`, `~` relative) are `word` arguments: `number` would reject `~`, and `block_pos` is integer only.
- [ ] The `goto ... then` tail is only completed without a formation (the formation-plus-`then` product is left out).
- [ ] Tree size is a guess: about 800 paths per root (1,600 registrations in total) go to every player at join.
      If the join packet is too big, drop the formation literals from the full selector forms first.
