# Status

Written blind against Horizon `a589a3dd9`, updated to `b13dbf0687b5` (main): `horizon-mixins plan` passes. No engine build, no server run. `core/` builds and its tests pass
(`cargo test` in `core/`); `helpers/` (the Horizon glue) has never been compiled.

`written` = the behaviour is coded end to end (core and glue); `partly` = coded with a stated hole; `not started` =
nothing yet. Every line is untested on a server.

## Bots and commands

| Feature | State | Note |
|---|---|---|
| Spawn (one, named, count up to max-bots, name tables, skin from an account, `none`) | written | offline UUIDs as on Paper |
| A bot is a real player (inventory, hunger, effects, vanilla damage and knockback) | written | virtual connection, GAPS.md |
| No join message | not started | GAPS.md: quiet join |
| Appears where spawned | partly | teleported after placement (a hop from spawn) |
| Skins: own name, `skinFrom`, random pool (missing/always/off), no repeats in a round, `skin-cache.yml`, SkinsRestorer cooperation | partly | Mojang lookups from the mod; nothing to cooperate with on Horizon |
| `remove`, `removeall`, `list`, `reload` | written | |
| `restock [instant]` | written | the ender-chest show is the brain's regear; `restock` without `instant` backs off, then fills |
| `view inventory/enderchest` | partly | a read-only chest view (editing needs menus that move items: GAPS.md) |
| `menu`: hub, settings by group (also one bot's or faction's values), techniques, bots (pause/resume, remove all), factions, kits (take one), presets | written | Horizon's server menus |
| Kit editor menu (inventory and ender chest pages; click your item to add a copy, a kit slot to take it out; clear) | written | copies, not moves: menus move no items (GAPS.md) |
| Selectors (`all`, `latest`, `faction`, `overfaction`/`of`, `random[...]` with every filter, `list:`) | written | |
| `attack`, `fight`, `stop` | written | |
| `retreat [to]`, `retreat stop` | written | |
| `hide` (invisibility potion, armour off; crouch without), `hide stop` | written | armour goes back on through auto-equip |
| `tether` (escort, one weak warning hit, hostile after 5 s), `stop`, `info` | written | |
| `follow` in 11 formations, sizes, loose formations, `follow stop` | written | |
| `guard`/`protect` (post, radius, sectors, walk back), `guard stop` | written | |
| `pause` / `resume` / list paused | written | |
| `tp`, `tp portal [ground]` (real frames, 3 or 5 per portal), `tp pearl` (real pearls from above) | written | portals' ground height is not looked up (they float at the target's height) |
| Patrol routes (`path create/add/remove/delete/list/assign/stop`), turn back at the ends, waypoint commands with placeholders | written | |
| `set`, `settings [list/search/bot/faction]`, `*` marks defaults | written | the menu opens too; written to the config keeping comments |
| `difficulty` (knobs, six presets, per bot, per faction, clear) | written | the six presets' numbers are ours |
| `playstyle`, `randomplaystyle`, `randomplaystylechances` | written | |
| `techniques [bot/faction] [<t> on/off/variant]` | written | |
| Factions: create, disband, join (name/selector/range), leave, hostile, peace, ally, unally, leader, follow, goto (then hold/attack), formation, skin (uniform), kit tables (set/list/clear/give), info, list, set, settings | written | uniforms apply from the next join |
| The `player` faction for every real player | written | |
| Leader's call to arms (`leader-call-seconds`) | written | |
| Overfactions: create, disband, add, remove, hostile, peace, leader, follow, tp, info, list | written | |
| Kits: save (self or a bot), give (players too), givefaction, rename (tables follow), delete, list, edit | partly | saving and the editor keep enchantments, potion and damage only (GAPS.md) |
| Presets: show, apply, save, delete, off; built-ins casual/sweaty/passive/easy/normal/hard/expert | written | seven built-in presets (casual, easy, normal, hard, expert, sweaty, passive) |
| Scripts: create, add, show, remove, delete, run with `{1}`..`{9}`, `wait`, `latest`, one at a time, 200 lines, `#` comments | written | non-`horizonbot` lines run with console permission (GAPS.md) |
| Companions: `owner set/clear/list/help`, chat orders (follow, guard, stay, stop, free, attack, cancel, summon, status, help), hidden from chat, self-defence, parking while the owner is away | written | |
| `diagnose <bot or player> [reset]` | written | states, relations, settings that keep it calm, hits other mods cancelled |
| `arena start/stop/report` | partly | rounds, wins, time to kill, hits, damage, crits and smashes (from the hitter's fall), best combo; totems, heals and inventory opens are not counted |
| `debugview` | partly | shown on operators' action bars (GAPS.md) |
| `stats [name]`, `stats top [kills, kd, accuracy, damage]`, `stats reset` | written | damage is before armour (GAPS.md) |
| `debug <bot> [invulnerable]`, `givebow`, `drop`, `resetattr`, `clearplayerdata` | partly | invulnerable as Resistance 255 |
| `bots-relog` (back after a restart) | written | `bots.yml` entry layout: name, world, position, rotation, skin source, settings |
| `bypass-damage-protection` | written | a RETURN injection on the damage hook un-cancels hits involving a bot |
| `auth-autologin`, `force-vulnerable` | not started | login-plugin settings kept for config completeness; nothing to do on Horizon |
| Ghosts: record (movement), stoprecord, spawn (replay forever), stare (vanish within 6 blocks after 5 s), skins, lines every 6 s, remove, list, recordings | partly | block changes are not recorded (GAPS.md); damage to ghosts is cancelled, but they collide and show in the tab list |
| Data files (`factions.yml`, `kits.yml`, `stats.yml`, `paths.yml`, `scripts.yml`, `bots.yml`, `bot-settings.yml`, `skin-cache.yml`, `messages.yml`) | written | block-style YAML, read and written back in the same layout |
| `messages.yml` overrides by key | written | 50 chat/error/prompt/stats keys plus the `cmd.*` keys, every line overridable |
| Config as TOML (`config/horizonbot-common.toml`), comments kept on `set` | written | every setting with its limits; defaults live in `core/default.toml` and `core/src/settings_table.rs` (a test keeps them equal) |

## The brain

| Feature | State | Note |
|---|---|---|
| Client physics (walk, sprint, jump with sprint boost, gravity, drag, friction, step-up, water, lava, cobwebs, ladders, sneaking at edges, slow falling, levitation, speed, jump boost) | written | tested against vanilla's numbers; coarse block shapes (GAPS.md) |
| Elytra flight and rockets | partly | condensed glide maths |
| A* pathfinding (walk, step, drop up to 3, climb, swim), smoothing, straight line when clear | written | |
| Building: bridge, pillar (max), dig soft cover only, cooldown, budget a minute, `extra-breakable` | written | |
| Bunny hops on straight stretches, own rhythm; wander; idle fidgets | written | |
| Boats | not started | |
| Detection: FOV, hearing, line of sight, touch radius, memory, invisible/sneaking/armour multipliers | partly | hearing from hits and sprinting only (GAPS.md: events) |
| Targeting: auto-target, players, passive mobs, revenge, friendly fire, bot-vs-bot, attack-invincible, max distance, priority-hp, pile cap, focus fire, peel, spawn grace, reaction-ticks sizing up | written | |
| Aim: rate-limited turning, aim knob noise, aim height, tracking delay, crosshair-decided hits, miss chance, ping delay | written | |
| Melee: standoff, strafing, spacing, opening walk-in, crits on the fall, attack cooldown per weapon (cooldown trick), hit select, d-tap, sweeps, whiff punish, tempo, defensive chase limit, edge play | written | |
| Shield disable with an axe (chance, speed knob), shield read | written | |
| Mace: special-weapon chance, mace swap, wind-charge launch, smash on the fall, range bonus | written | |
| Mace pearl launch (pearl up, wind charge on it) | written | |
| Jump reset / wc reset (straight back up after a smash) | partly | relaunch only from the ground |
| Wind Burst chains on the ground | partly | the server's Wind Burst throw comes in as knockback and the next fall smashes again; no aiming of the chain |
| Elytra mace (climb, dive, smash, passes, rest, elytra chain) | partly | no rocketless elytra from height |
| Attribute swaps: breach swap, mace stun (one-tick/easy/optimal), spear lunge, swap cooldown | partly | whether Horizon reproduces the swap is engine-dependent: test first |
| Spear reach jab, spear stun slam, de-tap finishers (spear-mace, breach), spear dash / dash chain / spear macing, ground backstab | partly | written blind and coarse (the dash is a charge and a jab on one tick); double pop (on a totem pop read from the hands and health) and wc self reset written; elytra backstab not started |
| Wind charge cancel, shield drain | written | |
| Bows and crossbows: draw time, lead prediction, volleys, shot gaps, rests, optimal range, crossbow rockets | written | |
| Mace-aware ranged play (slow falling arrows against mace carriers) | written | arrows reordered so vanilla picks them first |
| Cart PvP with the self-damage check | partly | TNT falloff without blocks in the way or difficulty |
| Orbital Strike Cannon, stasis | not started | other plugins' items |
| Pearls: escape, chase, cooldowns | written | |
| Cobwebs: range, line of sight, per-bot and squad-wide cooldowns; web prediction, head web, hit web | written | |
| Web escape (water, delay, fumble), web cut (shears or sword), chorus and fire escapes | written | |
| Water clutch (window widening with speed, clutch knob, pickup), block clutch | written | |
| Drain enemy water | partly | needs a fluid-placed event (GAPS.md) |
| Totems: delay, panic lockouts, offhand auto/totem/shield, mace totem | written | |
| Healing: splash at feet, drink, gapples (plain first), potion gap, advanced (buy room first), block-off, show items | written | |
| Shield: auto-shield when hurt, against an incoming smash (shield/flee/take) | written | |
| Food: threshold, best food, hunger stop, backing off while eating | written | |
| Buff potions before a fight | written | |
| Auto-equip armour, hotbar-only with inventory refills | written | |
| Mending: XP orbs and bottles | written | |
| Regear from an ender chest (leave, place, open, take, close, mine back), regear turns | written | |
| Regear from a carried shulker box | partly | nothing taken yet (GAPS.md) |
| Shulker restock, shulker steal, break stashes | partly | break stashes only |
| Personality from the name, playstyles (6), panic or lock-in, combo answers, damage model, read opponent | partly | the read keeps shield and crit habits (axe kept up, early shield); pearls are not read |
| Fishing rod, trident, thrown potions | written | |
| Lava play | written | |
| Army: reaction ripple | written | new targets only, by distance and a per-name moment |
| Army: pile cap, skill spread, morale (breaking, leader holds), roles (archer, flanker, healer), ally care (no shots through allies, no sweeps into them, no standing inside each other) | written | |
| After a fight: looting the good stuff, crouching | written | |

## What to test first once a build is possible

1. **The bot joins at all**: `horizon-mixins apply`, build, `/horizonbot spawn Test none` on a throwaway server. The hub
   must admit the `Joined` from the injected module, place the bot, and the bot's channel must receive the login and a
   `PlayerPosition`; the bot answers with `AcceptTeleportation` and `PlayerLoaded`. Watch for kicks (timeout,
   "moved too quickly", packet rate).
2. **It moves without being pulled back**: walk it with `/horizonbot follow Test <you>`; the server must accept the
   physics' positions (count teleports back), the anticheat skip must hold.
3. **It hits**: a zombie, then a player with `/horizonbot attack`; check damage, knockback arriving as `SetEntityMotion`
   and being taken into the physics, and the attack cooldown.
4. **Commands from the glue**: `kit give` (the `/item replace` lines with components), `tp`, `tp portal`, `tp pearl`.
5. **The files**: put sample `factions.yml`, `kits.yml`, `stats.yml`, `paths.yml` and `scripts.yml` into
   `config/horizonbot/` and check they load and are written back unchanged in layout.
6. **Attribute swapping** on Horizon: breach swap and mace stun, against vanilla's behaviour.
7. Performance with 50 bots: the decode filter, block reads (cached per tick), A* budgets.

## Blind assumptions to check at the first build

- The bundle shape: two `[[module]]`s (one into `client_lib.rs`, one into `game.rs`, the latter private to `game`),
  helpers calling `crate::game::horizonbot_glue::...`, and the core crate's re-export `horizon_client::horizonbot::horizonbot_core`
  reaching the game side.
- The cancellable raw injection on `horizon_client::lan::anticheat::Hub::flag_to` answering `ControlFlow<bool>`.
- `living_incoming_damage` bound with `canceled` as `&mut bool` (ghosts' damage is cancelled there).
- The bypass tail (`damage-after`) is a RETURN on a unit hook, bound to `$value: ()`: main has no after-hook, so it runs at every exit of `native_living_incoming_damage`. The damage HEAD passes `amount` by value (`*amount`), so a change to `amount` is not written back; main's hook takes `&mut f32`.
- The `VERIFY` lines in `helpers/glue.rs`: `world.block` / `state_string` for block reads, `PlayerEffects::saved`,
  `PlayerMotion::fire_ticks`, `Guest::next_window`, `Hub::send_inventory`, `StackData`'s `custom_name` and the lore in
  `other` for menu items.
- Other mods' login hooks see bots as players (address `127.0.0.1:0`, brand `horizonbot`): a login gate that kicks unknown
  players kicks bots too unless it skips them.

## Commands, permissions, texts

- Commands: `/horizonbot` (aliases `/hbot`, `/bot`); the ghost commands are `/horizonbot ghost record|stoprecord|spawn|stare|remove|list|recordings`.
- Permissions: `horizonbot.admin` (everything but stats) and `horizonbot.stats` (open to all) are the intended nodes. In code
  they are operator level 2 and open-to-all; Horizon has no permission-node hook yet (GAPS.md).
- Texts: all player-facing lines are in `core/src/messages.rs` and can be overridden per key in `config/horizonbot/messages.yml`.
- Config defaults and the seven presets are in `core/default.toml`; the difficulty levels in `core/src/settings.rs`.
- Not built, not run on a server. The Rust edits are covered by `cargo test` in `core/` only.

## Command completion (SWARM2)

- `native_register_commands` used to get one catch-all usage per root (`horizonbot [args:text]`), so the client
  completed nothing. Now `command::usages()` gives one usage per sub-command path (about 800 on `/horizonbot`, plus the catch-all as last fallback; `/hbot` and `/bot` get the catch-all only) and 8 for
  `/horizonbot ghost` plus its catch-all, at operator level 2.
- Literals come from the parser's own tables (`FORMATIONS`, `PLAYSTYLES`, `DIFFICULTY_LEVELS`, `KNOBS`, the
  technique specs), so the completions follow those tables. Selectors are expanded per form (`all`, `latest`,
  `random`, `faction`, `overfaction`, a bot name). Bot names use `player_name`, so online bots are offered.
- Dispatch is unchanged: every usage still ends in `perform_command`, which hands the raw line to the parser.
- Not built, not run (SWARM rule). The usage strings were checked with a Python mirror of the generator: no
  duplicate paths, no duplicate argument names, `text` only last, optional arguments only at the end.
- Help text (`plugin.rs` HELP) now names `menu`, `view`, `restock`, `randomplaystyle`, `randomplaystylechances`.
- `diagnose` and player info no longer print Rust `Debug` output (`Some("red")`, `Some(12)`); they print the
  faction name, the target name and busy yes/no.

## Many bots (target: 300 without slow ticks)

Written, untested on a server (the glue and the Horizon patch have not been built):

- `[horizon] max-bots` (1000) caps the bots; `bot-view-distance` (2) is the view distance a bot asks for.
- Bots are marked fake players on the server (`native_set_player_fake`, a local Horizon patch, `docs` of the
  PR notes): they hold a 3x3 ticket and a small loaded area, and do not anchor natural mob spawning.
- The glue gives each bot indices into one shared list of what it perceives (all players, mobs within reach),
  instead of a copy of every player and mob per bot; block states are classified once per state id, not once per
  block per tick; one scan of the ground items per tick; no full player list per join.
- Spans `horizonbot_perceive`, `horizonbot_think`, `horizonbot_engine`, `horizonbot_act`, `horizonbot_answer` show in `horizon-report`
  and the slow-tick line.
- `cargo test --release bench_ -- --ignored --nocapture` in `core/` ticks the decision logic of 80 and 300 bots.
