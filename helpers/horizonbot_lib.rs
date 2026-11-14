// HorizonBot's state shared by its hooks, a module of horizon-client-lib (`horizon_client::horizonbot`):
// the lib-side hooks (chat, the anticheat) and the game-side ones (the tick, commands, damage)
// both reach it. It also holds the bots' connections: a bot is a player with no client, joined
// through the same event channel the network threads use (`NetEvent`), with a test channel as
// its socket (`PlayerSender::new`), so the hub treats it as any guest.
//
// BLIND: written against a589a3dd9 without a build. Marked `VERIFY` where a name or a field is
// read from the source but not compiled.

pub use horizonbot_core;

use crate::lan::{Joined, NetEvent, Out, PlayerSender, Shared};
use horizon_protocol::packets::play;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicI32, AtomicUsize};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

/// A bot's (or a ghost's) connection.
pub struct BotConn {
    pub conn: u32,
    pub uuid: u128,
    pub name: String,
    /// `name` in lower case, the plugin's key for the bot (made once, not every tick).
    pub key: String,
    /// The player's entity id once known (0 until then): it never changes while the bot is on.
    pub entity_id: i32,
    pub ghost: bool,
    /// What the server sends this "client": decoded and answered by the tick.
    pub rx: Receiver<Out>,
    /// The use-item sequence number a client counts up.
    pub sequence: i32,
    pub sprinting: bool,
    pub input: u8,
    /// `PlayerLoaded` sent.
    pub loaded: bool,
    /// The teleport the server waits to hear accepted.
    pub teleport: Option<(i32, [f64; 3])>,
    /// Knockback the server sent, for the brain's physics.
    pub motion: Option<[f64; 3]>,
    pub dead: bool,
}

/// Lines and effects the lib-side hooks queue for the next tick.
#[derive(Default)]
pub struct Queue {
    pub effects: Vec<horizonbot_core::plugin::Effect>,
    /// (victim entity, causing entity, amount)
    pub damage: Vec<(i32, i32, f32)>,
    /// (player uuid, cause): deaths heard.
    pub deaths: Vec<(u128, Option<String>)>,
}

pub struct Glue {
    pub plugin: Option<horizonbot_core::plugin::Plugin>,
    pub shared: Option<Arc<Shared>>,
    pub conns: Vec<BotConn>,
    pub queue: Queue,
    /// Skin lookups the game side does on threads: (account, value, signature) or None.
    pub skins_tx: Sender<(String, Option<(String, String)>)>,
    pub skins_rx: Receiver<(String, Option<(String, String)>)>,
    /// The skin lookup worker's job queue (account names), made on first use.
    pub skin_jobs: Option<Sender<String>>,
    /// Bots waiting for their skin before they join: (account, spawn effect).
    pub waiting: Vec<(String, horizonbot_core::plugin::Effect)>,
    /// The config text this mod wrote itself (its reload is not a reload).
    pub wrote_config: Option<String>,
    pub config_text: String,
    pub tick: u64,
    /// The hits of the last tick, for the brains' events (hurt by, landed on).
    pub last_damage: Vec<(i32, i32, f32)>,
    /// Teleports for bots not placed yet: (name, dimension, position, ticks left to try).
    pub pending_tp: Vec<(String, String, [f64; 3], u32)>,
    /// Bots admitted a few per tick (`joins-per-tick`): (name, uuid, skin, where it appears:
    /// dimension, position, yaw; None where no place is known).
    pub join_queue: std::collections::VecDeque<(String, u128, Option<(String, String)>, Option<(String, [f64; 3], f32)>)>,
}

impl Default for Glue {
    fn default() -> Self {
        let (skins_tx, skins_rx) = channel();
        Glue {
            plugin: None,
            shared: None,
            conns: Vec::new(),
            queue: Queue::default(),
            skins_tx,
            skins_rx,
            skin_jobs: None,
            waiting: Vec::new(),
            wrote_config: None,
            config_text: horizonbot_core::config::DEFAULT_TOML.to_owned(),
            tick: 0,
            last_damage: Vec::new(),
            pending_tp: Vec::new(),
            join_queue: std::collections::VecDeque::new(),
        }
    }
}

static GLUE: Mutex<Option<Glue>> = Mutex::new(None);
/// The bots' UUIDs, read on hot paths (the anticheat) without the big lock.
static BOT_UUIDS: RwLock<BTreeSet<u128>> = RwLock::new(BTreeSet::new());

pub fn glue() -> MutexGuard<'static, Option<Glue>> {
    GLUE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` with the state, made on first use.
pub fn with<R>(f: impl FnOnce(&mut Glue) -> R) -> R {
    let mut g = glue();
    f(g.get_or_insert_with(Glue::default))
}

/// As `with`, but a panic in the plugin is caught and logged instead of taking the server down
/// (where the build unwinds; with `panic = "abort"` it cannot be caught).
pub fn guarded<R: Default>(what: &str, f: impl FnOnce(&mut Glue) -> R) -> R {
    let mut g = glue();
    let state = g.get_or_insert_with(Glue::default);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(state))) {
        Ok(r) => r,
        Err(e) => {
            let msg = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_default();
            eprintln!("[horizonbot] {what} failed and was skipped: {msg}");
            R::default()
        }
    }
}

/// The ghosts' entity ids: their damage is cancelled.
static GHOST_IDS: RwLock<BTreeSet<i32>> = RwLock::new(BTreeSet::new());

pub fn is_ghost_entity(id: i32) -> bool {
    GHOST_IDS.read().map(|s| s.contains(&id)).unwrap_or(false)
}

pub fn set_ghost_entities(ids: BTreeSet<i32>) {
    if let Ok(mut s) = GHOST_IDS.write() {
        *s = ids;
    }
}

/// The bots' entity ids and whether `bypass-damage-protection` is on (read by the damage hook's
/// tail, which un-cancels damage involving a bot).
static BOT_IDS: RwLock<BTreeSet<i32>> = RwLock::new(BTreeSet::new());
static BYPASS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_bot_entities(ids: BTreeSet<i32>, bypass: bool) {
    if let Ok(mut s) = BOT_IDS.write() {
        *s = ids;
    }
    BYPASS.store(bypass, std::sync::atomic::Ordering::Relaxed);
}

/// Hits involving a bot that another mod cancelled (victim, attacker), for `/horizonbot diagnose`.
static CANCELLED: Mutex<Vec<(i32, i32)>> = Mutex::new(Vec::new());

pub fn note_cancelled(entity: i32, causing: i32) {
    let involves = BOT_IDS.read().map(|s| s.contains(&entity) || s.contains(&causing)).unwrap_or(false);
    if involves {
        if let Ok(mut c) = CANCELLED.lock() {
            if c.len() < 10_000 {
                c.push((entity, causing));
            }
        }
    }
}

pub fn take_cancelled() -> Vec<(i32, i32)> {
    CANCELLED.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default()
}

/// Whether a cancelled hit involving a bot is to go through anyway.
pub fn bypass_protection(entity: i32, causing: i32) -> bool {
    BYPASS.load(std::sync::atomic::Ordering::Relaxed)
        && BOT_IDS.read().map(|s| s.contains(&entity) || s.contains(&causing)).unwrap_or(false)
}

pub fn is_bot(uuid: u128) -> bool {
    BOT_UUIDS.read().map(|s| s.contains(&uuid)).unwrap_or(false)
}

fn set_bot(uuid: u128, on: bool) {
    if let Ok(mut s) = BOT_UUIDS.write() {
        if on {
            s.insert(uuid);
        } else {
            s.remove(&uuid);
        }
    }
}

/// The data folder: `config/horizonbot/` beside the config file.
pub fn data_dir() -> std::path::PathBuf {
    std::path::PathBuf::from("config").join("horizonbot")
}

/// Reads every data file the mod keeps.
pub fn read_files() -> horizonbot_core::plugin::Files {
    let mut files = horizonbot_core::plugin::Files::default();
    let dir = data_dir();
    for name in [
        "factions.yml",
        "kits.yml",
        "stats.yml",
        "paths.yml",
        "scripts.yml",
        "bots.yml",
        "bot-settings.yml",
        "skin-cache.yml",
        "messages.yml",
    ] {
        if let Ok(t) = std::fs::read_to_string(dir.join(name)) {
            files.texts.insert(name.to_owned(), t);
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir.join("ghost-recordings")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "txt") {
                if let (Some(n), Ok(t)) = (p.file_name().and_then(|n| n.to_str()), std::fs::read_to_string(&p)) {
                    files.texts.insert(format!("ghost-recordings/{n}"), t);
                }
            }
        }
    }
    files
}

pub fn write_file(rel: &str, text: &str) {
    let path = data_dir().join(rel);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Write beside, then rename: a crash mid-write never leaves half a file.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)).is_err() {
        eprintln!("[horizonbot] could not save {}", path.display());
    }
}

/// Brings a bot in: a `Joined` event as a network thread sends one after configuration.
/// `skin` is the textures property (value, signature).
pub fn connect(g: &mut Glue, name: &str, uuid: u128, skin: Option<(String, String)>, ghost: bool) -> bool {
    let Some(shared) = g.shared.clone() else { return false };
    if g.conns.iter().any(|c| c.uuid == uuid) {
        return false;
    }
    let (tx, rx) = channel();
    let sender = PlayerSender::new(tx, Arc::new(AtomicUsize::new(0)));
    let conn = shared.next_id();
    let properties = skin
        .map(|(value, signature)| {
            vec![horizon_protocol::packets::login::Property {
                name: "textures".into(),
                value,
                signature: Some(signature),
            }]
        })
        .unwrap_or_default();
    let joined = Joined {
        conn,
        uuid,
        name: name.to_owned(),
        properties,
        // The view distance a bot asks for (`[horizon] bot-view-distance`, 2 by default): it reads no
        // chunks, and what it asks for is all the server sends it, tracks for it and (as a fake
        // player) keeps loaded around it.
        info: horizon_protocol::packets::configuration::ClientInformation::new(
            g.plugin.as_ref().map_or(horizonbot_core::config::DEFAULT_BOT_VIEW_DISTANCE, |p| p.config.bot_view_distance) as i8,
        ),
        brand: "horizonbot".into(),
        sender,
        latency: Arc::new(AtomicI32::new(0)),
        // Loopback, port 0: what other mods take for a bot (as for any remote player).
        address: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        expected: false,
        configured: Default::default(),
    };
    // NOT `try_admit`: the whitelist and the player cap would turn a bot away. The bot is then
    // not in the online list the server-list ping counts (GAPS.md).
    if shared.events.send(NetEvent::Joined(Box::new(joined))).is_err() {
        return false;
    }
    g.conns.push(BotConn {
        conn,
        uuid,
        name: name.to_owned(),
        key: name.to_ascii_lowercase(),
        entity_id: 0,
        ghost,
        rx,
        sequence: 0,
        sprinting: false,
        input: 0,
        loaded: false,
        teleport: None,
        motion: None,
        dead: false,
    });
    set_bot(uuid, true);
    true
}

/// A serverbound packet from a bot's "client".
pub fn send(g: &Glue, conn: u32, packet: play::Serverbound) {
    if let Some(shared) = &g.shared {
        let _ = shared.events.send(NetEvent::Packet(conn, Box::new(packet), 0));
    }
}

/// The bot's connection ends (as a client closing its socket).
pub fn disconnect(g: &mut Glue, uuid: u128) {
    if let Some(i) = g.conns.iter().position(|c| c.uuid == uuid) {
        let c = g.conns.remove(i);
        if let Some(shared) = &g.shared {
            let _ = shared.events.send(NetEvent::Left(c.conn, "removed by horizonbot".into()));
        }
    }
    set_bot(uuid, false);
}

/// Where the connection `conn` sits in `g.conns`: `hint` (its place when the caller last looked)
/// is tried first, so a lookup per bot per tick does not scan every bot.
pub fn conn_slot(g: &Glue, conn: u32, hint: usize) -> Option<usize> {
    if g.conns.get(hint).is_some_and(|c| c.conn == conn) {
        return Some(hint);
    }
    g.conns.iter().position(|c| c.conn == conn)
}

/// Next use-item sequence of a bot.
pub fn next_sequence(g: &mut Glue, conn: u32) -> i32 {
    next_sequence_at(g, conn, usize::MAX)
}

/// `next_sequence` with the place of the connection in `g.conns` as the caller knows it.
pub fn next_sequence_at(g: &mut Glue, conn: u32, hint: usize) -> i32 {
    match conn_slot(g, conn, hint) {
        Some(i) => {
            g.conns[i].sequence += 1;
            g.conns[i].sequence
        }
        None => 0,
    }
}
