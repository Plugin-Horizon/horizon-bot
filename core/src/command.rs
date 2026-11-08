//! The `/horizonbot` (aliases `/hbot`, `/bot`) command grammar, `ghost` sub-commands included, parsed into [`Command`]s the plugin
//! carries out. Usage errors come back as text for the sender.

use crate::selector::{read_target, Selector, Target, Words};

#[derive(Clone, Debug, PartialEq)]
pub enum SpawnWhat {
    /// One bot, generated name.
    One,
    Named(String),
    Count(u32),
    Table(String, u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SkinArg {
    Default,
    None,
    From(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TpMode {
    Instant,
    Portal { ground: bool },
    Pearl,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    Hold,
    Attack(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Scope {
    Global,
    Bot(String),
    Faction(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Difficulty {
    List(Scope),
    Preset(Scope, String),
    Set(Scope, String, String),
    Clear(Scope),
}

#[derive(Clone, Debug, PartialEq)]
pub enum FactionCmd {
    Create(String),
    Disband(String),
    Join(String, Selector),
    JoinRange(String, f64),
    Leave(String),
    Hostile(String, String),
    Peace(String, String),
    Ally(String, String),
    Unally(String, String),
    Leader(String, Option<String>),
    Follow(String, Option<String>, Option<String>, Option<u32>),
    Goto { faction: String, to: Option<Target>, status: bool, formation: Option<String>, size: Option<u32>, then: Then },
    Formation(String, Option<(String, Option<u32>)>),
    Skin(String, Option<String>),
    SkinList,
    KitSet(String, String, u32),
    KitList(String),
    KitClear(String),
    KitGive(String),
    Info(String),
    List,
    Set(String, String, String),
    Settings(String, Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum OverfactionCmd {
    Create(String),
    Disband(String),
    Add(String, String),
    Remove(String),
    Hostile(String, String),
    Peace(String, String),
    Leader(String, Option<String>),
    Follow(String, Option<String>, Option<String>),
    Tp(String, Target),
    Info(String),
    List,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KitCmd {
    Save(String, Option<String>),
    Give(String, Selector),
    GiveFaction(String, Selector),
    Edit(String),
    Rename(String, String),
    Delete(String),
    List,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PresetCmd {
    Show,
    Apply(String),
    Save(String),
    Delete(String),
    Off,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScriptCmd {
    Create(String),
    Add(String, String),
    Show(String),
    Remove(String, usize),
    Delete(String),
    Run(String, Vec<String>),
    List,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathCmd {
    Create(String),
    Add(String),
    Remove(String, usize),
    Delete(String),
    List(Option<String>),
    Assign(Selector, String),
    Stop(Selector),
    CommandAdd(String, usize, String),
    CommandList(String, usize),
    CommandRemove(String, usize, usize),
    CommandClear(String, usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum OwnerCmd {
    Set(String, String),
    Clear(String),
    List(String),
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArenaCmd {
    Start { kit_a: String, kit_b: String, rounds: u32, skill_a: Option<u8>, skill_b: Option<u8> },
    Stop,
    Report,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GhostCmd {
    Record(String),
    StopRecord,
    Spawn { name: String, recording: String, skin: SkinArg, lines: Vec<String> },
    Stare { name: String, skin: SkinArg, lines: Vec<String> },
    Remove(String),
    List,
    Recordings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Help,
    Spawn { what: SpawnWhat, skin: SkinArg },
    Remove(String),
    RemoveAll,
    List,
    Menu,
    Restock(Selector, bool),
    View { enderchest: bool, bot: String },
    Reload,
    Attack(String, String),
    Fight(String, String),
    Stop(String),
    Retreat(Selector, Option<Target>),
    RetreatStop(Selector),
    Hide(Selector),
    HideStop(Selector),
    Tether { target: String, leader: String, escort: Selector, radius: f64 },
    TetherStop(String),
    TetherInfo(String),
    Follow { who: Selector, target: String, formation: Option<String>, size: Option<u32> },
    FollowStop(Selector),
    Guard { who: Selector, radius: f64, at: Option<Target> },
    GuardStop(Option<Selector>),
    Pause(Option<Selector>),
    Resume(Selector),
    Tp { mode: TpMode, who: Selector, to: Target, range: f64 },
    Path(PathCmd),
    Set(String, String),
    Settings { scope: Scope, filter: Option<String>, list: bool },
    Difficulty(Difficulty),
    PlaystyleList,
    Playstyle(String, Selector),
    PlaystyleClear(Selector),
    PlaystyleFaction(String, Option<String>),
    RandomPlaystyle(Selector),
    RandomPlaystyleChances(Option<(String, f64)>, bool),
    Techniques { scope: Scope, set: Option<(String, String)> },
    Faction(FactionCmd),
    Overfaction(OverfactionCmd),
    Kit(KitCmd),
    Preset(PresetCmd),
    Script(ScriptCmd),
    Owner(OwnerCmd),
    Diagnose(String, bool),
    Arena(ArenaCmd),
    DebugView(Selector),
    Stats(Option<String>),
    StatsTop(crate::stats::Top),
    StatsReset(String),
    Debug(String, Option<bool>),
    GiveBow(String),
    Drop(String, String, u32),
    ResetAttr(String),
    ClearPlayerData,
    Ghost(GhostCmd),
}

impl Command {
    /// Whether everyone may run it (`horizonbot.stats`); the rest needs `horizonbot.admin`.
    pub fn open_to_all(&self) -> bool {
        matches!(self, Command::Stats(_) | Command::StatsTop(_) | Command::Help)
    }
}

pub const FORMATIONS: &[&str] = &[
    "behind", "column", "line", "rows", "triangle", "wall", "open_circle", "square", "circle", "hexagon", "grid",
];

fn opt_formation(w: &mut Words) -> (Option<String>, Option<u32>) {
    let f = w
        .peek()
        .filter(|f| FORMATIONS.contains(&f.to_ascii_lowercase().as_str()))
        .map(|f| f.to_ascii_lowercase());
    if f.is_some() {
        w.next();
    }
    let size = w.peek().and_then(|s| s.parse::<u32>().ok());
    if size.is_some() {
        w.next();
    }
    (f, size)
}

fn read_scope(w: &mut Words) -> Result<Scope, String> {
    Ok(if w.eat("bot") {
        Scope::Bot(w.need("bot name")?.to_owned())
    } else if w.eat("faction") {
        Scope::Faction(w.need("faction name")?.to_ascii_lowercase())
    } else {
        Scope::Global
    })
}

fn word(w: &mut Words, what: &str) -> Result<String, String> {
    w.need(what).map(str::to_owned)
}

fn lower(w: &mut Words, what: &str) -> Result<String, String> {
    w.need(what).map(|s| s.to_ascii_lowercase())
}

fn read_skin_and_lines(w: &mut Words) -> (SkinArg, Vec<String>) {
    let mut skin = SkinArg::Default;
    if let Some(s) = w.peek().and_then(|s| s.strip_prefix("skin:")) {
        skin = if s.eq_ignore_ascii_case("none") { SkinArg::None } else { SkinArg::From(s.to_owned()) };
        w.next();
    }
    let rest = w.rest();
    let lines = if rest.is_empty() {
        Vec::new()
    } else {
        rest.split('|').map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect()
    };
    (skin, lines)
}

/// Parses everything after `/horizonbot` (or `/bot`).
pub fn parse(line: &str) -> Result<Command, String> {
    let mut w = Words::new(line);
    let Some(sub) = w.next() else {
        return Ok(Command::Help);
    };
    let cmd = match sub.to_ascii_lowercase().as_str() {
        "help" => Command::Help,
        "spawn" => {
            let what = match w.next() {
                None => SpawnWhat::One,
                Some(a) => {
                    if let Some(t) = a.strip_prefix("names:") {
                        let n = w.number("count")? as u32;
                        SpawnWhat::Table(t.to_owned(), n.clamp(1, 10_000))
                    } else if let Ok(n) = a.parse::<u32>() {
                        if !(1..=10_000).contains(&n) {
                            return Err("Spawn 1 to 10000 bots at a time (max-bots caps the total)".into());
                        }
                        SpawnWhat::Count(n)
                    } else {
                        if !crate::skins::valid_name(a) {
                            return Err("Names are up to 16 letters, digits or _".into());
                        }
                        SpawnWhat::Named(a.to_owned())
                    }
                }
            };
            let skin = match w.next() {
                None => SkinArg::Default,
                Some(s) if s.eq_ignore_ascii_case("none") => SkinArg::None,
                Some(s) => SkinArg::From(s.to_owned()),
            };
            Command::Spawn { what, skin }
        }
        "remove" => Command::Remove(word(&mut w, "bot")?),
        "removeall" => Command::RemoveAll,
        "list" => Command::List,
        "menu" => Command::Menu,
        "restock" | "regear" => {
            let s = Selector::read(&mut w, false)?;
            Command::Restock(s, w.eat("instant"))
        }
        "view" => {
            let kind = lower(&mut w, "inventory or enderchest")?;
            let ec = match kind.as_str() {
                "inventory" | "inv" => false,
                "enderchest" | "ec" => true,
                _ => return Err("view inventory|enderchest <bot>".into()),
            };
            Command::View { enderchest: ec, bot: word(&mut w, "bot")? }
        }
        "reload" => Command::Reload,
        "attack" => Command::Attack(word(&mut w, "bot")?, word(&mut w, "target")?),
        "fight" => Command::Fight(word(&mut w, "bot")?, word(&mut w, "bot")?),
        "stop" => Command::Stop(word(&mut w, "bot")?),
        "retreat" => {
            if w.eat("stop") {
                Command::RetreatStop(Selector::read(&mut w, true)?)
            } else {
                let s = Selector::read(&mut w, true)?;
                let to = if w.eat("to") { Some(read_target(&mut w)?) } else { None };
                Command::Retreat(s, to)
            }
        }
        "hide" => {
            if w.eat("stop") {
                Command::HideStop(Selector::read(&mut w, true)?)
            } else {
                Command::Hide(Selector::read(&mut w, true)?)
            }
        }
        "tether" => {
            if w.eat("stop") {
                Command::TetherStop(word(&mut w, "target")?)
            } else if w.eat("info") {
                Command::TetherInfo(word(&mut w, "target")?)
            } else {
                let target = word(&mut w, "target")?;
                let leader = word(&mut w, "leader")?;
                let escort = Selector::read(&mut w, true)?;
                let radius = w.opt_number().unwrap_or(20.0);
                if !(1.0..=512.0).contains(&radius) {
                    return Err("The radius goes from 1 to 512".into());
                }
                Command::Tether { target, leader, escort, radius }
            }
        }
        "follow" => {
            if w.eat("stop") {
                Command::FollowStop(Selector::read(&mut w, false)?)
            } else {
                let who = Selector::read(&mut w, true)?;
                let target = word(&mut w, "target")?;
                let (formation, size) = opt_formation(&mut w);
                Command::Follow { who, target, formation, size }
            }
        }
        "guard" | "protect" => {
            if w.eat("stop") {
                Command::GuardStop(if w.left() > 0 { Some(Selector::read(&mut w, false)?) } else { None })
            } else {
                let who = Selector::read(&mut w, false)?;
                let radius = w.opt_number().unwrap_or(12.0);
                if !(2.0..=128.0).contains(&radius) {
                    return Err("The radius goes from 2 to 128".into());
                }
                let at = if w.left() >= 3 { Some(read_target(&mut w)?) } else { None };
                Command::Guard { who, radius, at }
            }
        }
        "pause" => Command::Pause(if w.left() > 0 { Some(Selector::read(&mut w, false)?) } else { None }),
        "resume" | "unpause" => Command::Resume(Selector::read(&mut w, false)?),
        "tp" => {
            let mode = if w.eat("portal") {
                TpMode::Portal { ground: w.eat("ground") }
            } else if w.eat("pearl") {
                TpMode::Pearl
            } else if w.peek() == Some("rift") {
                return Err("tp takes portal or pearl".into());
            } else {
                TpMode::Instant
            };
            let who = Selector::read(&mut w, mode != TpMode::Instant)?;
            let to = read_target(&mut w)?;
            let range = if mode == TpMode::Instant { 0.0 } else { w.opt_number().unwrap_or(100.0).clamp(0.0, 1000.0) };
            Command::Tp { mode, who, to, range }
        }
        "path" => Command::Path(parse_path(&mut w)?),
        "set" => Command::Set(lower(&mut w, "setting")?, word(&mut w, "value")?),
        "settings" => {
            if w.eat("search") {
                Command::Settings { scope: Scope::Global, filter: Some(lower(&mut w, "word")?), list: false }
            } else if w.eat("list") {
                Command::Settings { scope: Scope::Global, filter: w.next().map(str::to_ascii_lowercase), list: true }
            } else {
                let scope = read_scope(&mut w)?;
                let filter = w.next().map(str::to_ascii_lowercase);
                let list = scope == Scope::Global && filter.is_some();
                Command::Settings { scope, filter, list }
            }
        }
        "difficulty" => {
            let scope = read_scope(&mut w)?;
            Command::Difficulty(match w.next() {
                None => Difficulty::List(scope),
                Some(x) if x.eq_ignore_ascii_case("list") => Difficulty::List(scope),
                Some(x) if x.eq_ignore_ascii_case("clear") => Difficulty::Clear(scope),
                Some(x) if x.eq_ignore_ascii_case("preset") => {
                    let level = lower(&mut w, "level")?;
                    if !crate::settings::DIFFICULTY_LEVELS.contains(&level.as_str()) {
                        return Err(format!("Levels: {}", crate::settings::DIFFICULTY_LEVELS.join(", ")));
                    }
                    Difficulty::Preset(scope, level)
                }
                Some(knob) => {
                    let knob = knob.to_ascii_lowercase();
                    if !crate::settings::KNOBS.contains(&knob.as_str()) {
                        return Err(format!("Knobs: {}", crate::settings::KNOBS.join(", ")));
                    }
                    Difficulty::Set(scope, knob, word(&mut w, "value")?)
                }
            })
        }
        "playstyle" => match w.next().map(str::to_ascii_lowercase).as_deref() {
            Some("list") | None => Command::PlaystyleList,
            Some("clear") => Command::PlaystyleClear(Selector::read(&mut w, true)?),
            Some("faction") => {
                let f = lower(&mut w, "faction")?;
                let s = lower(&mut w, "playstyle or clear")?;
                Command::PlaystyleFaction(f, (s != "clear").then_some(s))
            }
            Some(style) => {
                if !crate::config::PLAYSTYLES.contains(&style) {
                    return Err(format!("Playstyles: {}", crate::config::PLAYSTYLES.join(", ")));
                }
                Command::Playstyle(style.to_owned(), Selector::read(&mut w, true)?)
            }
        },
        "randomplaystyle" => Command::RandomPlaystyle(Selector::read(&mut w, true)?),
        "randomplaystylechances" => match w.next().map(str::to_ascii_lowercase).as_deref() {
            None | Some("list") => Command::RandomPlaystyleChances(None, false),
            Some("reset") => Command::RandomPlaystyleChances(None, true),
            Some(style) => {
                if !crate::config::PLAYSTYLES.contains(&style) {
                    return Err(format!("Playstyles: {}", crate::config::PLAYSTYLES.join(", ")));
                }
                Command::RandomPlaystyleChances(Some((style.to_owned(), w.number("weight")?.max(0.0))), false)
            }
        },
        "techniques" | "technique" => {
            let scope = read_scope(&mut w)?;
            let set = match w.next() {
                None => None,
                Some(t) => Some((t.to_ascii_lowercase(), lower(&mut w, "on, off or a variant")?)),
            };
            Command::Techniques { scope, set }
        }
        "faction" | "f" => Command::Faction(parse_faction(&mut w)?),
        "overfaction" | "of" => Command::Overfaction(parse_overfaction(&mut w)?),
        "kit" => Command::Kit(match lower(&mut w, "save, give, givefaction, edit, rename, delete or list")?.as_str() {
            "save" => KitCmd::Save(word(&mut w, "kit name")?, w.next().map(str::to_owned)),
            "give" => KitCmd::Give(word(&mut w, "kit")?, Selector::read(&mut w, true)?),
            "givefaction" => {
                let kit = word(&mut w, "kit")?;
                let s = match (w.peek(), w.peek_at(1)) {
                    (Some(_), None) if !w.peek().unwrap().contains('[') && w.peek() != Some("all") && w.peek() != Some("latest") => {
                        Selector::Faction(lower(&mut w, "faction")?)
                    }
                    _ => Selector::read(&mut w, true)?,
                };
                KitCmd::GiveFaction(kit, s)
            }
            "edit" => KitCmd::Edit(word(&mut w, "kit")?),
            "rename" => KitCmd::Rename(word(&mut w, "kit")?, word(&mut w, "new name")?),
            "delete" => KitCmd::Delete(word(&mut w, "kit")?),
            "list" => KitCmd::List,
            other => return Err(format!("No kit command {other}")),
        }),
        "preset" => Command::Preset(match w.next() {
            None => PresetCmd::Show,
            Some(p) => match p.to_ascii_lowercase().as_str() {
                "save" => PresetCmd::Save(lower(&mut w, "preset name")?),
                "delete" => PresetCmd::Delete(lower(&mut w, "preset name")?),
                "off" => PresetCmd::Off,
                name => PresetCmd::Apply(name.to_owned()),
            },
        }),
        "script" => Command::Script(match lower(&mut w, "create, add, show, remove, delete, run or list")?.as_str() {
            "create" => ScriptCmd::Create(word(&mut w, "script name")?),
            "add" => {
                let n = word(&mut w, "script name")?;
                let line = w.rest();
                if line.is_empty() {
                    return Err("script add <name> <command>".into());
                }
                ScriptCmd::Add(n, line)
            }
            "show" => ScriptCmd::Show(word(&mut w, "script name")?),
            "remove" => ScriptCmd::Remove(word(&mut w, "script name")?, w.number("line")? as usize),
            "delete" => ScriptCmd::Delete(word(&mut w, "script name")?),
            "run" => {
                let n = word(&mut w, "script name")?;
                let args = w.rest().split_whitespace().map(str::to_owned).collect();
                ScriptCmd::Run(n, args)
            }
            "list" => ScriptCmd::List,
            other => return Err(format!("No script command {other}")),
        }),
        "owner" => Command::Owner(match lower(&mut w, "set, clear, list or help")?.as_str() {
            "set" => OwnerCmd::Set(word(&mut w, "bot")?, word(&mut w, "player")?),
            "clear" => OwnerCmd::Clear(word(&mut w, "bot")?),
            "list" => OwnerCmd::List(word(&mut w, "player")?),
            _ => OwnerCmd::Help,
        }),
        "diagnose" => {
            let who = word(&mut w, "bot or player")?;
            Command::Diagnose(who, w.eat("reset"))
        }
        "arena" => Command::Arena(match lower(&mut w, "start, stop or report")?.as_str() {
            "start" => {
                let kit_a = word(&mut w, "kit A")?;
                let kit_b = word(&mut w, "kit B")?;
                let rounds = w.opt_number().unwrap_or(10.0).clamp(1.0, 1000.0) as u32;
                let skill_a = w.opt_number().map(|s| s.clamp(0.0, 100.0) as u8);
                let skill_b = w.opt_number().map(|s| s.clamp(0.0, 100.0) as u8);
                ArenaCmd::Start { kit_a, kit_b, rounds, skill_a, skill_b }
            }
            "stop" => ArenaCmd::Stop,
            _ => ArenaCmd::Report,
        }),
        "debugview" => Command::DebugView(Selector::read(&mut w, true)?),
        "stats" => match w.next() {
            None => Command::Stats(None),
            Some(t) if t.eq_ignore_ascii_case("top") => {
                let by = w.next().map_or(Some(crate::stats::Top::Kills), crate::stats::Top::parse);
                Command::StatsTop(by.ok_or("stats top [kills|kd|accuracy|damage]")?)
            }
            Some(r) if r.eq_ignore_ascii_case("reset") => Command::StatsReset(word(&mut w, "name or all")?),
            Some(name) => Command::Stats(Some(name.to_owned())),
        },
        "debug" => {
            let b = word(&mut w, "bot")?;
            let inv = if w.eat("invulnerable") {
                Some(match lower(&mut w, "true or false")?.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err("debug <bot> invulnerable <true|false>".into()),
                })
            } else {
                None
            };
            Command::Debug(b, inv)
        }
        "givebow" => Command::GiveBow(word(&mut w, "bot")?),
        "drop" => {
            let b = word(&mut w, "bot")?;
            let item = crate::items::full(&lower(&mut w, "item id")?);
            let n = w.opt_number().unwrap_or(1.0).clamp(1.0, 64.0 * 36.0) as u32;
            Command::Drop(b, item, n)
        }
        "resetattr" | "resetattributes" => Command::ResetAttr(word(&mut w, "player or bot")?),
        "clearplayerdata" => Command::ClearPlayerData,
        other => return Err(format!("Unknown command {other}. /horizonbot help lists them.")),
    };
    Ok(cmd)
}

fn parse_path(w: &mut Words) -> Result<PathCmd, String> {
    Ok(match lower(w, "create, add, remove, delete, list, assign, stop or command")?.as_str() {
        "create" => PathCmd::Create(word(w, "route name")?),
        "add" => PathCmd::Add(word(w, "route name")?),
        "remove" => PathCmd::Remove(word(w, "route name")?, w.number("point")? as usize),
        "delete" => PathCmd::Delete(word(w, "route name")?),
        "list" => PathCmd::List(w.next().map(str::to_owned)),
        "assign" => {
            let s = Selector::read(w, true)?;
            PathCmd::Assign(s, word(w, "route")?)
        }
        "stop" => PathCmd::Stop(Selector::read(w, true)?),
        "command" => {
            let p = word(w, "route")?;
            let i = w.number("waypoint")? as usize;
            match lower(w, "add, list, remove or clear")?.as_str() {
                "add" => {
                    let c = w.rest();
                    if c.is_empty() {
                        return Err("path command <path> <waypoint> add <command...>".into());
                    }
                    PathCmd::CommandAdd(p, i, c.trim_start_matches('/').to_owned())
                }
                "list" => PathCmd::CommandList(p, i),
                "remove" => PathCmd::CommandRemove(p, i, w.number("command number")? as usize),
                "clear" => PathCmd::CommandClear(p, i),
                other => return Err(format!("No path command {other}")),
            }
        }
        other => return Err(format!("No path command {other}")),
    })
}

fn parse_faction(w: &mut Words) -> Result<FactionCmd, String> {
    Ok(match lower(w, "a faction command")?.as_str() {
        "create" => FactionCmd::Create(lower(w, "faction name")?),
        "disband" => FactionCmd::Disband(lower(w, "faction name")?),
        "join" => {
            let f = lower(w, "faction")?;
            if w.eat("range") {
                let r = w.number("blocks")?;
                if !(0.0..=512.0).contains(&r) {
                    return Err("range goes from 0 to 512".into());
                }
                FactionCmd::JoinRange(f, r)
            } else {
                FactionCmd::Join(f, Selector::read(w, true)?)
            }
        }
        "leave" => {
            // Both `leave <member>` and the older `leave <faction> <member>`.
            let a = word(w, "member")?;
            FactionCmd::Leave(w.next().map_or(a, str::to_owned))
        }
        "hostile" => FactionCmd::Hostile(lower(w, "faction")?, lower(w, "faction")?),
        "peace" => FactionCmd::Peace(lower(w, "faction")?, lower(w, "faction")?),
        "ally" => FactionCmd::Ally(lower(w, "faction")?, lower(w, "faction")?),
        "unally" => FactionCmd::Unally(lower(w, "faction")?, lower(w, "faction")?),
        "leader" => {
            let f = lower(w, "faction")?;
            let l = word(w, "player, bot or clear")?;
            FactionCmd::Leader(f, (!l.eq_ignore_ascii_case("clear")).then_some(l))
        }
        "follow" => {
            let f = lower(w, "faction")?;
            let t = word(w, "target or stop")?;
            let (formation, size) = opt_formation(w);
            FactionCmd::Follow(f, (!t.eq_ignore_ascii_case("stop")).then_some(t), formation, size)
        }
        "goto" => {
            let faction = lower(w, "faction")?;
            if w.eat("stop") {
                return Ok(FactionCmd::Goto { faction, to: None, status: false, formation: None, size: None, then: Then::Hold });
            }
            if w.eat("status") {
                return Ok(FactionCmd::Goto { faction, to: None, status: true, formation: None, size: None, then: Then::Hold });
            }
            let to = read_target(w)?;
            let (formation, size) = opt_formation(w);
            let then = if w.eat("then") {
                if w.eat("attack") {
                    Then::Attack(word(w, "target")?)
                } else {
                    w.eat("hold");
                    Then::Hold
                }
            } else {
                Then::Hold
            };
            FactionCmd::Goto { faction, to: Some(to), status: false, formation, size, then }
        }
        "formation" => {
            let f = lower(w, "faction")?;
            let shape = lower(w, "shape or clear")?;
            if shape == "clear" {
                FactionCmd::Formation(f, None)
            } else {
                if !FORMATIONS.contains(&shape.as_str()) {
                    return Err(format!("Formations: {}", FORMATIONS.join(", ")));
                }
                FactionCmd::Formation(f, Some((shape, w.opt_number().map(|s| s.max(1.0) as u32))))
            }
        }
        "skin" | "uniform" => {
            let f = lower(w, "faction or list")?;
            if f == "list" {
                FactionCmd::SkinList
            } else {
                let a = word(w, "account or stop")?;
                FactionCmd::Skin(f, (!a.eq_ignore_ascii_case("stop")).then_some(a))
            }
        }
        "kit" => {
            let f = lower(w, "faction")?;
            match lower(w, "set, list, clear or give")?.as_str() {
                "set" => {
                    let k = word(w, "kit")?;
                    let weight = w.number("weight")?;
                    if !(0.0..=1000.0).contains(&weight) {
                        return Err("Weights go from 0 to 1000".into());
                    }
                    FactionCmd::KitSet(f, k, weight as u32)
                }
                "list" => FactionCmd::KitList(f),
                "clear" => FactionCmd::KitClear(f),
                "give" => FactionCmd::KitGive(f),
                other => return Err(format!("No faction kit command {other}")),
            }
        }
        "info" => FactionCmd::Info(lower(w, "faction")?),
        "list" => FactionCmd::List,
        "set" => FactionCmd::Set(lower(w, "faction")?, lower(w, "setting")?, word(w, "value")?),
        "settings" => FactionCmd::Settings(lower(w, "faction")?, w.next().map(str::to_ascii_lowercase)),
        other => return Err(format!("No faction command {other}")),
    })
}

fn parse_overfaction(w: &mut Words) -> Result<OverfactionCmd, String> {
    Ok(match lower(w, "an overfaction command")?.as_str() {
        "create" => OverfactionCmd::Create(lower(w, "name")?),
        "disband" => OverfactionCmd::Disband(lower(w, "name")?),
        "add" => OverfactionCmd::Add(lower(w, "overfaction")?, lower(w, "faction")?),
        "remove" => OverfactionCmd::Remove(lower(w, "faction")?),
        "hostile" => OverfactionCmd::Hostile(lower(w, "overfaction")?, lower(w, "overfaction or faction")?),
        "peace" => OverfactionCmd::Peace(lower(w, "overfaction")?, lower(w, "overfaction or faction")?),
        "leader" => {
            let o = lower(w, "overfaction")?;
            let l = word(w, "player, bot or clear")?;
            OverfactionCmd::Leader(o, (!l.eq_ignore_ascii_case("clear")).then_some(l))
        }
        "follow" => {
            let o = lower(w, "overfaction")?;
            let t = word(w, "target or stop")?;
            let (f, _) = opt_formation(w);
            OverfactionCmd::Follow(o, (!t.eq_ignore_ascii_case("stop")).then_some(t), f)
        }
        "tp" => {
            let o = lower(w, "overfaction")?;
            OverfactionCmd::Tp(o, read_target(w)?)
        }
        "info" => OverfactionCmd::Info(lower(w, "overfaction")?),
        "list" => OverfactionCmd::List,
        other => return Err(format!("No overfaction command {other}")),
    })
}

/// Parses everything after `/horizonbot ghost`.
pub fn parse_ghost(line: &str) -> Result<GhostCmd, String> {
    let mut w = Words::new(line);
    Ok(match lower(&mut w, "record, stoprecord, spawn, stare, remove, list or recordings")?.as_str() {
        "record" => GhostCmd::Record(word(&mut w, "recording name")?),
        "stoprecord" => GhostCmd::StopRecord,
        "spawn" => {
            let name = word(&mut w, "ghost name")?;
            let recording = word(&mut w, "recording")?;
            let (skin, lines) = read_skin_and_lines(&mut w);
            GhostCmd::Spawn { name, recording, skin, lines }
        }
        "stare" => {
            let name = word(&mut w, "ghost name")?;
            let (skin, lines) = read_skin_and_lines(&mut w);
            GhostCmd::Stare { name, skin, lines }
        }
        "remove" => GhostCmd::Remove(word(&mut w, "ghost name")?),
        "list" => GhostCmd::List,
        "recordings" => GhostCmd::Recordings,
        other => return Err(format!("No ghost command {other}")),
    })
}

/// Selector forms the short reader takes (`pause`, `guard`, `tp`, `restock`, `resume`, `follow stop`).
const SEL_SHORT: &[&str] = &["all", "faction <faction:word>", "<bot:player_name>"];
/// Selector forms the full reader takes (`hide`, `retreat`, `tp portal`, `playstyle`, ...). A
/// `random[...]` filter is one word the client does not complete; it still reaches the parser.
const SEL_FULL: &[&str] = &[
    "all",
    "latest",
    "random",
    "faction <faction:word>",
    "overfaction <overfaction:word>",
    "<bot:player_name>",
];
/// `faction join` has its own faction name in the usage, so it takes no faction or overfaction form.
const SEL_JOIN: &[&str] = &["all", "latest", "random", "<bot:player_name>"];

/// Every `/horizonbot` sub-command path, without the root word (every root gets them).
/// Literals are the fixed names and choices, `<name:kind>` the typed arguments. The choice lists
/// come from the tables the parser reads, so completion cannot drift from it.
fn sub_usages() -> Vec<String> {
    use crate::settings::{techniques, DIFFICULTY_LEVELS, KNOBS};
    let styles = crate::config::PLAYSTYLES;
    let techs: Vec<&str> = techniques().iter().map(|s| s.key).collect();
    let mut v: Vec<String> = Vec::new();

    for s in [
        "help", "spawn", "list", "menu", "removeall", "reload", "clearplayerdata", "pause", "playstyle",
        "playstyle list", "randomplaystylechances", "randomplaystylechances list", "randomplaystylechances reset",
        "preset", "preset off", "owner help", "stats", "stats top", "stats top kills", "stats top kd",
        "stats top accuracy", "stats top damage", "stats reset all", "arena", "arena report", "arena stop",
        "kit list",
    ] {
        v.push(s.to_owned());
    }
    for s in [
        "spawn <count:integer> [skin:word]",
        "spawn <count:integer> none",
        "spawn <name:word> [skin:word]",
        "spawn <name:word> none",
        "spawn <table:word> <count:integer> [skin:word]",
        "remove <bot:player_name>",
        "view inventory <bot:player_name>",
        "view inv <bot:player_name>",
        "view enderchest <bot:player_name>",
        "view ec <bot:player_name>",
        "attack <bot:player_name> <target:player_name>",
        "fight <bot:player_name> <other:player_name>",
        "stop <bot:player_name>",
        "givebow <bot:player_name>",
        "drop <bot:player_name> <item:item> [count:integer]",
        "debug <bot:player_name>",
        "debug <bot:player_name> invulnerable true",
        "debug <bot:player_name> invulnerable false",
        "resetattr <who:player_name>",
        "resetattributes <who:player_name>",
        "diagnose <who:player_name>",
        "diagnose <who:player_name> reset",
        "owner set <bot:player_name> <player:player_name>",
        "owner clear <bot:player_name>",
        "owner list <player:player_name>",
        "stats <name:player_name>",
        "stats reset <name:player_name>",
        "set <setting:word> <value:word>",
        "settings",
        "settings list",
        "settings list <filter:word>",
        "settings search <word:word>",
        "settings <filter:word>",
        "settings bot <bot:player_name>",
        "settings bot <bot:player_name> <filter:word>",
        "settings faction <faction:word>",
        "settings faction <faction:word> <filter:word>",
        "tether stop <target:player_name>",
        "tether info <target:player_name>",
        "path create <route:word>",
        "path add <route:word>",
        "path remove <route:word> <point:integer>",
        "path delete <route:word>",
        "path list",
        "path list <route:word>",
        "path command <route:word> <waypoint:integer> add <command:text>",
        "path command <route:word> <waypoint:integer> list",
        "path command <route:word> <waypoint:integer> remove <number:integer>",
        "path command <route:word> <waypoint:integer> clear",
        "preset save <name:word>",
        "preset delete <name:word>",
        "preset <name:word>",
        "script create <name:word>",
        "script add <name:word> <command:text>",
        "script show <name:word>",
        "script remove <name:word> <line:integer>",
        "script delete <name:word>",
        "script run <name:word> [args:text]",
        "script list",
        "arena start <kit_a:word> <kit_b:word>",
        "arena start <kit_a:word> <kit_b:word> <rounds:integer>",
        "arena start <kit_a:word> <kit_b:word> <rounds:integer> <skill_a:integer>",
        "arena start <kit_a:word> <kit_b:word> <rounds:integer> <skill_a:integer> <skill_b:integer>",
        "kit save <kit:word> [from:player_name]",
        "kit edit <kit:word>",
        "kit rename <kit:word> <name:word>",
        "kit delete <kit:word>",
    ] {
        v.push(s.to_owned());
    }

    for st in styles {
        v.push(format!("randomplaystylechances {st} <weight:number>"));
        v.push(format!("playstyle faction <faction:word> {st}"));
    }
    v.push("playstyle faction <faction:word> clear".into());

    for scope in ["", " bot <bot:player_name>", " faction <faction:word>"] {
        v.push(format!("difficulty{scope}"));
        v.push(format!("difficulty{scope} list"));
        v.push(format!("difficulty{scope} clear"));
        for level in DIFFICULTY_LEVELS {
            v.push(format!("difficulty{scope} preset {level}"));
        }
        for knob in KNOBS {
            v.push(format!("difficulty{scope} {knob} <value:word>"));
        }
    }

    for head in ["techniques", "technique"] {
        v.push(head.to_owned());
        for key in &techs {
            v.push(format!("{head} {key} on"));
            v.push(format!("{head} {key} off"));
        }
        for scope in ["bot <bot:player_name>", "faction <faction:word>"] {
            v.push(format!("{head} {scope}"));
            v.push(format!("{head} {scope} <technique:word> on"));
            v.push(format!("{head} {scope} <technique:word> off"));
        }
    }

    for g in ["guard", "protect"] {
        v.push(format!("{g} stop"));
        for sel in SEL_SHORT {
            v.push(format!("{g} stop {sel}"));
            v.push(format!("{g} {sel} [radius:number]"));
            v.push(format!("{g} {sel} <radius:number> <x:word> <y:word> <z:word>"));
        }
    }

    for sel in SEL_SHORT {
        v.push(format!("pause {sel}"));
        v.push(format!("resume {sel}"));
        v.push(format!("unpause {sel}"));
        v.push(format!("follow stop {sel}"));
        for head in ["restock", "regear"] {
            v.push(format!("{head} {sel}"));
            v.push(format!("{head} {sel} instant"));
        }
        v.push(format!("tp {sel} <to:player_name>"));
        v.push(format!("tp {sel} <x:word> <y:word> <z:word>"));
    }

    for sel in SEL_FULL {
        v.push(format!("retreat stop {sel}"));
        v.push(format!("retreat {sel}"));
        v.push(format!("retreat {sel} to <target:player_name>"));
        v.push(format!("retreat {sel} to <x:word> <y:word> <z:word>"));
        v.push(format!("hide {sel}"));
        v.push(format!("hide stop {sel}"));
        v.push(format!("tether <target:player_name> <leader:player_name> {sel} [radius:number]"));
        v.push(format!("follow {sel} <target:player_name> [size:integer]"));
        for shape in FORMATIONS {
            v.push(format!("follow {sel} <target:player_name> {shape} [size:integer]"));
        }
        for mode in ["portal", "portal ground", "pearl"] {
            v.push(format!("tp {mode} {sel} <to:player_name> [range:number]"));
            v.push(format!("tp {mode} {sel} <x:word> <y:word> <z:word> [range:number]"));
        }
        v.push(format!("debugview {sel}"));
        v.push("debugview off".to_string());
        v.push(format!("randomplaystyle {sel}"));
        v.push(format!("playstyle clear {sel}"));
        for st in styles {
            v.push(format!("playstyle {st} {sel}"));
        }
        v.push(format!("path assign {sel} <route:word>"));
        v.push(format!("path stop {sel}"));
        v.push(format!("kit give <kit:word> {sel}"));
        v.push(format!("kit givefaction <kit:word> {sel}"));
    }

    for head in ["faction", "f"] {
        v.push(format!("{head} list"));
        v.push(format!("{head} create <faction:word>"));
        v.push(format!("{head} disband <faction:word>"));
        v.push(format!("{head} join <faction:word> range <blocks:number>"));
        for sel in SEL_JOIN {
            v.push(format!("{head} join <faction:word> {sel}"));
        }
        v.push(format!("{head} leave <member:player_name>"));
        v.push(format!("{head} leave <faction:word> <member:player_name>"));
        for kw in ["hostile", "peace", "ally", "unally"] {
            v.push(format!("{head} {kw} <faction:word> <other:word>"));
        }
        v.push(format!("{head} leader <faction:word> <leader:player_name>"));
        v.push(format!("{head} leader <faction:word> clear"));
        v.push(format!("{head} follow <faction:word> stop"));
        v.push(format!("{head} follow <faction:word> <target:player_name> [size:integer]"));
        for shape in FORMATIONS {
            v.push(format!("{head} follow <faction:word> <target:player_name> {shape} [size:integer]"));
        }
        v.push(format!("{head} goto <faction:word> stop"));
        v.push(format!("{head} goto <faction:word> status"));
        for to in ["<to:player_name>", "<x:word> <y:word> <z:word>"] {
            v.push(format!("{head} goto <faction:word> {to} [size:integer]"));
            v.push(format!("{head} goto <faction:word> {to} then hold"));
            v.push(format!("{head} goto <faction:word> {to} then attack <target:player_name>"));
            for shape in FORMATIONS {
                v.push(format!("{head} goto <faction:word> {to} {shape} [size:integer]"));
            }
        }
        v.push(format!("{head} formation <faction:word> clear"));
        for shape in FORMATIONS {
            v.push(format!("{head} formation <faction:word> {shape} [size:integer]"));
        }
        for kw in ["skin", "uniform"] {
            v.push(format!("{head} {kw} list"));
            v.push(format!("{head} {kw} <faction:word> stop"));
            v.push(format!("{head} {kw} <faction:word> <account:player_name>"));
        }
        v.push(format!("{head} kit <faction:word> set <kit:word> <weight:number>"));
        v.push(format!("{head} kit <faction:word> list"));
        v.push(format!("{head} kit <faction:word> clear"));
        v.push(format!("{head} kit <faction:word> give"));
        v.push(format!("{head} info <faction:word>"));
        v.push(format!("{head} set <faction:word> <setting:word> <value:word>"));
        v.push(format!("{head} settings <faction:word> [filter:word]"));
    }

    for head in ["overfaction", "of"] {
        v.push(format!("{head} list"));
        v.push(format!("{head} create <name:word>"));
        v.push(format!("{head} disband <name:word>"));
        v.push(format!("{head} add <overfaction:word> <faction:word>"));
        v.push(format!("{head} remove <faction:word>"));
        v.push(format!("{head} hostile <overfaction:word> <target:word>"));
        v.push(format!("{head} peace <overfaction:word> <target:word>"));
        v.push(format!("{head} leader <overfaction:word> <leader:player_name>"));
        v.push(format!("{head} leader <overfaction:word> clear"));
        v.push(format!("{head} follow <overfaction:word> stop"));
        v.push(format!("{head} follow <overfaction:word> <target:player_name>"));
        for shape in FORMATIONS {
            v.push(format!("{head} follow <overfaction:word> <target:player_name> {shape}"));
        }
        v.push(format!("{head} tp <overfaction:word> <to:player_name>"));
        v.push(format!("{head} tp <overfaction:word> <x:word> <y:word> <z:word>"));
        v.push(format!("{head} info <overfaction:word>"));
    }
    v
}

/// Every `/horizonbot ghost` sub-command path. The catch-all after them takes the rest of the line.
fn ghost_sub_usages() -> Vec<String> {
    [
        "record <name:word>",
        "stoprecord",
        "spawn <name:word> <recording:word> [rest:text]",
        "stare <name:word> [rest:text]",
        "remove <name:word>",
        "list",
        "recordings",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// The usages the server's command tree gets (completion), with operator levels. Each root gets
/// its sub-command paths, then a catch-all last: a line none of them matches (free text, a typo)
/// still reaches the parser, which answers it.
pub fn usages() -> Vec<(String, u8)> {
    let mut v = Vec::new();
    for root in ["horizonbot", "hbot", "bot"] {
        // Full tree on /horizonbot only; the short aliases keep the free-text fallback (smaller command packet).
        if root == "horizonbot" {
            for sub in sub_usages() {
                v.push((format!("{root} {sub}"), 0));
            }
            for sub in ghost_sub_usages() {
                v.push((format!("{root} ghost {sub}"), 2));
            }
            v.push((format!("{root} ghost [args:text]"), 2));
        }
        v.push((format!("{root} [args:text]"), 0));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns() {
        assert_eq!(parse("spawn").unwrap(), Command::Spawn { what: SpawnWhat::One, skin: SkinArg::Default });
        assert_eq!(parse("spawn 20").unwrap(), Command::Spawn { what: SpawnWhat::Count(20), skin: SkinArg::Default });
        assert_eq!(parse("spawn 400").unwrap(), Command::Spawn { what: SpawnWhat::Count(400), skin: SkinArg::Default });
        assert!(parse("spawn 10001").is_err());
        assert_eq!(
            parse("spawn Marco Notch").unwrap(),
            Command::Spawn { what: SpawnWhat::Named("Marco".into()), skin: SkinArg::From("Notch".into()) }
        );
        assert_eq!(
            parse("spawn names:pirates 4 none").unwrap(),
            Command::Spawn { what: SpawnWhat::Table("pirates".into(), 4), skin: SkinArg::None }
        );
        assert!(parse("spawn ThisNameIsWayTooLong").is_err());
    }

    #[test]
    fn orders() {
        assert_eq!(
            parse("retreat overfaction north to 100 64 -250").unwrap(),
            Command::Retreat(
                Selector::Overfaction("north".into()),
                Some(Target::Pos { pos: crate::math::Vec3::new(100.0, 64.0, -250.0), relative: [false; 3] })
            )
        );
        assert_eq!(
            parse("follow faction red Steve circle").unwrap(),
            Command::Follow { who: Selector::Faction("red".into()), target: "Steve".into(), formation: Some("circle".into()), size: None }
        );
        assert_eq!(
            parse("tether Prisoner Boss faction guards 15").unwrap(),
            Command::Tether { target: "Prisoner".into(), leader: "Boss".into(), escort: Selector::Faction("guards".into()), radius: 15.0 }
        );
        assert_eq!(
            parse("guard Marco 10 100 64 -200").unwrap(),
            Command::Guard {
                who: Selector::Name("Marco".into()),
                radius: 10.0,
                at: Some(Target::Pos { pos: crate::math::Vec3::new(100.0, 64.0, -200.0), relative: [false; 3] })
            }
        );
        assert_eq!(
            parse("tp pearl random[limit=10,world=world] Steve 30").unwrap(),
            Command::Tp {
                mode: TpMode::Pearl,
                who: Selector::Random(crate::selector::RandomFilter { limit: 10, world: Some("world".into()), ..Default::default() }),
                to: Target::Entity("Steve".into()),
                range: 30.0
            }
        );
        assert!(parse("tp rift all Steve").is_err());
        assert_eq!(parse("pause").unwrap(), Command::Pause(None));
    }

    #[test]
    fn factions_kits_scripts() {
        assert_eq!(
            parse("faction goto bandits Steve rows 4 then attack Steve").unwrap(),
            Command::Faction(FactionCmd::Goto {
                faction: "bandits".into(),
                to: Some(Target::Entity("Steve".into())),
                status: false,
                formation: Some("rows".into()),
                size: Some(4),
                then: Then::Attack("Steve".into())
            })
        );
        assert_eq!(parse("f leave bandits Steve").unwrap(), Command::Faction(FactionCmd::Leave("Steve".into())));
        assert_eq!(parse("f leave Steve").unwrap(), Command::Faction(FactionCmd::Leave("Steve".into())));
        assert_eq!(
            parse("kit givefaction iron bandits").unwrap(),
            Command::Kit(KitCmd::GiveFaction("iron".into(), Selector::Faction("bandits".into())))
        );
        assert_eq!(
            parse("kit give iron random[limit=5,alreadyhaskit=false]").unwrap(),
            Command::Kit(KitCmd::Give(
                "iron".into(),
                Selector::Random(crate::selector::RandomFilter { limit: 5, alreadyhaskit: Some(false), ..Default::default() })
            ))
        );
        assert_eq!(
            parse("script add raid horizonbot faction join {1} latest").unwrap(),
            Command::Script(ScriptCmd::Add("raid".into(), "horizonbot faction join {1} latest".into()))
        );
        assert_eq!(
            parse("path command walls 2 add say {bot} reached the gate").unwrap(),
            Command::Path(PathCmd::CommandAdd("walls".into(), 2, "say {bot} reached the gate".into()))
        );
        assert_eq!(parse("stats top kd").unwrap(), Command::StatsTop(crate::stats::Top::Kd));
        assert!(parse("stats").unwrap().open_to_all());
        assert!(!parse("removeall").unwrap().open_to_all());
        assert_eq!(
            parse("difficulty faction rookies preset easy").unwrap(),
            Command::Difficulty(Difficulty::Preset(Scope::Faction("rookies".into()), "easy".into()))
        );
    }

    #[test]
    fn ghosts() {
        assert_eq!(
            parse_ghost("stare Steve skin:none Who's there?|...|Leave now").unwrap(),
            GhostCmd::Stare {
                name: "Steve".into(),
                skin: SkinArg::None,
                lines: vec!["Who's there?".into(), "...".into(), "Leave now".into()]
            }
        );
        assert_eq!(
            parse_ghost("spawn Keeper lap skin:Notch You shouldn't be here").unwrap(),
            GhostCmd::Spawn {
                name: "Keeper".into(),
                recording: "lap".into(),
                skin: SkinArg::From("Notch".into()),
                lines: vec!["You shouldn't be here".into()]
            }
        );
    }
}
