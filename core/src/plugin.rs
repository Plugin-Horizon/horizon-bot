//! The plugin itself, without the engine: every store, every bot, every command. The glue calls
//! in with what happened (a command, a chat line, a join, a hit, a death, a tick) and carries out
//! the [`Effect`]s that come back; each tick it asks each bot to [`think`](Plugin::think).

use crate::arena::{Arena, ArenaStep};
use crate::brain::{self, formation::Shape, squad::Squad, Bot, Decision, Perception};
use crate::command::{self, *};
use crate::config::Config;
use crate::factions::{Factions, Relation};
use crate::ghost::{Ghost, GhostKind, Recorder, Recording};
use crate::kits::{Kit, Kits};
use crate::math::{Rng, Vec3};
use crate::messages::Messages;
use crate::paths::{Patrol, Paths};
use crate::scripts::{Runner, Scripts, Sender};
use crate::selector::{self, Candidate, Selector, Target};
use crate::settings::{self, Settings};
use crate::skins::{RandomSkins, Skin, SkinPool};
use crate::stats::Stats;
use crate::store::{BotStore, SavedBot};
use crate::yaml::{self, Yaml};
use std::collections::{BTreeMap, BTreeSet};

/// Who ran a command.
#[derive(Clone, Debug, PartialEq)]
pub enum Who {
    Console,
    Player(PlayerInfo),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub uuid: u128,
    pub name: String,
    pub world: String,
    pub pos: Vec3,
    pub yaw: f32,
    pub level: u8,
    pub entity_id: i32,
    pub game_mode: i32,
}

impl Who {
    fn sender(&self) -> Sender {
        match self {
            Who::Console => Sender::Console,
            Who::Player(p) => Sender::Player(p.uuid),
        }
    }
    fn player(&self) -> Option<&PlayerInfo> {
        match self {
            Who::Player(p) => Some(p),
            Who::Console => None,
        }
    }
    fn is_admin(&self, admin_level: u8) -> bool {
        match self {
            Who::Console => true,
            Who::Player(p) => p.level >= admin_level,
        }
    }
}

/// Where a bot's skin comes from when it joins.
#[derive(Clone, Debug, PartialEq)]
pub enum SkinSource {
    None,
    Known(Skin),
    /// Look this account up first (the glue does the HTTP), then join.
    Fetch(String),
}

/// What the glue must do.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Tell { to: Sender, text: String },
    Broadcast(String),
    /// Bring a bot in as a player with no client.
    SpawnBot { name: String, uuid: u128, skin: SkinSource, world: Option<String>, pos: Option<Vec3>, yaw: f32 },
    RemoveBot { name: String, uuid: u128 },
    /// A command as the console (waypoint commands, scripts' non-HorizonBot lines).
    Console(String),
    /// A non-HorizonBot script line run as a player.
    AsPlayer { uuid: u128, line: String },
    SetBlocks { world: String, blocks: Vec<([i32; 3], String)> },
    /// Replace an inventory (a bot's or a real player's) with a kit.
    GiveKit { to: String, kit: Kit },
    Teleport { name: String, world: Option<String>, pos: Vec3 },
    /// Throw an ender pearl owned by this bot from `pos` with `vel`.
    Pearl { owner: u128, world: String, pos: Vec3, vel: Vec3 },
    SpawnGhost { name: String, uuid: u128, skin: SkinSource, world: String, pos: Vec3 },
    MoveGhost { name: String, pos: Vec3, yaw: f32, pitch: f32, sneaking: bool },
    RemoveGhost { name: String },
    /// A ghost says a line in chat under its own name.
    GhostSay { name: String, text: String },
    OpenView { viewer: u128, bot: String, enderchest: bool },
    OpenKitEditor { viewer: u128, kit: String },
    OpenMenu { viewer: u128, page: crate::menu::Page },
    /// Look up skins for the pool (background, gently).
    FetchPool(Vec<String>),
    SetInvulnerable { name: String, on: bool },
    ResetAttributes { name: String },
    DropItem { bot: String, item: String, count: u32 },
    GiveItems { bot: String, items: Vec<(String, u32)> },
    /// Fill the bot's inventory up to its kit at once (`restock instant`).
    Restock { bot: String, kit: Option<Kit> },
    /// Delete these offline players' files (data, stats, advancements).
    ClearPlayerData(Vec<u128>),
    /// A line over a bot (debug view); None clears it.
    NameLine { bot: String, text: Option<String> },
    /// Read a player's or a bot's inventory and hand it to `save_kit`.
    ReadKit { name: String, from: String, reply: Sender },
    /// Save a data file (relative to the mod's data folder).
    Save { file: String, text: String },
    /// The config's new text (written with comments kept).
    SaveConfig(String),
    /// Read the config and the data files again (`/horizonbot reload`).
    Reload,
}

pub struct Live {
    pub bot: Bot,
    pub world: String,
    pub skin_from: Option<String>,
    /// Placed by the server (has an entity id).
    pub joined: bool,
    /// Parked while its owner is offline.
    pub parked: bool,
    pub kit: Option<String>,
}

/// The files the plugin keeps, by name (`factions.yml`...), as text.
#[derive(Clone, Debug, Default)]
pub struct Files {
    pub texts: BTreeMap<String, String>,
}

impl Files {
    fn yaml(&self, name: &str) -> Yaml {
        self.texts.get(name).and_then(|t| yaml::parse(t).ok()).unwrap_or_else(Yaml::map)
    }
}

pub struct Plugin {
    pub config: Config,
    pub config_text: String,
    pub settings: Settings,
    pub messages: Messages,
    pub squad: Squad,
    pub kits: Kits,
    pub stats: Stats,
    pub paths: Paths,
    pub scripts: Scripts,
    pub runner: Runner,
    pub store: BotStore,
    pub skins: SkinPool,
    pub bots: BTreeMap<String, Live>,
    pub ghosts: BTreeMap<String, Ghost>,
    pub recordings: BTreeMap<String, Recording>,
    pub recorders: BTreeMap<u128, Recorder>,
    pub latest: Vec<String>,
    pub arena: Option<Arena>,
    pub tick: u64,
    pub rng: Rng,
    pub players: Vec<PlayerInfo>,
    /// Owner uuid -> next summon allowed.
    summon_ready: BTreeMap<u128, u64>,
    dirty: BTreeSet<&'static str>,
    /// The operator level `horizonbot.admin` stands for.
    pub admin_level: u8,
    /// Who sees debug lines.
    debug_all: bool,
    /// Bot (lower case) -> tick its regear completes.
    pending_restock: BTreeMap<String, u64>,
    /// The menu each viewer has open: its page and what each slot does.
    menus: BTreeMap<u128, (crate::menu::Page, Vec<crate::menu::Click>)>,
    /// Kit editors open: viewer -> (kit, showing its ender chest page).
    pub kit_edits: BTreeMap<u128, (String, bool)>,
    /// Hits on or by each bot that another mod cancelled.
    cancelled: BTreeMap<String, u64>,
}


impl Plugin {
    pub fn new(config_text: &str, files: &Files) -> Result<Plugin, String> {
        let config = Config::parse(config_text)?;
        let mut p = Plugin {
            settings: config.settings(),
            config,
            config_text: config_text.to_owned(),
            messages: Messages::default(),
            squad: Squad::default(),
            kits: Kits::default(),
            stats: Stats::default(),
            paths: Paths::default(),
            scripts: Scripts::default(),
            runner: Runner::default(),
            store: BotStore::default(),
            skins: SkinPool::default(),
            bots: BTreeMap::new(),
            ghosts: BTreeMap::new(),
            recordings: BTreeMap::new(),
            recorders: BTreeMap::new(),
            latest: Vec::new(),
            arena: None,
            tick: 0,
            rng: Rng::new(0x5eed),
            players: Vec::new(),
            summon_ready: BTreeMap::new(),
            dirty: BTreeSet::new(),
            admin_level: 2,
            debug_all: false,
            pending_restock: BTreeMap::new(),
            menus: BTreeMap::new(),
            kit_edits: BTreeMap::new(),
            cancelled: BTreeMap::new(),
        };
        p.load_files(files);
        Ok(p)
    }

    fn load_files(&mut self, files: &Files) {
        self.squad.factions = Factions::load(&files.yaml("factions.yml"));
        self.kits = Kits::load(&files.yaml("kits.yml"));
        self.stats = Stats::load(&files.yaml("stats.yml"));
        self.paths = Paths::load(&files.yaml("paths.yml"));
        self.scripts = Scripts::load(&files.yaml("scripts.yml"));
        self.store = BotStore::load(&files.yaml("bots.yml"));
        self.messages = Messages::load(&files.yaml("messages.yml"));
        self.settings.factions = self.squad.factions.settings_layers();
        self.settings.bots = crate::store::load_bot_settings(&files.yaml("bot-settings.yml"));
        self.skins = SkinPool::new(self.config.built_in_pool, &self.config.pool);
        self.skins.load_cache(&files.yaml("skin-cache.yml"));
        for (name, text) in &files.texts {
            if let Some(rec) = name.strip_prefix("ghost-recordings/").and_then(|n| n.strip_suffix(".txt")) {
                if let Ok(r) = Recording::from_text(text) {
                    self.recordings.insert(rec.to_owned(), r);
                }
            }
        }
    }

    /// Effects for the first tick: bring back the bots that were online (`bots-relog`) and look
    /// up the skin pool.
    pub fn start(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        let missing = self.skins.missing();
        if self.config.skins_random != RandomSkins::Off && !missing.is_empty() {
            out.push(Effect::FetchPool(missing));
        }
        if self.settings.snapshot(None, None).b("bots-relog") {
            for b in std::mem::take(&mut self.store.bots) {
                let skin = self.skin_for(&b.name, b.skin_from.as_deref(), false);
                self.add_live(&b.name, &b.world, b.pos, b.yaw, b.skin_from.clone(), skin, &mut out);
                if let Some(l) = self.bots.get_mut(&b.name.to_ascii_lowercase()) {
                    l.bot.orders.paused = b.paused;
                    l.bot.orders.owner = b.owner.clone();
                    l.kit = b.kit.clone();
                    if let Some(ps) = &b.playstyle {
                        l.bot.playstyle = brain::personality::Playstyle::parse(ps);
                    }
                }
            }
        }
        out
    }

    fn mark(&mut self, file: &'static str) {
        self.dirty.insert(file);
    }

    /// The data files that changed since the last call, as `Save` effects.
    pub fn saves(&mut self) -> Vec<Effect> {
        let dirty = std::mem::take(&mut self.dirty);
        let mut out = Vec::new();
        for f in dirty {
            let doc = match f {
                "factions.yml" => self.squad.factions.save(),
                "kits.yml" => self.kits.save(),
                "stats.yml" => self.stats.save(),
                "paths.yml" => self.paths.save(),
                "scripts.yml" => self.scripts.save(),
                "bots.yml" => self.bots_doc(),
                "bot-settings.yml" => crate::store::save_bot_settings(&self.settings.bots),
                "skin-cache.yml" => self.skins.save_cache(),
                _ => continue,
            };
            out.push(Effect::Save { file: f.to_owned(), text: yaml::dump(&doc) });
        }
        if self.stats.dirty && self.tick % 600 == 0 {
            self.stats.dirty = false;
            out.push(Effect::Save { file: "stats.yml".into(), text: yaml::dump(&self.stats.save()) });
        }
        out
    }

    fn bots_doc(&self) -> Yaml {
        let mut s = self.store.clone();
        s.bots = self
            .bots
            .values()
            .filter(|l| !l.bot.ghost)
            .map(|l| SavedBot {
                name: l.bot.name.clone(),
                world: l.world.clone(),
                pos: l.bot.body.pos,
                yaw: l.bot.body.yaw,
                pitch: l.bot.body.pitch,
                skin_from: l.skin_from.clone(),
                owner: l.bot.orders.owner.clone(),
                paused: l.bot.orders.paused,
                kit: l.kit.clone(),
                playstyle: Some(l.bot.playstyle.name().to_owned()).filter(|p| p != "default"),
            })
            .collect();
        s.save()
    }

    /// The server is stopping: the online bots go into `bots.yml` for the next start.
    pub fn stopping(&mut self) -> Vec<Effect> {
        let mut out = vec![Effect::Save { file: "bots.yml".into(), text: yaml::dump(&self.bots_doc()) }];
        self.mark("stats.yml");
        out.extend(self.saves());
        out
    }

    fn msg(&self, key: &str, args: &[&str]) -> String {
        crate::messages::colours(&self.messages.get(key, args))
    }

    fn tell(&self, who: &Who, text: impl Into<String>) -> Effect {
        Effect::Tell { to: who.sender(), text: text.into() }
    }

    fn snapshot_for(&self, name: &str) -> settings::Snapshot {
        let f = self.squad.factions.faction_of(name).map(str::to_owned);
        let mut snap = self.settings.snapshot(f.as_deref(), Some(name));
        // Skill spread: inside a faction each bot sits a little above or below, by its name.
        let spread = snap.f("skill-spread");
        let own = self.settings.bots.get(&name.to_ascii_lowercase()).is_some_and(|l| l.contains_key("skill"));
        if f.is_some() && spread > 0.0 && !own {
            let off = crate::brain::personality::Personality::from_name(name, 50.0).skill_offset;
            let skill = snap.f("skill") + off * spread;
            snap.set("skill", settings::Value::Int(skill.round() as i64));
        }
        snap
    }

    fn skin_for(&mut self, name: &str, from: Option<&str>, none: bool) -> SkinSource {
        if none {
            return match self.config.skins_random {
                RandomSkins::Off => SkinSource::None,
                _ => self.skins.next_random(&mut self.rng).map_or(SkinSource::None, |(_, s)| SkinSource::Known(s)),
            };
        }
        if let Some(acc) = from {
            return match self.skins.get(acc) {
                Some(s) => SkinSource::Known(s.clone()),
                None => SkinSource::Fetch(acc.to_owned()),
            };
        }
        let uniform = self
            .squad
            .factions
            .faction_of(name)
            .and_then(|f| self.squad.factions.factions.get(f))
            .and_then(|f| f.skin.clone());
        if let Some(u) = uniform {
            return match self.skins.get(&u) {
                Some(s) => SkinSource::Known(s.clone()),
                None => SkinSource::Fetch(u),
            };
        }
        match self.config.skins_random {
            RandomSkins::Always => self.skins.next_random(&mut self.rng).map_or(SkinSource::None, |(_, s)| SkinSource::Known(s)),
            RandomSkins::Off => SkinSource::Fetch(name.to_owned()),
            RandomSkins::Missing => match self.skins.cache.get(&name.to_ascii_lowercase()) {
                Some(Some(s)) => SkinSource::Known(s.clone()),
                // Known to have no skin of its own: one from the pool.
                Some(None) => self.skins.next_random(&mut self.rng).map_or(SkinSource::None, |(_, s)| SkinSource::Known(s)),
                None => SkinSource::Fetch(name.to_owned()),
            },
        }
    }

    /// A skin lookup the glue made came back (None: that account has no skin or doesn't exist).
    pub fn skin_fetched(&mut self, account: &str, skin: Option<Skin>) -> Option<Skin> {
        self.skins.put(account, skin.clone());
        self.mark("skin-cache.yml");
        match skin {
            Some(s) => Some(s),
            None if self.config.skins_random == RandomSkins::Missing => self.skins.next_random(&mut self.rng).map(|(_, s)| s),
            None => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add_live(&mut self, name: &str, world: &str, pos: Vec3, yaw: f32, skin_from: Option<String>, skin: SkinSource, out: &mut Vec<Effect>) {
        // `[horizon] max-bots` also holds for relogs and companions.
        if self.bots.len() >= self.config.max_bots as usize && !self.bots.contains_key(&name.to_ascii_lowercase()) {
            return;
        }
        let uuid = crate::uuid::offline(name);
        let snap = self.snapshot_for(name);
        let mut bot = Bot::new(name, uuid, pos, yaw, snap, self.tick);
        bot.faction = self.squad.factions.faction_of(name).map(str::to_owned);
        self.store.remember(name);
        self.mark("bots.yml");
        out.push(Effect::SpawnBot { name: name.to_owned(), uuid, skin, world: Some(world.to_owned()), pos: Some(pos), yaw });
        self.bots.insert(
            name.to_ascii_lowercase(),
            Live { bot, world: world.to_owned(), skin_from, joined: false, parked: false, kit: None },
        );
    }

    fn taken_names(&self) -> BTreeSet<String> {
        let mut t: BTreeSet<String> = self.bots.keys().cloned().collect();
        t.extend(self.players.iter().map(|p| p.name.to_ascii_lowercase()));
        t.extend(self.ghosts.keys().map(|g| g.to_ascii_lowercase()));
        t
    }

    fn candidates(&self) -> Vec<Candidate<'_>> {
        self.bots
            .values()
            .filter(|l| !l.bot.ghost)
            .map(|l| Candidate {
                name: &l.bot.name,
                faction: l.bot.faction.as_deref(),
                paused: l.bot.orders.paused,
                world: &l.world,
                has_items: l.kit.is_some(),
            })
            .collect()
    }

    /// The bots a selector names (`latest` as `latest` reads for this sender).
    pub fn select(&mut self, sel: &Selector, latest: &[String]) -> Vec<String> {
        let factions = self.squad.factions.clone();
        let of = |o: &str| factions.overfactions.get(o).map(|x| x.factions.clone()).unwrap_or_default();
        let cands = self.candidates();
        let mut rng = self.rng.clone();
        let names = selector::pick(sel, &cands, latest, &of, &mut rng);
        drop(cands);
        self.rng = rng;
        names
    }

    fn bot_mut(&mut self, name: &str) -> Option<&mut Live> {
        self.bots.get_mut(&name.to_ascii_lowercase())
    }

    fn origin(&self, who: &Who) -> (String, Option<Vec3>, f32) {
        match who {
            Who::Player(p) => (p.world.clone(), Some(p.pos), p.yaw),
            Who::Console => ("minecraft:overworld".into(), None, 0.0),
        }
    }

    fn resolve_target(&self, who: &Who, t: &Target) -> Option<(Option<String>, Vec3)> {
        match t {
            Target::Pos { .. } => {
                let origin = who.player().map_or(Vec3::ZERO, |p| p.pos);
                t.resolve_pos(origin).map(|p| (None, p))
            }
            Target::Entity(name) => {
                if let Some(l) = self.bots.get(&name.to_ascii_lowercase()) {
                    return Some((Some(l.world.clone()), l.bot.body.pos));
                }
                self.players
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(name))
                    .map(|p| (Some(p.world.clone()), p.pos))
            }
        }
    }

    // ---------------------------------------------------------------- commands

    /// `/horizonbot ...` (or `/bot`): `line` is everything after the command name.
    pub fn command(&mut self, who: &Who, line: &str) -> Vec<Effect> {
        let latest = self.latest.clone();
        self.command_with_latest(who, line, &latest)
    }

    fn command_with_latest(&mut self, who: &Who, line: &str, latest: &[String]) -> Vec<Effect> {
        if let Some(rest) = line.trim_start().strip_prefix("ghost") {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return self.ghost_command(who, rest.trim());
            }
        }
        let cmd = match command::parse(line) {
            Ok(c) => c,
            Err(e) => return vec![self.tell(who, e)],
        };
        if !cmd.open_to_all() && !who.is_admin(self.admin_level) {
            return vec![self.tell(who, self.msg("err.no-permission", &[]))];
        }
        let mut out = Vec::new();
        self.run(who, cmd, latest, &mut out);
        out.extend(self.saves());
        out
    }

    /// `/horizonbot ghost ...`
    pub fn ghost_command(&mut self, who: &Who, line: &str) -> Vec<Effect> {
        if !who.is_admin(self.admin_level) {
            return vec![self.tell(who, self.msg("err.no-permission", &[]))];
        }
        let mut out = Vec::new();
        match command::parse_ghost(line) {
            Ok(g) => self.run(who, Command::Ghost(g), &[], &mut out),
            Err(e) => out.push(self.tell(who, e)),
        }
        out.extend(self.saves());
        out
    }

    fn need_bot(&self, who: &Who, name: &str, out: &mut Vec<Effect>) -> bool {
        if self.bots.contains_key(&name.to_ascii_lowercase()) {
            true
        } else {
            out.push(self.tell(who, self.msg("err.no-bot", &[name])));
            false
        }
    }

    fn each(&mut self, names: &[String], mut f: impl FnMut(&mut Live)) -> usize {
        let mut n = 0;
        for name in names {
            if let Some(l) = self.bots.get_mut(&name.to_ascii_lowercase()) {
                f(l);
                n += 1;
            }
        }
        n
    }

    fn done(&self, who: &Who, n: usize, out: &mut Vec<Effect>) {
        if n == 0 {
            out.push(self.tell(who, self.msg("cmd.none-spawned", &[])));
        } else {
            out.push(self.tell(who, self.msg("cmd.done", &[&n.to_string()])));
        }
    }

    fn refresh_settings(&mut self) {
        self.settings.factions = self.squad.factions.settings_layers();
        let names: Vec<String> = self.bots.keys().cloned().collect();
        for n in names {
            let snap = self.snapshot_for(&n);
            if let Some(l) = self.bots.get_mut(&n) {
                l.bot.playstyle = brain::personality::Playstyle::parse(&snap.s("playstyle"));
                l.bot.settings = snap;
                l.bot.faction = self.squad.factions.faction_of(&n).map(str::to_owned);
            }
        }
    }

    #[allow(clippy::cognitive_complexity)]
    fn run(&mut self, who: &Who, cmd: Command, latest: &[String], out: &mut Vec<Effect>) {
        match cmd {
            Command::Help => out.push(self.tell(who, HELP)),
            Command::Spawn { what, skin } => self.spawn(who, what, skin, out),
            Command::Remove(name) => {
                if let Some(l) = self.bots.remove(&name.to_ascii_lowercase()) {
                    out.push(Effect::RemoveBot { name: l.bot.name.clone(), uuid: l.bot.uuid });
                    out.push(self.tell(who, self.msg("cmd.removed", &[&l.bot.name])));
                    self.mark("bots.yml");
                } else {
                    out.push(self.tell(who, self.msg("err.no-bot", &[&name])));
                }
            }
            Command::RemoveAll => {
                let all: Vec<Live> = std::mem::take(&mut self.bots).into_values().collect();
                for l in &all {
                    out.push(Effect::RemoveBot { name: l.bot.name.clone(), uuid: l.bot.uuid });
                }
                out.push(self.tell(who, self.msg("cmd.removed-all", &[&all.len().to_string()])));
                self.mark("bots.yml");
            }
            Command::List => {
                let names: Vec<String> = self
                    .bots
                    .values()
                    .filter(|l| !l.bot.ghost)
                    .map(|l| {
                        let state = if l.bot.orders.paused { " (paused)" } else if !l.joined { " (joining)" } else { "" };
                        format!("{}{}", l.bot.name, state)
                    })
                    .collect();
                out.push(self.tell(who, self.msg("cmd.list", &[&names.len().to_string(), &names.join(", ")])));
            }
            Command::Menu => match who.player() {
                Some(p) => out.push(Effect::OpenMenu { viewer: p.uuid, page: crate::menu::Page::Hub }),
                None => out.push(self.tell(who, "Menus are for players")),
            },
            Command::Restock(sel, instant) => {
                let names = self.select(&sel, latest);
                for n in &names {
                    let kit = self.bots.get(&n.to_ascii_lowercase()).and_then(|l| l.kit.clone()).and_then(|k| self.kits.kits.get(&k).cloned());
                    if instant {
                        out.push(Effect::Restock { bot: n.clone(), kit });
                    } else {
                        // The ender-chest show is not played yet: the bot backs off (regear) and
                        // is filled after the time the show takes.
                        let due = self.tick + 60 + self.rng.int(0, 40) as u64;
                        self.pending_restock.insert(n.to_ascii_lowercase(), due);
                        if let Some(l) = self.bot_mut(n) {
                            l.bot.orders.retreat = Some(brain::Retreat { to: brain::RetreatTo::Scatter(Vec3::new(1.0, 0.0, 0.0)) });
                        }
                    }
                }
                self.done(who, names.len(), out);
            }
            Command::View { enderchest, bot } => {
                if let (Some(p), true) = (who.player(), self.need_bot(who, &bot, out)) {
                    out.push(Effect::OpenView { viewer: p.uuid, bot, enderchest });
                }
            }
            Command::Reload => {
                // The glue re-reads the files and calls `reload`.
                self.runner.stop_all();
                out.push(Effect::Reload);
                out.push(self.tell(who, self.msg("cmd.reloaded", &[])));
            }
            Command::Attack(bot, target) => {
                if self.need_bot(who, &bot, out) {
                    let l = self.bot_mut(&bot).unwrap();
                    l.bot.orders.attack = Some(target.clone());
                    l.bot.orders.stopped = false;
                    l.bot.orders.retreat = None;
                    out.push(self.tell(who, format!("{bot} hunts {target}")));
                }
            }
            Command::Fight(a, b) => {
                if self.need_bot(who, &a, out) && self.need_bot(who, &b, out) {
                    self.bot_mut(&a).unwrap().bot.orders.attack = Some(b.clone());
                    self.bot_mut(&b).unwrap().bot.orders.attack = Some(a.clone());
                    out.push(self.tell(who, format!("{a} and {b} fight")));
                }
            }
            Command::Stop(bot) => {
                if self.need_bot(who, &bot, out) {
                    let l = self.bot_mut(&bot).unwrap();
                    l.bot.orders = brain::Orders { owner: l.bot.orders.owner.clone(), paused: l.bot.orders.paused, ..Default::default() };
                    l.bot.revenge = None;
                    l.bot.fight = Default::default();
                    out.push(self.tell(who, format!("{bot} stands down")));
                }
            }
            Command::Retreat(sel, to) => {
                let names = self.select(&sel, latest);
                let dest = match &to {
                    Some(Target::Entity(n)) => Some(brain::RetreatTo::Player(n.clone())),
                    Some(t @ Target::Pos { .. }) => self.resolve_target(who, t).map(|(_, p)| brain::RetreatTo::Point(p)),
                    None => None,
                };
                let mut rng = self.rng.clone();
                let n = self.each(&names, |l| {
                    let to = dest.clone().unwrap_or_else(|| {
                        let a = rng.unit() * std::f64::consts::TAU;
                        brain::RetreatTo::Scatter(Vec3::new(a.cos(), 0.0, a.sin()))
                    });
                    l.bot.orders.retreat = Some(brain::Retreat { to });
                    l.bot.orders.attack = None;
                });
                self.rng = rng;
                self.done(who, n, out);
            }
            Command::RetreatStop(sel) => {
                let names = self.select(&sel, latest);
                let n = self.each(&names, |l| l.bot.orders.retreat = None);
                self.done(who, n, out);
            }
            Command::Hide(sel) => {
                let names = self.select(&sel, latest);
                let n = self.each(&names, |l| l.bot.orders.hide = true);
                self.done(who, n, out);
            }
            Command::HideStop(sel) => {
                let names = self.select(&sel, latest);
                let n = self.each(&names, |l| {
                    l.bot.orders.hide = false;
                    l.bot.hidden_with_potion = false;
                });
                self.done(who, n, out);
            }
            Command::Tether { target, leader, escort, radius } => {
                let names = self.select(&escort, latest);
                let n = self.each(&names, |l| {
                    l.bot.orders.tether = Some(brain::TetherRole { target: target.clone(), leader: leader.clone(), radius });
                });
                self.squad.tethers.insert(target.to_ascii_lowercase(), Default::default());
                out.push(self.tell(who, format!("{n} bots keep {target} within {radius:.0} blocks of {leader}")));
            }
            Command::TetherStop(target) => {
                let t = target.to_ascii_lowercase();
                let names: Vec<String> = self.bots.values().filter(|l| l.bot.orders.tether.as_ref().is_some_and(|r| r.target.eq_ignore_ascii_case(&t))).map(|l| l.bot.name.clone()).collect();
                let n = self.each(&names, |l| {
                    l.bot.orders.tether = None;
                    if l.bot.orders.attack.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(&t)) {
                        l.bot.orders.attack = None;
                    }
                });
                self.squad.tethers.remove(&t);
                out.push(self.tell(who, format!("Released {target}; {n} bots stand down")));
            }
            Command::TetherInfo(target) => {
                let t = target.to_ascii_lowercase();
                let escort = self.bots.values().filter(|l| l.bot.orders.tether.as_ref().is_some_and(|r| r.target.eq_ignore_ascii_case(&t))).count();
                let state = self.squad.tethers.get(&t).cloned().unwrap_or_default();
                let s = if state.hostile { "hostile" } else if state.warned_at.is_some() { "warned" } else { "inside" };
                out.push(self.tell(who, format!("{target}: {escort} escorts, {s}")));
            }
            Command::Follow { who: sel, target, formation, size } => {
                let names = self.select(&sel, latest);
                let shape = formation.as_deref().and_then(Shape::parse).unwrap_or(Shape::Behind);
                let of = names.len();
                let mut i = 0;
                let n = self.each(&names, |l| {
                    l.bot.orders.follow = Some(brain::Follow { target: target.clone(), shape, size: size.unwrap_or(3) as usize, slot: i, of });
                    i += 1;
                });
                self.done(who, n, out);
            }
            Command::FollowStop(sel) => {
                let names = self.select(&sel, latest);
                let n = self.each(&names, |l| l.bot.orders.follow = None);
                self.done(who, n, out);
            }
            Command::Guard { who: sel, radius, at } => {
                let post = match at.as_ref().and_then(|t| self.resolve_target(who, t)) {
                    Some((_, p)) => Some(p),
                    None => who.player().map(|p| p.pos),
                };
                let Some(post) = post else {
                    out.push(self.tell(who, "From the console give x y z"));
                    return;
                };
                let names = self.select(&sel, latest);
                let of = names.len().max(1);
                let mut i = 0;
                let n = self.each(&names, |l| {
                    l.bot.orders.guard = Some(brain::Guard { post, radius, sector: i as f64 / of as f64 });
                    i += 1;
                });
                self.done(who, n, out);
            }
            Command::GuardStop(sel) => {
                let names = match sel {
                    Some(s) => self.select(&s, latest),
                    None => self.bots.values().map(|l| l.bot.name.clone()).collect(),
                };
                let n = self.each(&names, |l| l.bot.orders.guard = None);
                self.done(who, n, out);
            }
            Command::Pause(None) => {
                let paused: Vec<String> = self.bots.values().filter(|l| l.bot.orders.paused).map(|l| l.bot.name.clone()).collect();
                out.push(self.tell(who, format!("Paused: {}", if paused.is_empty() { "none".into() } else { paused.join(", ") })));
            }
            Command::Pause(Some(sel)) => {
                let names = self.select(&sel, latest);
                let n = self.each(&names, |l| l.bot.orders.paused = true);
                self.mark("bots.yml");
                self.done(who, n, out);
            }
            Command::Resume(sel) => {
                let names = self.select(&sel, latest);
                let n = self.each(&names, |l| l.bot.orders.paused = false);
                self.mark("bots.yml");
                self.done(who, n, out);
            }
            Command::Tp { mode, who: sel, to, range } => self.tp(who, mode, sel, to, range, latest, out),
            Command::Path(pc) => self.path(who, pc, latest, out),
            Command::Set(key, value) => match self.settings.set_global(&key, &value) {
                Ok(warn) => {
                    let v = self.settings.base.get(&key).cloned().unwrap();
                    match crate::config::write_setting(&self.config_text, &key, &v) {
                        Ok(t) => {
                            self.config_text = t.clone();
                            out.push(Effect::SaveConfig(t));
                        }
                        Err(e) => out.push(self.tell(who, format!("Not written to the config: {e}"))),
                    }
                    self.refresh_settings();
                    out.push(self.tell(who, self.msg("cmd.set", &[&key, &v.render()])));
                    if let Some(w) = warn {
                        out.push(self.tell(who, w));
                    }
                }
                Err(e) => out.push(self.tell(who, e)),
            },
            Command::Settings { scope, filter, list } => {
                let snap = match &scope {
                    Scope::Global => self.settings.snapshot(None, None),
                    Scope::Bot(b) => self.snapshot_for(b),
                    Scope::Faction(f) => self.settings.snapshot(Some(f), None),
                };
                let lines: Vec<String> = snap
                    .all()
                    .filter(|(s, _)| filter.as_ref().is_none_or(|f| s.group == f || s.key.contains(f.as_str())))
                    .map(|(s, v)| format!("{}{} = {}", s.key, if self.settings.is_default(s.key) { "*" } else { "" }, v.render()))
                    .collect();
                if !list {
                    if let Some(p) = who.player() {
                        let group = filter.clone().filter(|f| settings::groups().contains(&f.as_str()));
                        out.push(Effect::OpenMenu { viewer: p.uuid, page: crate::menu::Page::Settings { scope: scope.clone(), group, page: 0 } });
                    }
                }
                for chunk in lines.chunks(12) {
                    out.push(self.tell(who, chunk.join("  ")));
                }
            }
            Command::Difficulty(d) => self.difficulty(who, d, out),
            Command::PlaystyleList => out.push(self.tell(who, format!("Playstyles: {}", crate::config::PLAYSTYLES.join(", ")))),
            Command::Playstyle(style, sel) => {
                let names = self.select(&sel, latest);
                for n in &names {
                    let _ = self.settings.set_bot(n, "playstyle", &style);
                }
                self.mark("bot-settings.yml");
                self.refresh_settings();
                self.done(who, names.len(), out);
            }
            Command::PlaystyleClear(sel) => {
                let names = self.select(&sel, latest);
                for n in &names {
                    self.settings.clear_bot_keys(n, &["playstyle"]);
                }
                self.mark("bot-settings.yml");
                self.refresh_settings();
                self.done(who, names.len(), out);
            }
            Command::PlaystyleFaction(f, style) => {
                if !self.squad.factions.factions.contains_key(&f) {
                    out.push(self.tell(who, self.msg("err.no-faction", &[&f])));
                    return;
                }
                match &style {
                    Some(s) => {
                        let _ = self.settings.set_faction(&f, "playstyle", s);
                        self.squad.factions.factions.get_mut(&f).unwrap().settings.insert("playstyle".into(), settings::Value::Text(s.clone()));
                    }
                    None => {
                        self.squad.factions.factions.get_mut(&f).unwrap().settings.remove("playstyle");
                    }
                }
                self.mark("factions.yml");
                self.refresh_settings();
                out.push(self.tell(who, format!("{f}: {}", style.unwrap_or_else(|| "cleared".into()))));
            }
            Command::RandomPlaystyle(sel) => {
                let names = self.select(&sel, latest);
                for n in &names {
                    let s = brain::personality::roll_playstyle(&self.config.playstyle_weights, &mut self.rng);
                    let _ = self.settings.set_bot(n, "playstyle", s.name());
                }
                self.mark("bot-settings.yml");
                self.refresh_settings();
                self.done(who, names.len(), out);
            }
            Command::RandomPlaystyleChances(set, reset) => {
                let set_any = set.is_some();
                if reset {
                    self.config.playstyle_weights = crate::config::default_weights();
                }
                if let Some((style, w)) = set {
                    if let Some(e) = self.config.playstyle_weights.iter_mut().find(|(s, _)| *s == style) {
                        e.1 = w;
                    }
                }
                if reset || set_any {
                    if let Ok(t) = crate::config::write_weights(&self.config_text, &self.config.playstyle_weights) {
                        self.config_text = t.clone();
                        out.push(Effect::SaveConfig(t));
                    }
                }
                let total: f64 = self.config.playstyle_weights.iter().map(|(_, w)| w).sum();
                let line = self
                    .config
                    .playstyle_weights
                    .iter()
                    .map(|(s, w)| format!("{s} {:.0}%", if total > 0.0 { w * 100.0 / total } else { 0.0 }))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(self.tell(who, line));
            }
            Command::Techniques { scope, set } => {
                let techs = settings::techniques();
                if let Some((t, v)) = set {
                    let Some(spec) = techs.iter().find(|s| s.key == t) else {
                        out.push(self.tell(who, self.msg("err.no-technique", &[&t])));
                        return;
                    };
                    let v = match v.as_str() {
                        "on" => "true".to_owned(),
                        "off" => "false".to_owned(),
                        other => other.to_owned(),
                    };
                    let r = match &scope {
                        Scope::Global => self.settings.set_global(spec.key, &v).map(|_| ()),
                        Scope::Bot(b) => self.settings.set_bot(b, spec.key, &v),
                        Scope::Faction(f) => self.faction_set(f, spec.key, &v),
                    };
                    match r {
                        Ok(()) => {
                            self.after_scope_change(&scope, spec.key, out);
                            out.push(self.tell(who, format!("{} = {v}", spec.key)));
                        }
                        Err(e) => out.push(self.tell(who, e)),
                    }
                } else {
                    let snap = match &scope {
                        Scope::Global => self.settings.snapshot(None, None),
                        Scope::Bot(b) => self.snapshot_for(b),
                        Scope::Faction(f) => self.settings.snapshot(Some(f), None),
                    };
                    let line = techs.iter().map(|s| format!("{} {}", s.key, snap.s(s.key))).collect::<Vec<_>>().join(", ");
                    out.push(self.tell(who, line));
                }
            }
            Command::Faction(fc) => self.faction(who, fc, latest, out),
            Command::Overfaction(oc) => self.overfaction(who, oc, out),
            Command::Kit(kc) => self.kit(who, kc, latest, out),
            Command::Preset(pc) => self.preset(who, pc, out),
            Command::Script(sc) => self.script(who, sc, out),
            Command::Owner(oc) => self.owner(who, oc, out),
            Command::Diagnose(name, reset) => self.diagnose(who, &name, reset, out),
            Command::Arena(ac) => self.arena_cmd(who, ac, out),
            Command::DebugView(sel) => {
                // `debugview off` ends every bot's debug line; otherwise the command toggles the
                // selected bots together: all off if any was on, else all on.
                let off_all = matches!(&sel, Selector::Name(n) if n.eq_ignore_ascii_case("off"));
                let names = if off_all { self.bots.values().map(|l| l.bot.name.clone()).collect() } else { self.select(&sel, latest) };
                let any_on = off_all || names.iter().any(|n| self.bots.get(&n.to_ascii_lowercase()).is_some_and(|l| l.bot.debug_view));
                if matches!(sel, Selector::All) || off_all {
                    self.debug_all = !any_on;
                }
                let n = self.each(&names, |l| l.bot.debug_view = !any_on);
                if any_on {
                    for name in &names {
                        out.push(Effect::NameLine { bot: name.clone(), text: None });
                    }
                }
                self.done(who, n, out);
            }
            Command::Stats(name) => {
                let name = name.or_else(|| who.player().map(|p| p.name.clone()));
                match name {
                    None => out.push(self.tell(who, format!("{}: {}", self.messages.get("stats.everyone", &[]), self.stats.records.len()))),
                    Some(n) => match self.stats.find(&n) {
                        Some(r) => out.push(self.tell(who, stat_line(&self.messages, r, ""))),
                        None => out.push(self.tell(who, self.msg("stats.no-stats", &[&n]))),
                    },
                }
            }
            Command::StatsTop(by) => {
                let top = self.stats.top(by, 10);
                if top.is_empty() {
                    out.push(self.tell(who, self.msg("stats.none-yet", &[])));
                } else {
                    let label = match by {
                        crate::stats::Top::Kills => "kills",
                        crate::stats::Top::Kd => "K/D",
                        crate::stats::Top::Accuracy => "accuracy",
                        crate::stats::Top::Damage => "damage",
                    };
                    out.push(self.tell(who, self.msg("stats.top", &["10", label])));
                    for (i, r) in top.iter().enumerate() {
                        out.push(self.tell(who, stat_line(&self.messages, r, &format!("{}. ", i + 1))));
                    }
                }
            }
            Command::StatsReset(name) => {
                let n = self.stats.reset(&name);
                self.mark("stats.yml");
                out.push(self.tell(who, self.msg("stats.reset", &[&name])));
            }
            Command::Debug(name, inv) => {
                if !self.need_bot(who, &name, out) {
                    return;
                }
                if let Some(on) = inv {
                    out.push(Effect::SetInvulnerable { name: name.clone(), on });
                }
                let l = &self.bots[&name.to_ascii_lowercase()];
                let b = &l.bot;
                out.push(self.tell(
                    who,
                    format!(
                        "{}: mode {}, target {}, known {}, faction {}, built {} in the last minute, busy {}, paused {}, last call \"{}\"",
                        b.name,
                        b.last_mode,
                        if b.fight.target.is_some() { b.fight.target_name.as_str() } else { "none" },
                        b.known.len(),
                        b.faction.as_deref().unwrap_or("none"),
                        b.built.len(),
                        if b.busy.is_some() { "yes" } else { "no" },
                        b.orders.paused,
                        b.last_call
                    ),
                ));
            }
            Command::GiveBow(name) => {
                if self.need_bot(who, &name, out) {
                    out.push(Effect::GiveItems { bot: name, items: vec![("minecraft:bow".into(), 1), ("minecraft:arrow".into(), 64)] });
                }
            }
            Command::Drop(bot, item, n) => {
                if self.need_bot(who, &bot, out) {
                    out.push(Effect::DropItem { bot, item, count: n });
                }
            }
            Command::ResetAttr(name) => out.push(Effect::ResetAttributes { name }),
            Command::ClearPlayerData => {
                let live: BTreeSet<String> = self.bots.keys().cloned().collect();
                let gone: Vec<u128> = self
                    .store
                    .history
                    .iter()
                    .filter(|h| !live.contains(&h.to_ascii_lowercase()))
                    .map(|h| crate::uuid::offline(h))
                    .collect();
                out.push(self.tell(who, format!("Clearing the files of {} old bots", gone.len())));
                out.push(Effect::ClearPlayerData(gone));
            }
            Command::Ghost(g) => self.ghost(who, g, out),
        }
    }

    fn faction_set(&mut self, f: &str, key: &str, v: &str) -> Result<(), String> {
        let spec = settings::spec(key).ok_or_else(|| format!("No setting called {key}"))?;
        let val = spec.parse(v)?;
        let fac = self.squad.factions.factions.get_mut(f).ok_or_else(|| self.messages.get("err.no-faction", &[f]))?;
        fac.settings.insert(key.to_owned(), val);
        self.mark("factions.yml");
        Ok(())
    }

    fn after_scope_change(&mut self, scope: &Scope, key: &str, out: &mut Vec<Effect>) {
        match scope {
            Scope::Global => {
                if let Some(v) = self.settings.base.get(key).cloned() {
                    if let Ok(t) = crate::config::write_setting(&self.config_text, key, &v) {
                        self.config_text = t.clone();
                        out.push(Effect::SaveConfig(t));
                    }
                }
            }
            Scope::Bot(_) => self.mark("bot-settings.yml"),
            Scope::Faction(_) => self.mark("factions.yml"),
        }
        self.refresh_settings();
    }

    fn spawn(&mut self, who: &Who, what: SpawnWhat, skin: SkinArg, out: &mut Vec<Effect>) {
        let (world, pos, yaw) = self.origin(who);
        let pos = pos.unwrap_or(Vec3::new(0.5, 64.0, 0.5));
        let taken = self.taken_names();
        let mut names: Vec<String> = match &what {
            SpawnWhat::One => vec![crate::skins::generate_name(&mut self.rng, &taken)],
            SpawnWhat::Named(n) => {
                if taken.contains(&n.to_ascii_lowercase()) {
                    out.push(self.tell(who, format!("{n} is taken")));
                    return;
                }
                vec![n.clone()]
            }
            SpawnWhat::Count(c) => {
                let mut t = taken.clone();
                (0..*c)
                    .map(|_| {
                        let n = crate::skins::generate_name(&mut self.rng, &t);
                        t.insert(n.to_ascii_lowercase());
                        n
                    })
                    .collect()
            }
            SpawnWhat::Table(table, c) => {
                let Some(list) = self.config.name_tables.get(table).cloned() else {
                    out.push(self.tell(who, format!("No name table called {table}")));
                    return;
                };
                let got = crate::skins::from_table(&list, *c as usize, &taken, &mut self.rng);
                if got.len() < *c as usize {
                    out.push(self.tell(who, self.msg("cmd.spawned-some", &[&got.len().to_string(), &c.to_string()])));
                }
                got
            }
        };
        // `[horizon] max-bots`: only as many as fit under the cap.
        let room = (self.config.max_bots as usize).saturating_sub(self.bots.len());
        if names.len() > room {
            let asked = names.len();
            names.truncate(room);
            let text = if room == 0 {
                format!("Bot limit reached ({} bots, max-bots)", self.config.max_bots)
            } else {
                format!("Bot limit (max-bots {}): spawned {room} of {asked}", self.config.max_bots)
            };
            out.push(self.tell(who, text));
            if room == 0 {
                return;
            }
        }
        let skin_from = match &skin {
            SkinArg::From(a) => Some(a.clone()),
            _ => None,
        };
        let none = skin == SkinArg::None;
        let n = names.len();
        for (i, name) in names.iter().enumerate() {
            // A batch stands in a loose ring, not on one block.
            let at = if n > 1 {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                let r = 1.5 + (n as f64).sqrt() * 0.6;
                pos + Vec3::new(a.cos() * r, 0.0, a.sin() * r)
            } else {
                pos
            };
            let src = self.skin_for(name, skin_from.as_deref(), none);
            self.add_live(name, &world, at, yaw, skin_from.clone(), src, out);
        }
        self.latest = names.clone();
        if self.runner.running().is_some() {
            self.runner.note_spawned(names.clone());
        }
        out.push(self.tell(who, self.msg("cmd.spawned", &[&names.join(", ")])));
    }

    #[allow(clippy::too_many_arguments)]
    fn tp(&mut self, who: &Who, mode: TpMode, sel: Selector, to: Target, range: f64, latest: &[String], out: &mut Vec<Effect>) {
        let Some((world, dest)) = self.resolve_target(who, &to) else {
            out.push(self.tell(who, "No such player or bot"));
            return;
        };
        let names = self.select(&sel, latest);
        if names.is_empty() {
            out.push(self.tell(who, self.msg("cmd.none-spawned", &[])));
            return;
        }
        let world = world.unwrap_or_else(|| self.origin(who).0);
        match mode {
            TpMode::Instant => {
                for n in &names {
                    out.push(Effect::Teleport { name: n.clone(), world: Some(world.clone()), pos: dest });
                }
            }
            TpMode::Pearl => {
                let spots = crate::teleport::scatter(dest, names.len(), range, 2.5, &mut self.rng);
                for (n, s) in names.iter().zip(spots) {
                    let (start, vel) = crate::teleport::pearl_start(s);
                    if let Some(l) = self.bots.get(&n.to_ascii_lowercase()) {
                        out.push(Effect::Pearl { owner: l.bot.uuid, world: world.clone(), pos: start, vel });
                    }
                }
            }
            TpMode::Portal { ground } => {
                let portals = crate::teleport::portals(dest, names.len(), range, ground, &|_, _| None, &mut self.rng);
                let mut it = names.iter();
                for (portal, take) in portals {
                    out.push(Effect::SetBlocks { world: world.clone(), blocks: portal.blocks() });
                    for k in 0..take {
                        if let Some(n) = it.next() {
                            let off = Vec3::new(if k % 2 == 0 { -0.4 } else { 0.4 }, 0.0, 0.0);
                            out.push(Effect::Teleport { name: n.clone(), world: Some(world.clone()), pos: portal.inside() + off });
                        }
                    }
                }
            }
        }
        self.done(who, names.len(), out);
    }

    fn path(&mut self, who: &Who, pc: PathCmd, latest: &[String], out: &mut Vec<Effect>) {
        let r: Result<String, String> = match pc {
            PathCmd::Create(n) => self.paths.create(&n).map(|_| format!("Route {n} created")),
            PathCmd::Add(n) => match who.player() {
                Some(p) => self.paths.add(&n, &p.world, p.pos).map(|i| format!("Point {i} added to {n}")),
                None => Err("Points are added where a player stands".into()),
            },
            PathCmd::Remove(n, i) => self.paths.remove(&n, i).map(|_| format!("Point {i} removed")),
            PathCmd::Delete(n) => {
                let gone = self.paths.paths.remove(&n).is_some();
                for l in self.bots.values_mut() {
                    if l.bot.orders.patrol.as_ref().is_some_and(|p| p.path == n) {
                        l.bot.orders.patrol = None;
                        l.bot.orders.patrol_point = None;
                    }
                }
                if gone { Ok(format!("Route {n} deleted")) } else { Err(self.msg("err.no-path", &[&n])) }
            }
            PathCmd::List(None) => Ok(format!(
                "Routes: {}",
                self.paths.paths.values().map(|p| format!("{} ({})", p.name, p.waypoints.len())).collect::<Vec<_>>().join(", ")
            )),
            PathCmd::List(Some(n)) => match self.paths.paths.get(&n) {
                Some(p) => Ok(p
                    .waypoints
                    .iter()
                    .enumerate()
                    .map(|(i, w)| format!("{}: {:.0} {:.0} {:.0} {} ({} commands)", i + 1, w.pos.x, w.pos.y, w.pos.z, w.world, w.commands.len()))
                    .collect::<Vec<_>>()
                    .join(" | ")),
                None => Err(self.msg("err.no-path", &[&n])),
            },
            PathCmd::Assign(sel, route) => {
                if !self.paths.paths.contains_key(&route) {
                    Err(self.msg("err.no-path", &[&route]))
                } else {
                    let names = self.select(&sel, latest);
                    let n = self.each(&names, |l| l.bot.orders.patrol = Some(Patrol::new(&route)));
                    Ok(format!("{n} bots patrol {route}"))
                }
            }
            PathCmd::Stop(sel) => {
                let names = self.select(&sel, latest);
                let n = self.each(&names, |l| {
                    l.bot.orders.patrol = None;
                    l.bot.orders.patrol_point = None;
                });
                Ok(format!("{n} bots stop patrolling"))
            }
            PathCmd::CommandAdd(p, i, c) => self.paths.waypoint_mut(&p, i).map(|w| {
                w.commands.push(c);
                format!("Point {i} runs {} commands", w.commands.len())
            }),
            PathCmd::CommandList(p, i) => self.paths.waypoint_mut(&p, i).map(|w| {
                w.commands.iter().enumerate().map(|(k, c)| format!("{}: {c}", k + 1)).collect::<Vec<_>>().join(" | ")
            }),
            PathCmd::CommandRemove(p, i, k) => self.paths.waypoint_mut(&p, i).and_then(|w| {
                if k == 0 || k > w.commands.len() {
                    Err(format!("Point {i} has commands 1 to {}", w.commands.len()))
                } else {
                    w.commands.remove(k - 1);
                    Ok("Removed".into())
                }
            }),
            PathCmd::CommandClear(p, i) => self.paths.waypoint_mut(&p, i).map(|w| {
                w.commands.clear();
                "Cleared".into()
            }),
        };
        self.mark("paths.yml");
        match r {
            Ok(m) | Err(m) => out.push(self.tell(who, m)),
        }
    }

    fn difficulty(&mut self, who: &Who, d: Difficulty, out: &mut Vec<Effect>) {
        let scope_snap = |p: &Plugin, s: &Scope| match s {
            Scope::Global => p.settings.snapshot(None, None),
            Scope::Bot(b) => p.snapshot_for(b),
            Scope::Faction(f) => p.settings.snapshot(Some(f), None),
        };
        match d {
            Difficulty::List(scope) => {
                let snap = scope_snap(self, &scope);
                let line = settings::KNOBS.iter().map(|k| format!("{k} {}", snap.s(k))).collect::<Vec<_>>().join(", ");
                out.push(self.tell(who, format!("skill {}: {line}", snap.s("skill"))));
            }
            Difficulty::Preset(scope, level) => {
                let layer = settings::difficulty_preset(&level).unwrap_or_default();
                for (k, v) in &layer {
                    let v = v.render();
                    let _ = match &scope {
                        Scope::Global => self.settings.set_global(k, &v).map(|_| ()),
                        Scope::Bot(b) => self.settings.set_bot(b, k, &v),
                        Scope::Faction(f) => self.faction_set(f, k, &v),
                    };
                    self.after_scope_change(&scope, k, out);
                }
                out.push(self.tell(who, format!("Difficulty {level}")));
            }
            Difficulty::Set(scope, knob, v) => {
                let r = match &scope {
                    Scope::Global => self.settings.set_global(&knob, &v).map(|_| ()),
                    Scope::Bot(b) => self.settings.set_bot(b, &knob, &v),
                    Scope::Faction(f) => self.faction_set(f, &knob, &v),
                };
                match r {
                    Ok(()) => {
                        self.after_scope_change(&scope, &knob, out);
                        out.push(self.tell(who, format!("{knob} = {v}")));
                    }
                    Err(e) => out.push(self.tell(who, e)),
                }
            }
            Difficulty::Clear(scope) => {
                match &scope {
                    Scope::Bot(b) => self.settings.clear_bot_keys(b, settings::KNOBS),
                    Scope::Faction(f) => {
                        for k in settings::KNOBS {
                            if let Some(fac) = self.squad.factions.factions.get_mut(f) {
                                fac.settings.remove(*k);
                            }
                        }
                    }
                    Scope::Global => {
                        for k in settings::KNOBS {
                            self.settings.base.remove(*k);
                        }
                    }
                }
                self.after_scope_change(&scope, "skill", out);
                out.push(self.tell(who, "Knobs follow skill again"));
            }
        }
    }

    fn faction(&mut self, who: &Who, fc: FactionCmd, latest: &[String], out: &mut Vec<Effect>) {
        let fx = &mut self.squad.factions;
        let r: Result<String, String> = match fc {
            FactionCmd::Create(n) => fx.create(&n).map(|_| format!("Faction {n} created")),
            FactionCmd::Disband(n) => fx.disband(&n).map(|_| format!("Faction {n} disbanded")),
            FactionCmd::Join(f, sel) => {
                let names = match &sel {
                    Selector::Name(n) if !self.bots.contains_key(&n.to_ascii_lowercase()) => vec![n.clone()],
                    other => self.select(other, latest),
                };
                let fx = &mut self.squad.factions;
                let mut n = 0;
                let mut err = None;
                for m in &names {
                    match fx.join(&f, m) {
                        Ok(()) => n += 1,
                        Err(e) => err = Some(e),
                    }
                }
                err.map_or(Ok(format!("{n} joined {f}")), Err)
            }
            FactionCmd::JoinRange(f, r) => {
                let Some(p) = who.player() else {
                    out.push(self.tell(who, "range needs a player"));
                    return;
                };
                let near: Vec<String> = self.bots.values().filter(|l| l.bot.body.pos.distance(p.pos) <= r).map(|l| l.bot.name.clone()).collect();
                let fx = &mut self.squad.factions;
                let mut n = 0;
                for m in &near {
                    if fx.join(&f, m).is_ok() {
                        n += 1;
                    }
                }
                Ok(format!("{n} joined {f}"))
            }
            FactionCmd::Leave(m) => match fx.leave(&m) {
                Some(f) => Ok(format!("{m} left {f}")),
                None => Err(format!("{m} is in no faction")),
            },
            FactionCmd::Hostile(a, b) => fx.hostile(&a, &b).map(|_| format!("{a} and {b} are at war")),
            FactionCmd::Peace(a, b) => fx.peace(&a, &b).map(|_| format!("{a} and {b} are at peace")),
            FactionCmd::Ally(a, b) => fx.ally(&a, &b).map(|_| format!("{a} and {b} are allies")),
            FactionCmd::Unally(a, b) => fx.unally(&a, &b).map(|_| format!("{a} and {b} are no longer allies")),
            FactionCmd::Leader(f, l) => fx.set_leader(&f, l.as_deref()).map(|_| match l {
                Some(l) => format!("{l} leads {f}"),
                None => format!("{f} has no leader"),
            }),
            FactionCmd::Follow(f, target, formation, size) => {
                let names: Vec<String> = self.faction_bots(&f);
                let shape = formation.as_deref().and_then(Shape::parse).or_else(|| self.standing_formation(&f)).unwrap_or(Shape::Behind);
                let of = names.len();
                let mut i = 0;
                let n = self.each(&names, |l| {
                    l.bot.orders.follow = target.clone().map(|t| {
                        let f = brain::Follow { target: t, shape, size: size.unwrap_or(4) as usize, slot: i, of };
                        i += 1;
                        f
                    });
                });
                Ok(format!("{n} bots of {f} {}", if target.is_some() { "follow" } else { "stop following" }))
            }
            FactionCmd::Goto { faction, to, status, formation, size, then } => {
                let names = self.faction_bots(&faction);
                if status {
                    let arrived = names.iter().filter(|n| self.bots.get(&n.to_ascii_lowercase()).and_then(|l| l.bot.orders.march.as_ref()).is_some_and(|m| m.arrived)).count();
                    Ok(format!("{faction}: {arrived} of {} arrived", names.len()))
                } else if to.is_none() {
                    let n = self.each(&names, |l| l.bot.orders.march = None);
                    Ok(format!("{n} bots of {faction} stop marching"))
                } else {
                    let dest = match to.unwrap() {
                        Target::Entity(n) => brain::MarchTo::Player(n),
                        t => match self.resolve_target(who, &t) {
                            Some((_, p)) => brain::MarchTo::Point(p),
                            None => {
                                out.push(self.tell(who, "No such place"));
                                return;
                            }
                        },
                    };
                    let shape = formation.as_deref().and_then(Shape::parse).or_else(|| self.standing_formation(&faction)).unwrap_or(Shape::Rows);
                    let of = names.len();
                    let attack = match then {
                        Then::Attack(t) => Some(t),
                        Then::Hold => None,
                    };
                    let mut i = 0;
                    let n = self.each(&names, |l| {
                        l.bot.orders.march = Some(brain::March { to: dest.clone(), shape, size: size.unwrap_or(4) as usize, slot: i, of, then_attack: attack.clone(), arrived: false });
                        i += 1;
                    });
                    Ok(format!("{n} bots of {faction} march"))
                }
            }
            FactionCmd::Formation(f, shape) => match fx.factions.get_mut(&f) {
                Some(fac) => {
                    fac.formation = shape.clone();
                    Ok(format!("{f}: formation {}", shape.map_or("cleared".into(), |s| s.0)))
                }
                None => Err(self.messages.get("err.no-faction", &[&f])),
            },
            FactionCmd::Skin(f, acc) => match fx.factions.get_mut(&f) {
                Some(fac) => {
                    fac.skin = acc.clone();
                    Ok(format!("{f}: {}", acc.map_or("own skins (from the next join)".into(), |a| format!("in {a}'s skin (from the next join)"))))
                }
                None => Err(self.messages.get("err.no-faction", &[&f])),
            },
            FactionCmd::SkinList => Ok(fx
                .factions
                .iter()
                .filter_map(|(n, f)| f.skin.as_ref().map(|s| format!("{n}: {s}")))
                .collect::<Vec<_>>()
                .join(", ")),
            FactionCmd::KitSet(f, k, w) => {
                if !self.kits.kits.contains_key(&k) {
                    Err(self.messages.get("err.no-kit", &[&k]))
                } else {
                    fx.set_kit_weight(&f, &k, w).map(|_| format!("{f}: {k} weight {w}"))
                }
            }
            FactionCmd::KitList(f) => match fx.factions.get(&f) {
                Some(fac) => {
                    let total: u32 = fac.kits.iter().map(|(_, w)| w).sum();
                    Ok(fac.kits.iter().map(|(k, w)| format!("{k} {:.0}%", *w as f64 * 100.0 / total.max(1) as f64)).collect::<Vec<_>>().join(", "))
                }
                None => Err(self.messages.get("err.no-faction", &[&f])),
            },
            FactionCmd::KitClear(f) => match fx.factions.get_mut(&f) {
                Some(fac) => {
                    fac.kits.clear();
                    Ok(format!("{f}: kit table empty"))
                }
                None => Err(self.messages.get("err.no-faction", &[&f])),
            },
            FactionCmd::KitGive(f) => {
                let names = self.faction_bots(&f);
                let mut n = 0;
                for name in &names {
                    if let Some(k) = self.squad.factions.roll_kit(&f, &mut self.rng) {
                        if let Some(kit) = self.kits.kits.get(&k).cloned() {
                            let targets = kit.restock_targets();
                            out.push(Effect::GiveKit { to: name.clone(), kit });
                            if let Some(l) = self.bot_mut(name) {
                                l.kit = Some(k);
                                l.bot.restock_targets = Some(targets);
                            }
                            n += 1;
                        }
                    }
                }
                Ok(format!("{n} bots of {f} kitted"))
            }
            FactionCmd::Info(f) => match fx.factions.get(&f) {
                Some(fac) => Ok(format!(
                    "{f}: {} members ({} spawned), leader {}, allies [{}], enemies [{}], at peace with [{}]",
                    fac.members.len(),
                    self.bots.values().filter(|l| l.bot.faction.as_deref() == Some(f.as_str())).count(),
                    fac.leader.clone().unwrap_or_else(|| "none".into()),
                    fac.allies.join(", "),
                    fac.enemies.join(", "),
                    fac.peace.join(", ")
                )),
                None => Err(self.messages.get("err.no-faction", &[&f])),
            },
            FactionCmd::List => Ok(format!("Factions: {}", fx.factions.keys().cloned().collect::<Vec<_>>().join(", "))),
            FactionCmd::Set(f, k, v) => self.faction_set(&f, &k, &v).map(|_| format!("{f}: {k} = {v}")),
            FactionCmd::Settings(f, filter) => {
                let Some(fac) = fx.factions.get(&f) else {
                    out.push(self.tell(who, self.msg("err.no-faction", &[&f])));
                    return;
                };
                let own = fac.settings.clone();
                let snap = self.settings.snapshot(Some(&f), None);
                let lines: Vec<String> = snap
                    .all()
                    .filter(|(s, _)| filter.as_ref().is_none_or(|w| s.key.contains(w.as_str()) || s.group == w))
                    .map(|(s, v)| format!("{}{} = {}", s.key, if own.contains_key(s.key) { " (faction)" } else { "" }, v.render()))
                    .collect();
                if let Some(p) = who.player() {
                    out.push(Effect::OpenMenu { viewer: p.uuid, page: crate::menu::Page::Settings { scope: Scope::Faction(f.clone()), group: None, page: 0 } });
                }
                Ok(lines.join("  "))
            }
        };
        self.mark("factions.yml");
        self.refresh_settings();
        match r {
            Ok(m) | Err(m) => out.push(self.tell(who, m)),
        }
    }

    fn standing_formation(&self, f: &str) -> Option<Shape> {
        self.squad.factions.factions.get(f)?.formation.as_ref().and_then(|(s, _)| Shape::parse(s))
    }

    fn faction_bots(&self, f: &str) -> Vec<String> {
        self.bots.values().filter(|l| l.bot.faction.as_deref() == Some(f)).map(|l| l.bot.name.clone()).collect()
    }

    fn overfaction(&mut self, who: &Who, oc: OverfactionCmd, out: &mut Vec<Effect>) {
        let fx = &mut self.squad.factions;
        let r: Result<String, String> = match oc {
            OverfactionCmd::Create(n) => fx.of_create(&n).map(|_| format!("Overfaction {n} created")),
            OverfactionCmd::Disband(n) => fx.of_disband(&n).map(|_| format!("Overfaction {n} disbanded")),
            OverfactionCmd::Add(o, f) => fx.of_add(&o, &f).map(|_| format!("{f} joined {o}")),
            OverfactionCmd::Remove(f) => {
                if fx.of_remove(&f) {
                    Ok(format!("{f} left its coalition"))
                } else {
                    Err(format!("{f} is in no coalition"))
                }
            }
            OverfactionCmd::Hostile(a, b) => fx.of_relation(&a, &b, true).map(|_| format!("{a} and {b} are at war")),
            OverfactionCmd::Peace(a, b) => fx.of_relation(&a, &b, false).map(|_| format!("{a} and {b} are at peace")),
            OverfactionCmd::Leader(o, l) => match fx.overfactions.get_mut(&o) {
                Some(of) => {
                    of.leader = l.clone();
                    Ok(format!("{o}: leader {}", l.unwrap_or_else(|| "none".into())))
                }
                None => Err(self.messages.get("err.no-overfaction", &[&o])),
            },
            OverfactionCmd::Follow(o, target, formation) => {
                let names = self.select(&Selector::Overfaction(o.clone()), &[]);
                let shape = formation.as_deref().and_then(Shape::parse).unwrap_or(Shape::Behind);
                let of = names.len();
                let mut i = 0;
                let n = self.each(&names, |l| {
                    l.bot.orders.follow = target.clone().map(|t| {
                        let f = brain::Follow { target: t, shape, size: 4, slot: i, of };
                        i += 1;
                        f
                    });
                });
                Ok(format!("{n} bots of {o}"))
            }
            OverfactionCmd::Tp(o, to) => {
                self.tp(who, TpMode::Instant, Selector::Overfaction(o), to, 0.0, &[], out);
                return;
            }
            OverfactionCmd::Info(o) => match fx.overfactions.get(&o) {
                Some(of) => Ok(format!("{o}: factions [{}], enemies [{}], peace [{}], leader {}", of.factions.join(", "), of.enemies.join(", "), of.peace.join(", "), of.leader.clone().unwrap_or_else(|| "none".into()))),
                None => Err(self.messages.get("err.no-overfaction", &[&o])),
            },
            OverfactionCmd::List => Ok(format!("Overfactions: {}", fx.overfactions.keys().cloned().collect::<Vec<_>>().join(", "))),
        };
        self.mark("factions.yml");
        match r {
            Ok(m) | Err(m) => out.push(self.tell(who, m)),
        }
    }

    fn kit(&mut self, who: &Who, kc: KitCmd, latest: &[String], out: &mut Vec<Effect>) {
        match kc {
            KitCmd::Save(name, from) => {
                // The glue reads the inventory and calls `save_kit`.
                let source = from.unwrap_or_else(|| who.player().map(|p| p.name.clone()).unwrap_or_default());
                if source.is_empty() {
                    out.push(self.tell(who, "From the console name the bot to save"));
                } else {
                    out.push(Effect::ReadKit { name, from: source, reply: who.sender() });
                }
            }
            KitCmd::Give(k, sel) => {
                let Some(kit) = self.kits.kits.get(&k).cloned() else {
                    out.push(self.tell(who, self.msg("err.no-kit", &[&k])));
                    return;
                };
                let names = match &sel {
                    Selector::Name(n) if !self.bots.contains_key(&n.to_ascii_lowercase()) => vec![n.clone()],
                    s => self.select(s, latest),
                };
                for n in &names {
                    out.push(Effect::GiveKit { to: n.clone(), kit: kit.clone() });
                    if let Some(l) = self.bot_mut(n) {
                        l.kit = Some(k.clone());
                        l.bot.restock_targets = Some(kit.restock_targets());
                    }
                }
                self.mark("bots.yml");
                self.done(who, names.len(), out);
            }
            KitCmd::GiveFaction(k, sel) => {
                let Some(kit) = self.kits.kits.get(&k).cloned() else {
                    out.push(self.tell(who, self.msg("err.no-kit", &[&k])));
                    return;
                };
                let names = self.select(&sel, latest);
                for n in &names {
                    out.push(Effect::GiveKit { to: n.clone(), kit: kit.clone() });
                    if let Some(l) = self.bot_mut(n) {
                        l.kit = Some(k.clone());
                        l.bot.restock_targets = Some(kit.restock_targets());
                    }
                }
                self.done(who, names.len(), out);
            }
            KitCmd::Edit(k) => match who.player() {
                Some(p) => {
                    self.kits.kits.entry(k.clone()).or_default();
                    self.kit_edits.insert(p.uuid, (k.clone(), false));
                    out.push(Effect::OpenKitEditor { viewer: p.uuid, kit: k });
                }
                None => out.push(self.tell(who, "The kit editor is for players")),
            },
            KitCmd::Rename(a, b) => match self.kits.rename(&a, &b) {
                Ok(()) => {
                    self.squad.factions.rename_kit(&a, &b);
                    self.mark("factions.yml");
                    out.push(self.tell(who, format!("{a} is now {b}")));
                }
                Err(e) => out.push(self.tell(who, e)),
            },
            KitCmd::Delete(k) => {
                if self.kits.kits.remove(&k).is_some() {
                    out.push(self.tell(who, format!("Deleted kit {k}")));
                } else {
                    out.push(self.tell(who, self.msg("err.no-kit", &[&k])));
                }
            }
            KitCmd::List => out.push(self.tell(who, format!("Kits: {}", self.kits.kits.keys().cloned().collect::<Vec<_>>().join(", ")))),
        }
        self.mark("kits.yml");
    }

    /// `kit save` / the kit editor: the glue hands the inventory read from a player or a bot.
    pub fn save_kit(&mut self, name: &str, kit: Kit) -> Vec<Effect> {
        self.kits.kits.insert(name.to_owned(), kit);
        self.mark("kits.yml");
        self.saves()
    }

    fn preset(&mut self, who: &Who, pc: PresetCmd, out: &mut Vec<Effect>) {
        let r = match pc {
            PresetCmd::Show => Ok(format!(
                "Active: {}. Presets: {}",
                self.settings.active_preset.clone().unwrap_or_else(|| "none".into()),
                self.settings.presets.keys().cloned().collect::<Vec<_>>().join(", ")
            )),
            PresetCmd::Apply(n) => self.settings.apply_preset(&n).map(|_| format!("Preset {n}")).map_err(|_| self.msg("err.no-preset", &[&n])),
            PresetCmd::Off => self.settings.apply_preset("off").map(|_| "Preset off".to_owned()),
            PresetCmd::Save(n) => {
                self.settings.save_preset(&n);
                Ok(format!("Saved preset {n}"))
            }
            PresetCmd::Delete(n) => {
                if self.settings.delete_preset(&n) {
                    Ok(format!("Deleted preset {n}"))
                } else {
                    Err(self.msg("err.no-preset", &[&n]))
                }
            }
        };
        self.config.presets = self.settings.presets.clone();
        self.config.active_preset = self.settings.active_preset.clone();
        if let Ok(t) = crate::config::write_presets(&self.config_text, &self.settings.presets, self.settings.active_preset.as_deref()) {
            self.config_text = t.clone();
            out.push(Effect::SaveConfig(t));
        }
        self.refresh_settings();
        match r {
            Ok(m) | Err(m) => out.push(self.tell(who, m)),
        }
    }

    fn script(&mut self, who: &Who, sc: ScriptCmd, out: &mut Vec<Effect>) {
        let r: Result<String, String> = match sc {
            ScriptCmd::Create(n) => self.scripts.create(&n).map(|_| format!("Script {n} created")),
            ScriptCmd::Add(n, l) => self.scripts.add(&n, &l).map(|i| format!("Line {i} added")),
            ScriptCmd::Show(n) => match self.scripts.scripts.get(&n) {
                Some(lines) => Ok(lines.iter().enumerate().map(|(i, l)| format!("{}: {l}", i + 1)).collect::<Vec<_>>().join(" | ")),
                None => Err(self.msg("err.no-script", &[&n])),
            },
            ScriptCmd::Remove(n, i) => self.scripts.remove_line(&n, i).map(|l| format!("Removed: {l}")),
            ScriptCmd::Delete(n) => {
                if self.scripts.scripts.remove(&n).is_some() {
                    Ok(format!("Deleted script {n}"))
                } else {
                    Err(self.msg("err.no-script", &[&n]))
                }
            }
            ScriptCmd::Run(n, args) => self.runner.start(&self.scripts, &n, &args, who.sender()).map(|now| {
                if now { format!("Running {n}") } else { format!("{n} starts when the running script ends") }
            }),
            ScriptCmd::List => Ok(format!("Scripts: {}", self.scripts.scripts.keys().cloned().collect::<Vec<_>>().join(", "))),
        };
        self.mark("scripts.yml");
        match r {
            Ok(m) | Err(m) => out.push(self.tell(who, m)),
        }
    }

    fn owner(&mut self, who: &Who, oc: OwnerCmd, out: &mut Vec<Effect>) {
        match oc {
            OwnerCmd::Set(bot, player) => {
                if !self.players.iter().any(|p| p.name.eq_ignore_ascii_case(&player)) {
                    out.push(self.tell(who, self.msg("err.no-player", &[&player])));
                    return;
                }
                if self.need_bot(who, &bot, out) {
                    self.bot_mut(&bot).unwrap().bot.orders.owner = Some(player.clone());
                    self.mark("bots.yml");
                    out.push(self.tell(who, format!("{player} owns {bot}")));
                }
            }
            OwnerCmd::Clear(bot) => {
                if self.need_bot(who, &bot, out) {
                    self.bot_mut(&bot).unwrap().bot.orders.owner = None;
                    self.mark("bots.yml");
                    out.push(self.tell(who, format!("{bot} has no owner")));
                }
            }
            OwnerCmd::List(player) => {
                let owned: Vec<String> = self.owned_by(&player);
                out.push(self.tell(who, format!("{player}: {}", owned.join(", "))));
            }
            OwnerCmd::Help => out.push(self.tell(who, self.msg("chat.help", &[]))),
        }
    }

    fn owned_by(&self, player: &str) -> Vec<String> {
        self.bots
            .values()
            .filter(|l| l.bot.orders.owner.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(player)))
            .map(|l| l.bot.name.clone())
            .collect()
    }

    fn diagnose(&mut self, who: &Who, name: &str, reset: bool, out: &mut Vec<Effect>) {
        if let Some(l) = self.bots.get(&name.to_ascii_lowercase()) {
            let mut lines = vec![format!("{}: joined {}, paused {}, stopped {}", l.bot.name, l.joined, l.bot.orders.paused, l.bot.orders.stopped)];
            let s = &l.bot.settings;
            if !s.b("auto-target") {
                lines.push("auto-target is off: it only fights when ordered or hit".into());
            }
            if !s.b("target-players") {
                lines.push("target-players is off: players are ignored unless they start it".into());
            }
            if l.bot.faction.is_some() {
                lines.push(format!("faction {}: check its relations with /horizonbot faction info", l.bot.faction.as_deref().unwrap_or("none")));
            }
            let cancelled = self.cancelled.get(&name.to_ascii_lowercase()).copied().unwrap_or(0);
            if cancelled > 0 {
                lines.push(format!("{cancelled} hits on or by it were cancelled by another mod (protection?); bypass-damage-protection overrides that"));
            } else {
                lines.push("No hit on or by it was cancelled by another mod".into());
            }
            for line in lines {
                out.push(self.tell(who, line));
            }
            return;
        }
        if let Some(p) = self.players.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
            let near = self.bots.values().filter(|l| l.bot.body.pos.distance(p.pos) < 32.0).count();
            let mode = match p.game_mode {
                1 => "creative (bots leave you alone unless attack-invincible)",
                3 => "spectator",
                _ => "survival",
            };
            let fac = self.squad.factions.faction_of(&p.name).map(str::to_owned);
            out.push(self.tell(who, format!("{}: {mode}, faction {}, {near} bots within 32 blocks", p.name, fac.as_deref().unwrap_or("none"))));
            for l in self.bots.values().filter(|l| l.bot.body.pos.distance(p.pos) < 32.0) {
                let rel = self.squad.relation(l.bot.faction.as_deref(), fac.as_deref());
                out.push(self.tell(who, format!("  {}: relation {rel:?}, target-players {}, last call \"{}\"", l.bot.name, l.bot.settings.b("target-players"), l.bot.last_call)));
            }
            if reset {
                self.cancelled.clear();
                out.push(self.tell(who, "Hit records reset"));
            }
            return;
        }
        out.push(self.tell(who, self.msg("err.no-bot", &[name])));
    }

    fn arena_cmd(&mut self, who: &Who, ac: ArenaCmd, out: &mut Vec<Effect>) {
        match ac {
            ArenaCmd::Start { kit_a, kit_b, rounds, skill_a, skill_b } => {
                for k in [&kit_a, &kit_b] {
                    if !self.kits.kits.contains_key(k) {
                        out.push(self.tell(who, self.msg("err.no-kit", &[k])));
                        return;
                    }
                }
                let (world, pos, _) = self.origin(who);
                let center = pos.unwrap_or(Vec3::new(0.5, 64.0, 0.5));
                let arena = Arena::new(&world, center, &kit_a, &kit_b, rounds, skill_a, skill_b);
                let (a, b) = arena.starts();
                for (n, at) in [(arena.bot_a.clone(), a), (arena.bot_b.clone(), b)] {
                    if !self.bots.contains_key(&n.to_ascii_lowercase()) {
                        let skin = self.skin_for(&n, None, true);
                        self.add_live(&n, &world, at, 0.0, None, skin, out);
                    }
                }
                for (n, s) in [(&arena.bot_a, skill_a), (&arena.bot_b, skill_b)] {
                    if let Some(s) = s {
                        let _ = self.settings.set_bot(n, "skill", &s.to_string());
                    }
                }
                self.refresh_settings();
                self.arena = Some(arena);
                out.push(self.tell(who, format!("Arena: {rounds} rounds, {kit_a} vs {kit_b}")));
            }
            ArenaCmd::Stop => {
                if let Some(a) = self.arena.take() {
                    for line in a.report() {
                        out.push(self.tell(who, line));
                    }
                    for n in [a.bot_a, a.bot_b] {
                        if let Some(l) = self.bots.remove(&n.to_ascii_lowercase()) {
                            out.push(Effect::RemoveBot { name: l.bot.name.clone(), uuid: l.bot.uuid });
                        }
                    }
                }
            }
            ArenaCmd::Report => match &self.arena {
                Some(a) => {
                    for line in a.report() {
                        out.push(self.tell(who, line));
                    }
                }
                None => out.push(self.tell(who, "No arena running")),
            },
        }
    }

    fn ghost(&mut self, who: &Who, g: GhostCmd, out: &mut Vec<Effect>) {
        match g {
            GhostCmd::Record(name) => match who.player() {
                Some(p) => {
                    self.recorders.insert(p.uuid, Recorder { name: name.clone(), rec: Recording { world: p.world.clone(), ..Default::default() } });
                    out.push(self.tell(who, format!("Recording {name}: /ghost stoprecord to finish")));
                }
                None => out.push(self.tell(who, "Only a player can be recorded")),
            },
            GhostCmd::StopRecord => {
                let Some(p) = who.player() else { return };
                match self.recorders.remove(&p.uuid) {
                    Some(r) => {
                        let undo = r.undo();
                        if !undo.is_empty() {
                            out.push(Effect::SetBlocks { world: r.rec.world.clone(), blocks: undo });
                        }
                        out.push(Effect::Save { file: format!("ghost-recordings/{}.txt", r.name), text: r.rec.to_text() });
                        out.push(self.tell(who, format!("Saved {} ({} ticks)", r.name, r.rec.frames.len())));
                        self.recordings.insert(r.name, r.rec);
                    }
                    None => out.push(self.tell(who, "You are not recording")),
                }
            }
            GhostCmd::Spawn { name, recording, skin, lines } => {
                let Some(rec) = self.recordings.get(&recording) else {
                    out.push(self.tell(who, format!("No recording called {recording}")));
                    return;
                };
                let (world, start) = (rec.world.clone(), rec.frames.first().map_or(Vec3::ZERO, |f| f.pos));
                let source = self.ghost_skin(&name, &skin);
                out.push(Effect::SpawnGhost { name: name.clone(), uuid: crate::uuid::offline(&format!("ghost:{name}")), skin: source, world, pos: start });
                self.ghosts.insert(name.clone(), Ghost { name: name.clone(), kind: GhostKind::Replay { recording, frame: 0 }, lines, next_line: 0, skin_from: skin_name(&skin) });
                out.push(self.tell(who, format!("Ghost {name} walks")));
            }
            GhostCmd::Stare { name, skin, lines } => {
                let Some(p) = who.player() else {
                    out.push(self.tell(who, "A staring ghost appears where a player stands"));
                    return;
                };
                let source = self.ghost_skin(&name, &skin);
                out.push(Effect::SpawnGhost { name: name.clone(), uuid: crate::uuid::offline(&format!("ghost:{name}")), skin: source, world: p.world.clone(), pos: p.pos });
                self.ghosts.insert(name.clone(), Ghost { name: name.clone(), kind: GhostKind::Stare { pos: p.pos, spawned: self.tick }, lines, next_line: 0, skin_from: skin_name(&skin) });
            }
            GhostCmd::Remove(name) => {
                if self.ghosts.remove(&name).is_some() {
                    out.push(Effect::RemoveGhost { name: name.clone() });
                    out.push(self.tell(who, format!("Removed ghost {name}")));
                } else {
                    out.push(self.tell(who, format!("No ghost called {name}")));
                }
            }
            GhostCmd::List => out.push(self.tell(who, format!("Ghosts: {}", self.ghosts.keys().cloned().collect::<Vec<_>>().join(", ")))),
            GhostCmd::Recordings => out.push(self.tell(who, format!("Recordings: {}", self.recordings.keys().cloned().collect::<Vec<_>>().join(", ")))),
        }
    }

    fn ghost_skin(&mut self, name: &str, skin: &SkinArg) -> SkinSource {
        match skin {
            SkinArg::None => SkinSource::None,
            SkinArg::From(a) => self.skins.get(a).cloned().map_or(SkinSource::Fetch(a.clone()), SkinSource::Known),
            SkinArg::Default => self.skins.get(name).cloned().map_or(SkinSource::Fetch(name.to_owned()), SkinSource::Known),
        }
    }

    // ---------------------------------------------------------------- menus

    /// Builds a page for a viewer (and remembers what its slots do).
    pub fn menu(&mut self, viewer: u128, page: crate::menu::Page) -> crate::menu::Menu {
        let settings = &self.settings;
        let factions = &self.squad.factions;
        let snap = |s: &Scope| match s {
            Scope::Global => settings.snapshot(None, None),
            Scope::Bot(b) => settings.snapshot(factions.faction_of(b), Some(b)),
            Scope::Faction(f) => settings.snapshot(Some(f), None),
        };
        let view = crate::menu::View {
            snapshot: &snap,
            bots: self.bots.values().filter(|l| !l.bot.ghost).map(|l| (l.bot.name.clone(), l.bot.orders.paused, l.bot.faction.clone())).collect(),
            factions: self.squad.factions.factions.iter().map(|(n, f)| (n.clone(), f.members.len())).collect(),
            kits: self.kits.kits.keys().cloned().collect(),
            presets: self.settings.presets.keys().cloned().collect(),
            active_preset: self.settings.active_preset.clone(),
        };
        let m = crate::menu::build(&page, &view);
        self.menus.insert(viewer, (page, m.clicks.clone()));
        m
    }

    /// A click in a viewer's menu: what to do, and the menu to show after it.
    pub fn menu_click(&mut self, who: &Who, slot: usize, right: bool) -> (Vec<Effect>, Option<crate::menu::Menu>) {
        let Some(p) = who.player().cloned() else { return (Vec::new(), None) };
        let Some((page, clicks)) = self.menus.get(&p.uuid).cloned() else { return (Vec::new(), None) };
        let mut out = Vec::new();
        match clicks.get(slot).cloned().unwrap_or(crate::menu::Click::Nothing) {
            crate::menu::Click::Nothing => return (out, None),
            crate::menu::Click::Open(next) => {
                let m = self.menu(p.uuid, next);
                return (out, Some(m));
            }
            crate::menu::Click::Command(line) => {
                out.extend(self.command(who, &line.replace("{viewer}", &p.name)));
            }
            crate::menu::Click::Setting(scope, key, _) => {
                if !who.is_admin(self.admin_level) {
                    out.push(self.tell(who, self.msg("err.no-permission", &[])));
                    return (out, None);
                }
                let spec = settings::spec(key).unwrap();
                let snap = match &scope {
                    Scope::Global => self.settings.snapshot(None, None),
                    Scope::Bot(b) => self.snapshot_for(b),
                    Scope::Faction(f) => self.settings.snapshot(Some(f), None),
                };
                let v = crate::menu::stepped(spec, &snap, !right);
                let r = match &scope {
                    Scope::Global => self.settings.set_global(key, &v).map(|_| ()),
                    Scope::Bot(b) => self.settings.set_bot(b, key, &v),
                    Scope::Faction(f) => self.faction_set(f, key, &v),
                };
                match r {
                    Ok(()) => self.after_scope_change(&scope, key, &mut out),
                    Err(e) => out.push(self.tell(who, e)),
                }
                out.extend(self.saves());
            }
        }
        let m = self.menu(p.uuid, page);
        (out, Some(m))
    }

    pub fn menu_closed(&mut self, viewer: u128) {
        self.menus.remove(&viewer);
        self.kit_edits.remove(&viewer);
    }

    /// The kit editor's model: the kit's slots in the menu's order (bag, hotbar, armour feet first,
    /// off hand) or its ender chest, 45 at most.
    pub fn kit_editor_slots(&self, viewer: u128) -> Option<(String, bool, Vec<Option<crate::kits::KitItem>>)> {
        let (name, ender) = self.kit_edits.get(&viewer)?.clone();
        let kit = self.kits.kits.get(&name).cloned().unwrap_or_default();
        let slots = if ender {
            let mut e = kit.enderchest.clone();
            e.resize(27, None);
            e
        } else {
            let st = kit.storage();
            let mut v: Vec<Option<crate::kits::KitItem>> = st[9..36].to_vec();
            v.extend(st[0..9].iter().cloned());
            v.extend(kit.armour_slots());
            v.push(kit.offhand_item());
            v
        };
        Some((name, ender, slots))
    }

    /// A click in the kit editor: `kit_slot` (menu order) taken out, or `add` put into the first
    /// free slot (armour into its own, a shield or totem into the off hand when free).
    pub fn kit_editor_change(&mut self, viewer: u128, kit_slot: Option<usize>, add: Option<crate::kits::KitItem>, toggle_page: bool, clear: bool) -> Vec<Effect> {
        let Some((name, ender)) = self.kit_edits.get(&viewer).cloned() else { return Vec::new() };
        if toggle_page {
            self.kit_edits.insert(viewer, (name, !ender));
            return Vec::new();
        }
        let kit = self.kits.kits.entry(name.clone()).or_default();
        if clear {
            *kit = Kit::default();
        } else if ender {
            kit.enderchest.resize(27, None);
            if let Some(i) = kit_slot.filter(|i| *i < 27) {
                kit.enderchest[i] = None;
            }
            if let Some(it) = add {
                if let Some(free) = kit.enderchest.iter().position(Option::is_none) {
                    kit.enderchest[free] = Some(it);
                }
            }
        } else {
            // Normalise to Paper's layout: 41 contents, armour and off hand set apart too.
            let mut st = kit.storage();
            let mut armour = kit.armour_slots().to_vec();
            let mut off = kit.offhand_item();
            if let Some(i) = kit_slot {
                match i {
                    0..=26 => st[i + 9] = None,
                    27..=35 => st[i - 27] = None,
                    36..=39 => armour[i - 36] = None,
                    40 => off = None,
                    _ => {}
                }
            }
            if let Some(it) = add {
                let armour_slot = crate::items::armor(&it.id).map(|(s, _, _)| s as usize);
                if let Some(a) = armour_slot.filter(|a| armour[*a].is_none()) {
                    armour[a] = Some(it);
                } else if off.is_none() && (it.id.ends_with("shield") || it.id.ends_with("totem_of_undying")) {
                    off = Some(it);
                } else if let Some(free) = st.iter().position(Option::is_none) {
                    st[free] = Some(it);
                }
            }
            let mut contents = st.clone();
            contents.extend(armour.iter().cloned());
            contents.push(off.clone());
            kit.contents = contents;
            kit.armour = armour;
            kit.offhand = off;
        }
        self.mark("kits.yml");
        self.saves()
    }

    // ---------------------------------------------------------------- events

    pub fn player_joined(&mut self, p: PlayerInfo) -> Vec<Effect> {
        self.squad.factions.player_joined(&p.name);
        self.mark("factions.yml");
        self.players.retain(|q| q.uuid != p.uuid);
        self.players.push(p);
        self.saves()
    }

    /// A real player left: their bots are parked above the holding world's spawn and paused.
    pub fn player_left(&mut self, uuid: u128, name: &str) -> Vec<Effect> {
        self.players.retain(|q| q.uuid != uuid);
        self.recorders.remove(&uuid);
        let mut out = Vec::new();
        let world = self.config.holding_world.clone();
        for b in self.owned_by(name) {
            if let Some(l) = self.bot_mut(&b) {
                l.parked = true;
                l.bot.orders.paused = true;
            }
            out.push(Effect::Teleport { name: b.clone(), world: Some(world.clone()), pos: Vec3::new(0.5, 300.0, 0.5) });
        }
        out
    }

    /// The server placed a bot: it has an entity id now.
    pub fn bot_joined(&mut self, name: &str, entity_id: i32, pos: Vec3) {
        if let Some(l) = self.bot_mut(name) {
            l.joined = true;
            l.bot.entity_id = entity_id;
            l.bot.body.pos = pos;
        }
    }

    pub fn is_bot(&self, name: &str) -> bool {
        self.bots.contains_key(&name.to_ascii_lowercase())
    }

    fn name_of_entity(&self, id: i32) -> Option<(u128, String, bool)> {
        if id == 0 {
            return None;
        }
        if let Some(l) = self.bots.values().find(|l| l.bot.entity_id == id) {
            return Some((l.bot.uuid, l.bot.name.clone(), true));
        }
        self.players.iter().find(|p| p.entity_id == id).map(|p| (p.uuid, p.name.clone(), false))
    }

    /// Damage between entities (stats, leader calls).
    pub fn damaged(&mut self, victim: i32, attacker: i32, amount: f64) {
        let (Some(v), Some(a)) = (self.name_of_entity(victim), self.name_of_entity(attacker)) else { return };
        if !(v.2 || a.2) {
            return;
        }
        self.stats.damage((a.0, &a.1, a.2), (v.0, &v.1, v.2), amount);
        // The arena's per-side counts.
        if let Some(ar) = self.arena.as_mut() {
            let (aa, ab) = (ar.bot_a.eq_ignore_ascii_case(&a.1), ar.bot_b.eq_ignore_ascii_case(&a.1));
            if aa || ab {
                let fell = self.bots.get(&a.1.to_ascii_lowercase()).is_some_and(|l| l.bot.body.fall_distance > 1.5);
                let mace = self.bots.get(&a.1.to_ascii_lowercase()).is_some_and(|l| l.bot.fight.mace_mode);
                let (hitter, hit) = if aa { (&mut ar.a, &mut ar.b) } else { (&mut ar.b, &mut ar.a) };
                hitter.hit(amount, fell && !mace, fell && mace);
                hitter.swings += 1;
                hit.got_hit();
            }
        }
        if a.2 {
            self.stats.swing(a.0, &a.1, true, true);
        }
        // The leader's fights are the faction's.
        let secs = self.settings.snapshot(None, None).i("leader-call-seconds").max(0) as u32;
        for (leader, other) in [(&a, &v), (&v, &a)] {
            if let Some(f) = self.squad.factions.faction_of(&leader.1).map(str::to_owned) {
                if self.squad.factions.leader_of(&f).is_some_and(|l| l.eq_ignore_ascii_case(&leader.1)) {
                    let of = self.squad.factions.faction_of(&other.1).map(str::to_owned);
                    self.squad.factions.leader_call(&f, &other.1, of.as_deref(), secs);
                }
            }
        }
    }

    /// Another mod cancelled a hit involving a bot (counted for `/horizonbot diagnose`).
    pub fn cancelled_hit(&mut self, victim: i32, attacker: i32) {
        for id in [victim, attacker] {
            if let Some((_, name, true)) = self.name_of_entity(id) {
                *self.cancelled.entry(name.to_ascii_lowercase()).or_default() += 1;
            }
        }
    }

    /// A swing that hit nobody (stats).
    pub fn whiffed(&mut self, attacker: i32) {
        if let Some(a) = self.name_of_entity(attacker).filter(|a| a.2) {
            self.stats.swing(a.0, &a.1, true, false);
        }
    }

    /// Someone died. A dead bot leaves (its items drop as a player's do).
    pub fn died(&mut self, victim: i32, killer: i32) -> Vec<Effect> {
        let mut out = Vec::new();
        let (v, k) = (self.name_of_entity(victim), self.name_of_entity(killer));
        if let Some(v) = &v {
            if v.2 || k.as_ref().is_some_and(|k| k.2) {
                self.stats.kill(k.as_ref().map(|k| (k.0, k.1.as_str(), k.2)), (v.0, &v.1, v.2));
            }
            if v.2 {
                if let Some(a) = self.arena.as_mut() {
                    if a.bot_a.eq_ignore_ascii_case(&v.1) || a.bot_b.eq_ignore_ascii_case(&v.1) {
                        let a_died = a.bot_a.eq_ignore_ascii_case(&v.1);
                        a.died(self.tick, a_died);
                        // Arena bots respawn for the next round.
                        return out;
                    }
                }
                if let Some(l) = self.bots.remove(&v.1.to_ascii_lowercase()) {
                    out.push(Effect::RemoveBot { name: l.bot.name.clone(), uuid: l.bot.uuid });
                    self.mark("bots.yml");
                }
            }
        }
        out
    }

    /// A chat line from a real player: a companion order is cancelled for everyone and answered.
    pub fn chat(&mut self, uuid: u128, name: &str, message: &str) -> (bool, Vec<String>, Vec<Effect>) {
        let own = self.owned_by(name);
        let Some(order) = crate::companion::parse(message, &own) else {
            return (false, Vec::new(), Vec::new());
        };
        let mut replies = Vec::new();
        let mut out = Vec::new();
        let targets: Vec<String> = if order.only.is_empty() { own.clone() } else { order.only.clone() };
        let me = self.players.iter().find(|p| p.uuid == uuid).cloned();
        use crate::companion::Order;
        for b in &targets {
            let Some(l) = self.bots.get_mut(&b.to_ascii_lowercase()) else {
                replies.push(self.messages.get("chat.not-here-one", &[b]));
                continue;
            };
            let o = &mut l.bot.orders;
            match &order.order {
                Order::Follow => {
                    o.follow = Some(brain::Follow { target: name.to_owned(), shape: Shape::Behind, size: 3, slot: 0, of: 1 });
                    o.guard = None;
                    o.paused = false;
                    o.stopped = false;
                    replies.push(self.messages.get("chat.following", &[b]));
                }
                Order::Guard => {
                    if let Some(p) = &me {
                        o.guard = Some(brain::Guard { post: p.pos, radius: crate::companion::GUARD_RADIUS, sector: 0.0 });
                        o.follow = None;
                        o.paused = false;
                        replies.push(self.messages.get("chat.guarding", &[b]));
                    }
                }
                Order::Stay => {
                    o.paused = true;
                    replies.push(self.messages.get("chat.staying", &[b]));
                }
                Order::Stop => {
                    o.stopped = true;
                    o.attack = None;
                    o.follow = None;
                    o.guard = None;
                    l.bot.revenge = None;
                    replies.push(self.messages.get("chat.stopped", &[b]));
                }
                Order::Free => {
                    let owner = o.owner.clone();
                    *o = brain::Orders { owner, ..Default::default() };
                    replies.push(self.messages.get("chat.free", &[b]));
                }
                Order::Attack(t) => {
                    o.attack = Some(t.clone());
                    o.stopped = false;
                    o.paused = false;
                    replies.push(self.messages.get("chat.hunting", &[b, t]));
                }
                Order::Cancel => {
                    let t = o.attack.take().unwrap_or_default();
                    l.bot.revenge = None;
                    l.bot.fight = Default::default();
                    replies.push(self.messages.get("chat.cancelled", &[b, &t]));
                }
                Order::Summon => {
                    let ready = self.summon_ready.get(&uuid).copied().unwrap_or(0);
                    if self.tick < ready {
                        replies.push(self.messages.get("chat.summon-wait", &[&((ready - self.tick) / 20 + 1).to_string()]));
                        break;
                    }
                    if let Some(p) = &me {
                        l.parked = false;
                        o.paused = false;
                        o.attack = None;
                        o.retreat = None;
                        o.march = None;
                        out.push(Effect::Teleport { name: b.clone(), world: Some(p.world.clone()), pos: p.pos });
                        replies.push(self.messages.get("chat.summoned", &[b]));
                    }
                }
                Order::Status => {
                    let owner = name.to_owned();
                    let state = if l.parked {
                        self.messages.get("chat.state-away", &[])
                    } else if o.paused {
                        self.messages.get("chat.state-paused", &[])
                    } else if let Some(t) = l.bot.fight.target {
                        let _ = t;
                        self.messages.get("chat.state-fighting", &[&l.bot.fight.target_name])
                    } else if o.follow.is_some() {
                        self.messages.get("chat.state-following", &[&owner])
                    } else if o.guard.is_some() {
                        self.messages.get("chat.state-guarding", &[])
                    } else if o.march.is_some() {
                        self.messages.get("chat.state-marching", &[&owner])
                    } else {
                        self.messages.get("chat.state-free", &[])
                    };
                    replies.push(format!("{b}: {state}"));
                }
                Order::Help => {
                    replies.push(self.messages.get("chat.help", &[]));
                    break;
                }
            }
        }
        if order.order == Order::Summon {
            self.summon_ready.insert(uuid, self.tick + crate::companion::SUMMON_COOLDOWN_TICKS);
        }
        self.mark("bots.yml");
        (true, replies.into_iter().map(|r| crate::messages::colours(&r)).collect(), out)
    }

    /// A ghost recorder's movement and block changes.
    pub fn record(&mut self, uuid: u128, pos: Vec3, yaw: f32, pitch: f32, sneaking: bool) {
        if let Some(r) = self.recorders.get_mut(&uuid) {
            r.frame(pos, yaw, pitch, sneaking);
        }
    }
    pub fn record_block(&mut self, uuid: u128, pos: [i32; 3], before: &str, after: &str) {
        if let Some(r) = self.recorders.get_mut(&uuid) {
            r.block(pos, before, after);
        }
    }

    // ---------------------------------------------------------------- the tick

    /// Once per server tick, before the bots think: players' positions, scripts, ghosts, arena,
    /// patrols, morale.
    pub fn begin_tick(&mut self, tick: u64, players: Vec<PlayerInfo>) -> Vec<Effect> {
        self.tick = tick;
        self.players = players;
        self.squad.begin_tick(tick);
        let mut out = Vec::new();
        if tick == 1 {
            out.extend(self.start());
        }
        // Scripts: their lines run as whoever started them.
        for step in self.runner.tick() {
            let who = match step.sender {
                Sender::Console => Who::Console,
                Sender::Player(u) => match self.players.iter().find(|p| p.uuid == u) {
                    Some(p) => Who::Player(p.clone()),
                    None => continue,
                },
            };
            let mut words = step.line.split_whitespace();
            match words.next() {
                Some("horizonbot" | "hbot" | "bot") => {
                    let rest: String = words.collect::<Vec<_>>().join(" ");
                    let latest = if step.latest.is_empty() { self.latest.clone() } else { step.latest.clone() };
                    out.extend(self.command_with_latest(&who, &rest, &latest));
                }
                _ => match step.sender {
                    Sender::Console => out.push(Effect::Console(step.line.clone())),
                    Sender::Player(u) => out.push(Effect::AsPlayer { uuid: u, line: step.line.clone() }),
                },
            }
        }
        // Ghosts.
        let player_pos: Vec<Vec3> = self.players.iter().map(|p| p.pos).collect();
        let mut vanished = Vec::new();
        for g in self.ghosts.values_mut() {
            let t = g.tick(tick, &self.recordings, &player_pos);
            if let Some(pos) = t.pos {
                out.push(Effect::MoveGhost { name: g.name.clone(), pos, yaw: t.yaw, pitch: t.pitch, sneaking: t.sneaking });
            }
            if !t.blocks.is_empty() {
                let world = match &g.kind {
                    GhostKind::Replay { recording, .. } => self.recordings.get(recording).map(|r| r.world.clone()).unwrap_or_default(),
                    GhostKind::Stare { .. } => String::new(),
                };
                out.push(Effect::SetBlocks { world, blocks: t.blocks });
            }
            if let Some(line) = t.say {
                out.push(Effect::GhostSay { name: g.name.clone(), text: line });
            }
            if t.vanish {
                vanished.push(g.name.clone());
            }
        }
        for v in vanished {
            self.ghosts.remove(&v);
            out.push(Effect::RemoveGhost { name: v });
        }
        // The arena.
        if let Some(step) = self.arena.as_mut().and_then(|a| a.tick(tick)) {
            let a = self.arena.clone().unwrap();
            match step {
                ArenaStep::StartRound => {
                    let (pa, pb) = a.starts();
                    for (n, at, kit, other) in [(&a.bot_a, pa, &a.a.kit, &a.bot_b), (&a.bot_b, pb, &a.b.kit, &a.bot_a)] {
                        out.push(Effect::Teleport { name: n.clone(), world: Some(a.world.clone()), pos: at });
                        if let Some(k) = self.kits.kits.get(kit).cloned() {
                            out.push(Effect::GiveKit { to: n.clone(), kit: k });
                        }
                        if let Some(l) = self.bot_mut(n) {
                            l.bot.orders.attack = Some(other.clone());
                        }
                    }
                }
                ArenaStep::Report(lines) => {
                    for l in lines {
                        out.push(Effect::Broadcast(l));
                    }
                }
            }
        }
        // Patrols: the current point, the waypoint commands on arrival.
        let mut cmds = Vec::new();
        for l in self.bots.values_mut() {
            let Some(patrol) = l.bot.orders.patrol.clone() else { continue };
            let Some(path) = self.paths.paths.get(&patrol.path) else { continue };
            if path.waypoints.is_empty() {
                continue;
            }
            if l.bot.patrol_arrived {
                l.bot.patrol_arrived = false;
                let w = &path.waypoints[patrol.index.min(path.waypoints.len() - 1)];
                for c in &w.commands {
                    cmds.push(crate::paths::fill(c, &l.bot.name, &patrol.path, patrol.index, w));
                }
                let mut p = patrol;
                p.advance(path.waypoints.len());
                l.bot.orders.patrol = Some(p);
            }
            let i = l.bot.orders.patrol.as_ref().map_or(0, |p| p.index).min(path.waypoints.len() - 1);
            l.bot.orders.patrol_point = Some(path.waypoints[i].pos);
        }
        out.extend(cmds.into_iter().map(Effect::Console));
        // Morale and focus per faction.
        let mut morale: BTreeMap<String, (u32, u32, u32)> = BTreeMap::new();
        let mut focus: BTreeMap<String, BTreeMap<i32, u32>> = BTreeMap::new();
        for l in self.bots.values() {
            if let Some(f) = &l.bot.faction {
                let e = morale.entry(f.clone()).or_default();
                e.1 += 1;
                if let Some(t) = l.bot.fight.target {
                    *focus.entry(f.clone()).or_default().entry(t).or_default() += 1;
                    e.2 = e.2.max(1);
                }
            }
        }
        for (f, (_, now, fighting)) in morale.iter_mut() {
            let prev = self.squad.morale.get(f).copied();
            let start = match prev {
                Some((s, _, en)) if en > 0 && *fighting > 0 => s,
                _ => *now,
            };
            // Enemies near: the other side's bots in a fight with this faction.
            let enemies = if *fighting > 0 {
                self.bots
                    .values()
                    .filter(|o| o.bot.fight.target.is_some() && o.bot.faction.as_deref() != Some(f.as_str()) && self.squad.relation(Some(f), o.bot.faction.as_deref()) == Relation::War)
                    .count() as u32
            } else {
                0
            };
            self.squad.morale.insert(f.clone(), (start, *now, enemies));
        }
        self.squad.focus = focus
            .into_iter()
            .filter_map(|(f, m)| m.into_iter().max_by_key(|(_, n)| *n).map(|(id, _)| (f, id)))
            .collect();
        // Regears that finished their time.
        let due: Vec<String> = self.pending_restock.iter().filter(|(_, t)| tick >= **t).map(|(n, _)| n.clone()).collect();
        for n in due {
            self.pending_restock.remove(&n);
            if let Some(l) = self.bots.get_mut(&n) {
                l.bot.orders.retreat = None;
                let kit = l.kit.clone().and_then(|k| self.kits.kits.get(&k).cloned());
                out.push(Effect::Restock { bot: l.bot.name.clone(), kit });
            }
        }
        // Debug lines.
        // One line every fourth tick, the bots in turn (the lines go to the operators' action
        // bars: a line per bot per tick flooded them), and none once no operator is on to read them.
        if self.players.iter().all(|p| p.level < 2) {
            for l in self.bots.values_mut() {
                l.bot.debug_view = false;
            }
            self.debug_all = false;
        } else if tick % 4 == 0 {
            let mut shown: Vec<&Live> = self.bots.values().filter(|l| l.bot.debug_view).collect();
            if !shown.is_empty() {
                shown.sort_by(|a, b| a.bot.name.cmp(&b.bot.name));
                let l = shown[(tick / 4) as usize % shown.len()];
                out.push(Effect::NameLine {
                    bot: l.bot.name.clone(),
                    text: Some(format!("{} | {} | {}", l.bot.last_mode, l.bot.fight.target_name, l.bot.last_call)),
                });
            }
        }
        out.extend(self.saves());
        out
    }

    /// One bot's turn. None for a bot not placed yet.
    pub fn think(&mut self, name: &str, p: &Perception) -> Option<Decision> {
        let key = name.to_ascii_lowercase();
        let l = self.bots.get_mut(&key)?;
        if !l.joined {
            return None;
        }
        if l.bot.settings.generation != self.settings.generation {
            let f = self.squad.factions.faction_of(name).map(str::to_owned);
            let snap = self.snapshot_for(name);
            let l = self.bots.get_mut(&key)?;
            l.bot.settings = snap;
            l.bot.faction = f;
        }
        let l = self.bots.get_mut(&key)?;
        Some(l.bot.tick(p, &mut self.squad))
    }

    /// The level a bot is in now (portals and teleports move it).
    pub fn bot_world(&mut self, name: &str, dimension: &str) {
        if let Some(l) = self.bot_mut(name) {
            if l.world != dimension {
                l.world = dimension.to_owned();
            }
        }
    }

    /// The server moved a bot (teleport, knockback): its client physics follow.
    pub fn moved(&mut self, name: &str, pos: Option<Vec3>, vel: Option<Vec3>) {
        if let Some(l) = self.bot_mut(name) {
            if let Some(p) = pos {
                l.bot.body.pos = p;
                l.bot.body.fall_distance = 0.0;
                l.bot.path.clear();
            }
            if let Some(v) = vel {
                l.bot.body.vel = v;
            }
        }
    }

    /// `/horizonbot reload`: the files again (bots stay, scripts stop).
    pub fn reload(&mut self, config_text: &str, files: &Files) -> Result<(), String> {
        let config = Config::parse(config_text)?;
        self.settings = config.settings();
        self.config = config;
        self.config_text = config_text.to_owned();
        self.load_files(files);
        self.runner.stop_all();
        self.refresh_settings();
        Ok(())
    }
}

fn skin_name(s: &SkinArg) -> Option<String> {
    match s {
        SkinArg::From(a) => Some(a.clone()),
        _ => None,
    }
}

fn stat_line(m: &Messages, r: &crate::stats::Record, prefix: &str) -> String {
    m.get(
        "stats.line",
        &[
            prefix,
            &r.name,
            &r.kills.to_string(),
            &r.deaths.to_string(),
            &format!("{:.2}", r.kd()),
            &format!("{:.0}", r.accuracy() * 100.0),
            &r.landed.to_string(),
            &r.swings.to_string(),
            &r.shot_hits.to_string(),
            &format!("{:.0}", r.dealt),
            &format!("{:.0}", r.taken),
        ],
    )
}

const HELP: &str = "HorizonBot: spawn, remove, removeall, list, menu, view, restock, attack, fight, stop, retreat, hide, tether, follow, guard, pause, resume, tp, path, set, settings, difficulty, playstyle, randomplaystyle, randomplaystylechances, techniques, faction (f), overfaction (of), kit, preset, script, owner, diagnose, arena, debugview, stats, debug, givebow, drop, resetattr, clearplayerdata, reload. /ghost record|stoprecord|spawn|stare|remove|list|recordings.";

#[cfg(test)]
mod tests;
