# HorizonBot

PvP fighter bots for [Horizon](https://github.com/horizon-rust/horizon) servers, as a native mod: bots that join as
real players with no client and fight the way people do — jump crits, shields and axes, the mace, bows, pearls,
cobwebs, water buckets — in factions and armies, with kits, presets, scripts, patrols and companions.

> **State: written blind.** The code was written against Horizon `a589a3dd9` without building the engine or running a
> server. The engine-independent part (`core/`, most of the code) builds and passes its tests; the glue to
> Horizon (`helpers/`) has never been compiled. See [STATUS.md](STATUS.md) for what is written and what to test
> first, and [GAPS.md](GAPS.md) for what Horizon lacks.

## How it works

- **A bot is a player with no client.** It joins through the server's own player path: the mod hands the hub a
  `Joined` event as a network thread would, with an in-process channel as its socket. To the server and to every
  other mod it is a player like any other (inventory, armour, hunger, effects, a skin, vanilla knockback and damage).
- **It plays like a client.** Each tick its brain (`core/src/brain/`) decides the keys it holds, where it looks and
  what it clicks; its own client physics (`core/src/physics.rs`, after vanilla's player travel) turn the keys into a
  position, and the glue sends the packets a client sends (`PlayerInput`, `MovePlayerPosRot`, `Attack`, `UseItem`,
  `UseItemOn`, `PlayerAction`, `SetCarriedItem`, `ClientTickEnd`). Teleports are accepted, knockback taken, chunk
  batches acknowledged.
- **Nothing is frame-perfect.** One `skill` dial (0–100) splits into knobs (aim, reaction, crit timing, technique,
  tracking, clutch...). Turning is rate-limited, attacks wait out a simulated ping, swings land only where the
  crosshair is, and clutch moves can be fumbled.

## Features

Bot management (spawn any number a command up to `[horizon] max-bots` in all (0 = no limit, the default), let in `joins-per-tick` (5) a tick, name tables, random skins from a pool, `latest`), selectors (`all`, `latest`,
`faction`, `overfaction`, `random[...]`, `list:`), combat orders (attack, fight, stop, retreat, hide, tether),
movement (follow in eleven formations, guard, patrol routes with waypoint commands, marches, pause, teleports
including portal and pearl arrivals), detection (field of view, hearing, line of sight, memory, sneaking and
invisibility), melee (spacing, strafing, crits, axes against shields, sweeps, whiff punishes, tempo), the mace
(wind-charge launches, smashes, elytra mace), weapon swaps, spears, bows and crossbows with lead, pearls, cobwebs,
potions, totems with a human pause, healing that buys room first, water-bucket clutches, draining, regearing from an
ender chest, pathfinding with bridging, pillaring and digging soft cover only, factions (war, peace, alliances,
leaders and their calls to arms, overfactions, per-faction settings, uniforms, random kit tables), morale, focus
fire, peeling, roles, kits, presets, difficulty presets, playstyles, scripts with parameters and waits, companions
ordered in chat, stats, an arena, a debug view, chest menus for every setting, and ghosts (recorded paths replayed,
or staring watchers).

The full list, with what is written and what is not, is in [STATUS.md](STATUS.md).

## Commands

`/horizonbot` (aliases `/hbot` and `/bot`): `spawn`, `remove`, `removeall`, `list`, `restock`,
`view`, `reload`, `attack`, `fight`, `stop`, `retreat`, `hide`, `tether`, `follow`, `guard`, `pause`, `resume`, `tp`,
`path`, `set`, `settings`, `difficulty`, `playstyle`, `randomplaystyle`, `randomplaystylechances`, `techniques`,
`faction` (`f`), `overfaction` (`of`), `kit`, `preset`, `script`, `owner`, `diagnose`, `arena`, `debugview`, `stats`,
`debug`, `givebow`, `drop`, `resetattr`, `clearplayerdata`, and `ghost record|stoprecord|spawn|stare|remove|list|recordings`.
Tab completion covers every sub-command. Everything but `stats` needs operator level 2 (permission node
`horizonbot.admin`; `stats` is `horizonbot.stats`, open to all).

## Config and data

- `config/horizonbot-common.toml`: every setting, grouped in sections (`[combat] skill = 45`, `[ranged] enabled = true`,
  `[presets.sweaty]`...). Saving the file reloads it; `/horizonbot set` writes into it and keeps its comments.
- `config/horizonbot/`: the data files in block-style YAML — `factions.yml`, `kits.yml` (Minecraft item
  serialisation, read and written back unchanged), `stats.yml`, `paths.yml`, `scripts.yml`, `bots.yml`,
  `bot-settings.yml`, `skin-cache.yml`, `messages.yml` (per-key overrides of every player-facing line); ghost
  recordings in `ghost-recordings/`.
- Bots get offline-mode UUIDs from their names, so `stats.yml` and player files line up.
- Settings layer from the config file, over the active preset, over the faction, over the single bot.
  Built-in presets: `casual`, `easy`, `normal`, `hard`, `expert`, `sweaty`, `passive`.

## Build

A bundle for `horizon-mixins` (`horizon-mixin.toml`): a core crate (`core/`, std, `toml` and `toml_edit` only), two
modules (`helpers/horizonbot_lib.rs` into the client library, `helpers/glue.rs` into the game) and hook injections. Build it
as any native mod (`horizon-launcher native-build <this folder> ...`).

`cargo test` in `core/` runs the unit and behaviour tests: the YAML reader and writer against the data-file
layouts, settings and their layering, the config file, the command grammar, selectors, factions, kits, scripts,
skins, the physics against vanilla's numbers (sprint speed, jump height, steps, cobwebs), pathfinding, formations,
and the brain in small test worlds (it walks up to a zombie and hits it, a player behind a wall goes unnoticed,
sneaking shortens the notice range, the totem goes up after its pause, ping delays the attack, followers keep their
slot, the water clutch fires inside its window, the ender-chest regear runs through).

## Contributing

Pull requests and improvements are always welcome! Direct pushes to main are disabled, every change goes through review.

## Licence

MIT, see [LICENSE](LICENSE).
