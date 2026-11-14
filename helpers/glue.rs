// HorizonBot's game side, a module of horizon-client's `game` (`game::horizonbot_glue`): it turns the
// server's state into what each bot perceives, runs the plugin and the brains, and carries out
// what comes back, as packets of each bot's "client" or as console commands.
//
// BLIND: written against a589a3dd9 without a build. `VERIFY` marks a name or a field read from
// the source but never compiled; `GAP` marks a workaround for a missing hook (GAPS.md).

use super::Game;
use horizon_client::mod_api::{NativeCommand, NativeServerPlayer, Text};
use horizon_client::horizonbot::{self as pb, horizonbot_core as pc};
use pc::brain::{Action, Event, Ground, Hand, Perception, SelfView, Seen};
use pc::math::Vec3;
use pc::plugin::{Effect, PlayerInfo, SkinSource, Who};
use horizon_protocol::packets::play::{self, Clientbound as C, Serverbound as S};
use std::cell::RefCell;
use std::collections::HashMap;

// ------------------------------------------------------------------ the plugin's start

/// The config file came (or changed): make the plugin, or reload it.
pub(super) fn config_text(text: &str, reloading: bool) {
    pb::with(|g| {
        if reloading && g.wrote_config.as_deref() == Some(text) {
            // `/horizonbot set`.
            return;
        }
        g.config_text = text.to_owned();
        let files = pb::read_files();
        match g.plugin.as_mut() {
            Some(p) if reloading => {
                if let Err(e) = p.reload(text, &files) {
                    eprintln!("[horizonbot] config not reloaded, keeping the old one: {e}");
                }
            }
            _ => match pc::plugin::Plugin::new(text, &files) {
                Ok(p) => g.plugin = Some(p),
                Err(e) => eprintln!("[horizonbot] config not read: {e}"),
            },
        }
    });
}

pub(super) fn register_configs(configs: &mut Vec<super::native_configs::NativeConfig>) {
    configs.push(super::native_configs::NativeConfig::new(
        "horizonbot",
        super::native_configs::NativeConfigKind::Common,
        pc::config::DEFAULT_TOML,
    ));
}

pub(super) fn register_commands(commands: &mut Vec<NativeCommand>) {
    for (usage, level) in pc::command::usages() {
        commands.push(if level == 0 { NativeCommand::new(&usage) } else { NativeCommand::op(&usage, level) });
    }
}

// ------------------------------------------------------------------ hooks without a host

/// `native_living_incoming_damage`: no host there, so the hit is queued for the next tick.
pub(super) fn damage(entity: i32, causing: i32, amount: f32, canceled: bool) {
    if causing == 0 || canceled {
        return;
    }
    pb::with(|g| g.queue.damage.push((entity, causing, amount)));
}

// ------------------------------------------------------------------ players

fn info_of(p: &NativeServerPlayer) -> PlayerInfo {
    PlayerInfo {
        uuid: p.uuid,
        name: p.name.clone(),
        world: p.dimension.clone(),
        pos: Vec3::from_array(p.position),
        yaw: p.yaw,
        level: p.permission,
        entity_id: p.entity_id,
        game_mode: p.game_mode,
    }
}

pub(super) fn logged_in(game: &mut Game<'_>, uuid: u128, name: &str) {
    // A bot is looked up in the hub directly: listing every player for each join made a join of
    // hundreds of bots quadratic (and the costly part of the tick they joined in).
    if pb::is_bot(uuid) {
        let found = game.lan.as_ref().and_then(|h| h.hub.guest_by_uuid(uuid)).map(|g| (g.entity_id, g.position));
        pb::with(|g| {
            g.fresh.push((uuid, name.to_owned()));
            if let (Some(plugin), Some((entity, position))) = (g.plugin.as_mut(), found) {
                plugin.bot_joined(name, entity, Vec3::from_array(position));
            }
        });
        return;
    }
    let player = game.native_server_player(uuid);
    let effects = pb::with(|g| {
        let Some(plugin) = g.plugin.as_mut() else { return Vec::new() };
        match &player {
            Some(p) => plugin.player_joined(info_of(p)),
            None => Vec::new(),
        }
    });
    run_effects(game, effects);
}

pub(super) fn logged_out(game: &mut Game<'_>, uuid: u128, name: &str) {
    if pb::is_bot(uuid) {
        return;
    }
    let effects = pb::with(|g| g.plugin.as_mut().map(|p| p.player_left(uuid, name)).unwrap_or_default());
    run_effects(game, effects);
}

pub(super) fn player_died(game: &mut Game<'_>, uuid: u128) {
    // The killer is the last entity that hurt them (queued damage of this tick or the last).
    let victim = game.lan.as_ref().and_then(|h| h.hub.guest_by_uuid(uuid)).map(|g| g.entity_id);
    let effects = pb::with(|g| {
        let Some(v) = victim else { return Vec::new() };
        let killer = g.queue.damage.iter().rev().find(|(e, _, _)| *e == v).map_or(0, |(_, c, _)| *c);
        if let Some(c) = g.conns.iter_mut().find(|c| c.uuid == uuid) {
            c.dead = true;
        }
        g.plugin.as_mut().map(|p| p.died(v, killer)).unwrap_or_default()
    });
    run_effects(game, effects);
}

pub(super) fn stopping(game: &mut Game<'_>) {
    let effects = pb::with(|g| g.plugin.as_mut().map(|p| p.stopping()).unwrap_or_default());
    run_effects(game, effects);
    // Each bot leaves; quietly, as with the server.
    pb::with(|g| {
        let uuids: Vec<u128> = g.conns.iter().map(|c| c.uuid).collect();
        for u in uuids {
            pb::disconnect(g, u);
        }
    });
}

// ------------------------------------------------------------------ commands

pub(super) fn perform_command(game: &mut Game<'_>, player: &NativeServerPlayer, command: &str, ran: &mut bool) {
    let mut words = command.splitn(2, ' ');
    let root = words.next().unwrap_or("");
    let rest = words.next().unwrap_or("").trim();
    if !matches!(root, "horizonbot" | "hbot" | "bot") {
        return;
    }
    *ran = true;
    // A single player's owner console reaches here as level 4; a real console is `Who::Console`
    // only through scripts (no console command hook: GAPS.md).
    let who = Who::Player(info_of(player));
    let effects = pb::guarded("a command", |g| {
        let Some(p) = g.plugin.as_mut() else {
            return vec![Effect::Tell { to: pc::scripts::Sender::Player(player.uuid), text: "HorizonBot has no config yet".into() }];
        };
        p.command(&who, rest)
    });
    // Inside a player's hook other players are out of reach and console commands fail
    // (`native_run_command`): answer the sender now, queue the rest for the tick.
    let (now, later): (Vec<Effect>, Vec<Effect>) = effects.into_iter().partition(|e| matches!(e, Effect::Tell { to: pc::scripts::Sender::Player(u), .. } if *u == player.uuid));
    for e in now {
        if let Effect::Tell { text, .. } = e {
            game.native_feedback(Text::literal(&text));
        }
    }
    pb::with(|g| g.queue.effects.extend(later));
}

/// Reloads the files when `/horizonbot reload` ran (the plugin only answers it).
fn reload_files() {
    pb::with(|g| {
        let text = std::fs::read_to_string("config/horizonbot-common.toml").unwrap_or_else(|_| g.config_text.clone());
        let files = pb::read_files();
        if let Some(p) = g.plugin.as_mut() {
            if let Err(e) = p.reload(&text, &files) {
                eprintln!("[horizonbot] reload failed: {e}");
            }
        }
    });
}

// ------------------------------------------------------------------ the server tick

/// The tick's start, before any level ticks: the bots' clients read what the server sent,
/// the plugin's own tick runs (scripts, ghosts, arena, patrols) and its effects are carried out.
pub(super) fn server_tick_start(game: &mut Game<'_>) {
    // The ghosts' entity ids, for the damage hook.
    if let Some(host) = game.lan.as_ref() {
        let ghosts: std::collections::BTreeSet<u128> = pb::with(|g| g.conns.iter().filter(|c| c.ghost).map(|c| c.uuid).collect());
        pb::set_ghost_entities(host.hub.guests.iter().filter(|g| ghosts.contains(&g.uuid)).map(|g| g.entity_id).collect());
        let bypass = pb::with(|g| g.plugin.as_ref().is_some_and(|p| p.settings.get("bypass-damage-protection", None, None).as_bool()));
        pb::set_bot_entities(host.hub.guests.iter().filter(|g| pb::is_bot(g.uuid)).map(|g| g.entity_id).collect(), bypass);
    }
    // The event channel of the network threads, once.
    if let Some(host) = game.lan.as_ref() {
        let shared = host.hub.lan.shared.clone();
        pb::with(|g| {
            if g.shared.is_none() {
                g.shared = Some(shared);
            }
        });
    } else {
        return;
    }
    // Probe only: a file `probe_spawn` in the server folder holds a bot count; spawn that many.
    if let Ok(t) = std::fs::read_to_string("probe_spawn") {
        let _ = std::fs::remove_file("probe_spawn");
        let n: u32 = t.trim().parse().unwrap_or(0);
        let mut left = n;
        while left > 0 {
            let k = left.min(50);
            left -= k;
            let effects = pb::guarded("a probe spawn", |g| g.plugin.as_mut().map(|p| p.command(&Who::Console, &format!("spawn {k}"))).unwrap_or_default());
            run_effects(game, effects);
        }
    }
    admit_queued(game);
    mark_fake_players(game);
    horizon_client::spans::time("horizonbot_answer", || answer_server(game));
    // The real players only: a record built for each of hundreds of bots was a large part of the tick.
    let real: Vec<u128> = game
        .lan
        .as_ref()
        .map(|h| h.hub.guests.iter().filter(|g| !pb::is_bot(g.uuid)).map(|g| g.uuid).collect())
        .unwrap_or_default();
    let players: Vec<PlayerInfo> = real.into_iter().filter_map(|u| game.native_server_player(u)).map(|p| info_of(&p)).collect();
    // Skins that came back from their lookups.
    let mut effects = pb::with(|g| {
        let mut out = Vec::new();
        while let Ok((account, skin)) = g.skins_rx.try_recv() {
            let skin = skin.map(|(value, signature)| pc::skins::Skin { value, signature });
            let chosen = g.plugin.as_mut().and_then(|p| p.skin_fetched(&account, skin));
            let waiting: Vec<usize> = g.waiting.iter().enumerate().filter(|(_, (a, _))| a.eq_ignore_ascii_case(&account)).map(|(i, _)| i).collect();
            for i in waiting.into_iter().rev() {
                let (_, mut e) = g.waiting.remove(i);
                if let Effect::SpawnBot { skin: s, .. } | Effect::SpawnGhost { skin: s, .. } = &mut e {
                    *s = chosen.clone().map_or(SkinSource::None, SkinSource::Known);
                }
                out.push(e);
            }
        }
        out
    });
    let (tick, more) = pb::guarded("the plugin's tick", |g| {
        let tick = g.tick;
        let mut out = std::mem::take(&mut g.queue.effects);
        let damage = std::mem::take(&mut g.queue.damage);
        if let Some(p) = g.plugin.as_mut() {
            for &(v, c, a) in &damage {
                p.damaged(v, c, f64::from(a));
            }
            for (v, c) in pb::take_cancelled() {
                p.cancelled_hit(v, c);
            }
            // Ghost recordings follow their players every tick.
            for pl in &players {
                p.record(pl.uuid, pl.pos, pl.yaw, 0.0, false);
            }
            out.extend(p.begin_tick(tick, players));
        }
        g.last_damage = damage;
        g.tick += 1;
        (tick, out)
    });
    effects.extend(more);
    let _ = tick;
    run_effects(game, effects);
    // Teleports waiting for their bot to be placed.
    // (Only while one waits: listing every player is the costly part with hundreds of bots.)
    let waiting = pb::with(|g| !g.pending_tp.is_empty());
    let placed: Vec<String> =
        if waiting { game.native_server_players().into_iter().map(|p| p.name.to_ascii_lowercase()).collect() } else { Vec::new() };
    let due = pb::with(|g| {
        let mut due = Vec::new();
        g.pending_tp.retain_mut(|(name, dim, pos, left)| {
            if placed.contains(&name.to_ascii_lowercase()) {
                due.push((name.clone(), dim.clone(), *pos));
                return false;
            }
            *left = left.saturating_sub(1);
            *left > 0
        });
        due
    });
    for (name, dim, pos) in due {
        teleport_by_name(game, &name, &dim, pos);
    }
    // A bot starts as a fresh survival player with nothing on it: the server may force creative, and an
    // old save under the same name may hold items.
    let fresh = pb::with(|g| std::mem::take(&mut g.fresh));
    for (uuid, name) in fresh {
        game.native_set_game_mode(uuid, 0);
        game.native_run_command(&format!("clear {name}"));
    }
}

/// Lets queued bots in: at most `joins-per-tick` a tick, and no more once the joins of this tick have
/// used `JOIN_BUDGET_MS` of wall time (at least one a tick), so a big spawn does not stall one tick.
fn admit_queued(game: &mut Game<'_>) {
    // The time since this ran last = how long the last server tick took (`joins_for_tick`).
    let last_tick_ms = LAST_ADMIT.with(|c| {
        let now = std::time::Instant::now();
        c.replace(Some(now)).map_or(0.0, |t| now.duration_since(t).as_secs_f64() * 1000.0)
    });
    pb::with(|g| {
        // Chunks held for bots that have joined by now: let go after a while (a bot keeps its own).
        if !g.held_chunks.is_empty() {
            let due: Vec<(String, i32, i32)> = g
                .held_chunks
                .iter_mut()
                .filter_map(|(k, t)| {
                    *t = t.saturating_sub(1);
                    (*t == 0).then(|| k.clone())
                })
                .collect();
            for k in due {
                g.held_chunks.remove(&k);
                game.native_chunk_load_release("horizonbot:spawn", &k.0, [k.1, k.2]);
            }
        }
        if g.join_queue.is_empty() {
            return;
        }
        let per_tick = g.plugin.as_ref().map_or(pc::config::DEFAULT_JOINS_PER_TICK, |p| p.config.joins_per_tick);
        let per_tick = pc::config::joins_for_tick(per_tick, last_tick_ms);
        let started = std::time::Instant::now();
        let mut joined = 0u32;
        // A bot whose chunk is not loaded yet waits (and the chunk is asked for): placed there, it would
        // fall through the empty ground and die. Waiting bots go to the back; after 600 ticks one comes in anyway.
        let mut waiting = Vec::new();
        let mut scanned = 0usize;
        let total = g.join_queue.len();
        while scanned < total && pc::config::may_join_more(per_tick, joined, started.elapsed().as_secs_f64() * 1000.0) {
            scanned += 1;
            let Some((name, uuid, skin, spawn)) = g.join_queue.pop_front() else { break };
            if let Some((dim, pos, _)) = &spawn {
                let chunk = [(pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4];
                if !game.native_chunk_is_loaded(dim, chunk) {
                    let waited = g.chunk_wait.entry(name.to_ascii_lowercase()).or_insert(0);
                    *waited += 1;
                    if *waited < 600 {
                        if *waited % 20 == 1 {
                            game.native_chunk_load_request("horizonbot:spawn", dim, chunk);
                        }
                        waiting.push((name, uuid, skin, spawn));
                        continue;
                    }
                }
                g.chunk_wait.remove(&name.to_ascii_lowercase());
                // Held while the bot joins and finds its feet.
                g.held_chunks.insert((dim.clone(), chunk[0], chunk[1]), 400);
                game.native_chunk_load_request("horizonbot:spawn", dim, chunk);
            }
            // Announced before the join, so the hub makes the bot a fake player from its first packet,
            // and places it where it was spawned (no hop from the world spawn).
            let placed = match &spawn {
                Some((dim, pos, yaw)) => game.native_expect_fake_player_at(&name, dim, *pos, *yaw),
                None => game.native_expect_fake_player(&name, true),
            };
            if !pb::connect(g, &name, uuid, skin, false) {
                game.native_expect_fake_player(&name, false);
                eprintln!("[horizonbot] {name} could not join");
            } else if let (false, Some((dim, pos, _))) = (placed, &spawn) {
                // Fallback only (the place could not be announced): a teleport once it is placed.
                let wait = pc::config::placement_wait_ticks(g.join_queue.len());
                g.pending_tp.push((name.clone(), dim.clone(), *pos, wait));
            }
            joined += 1;
        }
        g.join_queue.extend(waiting);
    });
}

thread_local! {
    /// When `admit_queued` last ran.
    static LAST_ADMIT: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    /// The bots already marked as fake players on the server.
    static FAKE_MARKED: RefCell<std::collections::HashSet<u128>> = RefCell::new(std::collections::HashSet::new());
}

/// A bot is a fake player to the server (`native_set_player_fake`: local perf patch for fake
/// players): it keeps only a small area around it loaded and ticking and does not anchor natural
/// mob spawning, instead of a whole view distance each. Marked once it has joined the hub (tried
/// every tick until it takes).
fn mark_fake_players(game: &mut Game<'_>) {
    let uuids: Vec<u128> = pb::with(|g| g.conns.iter().map(|c| c.uuid).collect());
    let todo: Vec<u128> = FAKE_MARKED.with(|m| {
        let mut m = m.borrow_mut();
        let mut todo = Vec::new();
        let mut known = 0;
        for &u in &uuids {
            if m.contains(&u) {
                known += 1;
            } else {
                todo.push(u);
            }
        }
        // Someone marked has left (the connections are all different): forget them, with a set.
        if known < m.len() {
            let live: std::collections::HashSet<u128> = uuids.iter().copied().collect();
            m.retain(|u| live.contains(u));
        }
        todo
    });
    for u in todo {
        if game.native_set_player_fake(u, true) {
            FAKE_MARKED.with(|m| m.borrow_mut().insert(u));
        }
    }
}

/// Each bot's "client" handles what the server sent it: teleports are accepted, knockback taken,
/// chunk batches acknowledged, keep-alives and pings answered, the arrival confirmed.
fn answer_server(game: &mut Game<'_>) {
    let kicked: Vec<(u128, Option<String>)> = pb::with(|g| {
        let mut out: Vec<(u32, S)> = Vec::new();
        let mut moved: Vec<(String, Option<Vec3>, Option<Vec3>)> = Vec::new();
        let mut kicked: Vec<u128> = Vec::new();
        // Knockback the server sent: the engine's own momentum for the bot's next tick starts from it.
        let mut pushed: Vec<(u128, [f64; 3])> = Vec::new();
        for c in g.conns.iter_mut() {
            // A bot's entity id is read from the hub once (it never changes), not for every bot each tick.
            if c.entity_id == 0 {
                c.entity_id = game.lan.as_ref().and_then(|h| h.hub.guest_by_uuid(c.uuid)).map_or(0, |g| g.entity_id);
            }
            let my_id = if c.entity_id == 0 { -1 } else { c.entity_id };
            while let Ok(o) = c.rx.try_recv() {
                let bodies = match o {
                    horizon_client::lan::Out::Packet(b) => vec![b],
                    horizon_client::lan::Out::Packets(v) => v,
                    horizon_client::lan::Out::Disconnect(_) => {
                        c.dead = true;
                        kicked.push(c.uuid);
                        continue;
                    }
                };
                for body in bodies {
                    let mut r = horizon_protocol::buf::Reader::new(&body);
                    let Ok(id) = r.varint() else { continue };
                    // Only the few a bot answers are decoded (chunks are the bulk of the traffic).
                    use horizon_protocol::ids::play::clientbound as cb;
                    if ![cb::PLAYER_POSITION, cb::SET_ENTITY_MOTION, cb::CHUNK_BATCH_FINISHED, cb::KEEP_ALIVE, cb::PING, cb::PLAYER_COMBAT_KILL, cb::RESPAWN].contains(&id) {
                        continue;
                    }
                    let Ok(packet) = C::decode(id, r.rest()) else { continue };
                    match packet {
                        C::PlayerPosition(p) => {
                            let prev = c.teleport.map_or([0.0; 3], |t| t.1);
                            let pos: [f64; 3] = std::array::from_fn(|i| if p.relatives & (1 << i) != 0 { prev[i] + p.position[i] } else { p.position[i] });
                            out.push((c.conn, S::AcceptTeleportation(play::AcceptTeleportation { id: p.teleport, x: pos[0], y: pos[1], z: pos[2], yaw: p.yaw, pitch: p.pitch })));
                            c.teleport = Some((p.teleport, pos));
                            moved.push((c.name.clone(), Some(Vec3::from_array(pos)), Some(Vec3::from_array(p.delta))));
                            if !c.loaded {
                                c.loaded = true;
                                out.push((c.conn, S::PlayerLoaded(horizon_protocol::packets::Empty)));
                            }
                        }
                        C::SetEntityMotion(m) if m.id == my_id => {
                            c.motion = Some(m.velocity);
                            pushed.push((c.uuid, m.velocity));
                            moved.push((c.name.clone(), None, Some(Vec3::from_array(m.velocity))));
                        }
                        C::ChunkBatchFinished(_) => out.push((c.conn, S::ChunkBatchReceived(play::ChunkBatchReceived { chunks_per_tick: 32.0 }))),
                        C::KeepAlive(k) => out.push((c.conn, S::KeepAlive(k))),
                        C::Ping(p) => out.push((c.conn, S::Pong(p))),
                        C::PlayerCombatKill(_) => c.dead = true,
                        C::Respawn(_) => c.dead = false,
                        _ => {}
                    }
                }
            }
        }
        for (conn, p) in out {
            pb::send(g, conn, p);
        }
        // `engine_move` starts from the last engine tick's velocity, so a push the server sent
        // (a hit's knockback, a pearl, a fall into a bounce) has to replace it there too, or the
        // bot walks through its own knockback as if nothing hit it.
        for (uuid, v) in pushed {
            let at = game.lan.as_ref().and_then(|h| h.hub.guest_by_uuid(uuid)).map(|g| Vec3::from_array(g.position));
            ENGINE_MOVES.with(|e| {
                let mut e = e.borrow_mut();
                if let Some(at) = at {
                    e.insert(uuid, (at, v));
                } else if let Some(entry) = e.get_mut(&uuid) {
                    entry.1 = v;
                }
            });
        }
        if let Some(plugin) = g.plugin.as_mut() {
            for (name, pos, vel) in moved {
                plugin.moved(&name, pos, vel);
            }
        }
        // A kicked bot's "client" closes the connection, as a real one does on the disconnect
        // packet: the server then takes the player out and says it left (not a quiet leave).
        kicked
            .into_iter()
            .map(|uuid| {
                let name = g.plugin.as_mut().and_then(|p| p.kicked(uuid));
                pb::disconnect(g, uuid);
                (uuid, name)
            })
            .collect()
    });
    for (uuid, name) in kicked {
        FAKE_MARKED.with(|m| m.borrow_mut().remove(&uuid));
        if let Some(name) = name {
            game.native_expect_fake_player(&name, false);
        }
    }
}

// ------------------------------------------------------------------ a level's bots think

thread_local! {
    /// What a block state is to a bot, by state id (an index, no hashing): stringifying and
    /// classifying a state was done for every block a bot looked at, every tick (physics, paths
    /// and every line of sight), and was the glue's largest cost. A state id means the same for
    /// as long as the server runs.
    static STATES: RefCell<Vec<Option<pc::world::Block>>> = const { RefCell::new(Vec::new()) };
}

/// The level in place as a bot reads it.
struct LevelView<'a, 'g> {
    game: &'a Game<'g>,
    bottom: i32,
    top: i32,
    /// The last chunk found loaded (the level does not change while bots think): consecutive reads
    /// mostly fall in the same chunk, so the chunk map is asked once.
    last_chunk: std::cell::Cell<(i32, i32)>,
}

impl<'a, 'g> LevelView<'a, 'g> {
    fn new(game: &'a Game<'g>) -> Self {
        LevelView { game, bottom: game.world.bottom_y(), top: game.world.top_y(), last_chunk: std::cell::Cell::new((i32::MAX, i32::MAX)) }
    }
}

/// Block reads of the bots' thinking since the last report (a profiling counter).
static BLOCK_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
impl pc::world::BlockView for LevelView<'_, '_> {
    fn block(&self, x: i32, y: i32, z: i32) -> pc::world::Block {
        if y < self.bottom || y >= self.top {
            return pc::world::Block::air();
        }
        let world = &self.game.world;
        let key = (x >> 4, z >> 4);
        if self.last_chunk.get() != key {
            if !world.chunks.contains_key(&key) {
                return pc::world::Block::unloaded();
            }
            self.last_chunk.set(key);
        }
        // VERIFY: `world.block` and `state_string` (game/agent.rs) as the level refs use them.
        let state = world.block(x, y, z);
        BLOCK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let idx = state.0 as usize;
        STATES.with(|m| {
            let mut m = m.borrow_mut();
            if idx >= m.len() {
                m.resize(idx + 1, None);
            }
            m[idx].get_or_insert_with(|| pc::world::classify(&self.game.state_string(state))).clone()
        })
    }
}

fn stack_of(s: &horizon_client::items::ItemStack) -> pc::items::Stack {
    let mut st = pc::items::Stack::new(&s.item, s.count);
    st.damage = s.damage;
    if let Some(x) = &s.extra {
        st.enchants = x.enchantments.iter().map(|(e, l)| (e.to_string(), *l)).collect();
        st.potion = x.potion.as_ref().map(|p| p.to_string());
    }
    st
}

fn mob_shape(kind: &str) -> (f64, f64, bool, bool) {
    // (width, height, hostile, passive)
    let k = kind.strip_prefix("minecraft:").unwrap_or(kind);
    let hostile = matches!(
        k,
        "zombie" | "husk" | "drowned" | "skeleton" | "stray" | "bogged" | "creeper" | "spider" | "cave_spider"
            | "witch" | "pillager" | "vindicator" | "evoker" | "ravager" | "blaze" | "wither_skeleton"
            | "zombie_villager" | "phantom" | "slime" | "magma_cube" | "hoglin" | "zoglin" | "piglin_brute"
            | "guardian" | "elder_guardian" | "silverfish" | "endermite" | "vex" | "breeze" | "warden" | "creaking"
            | "ghast" | "shulker" | "parched" | "zombie_horse"
    );
    let (w, h) = match k {
        "spider" => (1.4, 0.9),
        "cave_spider" => (0.7, 0.5),
        "creeper" => (0.6, 1.7),
        "skeleton" | "stray" | "bogged" => (0.6, 1.99),
        "wither_skeleton" => (0.7, 2.4),
        "enderman" => (0.6, 2.9),
        "cow" | "mooshroom" => (0.9, 1.4),
        "pig" => (0.9, 0.9),
        "sheep" => (0.9, 1.3),
        "chicken" => (0.4, 0.7),
        "villager" | "wandering_trader" | "witch" | "pillager" | "vindicator" | "evoker" => (0.6, 1.95),
        "ravager" => (1.95, 2.2),
        "iron_golem" => (1.4, 2.7),
        _ => (0.6, 1.95),
    };
    (w, h, hostile, !hostile)
}

/// A bot as a level's tick sees it: who it is, its connection, its name in lower case (the plugin's
/// key) and where its connection sat in `Glue::conns` when the tick began (checked before it is used).
struct BotRef {
    uuid: u128,
    conn: u32,
    name: String,
    key: String,
    slot: usize,
}

thread_local! {
    /// What each bot knows of itself, kept from tick to tick: its inventory view is rebuilt only when
    /// the inventory changed (the fingerprint), not allocated afresh for 68 slots every tick.
    static SELVES: RefCell<HashMap<u128, (u64, SelfView)>> = RefCell::new(HashMap::new());
}

fn mix(h: u64, x: u64) -> u64 {
    (h.rotate_left(5) ^ x).wrapping_mul(0x517c_c1b7_2722_0a95)
}

fn mix_str(mut h: u64, s: &str) -> u64 {
    let mut b = s.as_bytes();
    while b.len() >= 8 {
        let mut a = [0u8; 8];
        a.copy_from_slice(&b[..8]);
        h = mix(h, u64::from_le_bytes(a));
        b = &b[8..];
    }
    for &x in b {
        h = mix(h, u64::from(x));
    }
    mix(h, s.len() as u64)
}

/// A fingerprint of what `build_inventory` reads from a slot (item, count, damage, enchantments, potion).
fn slot_fp(h: u64, s: &Option<horizon_client::items::ItemStack>) -> u64 {
    let Some(s) = s else { return mix(h, 0) };
    let mut h = mix_str(mix(h, 1), &s.item);
    h = mix(mix(h, s.count as u64), s.damage as u64);
    if let Some(x) = &s.extra {
        for (e, l) in x.enchantments.iter() {
            h = mix(mix_str(h, e), *l as u64);
        }
        if let Some(p) = &x.potion {
            h = mix_str(mix(h, 2), p);
        }
    }
    h
}

/// A fingerprint of everything `build_inventory` reads, without allocating.
fn inventory_fp(
    inv: &[Option<horizon_client::items::ItemStack>],
    armor: &[Option<horizon_client::items::ItemStack>],
    offhand: &Option<horizon_client::items::ItemStack>,
    selected: usize,
    ender: &[Option<horizon_client::items::ItemStack>],
) -> u64 {
    let mut h = mix(0, selected as u64);
    for s in inv.iter().take(36).chain(armor.iter()) {
        h = slot_fp(h, s);
    }
    h = slot_fp(h, offhand);
    for s in ender.iter().take(27) {
        h = slot_fp(h, s);
    }
    mix(h, (inv.len() + ender.len()) as u64)
}

/// The brain's view of a guest's inventory.
fn build_inventory(
    slots: &[Option<horizon_client::items::ItemStack>],
    armor: &[Option<horizon_client::items::ItemStack>],
    offhand: &Option<horizon_client::items::ItemStack>,
    selected: usize,
    ender: &[Option<horizon_client::items::ItemStack>],
) -> pc::brain::inventory::Inventory {
    let mut inv = pc::brain::inventory::Inventory::new();
    for (i, s) in slots.iter().enumerate().take(36) {
        inv.slots[i] = s.as_ref().map(stack_of);
    }
    // The guest's armour is head..feet (`Guest::armor`); the brain's feet first.
    for (i, s) in armor.iter().enumerate() {
        inv.armor[3 - i] = s.as_ref().map(stack_of);
    }
    inv.offhand = offhand.as_ref().map(stack_of);
    inv.selected = selected;
    for (i, s) in ender.iter().enumerate().take(27) {
        inv.ender[i] = s.as_ref().map(stack_of);
    }
    inv
}

/// How far a bot hears sprinting players (`events_for`).
const HEARD_REACH: f64 = 33.0;

/// Adds the player named `name` (any case) to `out`, if one is on. The name table is made on first use.
fn add_named(names: &mut Option<HashMap<String, u32>>, seen: &[Seen], players: &[u32], name: &str, out: &mut Vec<u32>) {
    let table = names.get_or_insert_with(|| {
        let mut m: HashMap<String, u32> = HashMap::new();
        for &i in players {
            m.entry(seen[i as usize].name.to_ascii_lowercase()).or_insert(i);
        }
        m
    });
    if let Some(&i) = table.get(&name.to_ascii_lowercase()) {
        out.push(i);
    }
}

/// Adds the player with entity id `id` to `out`, if one is on.
fn add_id(seen: &[Seen], by_id: &HashMap<i32, u32>, id: i32, out: &mut Vec<u32>) {
    if let Some(&i) = by_id.get(&id) {
        if seen[i as usize].is_player {
            out.push(i);
        }
    }
}

/// The players a bot perceives however far they are: the brain looks them up by name or id
/// (`Perception::by_name` / `get`: its orders, its faction's leader and calls, who hurt it, its fight
/// target) and would otherwise find nobody past its reach. Everyone else only matters within reach.
#[allow(clippy::too_many_arguments)]
fn explicit_players(
    plugin: &pc::plugin::Plugin,
    key: &str,
    seen: &[Seen],
    players: &[u32],
    by_id: &HashMap<i32, u32>,
    events: &[Event],
    names: &mut Option<HashMap<String, u32>>,
    out: &mut Vec<u32>,
) {
    let Some(live) = plugin.bots.get(key) else { return };
    let bot = &live.bot;
    if let Some(n) = &bot.orders.attack {
        add_named(names, seen, players, n, out);
    }
    if let Some(t) = &bot.orders.tether {
        add_named(names, seen, players, &t.target, out);
        add_named(names, seen, players, &t.leader, out);
    }
    if let Some(r) = &bot.orders.retreat {
        if let pc::brain::RetreatTo::Player(n) = &r.to {
            add_named(names, seen, players, n, out);
        }
    }
    if let Some(m) = &bot.orders.march {
        if let pc::brain::MarchTo::Player(n) = &m.to {
            add_named(names, seen, players, n, out);
        }
    }
    if let Some(f) = &bot.orders.follow {
        add_named(names, seen, players, &f.target, out);
    }
    if let Some(f) = bot.faction.as_deref() {
        if let Some(l) = plugin.squad.factions.leader_of(f) {
            add_named(names, seen, players, l, out);
        }
        for (k, _) in plugin.squad.factions.calls.iter() {
            if k.0 == f {
                add_named(names, seen, players, &k.1, out);
            }
        }
    }
    if let Some((id, _)) = bot.revenge {
        add_id(seen, by_id, id, out);
    }
    if let Some(id) = bot.fight.target {
        add_id(seen, by_id, id, out);
    }
    for e in events {
        match e {
            Event::Hurt { by, .. } | Event::Heard { by, .. } => add_id(seen, by_id, *by, out),
            Event::Landed { entity } => add_id(seen, by_id, *entity, out),
            _ => {}
        }
    }
}

/// `level_tick_pre`: the bots in this level think and act (mobs and blocks are readable only
/// with the level in place).
pub(super) fn level_tick(game: &mut Game<'_>, dimension: &str) {
    if game.lan.is_none() {
        return;
    }
    let (tick, bots, damage): (u64, Vec<BotRef>, Vec<(i32, i32, f32)>) = pb::with(|g| {
        (
            g.tick,
            g.conns
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.ghost)
                .map(|(slot, c)| BotRef { uuid: c.uuid, conn: c.conn, name: c.name.clone(), key: c.key.clone(), slot })
                .collect(),
            g.last_damage.clone(),
        )
    });
    if bots.is_empty() {
        return;
    }
    // What everyone can see: the players (bots too) of this level and its mobs.
    let perceive_span = horizon_client::spans::guard("horizonbot_perceive");
    let mut seen: Vec<Seen> = Vec::new();
    let mut selves: HashMap<u128, (u64, SelfView)> = SELVES.with(|s| std::mem::take(&mut *s.borrow_mut()));
    let mut ground: Vec<Ground> = Vec::new();
    {
        let Some(host) = game.lan.as_ref() else { return };
        // The glue stays locked while the guests are read (nothing in the loop takes it): the factions
        // are borrowed, not cloned every tick.
        pb::with(|glue| {
            let factions = glue.plugin.as_ref().map(|p| &p.squad.factions);
            for gst in host.hub.guests.iter().filter(|g| g.placed && g.dimension == dimension) {
                let effects = gst.effects.saved(); // VERIFY: PlayerEffects::saved
                let held = gst.inventory.get(gst.selected).and_then(Option::as_ref).map(|s| s.item.to_string());
                let offhand = gst.offhand.as_ref().map(|s| s.item.to_string());
                let armor: f64 = gst
                    .armor
                    .iter()
                    .flatten()
                    .filter_map(|s| pc::items::armor(&s.item).map(|(_, p, _)| p))
                    .sum();
                let is_bot = pb::is_bot(gst.uuid);
                seen.push(Seen {
                    id: gst.entity_id,
                    uuid: gst.uuid,
                    name: gst.name.clone(),
                    // (The brain never reads a player's kind: no string to make.)
                    kind: String::new(),
                    is_player: true,
                    is_bot,
                    faction: factions.and_then(|f| f.faction_of(&gst.name).map(str::to_owned)),
                    pos: Vec3::from_array(gst.position),
                    vel: Vec3::from_array(gst.position) - Vec3::from_array(gst.previous_position),
                    yaw: gst.yaw,
                    pitch: gst.pitch,
                    on_ground: gst.on_ground,
                    health: gst.health,
                    max_health: 20.0,
                    armor,
                    totem_shown: held.as_deref() == Some("minecraft:totem_of_undying") || offhand.as_deref() == Some("minecraft:totem_of_undying"),
                    // GAP: no "is blocking" on a guest; a shield in hand while using is the best read.
                    blocking: offhand.as_deref() == Some("minecraft:shield") && gst.living_flags & 1 != 0,
                    sneaking: gst.sneaking,
                    sprinting: gst.sprinting,
                    invisible: effects.iter().any(|e| e.id.ends_with("invisibility")),
                    gliding: gst.fall_flying,
                    game_mode: gst.game_mode,
                    held,
                    offhand,
                    hostile: false,
                    passive: false,
                    width: 0.6,
                    height: if gst.sneaking { 1.5 } else { 1.8 },
                    eye_height: if gst.sneaking { 1.27 } else { 1.62 },
                    swung: false,
                    using: gst.living_flags & 1 != 0,
                });
                if is_bot {
                    // The bot's view of itself is kept: only what changes every tick is written again, and
                    // the inventory only when its fingerprint changed.
                    let fp = inventory_fp(&gst.inventory, &gst.armor, &gst.offhand, gst.selected, &gst.ender_items);
                    let (old_fp, view) = selves.entry(gst.uuid).or_insert_with(|| (!fp, SelfView::default()));
                    if *old_fp != fp {
                        view.inventory = build_inventory(&gst.inventory, &gst.armor, &gst.offhand, gst.selected, &gst.ender_items);
                        *old_fp = fp;
                    }
                    view.health = gst.health;
                    view.absorption = gst.saved_absorption.unwrap_or(0.0);
                    view.food = gst.food;
                    view.saturation = gst.saturation;
                    view.effects.clear();
                    // SelfView::effects is (id, amplifier, ticks left): the amplifier comes first.
                    view.effects.extend(effects.iter().map(|e| (e.id.trim_start_matches("minecraft:").to_owned(), e.amplifier.max(0) as u32, e.duration.max(0) as u32)));
                    view.game_mode = gst.game_mode;
                    // VERIFY: `PlayerMotion::fire_ticks` (as `SavedBody::of` reads it).
                    view.on_fire = gst.motion.as_ref().is_some_and(|m| m.fire_ticks > 0);
                    // GAP: item cooldowns of a guest are not read here (the map stays empty).
                    if view.dimension != gst.dimension {
                        view.dimension = gst.dimension.clone();
                    }
                    view.dead = gst.dead;
                    view.invulnerable = false;
                }
            }
        });
    }
    for m in game.native_mobs(dimension) {
        let (w, h, hostile, passive) = mob_shape(&m.kind);
        seen.push(Seen {
            id: m.id as i32,
            uuid: 0,
            name: m.kind.clone(),
            kind: m.kind.clone(),
            pos: Vec3::from_array(m.pos),
            yaw: m.yaw,
            health: m.health,
            max_health: m.max_health,
            hostile,
            passive,
            width: w,
            height: h,
            eye_height: h * 0.85,
            on_ground: true,
            ..Default::default()
        });
    }
    // Who each bot perceives is a list of indices into `seen`, so no bot copies the others: the
    // players and the mobs within the bot's reach (and the few it must know of by name or id, however
    // far), from a grid of each.
    let mut player_idx: Vec<u32> = Vec::new();
    let mut player_grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    let mut mob_grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    let mut by_id: HashMap<i32, u32> = HashMap::new();
    let mut by_uuid: HashMap<u128, u32> = HashMap::new();
    for (i, s) in seen.iter().enumerate() {
        by_id.insert(s.id, i as u32);
        let cell = ((s.pos.x.floor() as i32) >> 4, (s.pos.z.floor() as i32) >> 4);
        if s.is_player {
            player_idx.push(i as u32);
            by_uuid.insert(s.uuid, i as u32);
            player_grid.entry(cell).or_default().push(i as u32);
        } else {
            mob_grid.entry(cell).or_default().push(i as u32);
        }
    }
    // Items on the ground near any bot (after-fight looting): one scan of the level's items, not one
    // per bot (each scan walks every dropped item). A bot within 8 blocks of an item is at most one
    // 16-block cell away from it, so only the item's cell and its neighbours are asked.
    let mut spot_grid: HashMap<(i32, i32), Vec<Vec3>> = HashMap::new();
    for b in &bots {
        if let Some(&i) = by_uuid.get(&b.uuid) {
            let at = seen[i as usize].pos;
            spot_grid.entry(((at.x.floor() as i32) >> 4, (at.z.floor() as i32) >> 4)).or_default().push(at);
        }
    }
    // VERIFY: native_item_entities(dimension, min, max) (API 41).
    for it in game.native_item_entities(dimension, [-3.0e7, -4096.0, -3.0e7], [3.0e7, 4096.0, 3.0e7]) {
        let at = Vec3::from_array(it.position);
        let (cx, cz) = ((at.x.floor() as i32) >> 4, (at.z.floor() as i32) >> 4);
        let near = (cx - 1..=cx + 1).any(|x| {
            (cz - 1..=cz + 1).any(|z| {
                spot_grid.get(&(x, z)).is_some_and(|spots| {
                    spots.iter().any(|b| (at.x - b.x).abs() <= 8.0 && (at.y - b.y).abs() <= 4.0 && (at.z - b.z).abs() <= 8.0)
                })
            })
        });
        if near {
            ground.push(Ground { id: it.id as i32, item: it.stack.item.to_string(), count: it.stack.count, pos: at });
        }
    }
    // Where whoever struck last tick stood, for the bots that hear it: (victim, striker, position).
    let heard: Vec<(i32, i32, Vec3)> = damage.iter().filter_map(|&(victim, causing, _)| by_id.get(&causing).map(|&i| (victim, causing, seen[i as usize].pos))).collect();
    drop(perceive_span);
    // Think.
    let think_span = horizon_client::spans::guard("horizonbot_think");
    let view = LevelView::new(game);
    let decisions: Vec<(usize, pc::brain::Decision, Vec3, bool, bool)> = pb::guarded("the bots' turn", |g| {
        let mut out = Vec::new();
        let Some(plugin) = g.plugin.as_mut() else { return out };
        let mut idx: Vec<u32> = Vec::new();
        let mut near: Vec<u32> = Vec::new();
        let mut extra: Vec<u32> = Vec::new();
        let mut names: Option<HashMap<String, u32>> = None;
        for (bi, b) in bots.iter().enumerate() {
            let Some((_, me)) = selves.get(&b.uuid) else { continue };
            let Some(&mine) = by_uuid.get(&b.uuid) else { continue };
            plugin.bot_world(&b.name, dimension);
            let my_pos = seen[mine as usize].pos;
            // How far a bot perceives: the farthest it can notice a target, and a margin.
            let reach = plugin.bots.get(&b.key).map_or(48.0, |l| l.bot.settings.f("max-target-distance").max(16.0) + 16.0);
            // The players near it (and, for what it hears, a little farther), in the order of `seen`.
            let ask = reach.max(HEARD_REACH);
            near.clear();
            let (p0x, p1x) = (((my_pos.x - ask).floor() as i32) >> 4, ((my_pos.x + ask).floor() as i32) >> 4);
            let (p0z, p1z) = (((my_pos.z - ask).floor() as i32) >> 4, ((my_pos.z + ask).floor() as i32) >> 4);
            let mut cells = 0;
            for cx in p0x..=p1x {
                for cz in p0z..=p1z {
                    if let Some(cell) = player_grid.get(&(cx, cz)) {
                        near.extend_from_slice(cell);
                        cells += 1;
                    }
                }
            }
            if cells > 1 {
                near.sort_unstable();
            }
            let t_events = std::time::Instant::now();
            let events = events_for(&seen, &near, &heard, mine, &damage);
            pc::brain::prof_add(pc::brain::P_GLUE_EVENTS, t_events.elapsed().as_nanos() as u64);
            idx.clear();
            idx.extend(near.iter().copied().filter(|&i| i != mine && seen[i as usize].pos.distance(my_pos) <= reach));
            extra.clear();
            explicit_players(plugin, &b.key, &seen, &player_idx, &by_id, &events, &mut names, &mut extra);
            if !extra.is_empty() {
                idx.extend(extra.iter().copied().filter(|&i| i != mine));
                idx.sort_unstable();
                idx.dedup();
            }
            let (c0x, c1x) = (((my_pos.x - reach).floor() as i32) >> 4, ((my_pos.x + reach).floor() as i32) >> 4);
            let (c0z, c1z) = (((my_pos.z - reach).floor() as i32) >> 4, ((my_pos.z + reach).floor() as i32) >> 4);
            for cx in c0x..=c1x {
                for cz in c0z..=c1z {
                    if let Some(cell) = mob_grid.get(&(cx, cz)) {
                        idx.extend(cell.iter().copied().filter(|&i| seen[i as usize].pos.distance(my_pos) <= reach));
                    }
                }
            }
            let p = Perception { tick, me, others: pc::brain::Others::indexed(&seen, &idx), ground: &ground, events: &events, world: &view };
            // (Per-bot spans cost a clock read, a CPU-time read and a push each, 500 times a tick: a counter instead.)
            let t_brain = std::time::Instant::now();
            let thought = plugin.think(&b.name, &p);
            pc::brain::prof_add(pc::brain::P_GLUE_BRAIN, t_brain.elapsed().as_nanos() as u64);
            if let Some(d) = thought {
                let body = &plugin.bots[&b.key].bot.body;
                out.push((bi, d, body.pos, body.on_ground, body.horizontal_collision));
            }
        }
        out
    });
    drop(view);
    selves.retain(|u, _| pb::is_bot(*u));
    SELVES.with(|s| *s.borrow_mut() = selves);
    horizon_client::spans::counters(&[("horizonbot_block_reads", BLOCK_CALLS.swap(0, std::sync::atomic::Ordering::Relaxed))]);
    horizon_client::spans::counters(&pc::brain::prof_take());
    drop(think_span);
    // Act: the packets a client sends this tick, in a client's order.
    let act_span = horizon_client::spans::guard("horizonbot_engine_act");
    for (bi, d, pos, on_ground, hcol) in decisions {
        let b = &bots[bi];
        let t_engine = std::time::Instant::now();
        let moved = engine_move(game, b, &d);
        pc::brain::prof_add(pc::brain::P_GLUE_ENGINE, t_engine.elapsed().as_nanos() as u64);
        let (pos, on_ground, hcol) = moved.unwrap_or((pos, on_ground, hcol));
        let t_act = std::time::Instant::now();
        act(game, b, d, pos, on_ground, hcol);
        pc::brain::prof_add(pc::brain::P_GLUE_ACT, t_act.elapsed().as_nanos() as u64);
    }
    drop(act_span);
}

thread_local! {
    /// Each bot's last engine-played tick: where it ended and its velocity (the server doesn't
    /// keep a bot's momentum), by UUID.
    static ENGINE_MOVES: RefCell<HashMap<u128, (Vec3, [f64; 3])>> = RefCell::new(HashMap::new());
}

/// The bot's tick played by Horizon's own player movement (hook `native_simulate_player_move`,
/// API player_bots) instead of the core's physics model: the core's body takes the engine's
/// answer, so the brain plans from where vanilla's physics put it. None (the core's step stays)
/// where the engine can't play it (riding, a spectator, out of reach).
fn engine_move(game: &mut Game<'_>, b: &BotRef, d: &pc::brain::Decision) -> Option<(Vec3, bool, bool)> {
    let uuid = b.uuid;
    let at = game.lan.as_ref()?.hub.guest_by_uuid(uuid).map(|g| Vec3::from_array(g.position))?;
    // The velocity of the last engine tick, while the bot is still where that tick left it
    // (a teleport or a setback starts again from the server's copy).
    let velocity = ENGINE_MOVES.with(|m| m.borrow().get(&uuid).filter(|(pos, _)| (*pos - at).length() < 1.0e-3).map(|(_, v)| *v));
    let input = horizon_client::mod_api::NativePlayerInput {
        forward: d.input.forward as f32,
        strafe: d.input.strafe as f32,
        jump: d.input.jump,
        sneak: d.input.sneak,
        sprint: d.input.sprint,
        yaw: d.yaw,
        pitch: d.pitch,
        velocity,
    };
    let Some(m) = game.native_simulate_player_move(uuid, input) else {
        ENGINE_MOVES.with(|e| e.borrow_mut().remove(&uuid));
        return None;
    };
    let pos = Vec3::from_array(m.position);
    ENGINE_MOVES.with(|e| e.borrow_mut().insert(uuid, (pos, m.velocity)));
    pb::with(|g| {
        if let Some(live) = g.plugin.as_mut().and_then(|p| p.bots.get_mut(&b.key)) {
            let body = &mut live.bot.body;
            body.pos = pos;
            body.vel = Vec3::from_array(m.velocity);
            body.on_ground = m.on_ground;
            body.horizontal_collision = m.horizontal_collision;
            body.sprinting = m.sprinting;
        }
    });
    Some((pos, m.on_ground, m.horizontal_collision))
}

/// Events the brain hears: last tick's hits on it (revenge) and by it (tempo), and the
/// sprinting and fighting players within hearing. GAP: no totem, eating, pearl or water events
/// per player reach a mod yet; those reads stay off.
fn events_for(seen: &[Seen], players: &[u32], heard: &[(i32, i32, Vec3)], mine: u32, damage: &[(i32, i32, f32)]) -> Vec<Event> {
    let me = &seen[mine as usize];
    let mut out = Vec::new();
    // Nothing is heard past the farthest `hearing` the plugin allows (32 blocks).
    const HEARD: f64 = 33.0;
    for &(victim, causing, amount) in damage {
        if victim == me.id {
            out.push(Event::Hurt { by: causing, amount, kind: String::new() });
        }
        if causing == me.id {
            out.push(Event::Landed { entity: victim });
        }
    }
    for &(victim, causing, pos) in heard {
        if victim != me.id && causing != me.id && pos.distance(me.pos) <= HEARD {
            out.push(Event::Heard { pos, loud: true, by: causing });
        }
    }
    for &i in players {
        let s = &seen[i as usize];
        if i != mine && s.sprinting && s.pos.distance(me.pos) <= HEARD {
            out.push(Event::Heard { pos: s.pos, loud: false, by: s.id });
        }
    }
    out
}

fn face_dir(face: u8) -> i32 {
    i32::from(face)
}

#[allow(clippy::too_many_arguments)]
fn act(game: &mut Game<'_>, b: &BotRef, d: pc::brain::Decision, pos: Vec3, on_ground: bool, hcol: bool) {
    let (uuid, conn, slot) = (b.uuid, b.conn, b.slot);
    let mut packets: Vec<S> = Vec::new();
    let mut direct: Vec<Action> = Vec::new();
    // Slot changes first (a client sends them before the click that uses them).
    for a in &d.actions {
        if let Action::SelectSlot(s) = a {
            packets.push(S::SetCarriedItem(play::SetCarriedItem { slot: *s as i16 }));
        }
    }
    let flags = d.input.flags();
    let (sprint_was, input_was) = pb::with(|g| pb::conn_slot(g, conn, slot).map_or((false, 0), |i| (g.conns[i].sprinting, g.conns[i].input)));
    if flags != input_was {
        packets.push(S::PlayerInput(play::PlayerInput { flags }));
    }
    if d.input.sprint != sprint_was {
        let entity = game.lan.as_ref().and_then(|h| h.hub.guest_by_uuid(uuid)).map_or(0, |g| g.entity_id);
        packets.push(S::PlayerCommand(play::PlayerCommand { id: entity, action: if d.input.sprint { 1 } else { 2 }, data: 0 }));
    }
    let move_flags = u8::from(on_ground) | (u8::from(hcol) << 1);
    packets.push(S::MovePlayerPosRot(play::MovePlayerPosRot { x: pos.x, y: pos.y, z: pos.z, yaw: d.yaw, pitch: d.pitch, flags: move_flags }));
    let seq = || pb::with(|g| pb::next_sequence_at(g, conn, slot));
    for a in d.actions {
        match a {
            Action::SelectSlot(_) => {}
            Action::Attack(id) => packets.push(S::Attack(play::Attack { id })),
            Action::Swing(_) => packets.push(S::Punch(horizon_protocol::packets::Empty)),
            Action::Use(h) => packets.push(S::UseItem(play::UseItem { hand: hand(h), sequence: seq(), yaw: d.yaw, pitch: d.pitch })),
            Action::UseOn { pos, face, hand: h } => packets.push(S::UseItemOn(play::UseItemOn {
                hand: hand(h),
                pos,
                face: face_dir(face),
                cursor: [0.5, if face == 1 { 1.0 } else { 0.5 }, 0.5],
                inside: false,
                world_border: false,
                sequence: seq(),
            })),
            Action::Interact(id) => packets.push(S::Interact(play::Interact { id, hand: 0, location: [0.0, 0.9, 0.0], secondary: false })),
            Action::Release => packets.push(S::PlayerAction(play::PlayerAction { action: 6, pos: [0, 0, 0], direction: 0, sequence: 0 })),
            Action::SwapHands => packets.push(S::PlayerAction(play::PlayerAction { action: 7, pos: [0, 0, 0], direction: 0, sequence: 0 })),
            Action::StartDig { pos, face } => packets.push(S::PlayerAction(play::PlayerAction { action: 0, pos, direction: face, sequence: seq() })),
            Action::StopDig { pos, face } => packets.push(S::PlayerAction(play::PlayerAction { action: 3, pos, direction: face, sequence: seq() })),
            Action::Drop { all } => packets.push(S::PlayerAction(play::PlayerAction { action: if all { 4 } else { 5 }, pos: [0, 0, 0], direction: 0, sequence: 0 })),
            Action::Respawn => packets.push(S::ClientCommand(play::ClientCommand { action: 0 })),
            Action::Glide => {
                let entity = game.lan.as_ref().and_then(|h| h.hub.guest_by_uuid(uuid)).map_or(0, |g| g.entity_id);
                packets.push(S::PlayerCommand(play::PlayerCommand { id: entity, action: 6, data: 0 }));
            }
            Action::Sneak(_) => {}
            // Inventory moves: a client clicks its inventory screen (ContainerClick with hashed
            // slots and the menu's state id); in process the stacks are moved directly and the
            // client view is sent again.
            Action::ToOffhand(_) | Action::ToHotbar { .. } | Action::SwapSlots(..) | Action::Wear(_) | Action::Unwear(_) | Action::TakeFrom { .. } => direct.push(a),
            Action::CloseContainer => {
                // VERIFY: the window a guest has open (`Guest::next_window` counts them up).
                let window = game.lan.as_ref().and_then(|h| h.hub.guest_by_uuid(uuid)).map_or(0, |g| g.next_window);
                packets.push(S::ContainerClose(play::ContainerClose { container: window }));
            }
            Action::Chat(text) => {
                // GAP: a bot's signed chat; shown as a system line under its name.
                let name = &b.name;
                game.native_broadcast(Text::literal(&format!("<{name}> {text}")));
            }
            Action::Console(line) => {
                game.native_run_command(&line);
            }
        }
    }
    packets.push(S::ClientTickEnd(horizon_protocol::packets::Empty));
    pb::with(|g| {
        if let Some(i) = pb::conn_slot(g, conn, slot) {
            g.conns[i].input = flags;
            g.conns[i].sprinting = d.input.sprint;
        }
        for p in packets {
            pb::send(g, conn, p);
        }
    });
    if !direct.is_empty() {
        move_items(game, uuid, &direct);
    }
}

fn hand(h: Hand) -> i32 {
    match h {
        Hand::Main => 0,
        Hand::Off => 1,
    }
}

/// The inventory moves the brain asks for, done on the guest's stacks.
fn move_items(game: &mut Game<'_>, uuid: u128, moves: &[Action]) {
    let Some(host) = game.lan.as_mut() else { return };
    let Some(i) = host.hub.guests.iter().position(|g| g.uuid == uuid) else { return };
    {
        let g = &mut host.hub.guests[i];
        for m in moves {
            match m {
                Action::ToOffhand(slot) => {
                    if *slot < g.inventory.len() {
                        std::mem::swap(&mut g.inventory[*slot], &mut g.offhand);
                    }
                }
                Action::ToHotbar { slot, hotbar } => {
                    if *slot < g.inventory.len() && *hotbar < 9 {
                        g.inventory.swap(*slot, *hotbar);
                    }
                }
                Action::SwapSlots(a, b) => {
                    if *a < g.inventory.len() && *b < g.inventory.len() {
                        g.inventory.swap(*a, *b);
                    }
                }
                Action::Wear(slot) => {
                    if let Some(s) = g.inventory.get(*slot).cloned().flatten() {
                        if let Some((a, _, _)) = pc::items::armor(&s.item) {
                            // Guest armour is head..feet.
                            let idx = 3 - a as usize;
                            let worn = g.armor[idx].take();
                            g.armor[idx] = Some(s);
                            g.inventory[*slot] = worn;
                        }
                    }
                }
                Action::Unwear(a) => {
                    let idx = 3 - *a as usize;
                    if let Some(free) = g.inventory.iter().skip(9).position(Option::is_none).map(|p| p + 9) {
                        g.inventory[free] = g.armor[idx].take();
                    }
                }
                Action::TakeFrom { ender: true, wants, .. } => {
                    // Stack by stack out of its ender chest, as far as it wants each.
                    for (key, mut n) in wants.iter().cloned() {
                        for e in 0..g.ender_items.len() {
                            if n == 0 {
                                break;
                            }
                            let matches = g.ender_items[e].as_ref().is_some_and(|s| {
                                let st = stack_of(s);
                                let k = st.potion.as_ref().map_or(st.id.clone(), |p| format!("{}#{p}", st.id));
                                k == key
                            });
                            if !matches {
                                continue;
                            }
                            let Some(free) = (0..g.inventory.len()).find(|&j| g.inventory[j].is_none()) else { break };
                            let mut stack = g.ender_items[e].take().unwrap();
                            if stack.count > n {
                                let mut rest = stack.clone();
                                rest.count -= n;
                                stack.count = n;
                                g.ender_items[e] = Some(rest);
                            }
                            n -= stack.count.min(n);
                            g.inventory[free] = Some(stack);
                        }
                    }
                }
                Action::TakeFrom { ender: false, .. } => {
                    // GAP: a placed shulker box's slots are reachable with the level in place
                    // (`native_container_slots` / `native_extract_item`), but not from here with
                    // the hub borrowed; the box is opened and mined back without taking yet.
                }
                _ => {}
            }
        }
    }
    host.hub.send_inventory(i); // VERIFY: Hub::send_inventory(i) resends the whole inventory
}

// ------------------------------------------------------------------ effects

fn run_effects(game: &mut Game<'_>, effects: Vec<Effect>) {
    for e in effects {
        run_effect(game, e);
    }
    drain_kits(game);
}

thread_local! {
    /// Kits waiting to be given: (bot, kit), in the order they were asked for.
    static KIT_QUEUE: RefCell<std::collections::VecDeque<(String, pc::kits::Kit)>> = RefCell::new(std::collections::VecDeque::new());
}

/// Gives queued kits for at most `KIT_BUDGET_MS` of wall time per call (at least one), so a kit for hundreds of
/// bots is spread over a few seconds instead of one long tick. Runs at the end of every `run_effects`, which
/// the tick start calls each tick.
const KIT_BUDGET_MS: f64 = 5.0;
fn drain_kits(game: &mut Game<'_>) {
    let started = std::time::Instant::now();
    while let Some((to, kit)) = KIT_QUEUE.with(|q| q.borrow_mut().pop_front()) {
        give_kit(game, &to, &kit);
        if started.elapsed().as_secs_f64() * 1000.0 >= KIT_BUDGET_MS {
            break;
        }
    }
}

fn tell(game: &mut Game<'_>, to: &pc::scripts::Sender, text: &str) {
    match to {
        pc::scripts::Sender::Console => eprintln!("[horizonbot] {text}"),
        pc::scripts::Sender::Player(u) => {
            game.native_tell(*u, Text::literal(text));
        }
    }
}

fn mc_name(id: &str) -> String {
    pc::items::full(id)
}

/// A kit item as an `/item` argument: `minecraft:iron_sword[minecraft:enchantments={...}] 1`.
fn item_arg(i: &pc::kits::KitItem) -> String {
    let comps: Vec<String> = i.components.iter().map(|(k, v)| format!("{k}={v}")).collect();
    if comps.is_empty() {
        format!("{} {}", i.id, i.count)
    } else {
        format!("{}[{}] {}", i.id, comps.join(","), i.count)
    }
}

fn give_kit(game: &mut Game<'_>, to: &str, kit: &pc::kits::Kit) {
    // Replaces what they carry through the vanilla commands so every
    // component survives as the server parses it.
    game.native_run_command(&format!("clear {to}"));
    for (i, it) in kit.storage().iter().enumerate() {
        if let Some(it) = it {
            let slot = if i < 9 { format!("hotbar.{i}") } else { format!("inventory.{}", i - 9) };
            game.native_run_command(&format!("item replace entity {to} {slot} with {}", item_arg(it)));
        }
    }
    for (i, it) in kit.armour_slots().iter().enumerate() {
        if let Some(it) = it {
            let slot = ["armor.feet", "armor.legs", "armor.chest", "armor.head"][i];
            game.native_run_command(&format!("item replace entity {to} {slot} with {}", item_arg(it)));
        }
    }
    if let Some(it) = kit.offhand_item() {
        game.native_run_command(&format!("item replace entity {to} weapon.offhand with {}", item_arg(&it)));
    }
    for (i, it) in kit.enderchest.iter().enumerate() {
        if let Some(it) = it {
            game.native_run_command(&format!("item replace entity {to} enderchest.{i} with {}", item_arg(it)));
        }
    }
}

/// One Mojang lookup: the account's textures property (value, signature).
fn lookup_skin(account: &str) -> Option<(String, String)> {
    let id: serde_json::Value = serde_json::from_str(
        &ureq::get(&format!("https://api.mojang.com/users/profiles/minecraft/{account}")).call().ok()?.into_string().ok()?,
    )
    .ok()?;
    let uuid = id["id"].as_str()?;
    let profile: serde_json::Value = serde_json::from_str(
        &ureq::get(&format!("https://sessionserver.mojang.com/session/minecraft/profile/{uuid}?unsigned=false"))
            .call()
            .ok()?
            .into_string()
            .ok()?,
    )
    .ok()?;
    let p = profile["properties"].as_array()?.iter().find(|p| p["name"] == "textures")?;
    Some((p["value"].as_str()?.to_owned(), p["signature"].as_str()?.to_owned()))
}

/// Queues a skin lookup on the one worker thread, which goes gently (a lookup every 300 ms: the
/// session server limits a server's rate).
fn fetch_skin(account: String) {
    pb::with(|g| {
        if g.skin_jobs.is_none() {
            let (jobs, rx) = std::sync::mpsc::channel::<String>();
            let results = g.skins_tx.clone();
            std::thread::Builder::new()
                .name("horizonbot-skins".into())
                .spawn(move || {
                    let mut done: std::collections::BTreeSet<String> = Default::default();
                    while let Ok(acc) = rx.recv() {
                        let key = acc.to_ascii_lowercase();
                        if !done.insert(key) {
                            continue;
                        }
                        let r = lookup_skin(&acc);
                        if results.send((acc, r)).is_err() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                })
                .ok();
            g.skin_jobs = Some(jobs);
        }
        if let Some(j) = &g.skin_jobs {
            let _ = j.send(account);
        }
    });
}

fn run_effect(game: &mut Game<'_>, e: Effect) {
    match e {
        Effect::Tell { to, text } => tell(game, &to, &text),
        Effect::Broadcast(text) => game.native_broadcast(Text::literal(&text)),
        Effect::SpawnBot { ref name, uuid, ref skin, ref world, pos, yaw } => {
            let skin = match skin {
                SkinSource::Fetch(acc) => {
                    let acc = acc.clone();
                    pb::with(|g| g.waiting.push((acc.clone(), e.clone())));
                    fetch_skin(acc);
                    return;
                }
                SkinSource::Known(s) => Some((s.value.clone(), s.signature.clone())),
                SkinSource::None => None,
            };
            // Let in a few per tick (`joins-per-tick`), see `admit_queued`. It appears where it was spawned
            // (the hub places it there when it joins): the queue carries the place.
            let spawn = pos.map(|p| (dimension(&world.clone().unwrap_or_else(|| "minecraft:overworld".into())), p.to_array(), yaw));
            pb::with(|g| g.join_queue.push_back((name.clone(), uuid, skin, spawn)));
        }
        Effect::RemoveBot { name, uuid } => {
            // Quiet: no "left the game" for a bot.
            if let Some(host) = game.lan.as_mut() {
                if let Some(g) = host.hub.guests.iter_mut().find(|g| g.uuid == uuid) {
                    g.quiet_leave = true;
                }
            }
            pb::with(|g| {
                g.join_queue.retain(|(_, u, _, _)| *u != uuid);
                pb::disconnect(g, uuid)
            });
            FAKE_MARKED.with(|m| m.borrow_mut().remove(&uuid));
            // The bot is gone: its name is no longer announced as a fake player.
            game.native_expect_fake_player(&name, false);
        }
        Effect::Console(line) => {
            if !game.native_run_command(&line) {
                eprintln!("[horizonbot] console command failed: {line}");
            }
        }
        Effect::AsPlayer { uuid, line } => {
            // GAP: no "run as this player" helper; `execute as` carries their name and position,
            // not their permission.
            let name = game.native_server_player(uuid).map(|p| p.name);
            if let Some(n) = name {
                game.native_run_command(&format!("execute as {n} at {n} run {line}"));
            }
        }
        Effect::SetBlocks { world, blocks } => {
            let dim = if world.is_empty() { "minecraft:overworld".to_owned() } else { dimension(&world) };
            for ([x, y, z], state) in blocks {
                game.native_run_command(&format!("execute in {dim} run setblock {x} {y} {z} {state}"));
            }
        }
        // Queued, not run here: a kit is ~60 vanilla commands per bot, and 500 bots in one tick took ~500 ms.
            Effect::GiveKit { to, kit } => KIT_QUEUE.with(|q| q.borrow_mut().push_back((to, kit))),
        Effect::Teleport { name, world, pos } => {
            let dim = dimension(&world.unwrap_or_else(|| "minecraft:overworld".into()));
            teleport_by_name(game, &name, &dim, [pos.x, pos.y, pos.z]);
            pb::with(|g| {
                if let Some(p) = g.plugin.as_mut() {
                    p.moved(&name, Some(pos), Some(Vec3::ZERO));
                }
            });
        }
        Effect::Pearl { owner, world, pos, vel } => {
            let [a, b, c, d] = uuid_ints(owner);
            game.native_run_command(&format!(
                "execute in {} run summon ender_pearl {} {} {} {{Motion:[{}d,{}d,{}d],Owner:[I;{a},{b},{c},{d}]}}",
                dimension(&world), pos.x, pos.y, pos.z, vel.x, vel.y, vel.z
            ));
        }
        Effect::SpawnGhost { ref name, uuid, ref skin, ref world, pos } => {
            let skin = match skin {
                SkinSource::Fetch(acc) => {
                    pb::with(|g| g.waiting.push((acc.clone(), e.clone())));
                    fetch_skin(acc.clone());
                    return;
                }
                SkinSource::Known(s) => Some((s.value.clone(), s.signature.clone())),
                SkinSource::None => None,
            };
            // Announced before the join, as for the bots (`admit_queued`), with the place it appears at.
            let at = dimension(world);
            let placed = game.native_expect_fake_player_at(name, &at, pos.to_array(), 0.0);
            if !placed {
                game.native_expect_fake_player(name, true);
            }
            if pb::with(|g| pb::connect(g, name, uuid, skin, true)) {
                if !placed {
                    // Fallback only: a teleport once it is placed.
                    pb::with(|g| g.pending_tp.push((name.clone(), at, pos.to_array(), 200)));
                }
                // GAP: invulnerable, untouchable and out of the tab list need hooks a guest lacks.
                game.native_run_command(&format!("effect give {name} resistance infinite 255 true"));
            } else {
                game.native_expect_fake_player(name, false);
            }
        }
        Effect::MoveGhost { name, pos, yaw, pitch, sneaking } => {
            let conn = pb::with(|g| g.conns.iter().find(|c| c.ghost && c.name.eq_ignore_ascii_case(&name)).map(|c| c.conn));
            if let Some(conn) = conn {
                pb::with(|g| {
                    pb::send(g, conn, S::PlayerInput(play::PlayerInput { flags: if sneaking { 32 } else { 0 } }));
                    pb::send(g, conn, S::MovePlayerPosRot(play::MovePlayerPosRot { x: pos.x, y: pos.y, z: pos.z, yaw, pitch, flags: 1 }));
                    pb::send(g, conn, S::ClientTickEnd(horizon_protocol::packets::Empty));
                });
            }
        }
        Effect::RemoveGhost { name } => {
            let uuid = pb::with(|g| g.conns.iter().find(|c| c.ghost && c.name.eq_ignore_ascii_case(&name)).map(|c| c.uuid));
            if let Some(u) = uuid {
                run_effect(game, Effect::RemoveBot { name, uuid: u });
            }
        }
        Effect::GhostSay { name, text } => game.native_broadcast(Text::literal(&format!("<{name}> {text}"))),
        Effect::OpenView { viewer, bot, enderchest } => {
            // A read-only chest view of the bot's bag or ender chest (GAP: editable needs menus
            // that move items). Its clicks are nobody's: HorizonBot's menu table does not know it.
            let stacks: Option<Vec<Option<horizon_client::items::ItemStack>>> = game.lan.as_ref().and_then(|h| {
                h.hub.guests.iter().find(|g| g.name.eq_ignore_ascii_case(&bot)).map(|g| {
                    if enderchest {
                        g.ender_items.iter().take(27).cloned().collect()
                    } else {
                        let mut v: Vec<_> = g.inventory.iter().skip(9).take(27).cloned().collect();
                        v.extend(g.inventory.iter().take(9).cloned());
                        v.extend(g.armor.iter().rev().cloned());
                        v.push(g.offhand.clone());
                        v
                    }
                })
            });
            match stacks {
                Some(items) => {
                    let rows = if enderchest { 3 } else { 5 };
                    let title = format!("{bot}: {}", if enderchest { "ender chest" } else { "inventory" });
                    game.native_open_menu(viewer, rows, Text::literal(&title), items);
                }
                None => {
                    game.native_tell(viewer, Text::literal(&format!("{bot} is not online")));
                }
            }
        }
        Effect::OpenKitEditor { viewer, kit: _ } => show_kit_editor(game, viewer),
        Effect::OpenMenu { viewer, page } => {
            let menu = pb::with(|g| g.plugin.as_mut().map(|p| p.menu(viewer, page)));
            if let Some(m) = menu {
                show_menu(game, viewer, &m);
            }
        }
        Effect::FetchPool(names) => {
            for n in names {
                fetch_skin(n);
            }
        }
        Effect::SetInvulnerable { name, on } => {
            // GAP: the invulnerable flag of a player; resistance 255 stands in.
            if on {
                game.native_run_command(&format!("effect give {name} resistance infinite 255 true"));
            } else {
                game.native_run_command(&format!("effect clear {name} resistance"));
            }
        }
        Effect::ResetAttributes { name } => {
            for a in ["max_health", "movement_speed", "attack_damage", "attack_speed", "armor", "armor_toughness", "knockback_resistance", "scale", "gravity", "jump_strength", "safe_fall_distance", "step_height"] {
                game.native_run_command(&format!("attribute {name} minecraft:{a} base reset"));
            }
        }
        Effect::DropItem { bot, item, count } => {
            game.native_run_command(&format!("execute at {bot} run summon item ~ ~ ~ {{Item:{{id:\"{}\",count:{count}}}}}", mc_name(&item)));
        }
        Effect::GiveItems { bot, items } => {
            for (i, n) in items {
                game.native_run_command(&format!("give {bot} {} {n}", mc_name(&i)));
            }
        }
        Effect::Restock { bot, kit } => {
            // What it is short of against its kit (and the standing minimums), given at once.
            let carried = carried_counts(game, &bot);
            let targets = kit.map(|k| k.restock_targets()).unwrap_or_else(|| pc::kits::Kit::default().restock_targets());
            for (key, n) in pc::kits::shortfall(&targets, &carried) {
                let (id, potion) = key.split_once('#').map_or((key.as_str(), None), |(a, b)| (a, Some(b)));
                let arg = match potion {
                    Some(p) => format!("{id}[minecraft:potion_contents={{potion:\"{p}\"}}]"),
                    None => id.to_owned(),
                };
                game.native_run_command(&format!("give {bot} {arg} {n}"));
            }
        }
        Effect::ClearPlayerData(uuids) => {
            if let Some(dir) = game.native_world_dir() {
                for u in uuids {
                    let id = pc::stats::uuid_text(u);
                    for (sub, ext) in [("playerdata", "dat"), ("playerdata", "dat_old"), ("stats", "json"), ("advancements", "json")] {
                        let _ = std::fs::remove_file(dir.join(sub).join(format!("{id}.{ext}")));
                    }
                }
            }
        }
        Effect::NameLine { bot, text } => {
            // GAP: a line over a player's head; the bot's own action bar stands in for the viewer
            // set (the debug view goes to operators' action bars).
            if let Some(t) = text {
                let ops: Vec<u128> = game.native_server_players().into_iter().filter(|p| p.permission >= 2 && !pb::is_bot(p.uuid)).map(|p| p.uuid).collect();
                for u in ops {
                    game.native_action_bar(u, Text::literal(&format!("{bot}: {t}")));
                }
            }
        }
        Effect::ReadKit { name, from, reply } => {
            let kit = read_kit(game, &from);
            match kit {
                Some(k) => {
                    let fx = pb::with(|g| g.plugin.as_mut().map(|p| p.save_kit(&name, k)).unwrap_or_default());
                    run_effects(game, fx);
                    tell(game, &reply, &format!("Saved kit {name} from {from}"));
                }
                None => tell(game, &reply, &format!("{from} is not online")),
            }
        }
        Effect::Save { file, text } => pb::write_file(&file, &text),
        Effect::Reload => reload_files(),
        Effect::SaveConfig(text) => {
            pb::with(|g| {
                g.wrote_config = Some(text.clone());
                g.config_text = text.clone();
            });
            if std::fs::write("config/horizonbot-common.toml", &text).is_err() {
                eprintln!("[horizonbot] could not write config/horizonbot-common.toml");
            }
        }
    }
}

fn dimension(world: &str) -> String {
    pc::world::dimension_id(world)
}

// ------------------------------------------------------------------ menus

/// A menu item as a stack with its name and lore. VERIFY: `StackData`'s `custom_name` and the lore
/// as an `other` component.
fn menu_stack(i: &pc::menu::MenuItem) -> horizon_client::items::ItemStack {
    let name = horizon_client::lan::data::component_nbt(&Text::literal(&i.name).component());
    let lore = horizon_world::nbt::Tag::List(
        i.lore.iter().map(|l| horizon_client::lan::data::component_nbt(&Text::literal(l).component())).collect(),
    );
    let data = horizon_items::item_data::StackData {
        custom_name: Some(name),
        other: vec![("minecraft:lore".to_owned(), lore)],
        ..Default::default()
    };
    horizon_client::items::ItemStack { item: std::sync::Arc::from(i.item.as_str()), count: i.count, damage: 0, extra: Some(Box::new(data)) }
}

fn show_menu(game: &mut Game<'_>, viewer: u128, m: &pc::menu::Menu) {
    let items: Vec<Option<horizon_client::items::ItemStack>> = m.items.iter().map(|i| i.as_ref().map(menu_stack)).collect();
    game.native_open_menu(viewer, m.rows, Text::literal(&m.title), items);
}

/// `native_menu_clicked`: a slot of HorizonBot's menu (no click moves anything; the menu is shown
/// again as it now is).
/// A kit item for the editor's display (id, count, enchantments, potion).
fn kit_stack(k: &pc::kits::KitItem) -> horizon_client::items::ItemStack {
    let st = k.to_stack();
    let data = horizon_items::item_data::StackData {
        enchantments: st.enchants.iter().map(|(e, l)| (std::sync::Arc::from(e.as_str()), *l)).collect(),
        potion: st.potion.as_deref().map(std::sync::Arc::from),
        ..Default::default()
    };
    horizon_client::items::ItemStack { item: std::sync::Arc::from(st.id.as_str()), count: st.count, damage: st.damage, extra: Some(Box::new(data)) }
}

/// The kit editor: the kit in the top 45 slots (bag, hotbar, armour, off hand; or its ender
/// chest), controls in the bottom row, the editor's own inventory below. Click an item of yours to
/// add a copy, a kit slot to take it out.
fn show_kit_editor(game: &mut Game<'_>, viewer: u128) {
    let Some((name, ender, slots)) = pb::with(|g| g.plugin.as_ref().and_then(|p| p.kit_editor_slots(viewer))) else { return };
    let mut items: Vec<Option<horizon_client::items::ItemStack>> = slots.iter().map(|s| s.as_ref().map(kit_stack)).collect();
    items.resize(54, None);
    let button = |id: &str, label: &str| menu_stack(&pc::menu::MenuItem { item: pc::items::full(id), count: 1, name: label.into(), lore: Vec::new() });
    items[45] = Some(button(if ender { "chest" } else { "ender_chest" }, if ender { "The inventory page" } else { "The ender chest page" }));
    items[49] = Some(button("lime_dye", "Done (saved as you go)"));
    items[53] = Some(button("barrier", "Clear this kit"));
    let title = format!("Kit {name}{}", if ender { ": ender chest" } else { "" });
    game.native_open_menu(viewer, 6, Text::literal(&title), items);
}

fn kit_editor_clicked(game: &mut Game<'_>, uuid: u128, slot: i32) -> bool {
    if !pb::with(|g| g.plugin.as_ref().is_some_and(|p| p.kit_edits.contains_key(&uuid))) {
        return false;
    }
    let (mut kit_slot, mut add, mut toggle, mut clear, mut done) = (None, None, false, false, false);
    match slot {
        0..=44 => kit_slot = Some(slot as usize),
        45 => toggle = true,
        49 => done = true,
        53 => clear = true,
        _ => {
            // The editor's own inventory below the menu: a copy of the clicked stack goes in.
            if let Some(inv) = super::native_menus::native_menu_inventory_slot(54, slot) {
                if let Some(stacks) = game.native_player_stacks(uuid) {
                    add = stacks.get(inv).cloned().flatten().map(|s| kit_item(&s));
                }
            }
        }
    }
    if done {
        game.native_close_menu(uuid);
        menu_closed(uuid);
        return true;
    }
    let effects = pb::with(|g| g.plugin.as_mut().map(|p| p.kit_editor_change(uuid, kit_slot, add, toggle, clear)).unwrap_or_default());
    run_effects(game, effects);
    show_kit_editor(game, uuid);
    true
}

pub(super) fn menu_clicked(game: &mut Game<'_>, uuid: u128, slot: i32, button: i32) {
    if kit_editor_clicked(game, uuid, slot) {
        return;
    }
    let Some(player) = game.native_server_player(uuid) else { return };
    let who = Who::Player(info_of(&player));
    let Ok(slot) = usize::try_from(slot) else { return };
    let (effects, menu) = pb::with(|g| g.plugin.as_mut().map(|p| p.menu_click(&who, slot, button == 1)).unwrap_or((Vec::new(), None)));
    run_effects(game, effects);
    if let Some(m) = menu {
        show_menu(game, uuid, &m);
    }
}

pub(super) fn menu_closed(uuid: u128) {
    pb::with(|g| {
        if let Some(p) = g.plugin.as_mut() {
            p.menu_closed(uuid);
        }
    });
}

fn uuid_ints(u: u128) -> [i32; 4] {
    [(u >> 96) as u32 as i32, (u >> 64) as u32 as i32, (u >> 32) as u32 as i32, u as u32 as i32]
}

fn carried_counts(game: &Game<'_>, name: &str) -> std::collections::BTreeMap<String, u32> {
    let mut m = std::collections::BTreeMap::new();
    let Some(host) = game.lan.as_ref() else { return m };
    let Some(g) = host.hub.guests.iter().find(|g| g.name.eq_ignore_ascii_case(name)) else { return m };
    for s in g.inventory.iter().flatten().chain(g.offhand.iter()).chain(g.armor.iter().flatten()) {
        let st = stack_of(s);
        let key = match &st.potion {
            Some(p) => format!("{}#{p}", st.id),
            None => st.id.clone(),
        };
        *m.entry(key).or_default() += st.count;
    }
    m
}

fn kit_item(s: &horizon_client::items::ItemStack) -> pc::kits::KitItem {
    let st = stack_of(s);
    let mut k = pc::kits::KitItem::new(&st.id, st.count);
    if !st.enchants.is_empty() {
        let body = st.enchants.iter().map(|(e, l)| format!("\"{e}\":{l}")).collect::<Vec<_>>().join(",");
        k.components.push(("minecraft:enchantments".into(), format!("{{{body}}}")));
    }
    if let Some(p) = &st.potion {
        k.components.push(("minecraft:potion_contents".into(), format!("{{potion:\"{p}\"}}")));
    }
    if st.damage > 0 {
        k.components.push(("minecraft:damage".into(), st.damage.to_string()));
    }
    // GAP: other components (names, trims, banner patterns) are not read back into text here.
    k
}

fn read_kit(game: &Game<'_>, name: &str) -> Option<pc::kits::Kit> {
    let host = game.lan.as_ref()?;
    let g = host.hub.guests.iter().find(|g| g.name.eq_ignore_ascii_case(name))?;
    let mut kit = pc::kits::Kit::default();
    kit.contents = g.inventory.iter().take(36).map(|s| s.as_ref().map(kit_item)).collect();
    // Paper's armour list is boots first; the guest's head first.
    kit.armour = g.armor.iter().rev().map(|s| s.as_ref().map(kit_item)).collect();
    kit.offhand = g.offhand.as_ref().map(kit_item);
    kit.enderchest = g.ender_items.iter().map(|s| s.as_ref().map(kit_item)).collect();
    Some(kit)
}

/// hooks-all `native_teleport_player` (API teleport_player): a bot is moved by UUID as `/tp`
/// moves it, from any hook, keeping its rotation, instead of an `execute in .. run tp` line by
/// name. Falls back to the command for a name that is not on.
fn teleport_by_name(game: &mut Game<'_>, name: &str, dim: &str, pos: [f64; 3]) {
    // A bot is found through its connection, not by listing every player.
    let key = name.to_ascii_lowercase();
    let bot = pb::with(|g| g.conns.iter().find(|c| c.key == key).map(|c| c.uuid));
    let player = match bot {
        Some(u) => game.native_server_player(u),
        None => game.native_server_players().into_iter().find(|p| p.name.eq_ignore_ascii_case(name)),
    };
    match player {
        // VERIFY: signature (uuid, dimension, pos, yaw, pitch) per the hooks-all PR body.
        Some(p) => {
            game.native_teleport_player(p.uuid, dim, pos, p.yaw, p.pitch);
        }
        None => {
            let [x, y, z] = pos;
            game.native_run_command(&format!("execute in {dim} run tp {name} {x} {y} {z}"));
        }
    }
}
