//! The bot's brain. Each tick it gets what the bot perceives ([`Perception`]) and answers with
//! what a player's hands would do ([`Decision`]): the keys held, where it looks, and the clicks
//! (attack, use, select a slot, place, dig...). Its own client physics then move it, and the glue
//! sends the result to the server as that client's packets.

pub mod combat;
pub mod formation;
pub mod inventory;
pub mod nav;
pub mod personality;
pub mod regear;
pub mod squad;
pub mod survival;
pub mod team;
pub mod techniques;

use crate::math::{approach_angle, look_vector, pitch_to, yaw_to, Rng, Vec3};
use crate::physics::{Body, Input};
use crate::settings::Snapshot;
use crate::world::BlockView;
use inventory::Inventory;
use personality::{Personality, Playstyle};
use std::collections::{BTreeMap, VecDeque};

/// Another living thing a bot can perceive (the glue fills these for everything within range).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    pub id: i32,
    pub uuid: u128,
    pub name: String,
    /// `minecraft:player`, `minecraft:zombie`...
    pub kind: String,
    pub is_player: bool,
    pub is_bot: bool,
    pub faction: Option<String>,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub health: f32,
    pub max_health: f32,
    /// Armour points worn.
    pub armor: f64,
    /// A totem in either hand.
    pub totem_shown: bool,
    pub blocking: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    pub invisible: bool,
    pub gliding: bool,
    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub game_mode: i32,
    pub held: Option<String>,
    pub offhand: Option<String>,
    pub hostile: bool,
    pub passive: bool,
    pub width: f64,
    pub height: f64,
    pub eye_height: f64,
    /// Swung an arm this tick.
    pub swung: bool,
    /// Using an item (eating, drinking, drawing a bow).
    pub using: bool,
}

impl Seen {
    pub fn aabb(&self) -> crate::math::Aabb {
        let h = self.width.max(0.1) / 2.0;
        crate::math::Aabb::new(
            Vec3::new(self.pos.x - h, self.pos.y, self.pos.z - h),
            Vec3::new(self.pos.x + h, self.pos.y + self.height.max(0.1), self.pos.z + h),
        )
    }
    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::new(0.0, self.eye_height, 0.0)
    }
    pub fn holds(&self, id: &str) -> bool {
        self.held.as_deref().is_some_and(|h| h.ends_with(id))
    }
    pub fn invulnerable_mode(&self) -> bool {
        self.game_mode == 1 || self.game_mode == 3
    }
}

/// Something that happened to or near the bot since the last tick.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Hurt, by an entity (0 = none) for this much.
    Hurt { by: i32, amount: f32, kind: String },
    /// A sound a player would turn toward: sprinting steps, a hit, a totem, eating, a pearl.
    Heard { pos: Vec3, loud: bool, by: i32 },
    /// Someone's totem popped.
    Totem { entity: i32 },
    /// Water placed (an escape or an MLG) at a block.
    WaterPlaced { pos: [i32; 3], by: i32 },
    /// A swing of `entity` missed everyone (a whiff the bot can punish).
    Whiff { entity: i32 },
    /// The bot's own hit landed on `entity`.
    Landed { entity: i32 },
    /// An entity died.
    Died { entity: i32 },
}

/// The bot's own state as its client knows it.
#[derive(Clone, Debug, Default)]
pub struct SelfView {
    pub health: f32,
    pub absorption: f32,
    pub food: i32,
    pub saturation: f32,
    /// (effect id without namespace, amplifier, ticks left)
    pub effects: Vec<(String, u32, u32)>,
    pub inventory: Inventory,
    pub game_mode: i32,
    pub on_fire: bool,
    /// Item id -> ticks left on its cooldown.
    pub cooldowns: BTreeMap<String, u32>,
    pub dimension: String,
    pub dead: bool,
    pub invulnerable: bool,
}

impl SelfView {
    pub fn has_effect(&self, id: &str) -> bool {
        self.effects.iter().any(|(e, _, _)| e == id)
    }
    pub fn cooldown(&self, item: &str) -> u32 {
        self.cooldowns.get(&crate::items::full(item)).copied().unwrap_or(0)
    }
}

/// An item on the ground (after-fight pickups, Mending orbs).
#[derive(Clone, Debug, PartialEq)]
pub struct Ground {
    pub id: i32,
    pub item: String,
    pub count: u32,
    pub pos: Vec3,
}

/// What one bot perceives this tick.
pub struct Perception<'a> {
    pub tick: u64,
    pub me: &'a SelfView,
    pub others: Others<'a>,
    pub ground: &'a [Ground],
    pub events: &'a [Event],
    pub world: &'a dyn BlockView,
}

/// Everything else a bot perceives. Either a plain slice, or indices into one slice that all the
/// bots of a tick share, so no bot copies the others (hundreds of bots each cloning every player
/// and mob was the glue's biggest cost). The bot itself is never in it.
#[derive(Clone, Copy, Debug)]
pub struct Others<'a> {
    all: &'a [Seen],
    only: Option<&'a [u32]>,
}

impl<'a> Others<'a> {
    /// The shared slice and the indices of the ones this bot perceives.
    pub fn indexed(all: &'a [Seen], only: &'a [u32]) -> Self {
        Others { all, only: Some(only) }
    }
    pub fn iter(&self) -> OthersIter<'a> {
        match self.only {
            None => OthersIter::All(self.all.iter()),
            Some(only) => OthersIter::Only { all: self.all, it: only.iter() },
        }
    }
    pub fn len(&self) -> usize {
        self.only.map_or(self.all.len(), <[u32]>::len)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub enum OthersIter<'a> {
    All(std::slice::Iter<'a, Seen>),
    Only { all: &'a [Seen], it: std::slice::Iter<'a, u32> },
}

impl<'a> Iterator for OthersIter<'a> {
    type Item = &'a Seen;
    fn next(&mut self) -> Option<&'a Seen> {
        match self {
            OthersIter::All(it) => it.next(),
            OthersIter::Only { all, it } => it.next().map(|&i| &all[i as usize]),
        }
    }
}

impl<'a> IntoIterator for Others<'a> {
    type Item = &'a Seen;
    type IntoIter = OthersIter<'a>;
    fn into_iter(self) -> OthersIter<'a> {
        self.iter()
    }
}

impl<'a> From<&'a [Seen]> for Others<'a> {
    fn from(all: &'a [Seen]) -> Self {
        Others { all, only: None }
    }
}

impl<'a, const N: usize> From<&'a [Seen; N]> for Others<'a> {
    fn from(all: &'a [Seen; N]) -> Self {
        Others { all, only: None }
    }
}

impl<'a> From<&'a Vec<Seen>> for Others<'a> {
    fn from(all: &'a Vec<Seen>) -> Self {
        Others { all, only: None }
    }
}

impl Perception<'_> {
    pub fn get(&self, id: i32) -> Option<&Seen> {
        self.others.iter().find(|s| s.id == id)
    }
    pub fn by_name(&self, name: &str) -> Option<&Seen> {
        self.others.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Main,
    Off,
}

/// A click or key a player's client sends.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Attack(i32),
    Swing(Hand),
    SelectSlot(usize),
    /// Start using what is in the hand (eat, drink, draw, block, throw).
    Use(Hand),
    /// Use the hand on a block face (place, pour a bucket, open).
    UseOn { pos: [i32; 3], face: u8, hand: Hand },
    /// Right click on an entity (board a boat).
    Interact(i32),
    /// Let go of the item in use (shoot the bow, lower the shield).
    Release,
    /// Swap main hand and off hand (F).
    SwapHands,
    /// Move an inventory slot into the off hand (inventory open, button 40).
    ToOffhand(usize),
    /// Swap an inventory slot with a hotbar slot (inventory open, number key).
    ToHotbar { slot: usize, hotbar: usize },
    /// Swap two inventory slots (inventory open).
    SwapSlots(usize, usize),
    /// Put on the armour piece in a slot (shift-click it from the inventory).
    Wear(usize),
    /// Take off armour into the bag (hide with an invisibility potion).
    Unwear(crate::items::ArmorSlot),
    StartDig { pos: [i32; 3], face: u8 },
    StopDig { pos: [i32; 3], face: u8 },
    /// Throw the held stack (all) or one item.
    Drop { all: bool },
    /// Start gliding (an elytra in the air).
    Glide,
    Sneak(bool),
    Chat(String),
    /// Take what it is short of out of the opened ender chest or the box at `pos`.
    TakeFrom { pos: [i32; 3], ender: bool, wants: Vec<(String, u32)> },
    CloseContainer,
    /// A console command (waypoint commands, path events).
    Console(String),
    Respawn,
}

#[derive(Clone, Debug, Default)]
pub struct Decision {
    pub input: Input,
    pub yaw: f32,
    pub pitch: f32,
    pub actions: Vec<Action>,
    /// What part of the brain is in charge (debug view).
    pub mode: &'static str,
}

/// A bot's standing orders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Orders {
    /// `/horizonbot attack`, a faction war call, a tether turned hostile: by name.
    pub attack: Option<String>,
    pub follow: Option<Follow>,
    pub guard: Option<Guard>,
    pub retreat: Option<Retreat>,
    pub hide: bool,
    pub paused: bool,
    /// `bot stop`: won't even hit back.
    pub stopped: bool,
    pub patrol: Option<crate::paths::Patrol>,
    /// The patrol's current point, set by the plugin each tick.
    pub patrol_point: Option<Vec3>,
    pub march: Option<March>,
    /// Escorting a tethered target.
    pub tether: Option<TetherRole>,
    pub owner: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Follow {
    pub target: String,
    pub shape: formation::Shape,
    pub size: usize,
    /// This bot's place in the formation and how many share it.
    pub slot: usize,
    pub of: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Guard {
    pub post: Vec3,
    pub radius: f64,
    /// Its own part of the perimeter (0..1 around).
    pub sector: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RetreatTo {
    Scatter(Vec3),
    Player(String),
    Point(Vec3),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Retreat {
    pub to: RetreatTo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct March {
    pub to: MarchTo,
    pub shape: formation::Shape,
    pub size: usize,
    pub slot: usize,
    pub of: usize,
    pub then_attack: Option<String>,
    pub arrived: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MarchTo {
    Point(Vec3),
    Player(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TetherRole {
    pub target: String,
    pub leader: String,
    pub radius: f64,
}

/// What a bot is currently doing with its hands over several ticks.
#[derive(Clone, Debug, PartialEq)]
pub enum Busy {
    /// Eating or drinking: until this tick.
    Using { until: u64, what: &'static str },
    /// Drawing a bow since.
    Drawing { since: u64 },
    Blocking { since: u64 },
    /// A dig in progress.
    Digging { pos: [i32; 3], until: u64 },
    /// The water placed for a clutch, to pick back up.
    WaterOut { pos: [i32; 3], since: u64 },
}

/// Where an elytra-mace flight is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ElytraPhase {
    #[default]
    Ground,
    TakeOff { since: u64 },
    Climb { passes: u8 },
    Dive { passes: u8 },
}

/// The brain's memory of one fight.
#[derive(Clone, Debug, Default)]
pub struct FightMemory {
    pub target: Option<i32>,
    pub target_name: String,
    /// Ticks since the bot's last swing.
    pub since_swing: u32,
    /// Hits landed and taken in a row (tempo).
    pub landed_streak: u32,
    pub taken_streak: u32,
    /// The tick of the last hit taken and of the last hit landed; a streak is only a streak
    /// while these are recent (`STREAK_TICKS`).
    pub taken_at: u64,
    pub landed_at: u64,
    /// Read of the opponent: how often they block, jump-reset, pearl.
    pub reads_block: f64,
    pub reads_crits: f64,
    pub reads_pearl: f64,
    /// Ticks the opponent's shield is known to be disabled.
    pub shield_down_until: u64,
    /// A whiff seen to punish, until this tick.
    pub punish_until: u64,
    /// The opening walk-in is done (first hit traded or close).
    pub opened: bool,
    /// For this fight: keep a totem in the off hand against a mace.
    pub mace_totem: Option<bool>,
    pub last_dist: f64,
    pub growing_gap: u32,
    pub strafe_sign: f64,
    pub strafe_switch: u64,
    /// The decided weapon for the mace dial, until this tick.
    pub mace_mode_until: u64,
    pub mace_mode: bool,
    /// The target showed a totem last tick (a pop is a totem gone at near-zero health).
    pub last_totem: bool,
    /// The target's totem popped at this tick.
    pub popped_at: Option<u64>,
    /// The axe stays out for their raised shield until this tick.
    pub axe_until: u64,
}

/// A bot.
pub struct Bot {
    pub name: String,
    pub uuid: u128,
    /// Its network entity id on the server (0 until it is placed).
    pub entity_id: i32,
    pub rng: Rng,
    pub body: Body,
    pub settings: Snapshot,
    pub personality: Personality,
    pub playstyle: Playstyle,
    pub orders: Orders,
    pub faction: Option<String>,
    pub spawned_tick: u64,
    pub fight: FightMemory,
    /// The item id it meant to hold in melee last tick: a different one restarts the swing meter.
    pub last_held: Option<String>,
    /// The item id really in the hand last tick: a switch outside melee (food, a pearl) restarts the meter too.
    pub seen_held: Option<String>,
    pub busy: Option<Busy>,
    /// Entities noticed: id -> (first seen tick, attack allowed from tick).
    pub noticed: BTreeMap<i32, (u64, u64)>,
    /// Entities known (seen recently): id -> last tick known.
    pub known: BTreeMap<i32, u64>,
    /// Who hurt it last and until when that counts (revenge).
    pub revenge: Option<(i32, u64)>,
    /// Actions waiting out the simulated ping: (due tick, action).
    pub outbox: VecDeque<(u64, Action)>,
    /// Clutch actions locked out by a panic, until.
    pub lockout: BTreeMap<&'static str, u64>,
    /// Cooldowns of its own tricks: name -> ready at tick.
    pub ready_at: BTreeMap<&'static str, u64>,
    /// Totem move scheduled at this tick.
    pub totem_at: Option<u64>,
    pub path: Vec<nav::Pos>,
    pub path_goal: Option<nav::Pos>,
    pub path_age: u64,
    /// Tick of the last path search (any outcome), for the minimum interval between searches.
    pub last_search: u64,
    /// Search nodes this bot may still expand this tick (the squad's shared budget), and how many it used.
    pub nav_left: usize,
    pub nav_used: usize,
    /// Tick since which a due search waits for the shared budget (0 = not waiting).
    pub nav_waiting_since: u64,
    /// What the squad's hard cap still allows this tick (a starved bot's small search is paid from it too).
    pub nav_hard_left: usize,
    /// No new search for the same unreachable goal before this tick (a search that failed or ran its whole
    /// budget without arriving), unless the goal has moved far.
    pub search_backoff_until: u64,
    /// The last straight-walk check: (feet, goal block, tick, result). It costs a block read per sample along
    /// the line, so it is reused for a few ticks while the bot and its goal stay about where they were.
    pub direct_cache: Option<(nav::Pos, nav::Pos, u64, bool)>,
    /// Line of sight to others as last looked up: id -> (tick, seen). Reused for a few ticks.
    pub sight_cache: std::cell::RefCell<BTreeMap<i32, SightEntry>>,
    /// The last search found no way to `path_goal`: not searched again until it is stale (a search
    /// that fails walks its whole budget, and with nothing to follow one ran every tick).
    pub path_failed: bool,
    /// Blocks placed or broken in the last minute (building budget).
    pub built: VecDeque<u64>,
    /// Target positions seen, for tracking delay: id -> recent positions.
    pub history: BTreeMap<i32, VecDeque<Vec3>>,
    /// The kit it was given (regear target).
    pub kit: Option<String>,
    pub last_mode: &'static str,
    /// The latest call worth showing ("whiffed", "their shield is down: all out").
    pub last_call: String,
    pub debug_view: bool,
    pub hidden_with_potion: bool,
    pub ghost: bool,
    /// The bot's position last tick (stuck detection).
    pub last_pos: Vec3,
    pub stuck_ticks: u32,
    pub wander_goal: Option<Vec3>,
    /// Set when the patrol point is reached (the plugin advances the route and clears it).
    pub patrol_arrived: bool,
    /// Enemy water to scoop up: (not before tick, block).
    pub drain_at: Option<(u64, [i32; 3])>,
    pub elytra: ElytraPhase,
    /// Lava placed at an enemy's feet: (tick, block) to take back.
    pub lava_out: Option<(u64, [i32; 3])>,
    /// What it restocks to (from its kit), set by the plugin.
    pub restock_targets: Option<std::collections::BTreeMap<String, u32>>,
    pub regear: Regear,
    /// A new target it will turn on at this tick (reaction ripple).
    pub ripple: Option<(i32, u64)>,
    /// Its aim error (yaw, pitch) in units of the aim knob's spread, drifting from tick to tick
    /// the way a hand does, not a fresh dice roll every tick.
    pub aim_err: [f32; 2],
}

/// A line of sight as looked up: (tick, seen, eye of the bot, eye of the other).
pub type SightEntry = (u64, bool, Vec3, Vec3);

/// A streak of hits taken or landed is forgotten after this many quiet ticks.
pub const STREAK_TICKS: u64 = 60;

/// A regear in progress (`regear.rs`).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Regear {
    #[default]
    Idle,
    /// Backing off / pearling away from the fight.
    Leaving { since: u64 },
    /// The chest or box is down at this block.
    Placed { pos: [i32; 3], since: u64, ender: bool },
    /// Taking what it is short of, until.
    Taking { pos: [i32; 3], until: u64, ender: bool },
    /// Mining it back up, until.
    Mining { pos: [i32; 3], until: u64 },
}

/// Minimum ticks between two path searches of one bot (with a path in hand / without one), plus up
/// to `NAV_SPACING_JITTER - 1` more ticks by uuid.
pub const NAV_SPACING: u64 = 10;
pub const NAV_SPACING_EMPTY: u64 = 4;
pub const NAV_SPACING_JITTER: u64 = 6;
/// A search is not started with less than this many nodes left in the tick's shared budget.
pub const NAV_MIN_BUDGET: usize = 300;
/// A bot whose search waited this long for the shared budget searches anyway (a small search), so the
/// bots late in the tick's order are not starved while the early ones use the budget up.
pub const NAV_STARVED_TICKS: u64 = 20;
pub const NAV_STARVED_BUDGET: usize = 800;
/// Nodes one search may expand: a first search toward a new goal, and a repeat toward about the same one.
pub const NAV_FIRST_BUDGET: usize = 2500;
pub const NAV_REPEAT_BUDGET: usize = 1000;

impl Bot {
    pub fn new(name: &str, uuid: u128, at: Vec3, yaw: f32, settings: Snapshot, tick: u64) -> Bot {
        let skill = settings.f("skill");
        let personality = if settings.b("personality") {
            Personality::from_name(name, skill)
        } else {
            Personality::plain()
        };
        let playstyle = Playstyle::parse(&settings.s("playstyle"));
        let mut body = Body::new(at);
        body.yaw = yaw;
        Bot {
            name: name.to_owned(),
            uuid,
            entity_id: 0,
            rng: Rng::new(crate::math::name_hash(name) ^ tick),
            body,
            settings,
            personality,
            playstyle,
            orders: Orders::default(),
            faction: None,
            spawned_tick: tick,
            fight: FightMemory { strafe_sign: 1.0, ..Default::default() },
            last_held: None,
            seen_held: None,
            busy: None,
            noticed: BTreeMap::new(),
            known: BTreeMap::new(),
            revenge: None,
            outbox: VecDeque::new(),
            lockout: BTreeMap::new(),
            ready_at: BTreeMap::new(),
            totem_at: None,
            path: Vec::new(),
            path_goal: None,
            path_age: 0,
            last_search: 0,
            nav_left: usize::MAX,
            nav_used: 0,
            nav_waiting_since: 0,
            nav_hard_left: usize::MAX,
            search_backoff_until: 0,
            direct_cache: None,
            sight_cache: Default::default(),
            path_failed: false,
            built: VecDeque::new(),
            history: BTreeMap::new(),
            kit: None,
            last_mode: "idle",
            last_call: String::new(),
            debug_view: false,
            hidden_with_potion: false,
            ghost: false,
            last_pos: at,
            stuck_ticks: 0,
            wander_goal: None,
            patrol_arrived: false,
            drain_at: None,
            elytra: ElytraPhase::Ground,
            lava_out: None,
            restock_targets: None,
            regear: Regear::Idle,
            ripple: None,
            aim_err: [0.0; 2],
        }
    }

    pub fn ready(&self, what: &'static str, tick: u64) -> bool {
        self.ready_at.get(what).is_none_or(|t| tick >= *t)
    }
    pub fn cool(&mut self, what: &'static str, tick: u64, ticks: u64) {
        self.ready_at.insert(what, tick + ticks);
    }
    pub fn locked(&self, what: &'static str, tick: u64) -> bool {
        self.lockout.get(what).is_some_and(|t| tick < *t)
    }

    /// The reaction delay in ticks for something new, from the reaction knob (a good player
    /// 150-250 ms, a poor one far more), with a little noise.
    pub fn reaction_ticks(&mut self) -> u64 {
        let k = self.settings.knob("reaction");
        let base = 14.0 - 10.5 * k;
        (base + self.rng.signed() * 1.5).round().max(2.0) as u64
    }

    /// Rolls a clutch action: Ok(true) do it now, Ok(false) hesitate this tick, Err = panicked
    /// (locked out for a moment).
    pub fn clutch_roll(&mut self, what: &'static str, tick: u64) -> bool {
        if self.locked(what, tick) {
            return false;
        }
        let p = self.settings.pct("mistake-chance");
        if self.rng.chance(p) {
            if self.rng.chance(0.3) {
                let t = 20 + self.rng.int(0, 40) as u64;
                self.lockout.insert(what, tick + t);
                self.last_call = format!("panicked: no {what} for a moment");
            }
            return false;
        }
        true
    }

    fn eye(&self) -> Vec3 {
        self.body.eye()
    }

    /// Turns toward a point at the turn-speed limit, with the aim knob's error. The error is a
    /// correlated drift (a hand wanders), so the crosshair does not shake from tick to tick.
    pub fn look_at(&mut self, d: &mut Decision, point: Vec3) {
        let max = self.settings.f("turn-speed") as f32;
        let aim = self.settings.knob("aim");
        let (ty, tp) = (yaw_to(self.eye(), point), pitch_to(self.eye(), point));
        let wobble = ((1.0 - aim) * 6.0) as f32;
        const KEEP: f32 = 0.8;
        let fresh = (1.0 - KEEP * KEEP).sqrt();
        let (gy, gp) = (self.rng.gauss() as f32, self.rng.gauss() as f32);
        self.aim_err[0] = self.aim_err[0] * KEEP + gy * fresh;
        self.aim_err[1] = self.aim_err[1] * KEEP + gp * fresh;
        let ty = ty + self.aim_err[0] * wobble * 0.5;
        let tp = tp + self.aim_err[1] * wobble * 0.3;
        d.yaw = approach_angle(d.yaw, ty, max);
        d.pitch = (d.pitch + (tp - d.pitch).clamp(-max, max)).clamp(-90.0, 90.0);
    }

    /// Runs toward a point: turns at the turn-speed limit first, and works the keys out from the
    /// yaw it will really have, so a bot running away runs away (not backwards into them).
    pub fn flee_to(&mut self, p: &Perception, d: &mut Decision, away: Vec3, sprint: bool) {
        let max = self.settings.f("turn-speed") as f32;
        d.yaw = approach_angle(d.yaw, yaw_to(self.body.pos, away), max);
        d.pitch = approach_angle(d.pitch, 0.0, max);
        self.walk_to(p, d, away, sprint);
    }

    /// Whether the crosshair is on an entity's box right now (aim-hits).
    pub fn crosshair_on(&self, d: &Decision, target: &Seen, reach: f64) -> bool {
        let eye = self.eye();
        let dir = look_vector(d.yaw, d.pitch);
        let b = target.aabb().inflate(0.1);
        b.ray(eye, eye + dir * (reach + 0.5)).is_some_and(|t| t * (reach + 0.5) <= reach + 0.05)
    }

    /// Distance from the bot's eyes to the nearest point of the target's box (vanilla's reach).
    pub fn reach_to(&self, target: &Seen) -> f64 {
        target.aabb().distance_to(self.eye())
    }

    /// Where the bot believes the target is: `tracking` ticks late.
    pub fn perceived(&mut self, t: &Seen) -> Vec3 {
        let delay = ((1.0 - self.settings.knob("tracking")) * 5.0).round() as usize + 1;
        let h = self.history.entry(t.id).or_default();
        h.push_back(t.pos);
        while h.len() > 12 {
            h.pop_front();
        }
        let i = h.len().saturating_sub(delay);
        h.get(i).copied().unwrap_or(t.pos)
    }

    /// Queues an action behind the simulated ping.
    pub fn send_late(&mut self, tick: u64, a: Action) {
        let ping = (self.settings.f("ping-millis") / 50.0).round() as u64;
        self.outbox.push_back((tick + ping, a));
    }

    /// Movement toward a point (keys relative to where the bot looks), with pathfinding when the
    /// straight line is blocked.
    pub fn walk_to(&mut self, p: &Perception, d: &mut Decision, goal: Vec3, sprint: bool) {
        let t_walk = std::time::Instant::now();
        self.walk_to_inner(p, d, goal, sprint);
        prof_add(P_WALK, t_walk.elapsed().as_nanos() as u64);
    }

    fn walk_to_inner(&mut self, p: &Perception, d: &mut Decision, goal: Vec3, sprint: bool) {
        let here = self.body.pos;
        let mut aim_point = goal;
        let feet = here.block();
        let goal_b = goal.block();
        let direct = (goal.y - here.y).abs() < 1.2 && {
            let far = here.horizontal_distance(goal) > 4.0;
            match self.direct_cache {
                // A long line was checked a moment ago from about here to about there: the same answer.
                Some((f, g, t, r))
                    if far && p.tick < t + 4 && f[1] == feet[1] && (f[0] - feet[0]).abs() + (f[2] - feet[2]).abs() <= 1 && (g[0] - goal_b[0]).abs() + (g[2] - goal_b[2]).abs() <= 1 =>
                {
                    r
                }
                _ => {
                    let t_direct = std::time::Instant::now();
                    let r = nav::straight_walkable(p.world, feet, [goal_b[0], feet[1], goal_b[2]]);
                    prof_add(P_DIRECT, t_direct.elapsed().as_nanos() as u64);
                    prof_add(P_N_DIRECT, 1);
                    if far {
                        self.direct_cache = Some((feet, goal_b, p.tick, r));
                    }
                    r
                }
            }
        };
        if !direct && here.horizontal_distance(goal) > 1.5 {
            // The goal moved too far for the old path to still lead there. "Too far" grows with the
            // distance: a target 40 blocks away that stepped 3 blocks is still where the path points.
            let reach_here = here.horizontal_distance(goal);
            let tolerance = ((reach_here / 5.0) as i32).clamp(2, 10);
            let moved_beyond = |tol: i32| {
                self.path_goal.is_none_or(|g| {
                    let dg = [g[0] - goal_b[0], g[1] - goal_b[1], g[2] - goal_b[2]];
                    dg[0].abs() + dg[1].abs() + dg[2].abs() > tol
                })
            };
            let stale = moved_beyond(tolerance);
            // A search that found nothing is not repeated soon, and then with a smaller budget: it
            // walks the whole budget (thousands of block reads) each time. Swimming bots search not
            // at all (no cell in water is standable); they swim straight.
            let retry_after = if self.path_failed { 120 } else { 40 };
            // Searches of one bot are spaced apart, each bot on its own phase (so they don't all land on
            // the same tick). With nothing to follow the wait is short.
            let spacing = if self.path.is_empty() { NAV_SPACING_EMPTY } else { NAV_SPACING };
            let due = self.last_search == 0 || p.tick >= self.last_search + spacing + (self.uuid % NAV_SPACING_JITTER as u128) as u64;
            // While the last search for this goal failed or ran its whole budget (`search_backoff_until`), only a
            // goal that moved three times as far wakes it: a moving target no path leads to (on a pillar, over a
            // wall) made every bot search again every 10-15 ticks and find nothing again.
            let wants = if p.tick < self.search_backoff_until {
                moved_beyond(tolerance * 3)
            } else {
                stale || p.tick > self.path_age + retry_after || (self.path.is_empty() && !self.path_failed)
            };
            let ready = !self.body.in_water && wants && due;
            let starved = ready && self.nav_waiting_since > 0 && p.tick >= self.nav_waiting_since + NAV_STARVED_TICKS && self.nav_hard_left >= NAV_STARVED_BUDGET;
            if ready && self.nav_left < NAV_MIN_BUDGET && !starved {
                if self.nav_waiting_since == 0 {
                    self.nav_waiting_since = p.tick.max(1);
                }
            }
            if ready && (self.nav_left >= NAV_MIN_BUDGET || starved) {
                self.nav_waiting_since = 0;
                // A first search toward a goal may walk a long way round (NAV_FIRST_BUDGET); a repeat toward the same
                // place (the old path's goal is still within the tolerance, so only the target's walk or the retry
                // timer asked) only has to repair the route: a smaller budget, it normally ends within a few hundred nodes.
                // (After a hopeless partial route the next search is a first one again, so a long detour goes on a stretch at a time.)
                let repeat = !self.path_failed && self.path_goal.is_some() && !stale && self.search_backoff_until == 0;
                let budget = if starved && self.nav_left < NAV_STARVED_BUDGET {
                    NAV_STARVED_BUDGET
                } else {
                    (if self.path_failed { 800 } else if repeat { NAV_REPEAT_BUDGET } else { NAV_FIRST_BUDGET }).min(self.nav_left)
                };
                let t_search = std::time::Instant::now();
                let (r, used) = nav::find_path_counted(p.world, feet, goal_b, 1.5, budget);
                prof_add(P_SEARCH, t_search.elapsed().as_nanos() as u64);
                prof_add(P_N_SEARCH, 1);
                prof_add(P_N_NODES, used as u64);
                if starved {
                    prof_add(P_N_STARVED, 1);
                }
                if repeat {
                    prof_add(P_N_REPEAT, 1);
                }
                self.nav_left = self.nav_left.saturating_sub(used);
                self.nav_hard_left = self.nav_hard_left.saturating_sub(used);
                self.nav_used += used;
                // Ran the whole (not squeezed) budget and did not arrive: the goal is out of reach or far round.
                let hopeless = used > budget && budget >= 800;
                let t_smooth = std::time::Instant::now();
                let found = r.map(|r| nav::smooth(p.world, &r));
                prof_add(P_SMOOTH, t_smooth.elapsed().as_nanos() as u64);
                self.path_failed = found.is_none();
                if found.is_none() {
                    self.search_backoff_until = p.tick + 60;
                    prof_add(P_N_FAILED, 1);
                } else if hopeless {
                    // The partial route toward the closest point is still followed; the next search from there waits.
                    self.search_backoff_until = p.tick + 40;
                    prof_add(P_N_HOPELESS, 1);
                } else {
                    self.search_backoff_until = 0;
                }
                self.path = found.unwrap_or_default();
                self.path_goal = Some(goal_b);
                self.path_age = p.tick;
                self.last_search = p.tick.max(1);
            }
            while let Some(next) = self.path.first() {
                let c = Vec3::new(next[0] as f64 + 0.5, next[1] as f64, next[2] as f64 + 0.5);
                if c.horizontal_distance(here) < 0.6 && (c.y - here.y).abs() < 1.1 {
                    self.path.remove(0);
                } else {
                    break;
                }
            }
            if let Some(next) = self.path.first() {
                aim_point = Vec3::new(next[0] as f64 + 0.5, next[1] as f64, next[2] as f64 + 0.5);
            }
        } else {
            self.path.clear();
        }
        let dir = aim_point - here;
        let (f, s) = nav::keys_toward(d.yaw, dir);
        d.input.forward = f;
        d.input.strafe = s;
        let speed = self.settings.f("speed");
        d.input.sprint = sprint && f > 0.0 && speed >= 1.0;
        // Jump up a step the walk can't take, or when stuck against something.
        if self.body.horizontal_collision && self.body.on_ground && aim_point.y >= here.y - 0.1 {
            d.input.jump = true;
        }
        if self.stuck_ticks > 12 && self.body.on_ground && here.horizontal_distance(goal) > 1.0 {
            d.input.jump = true;
            self.path.clear();
        }
        // Bunny hops on long straight stretches, each bot on its own rhythm.
        if self.settings.b("bhop") && direct && here.horizontal_distance(goal) > 8.0 && d.input.sprint {
            let phase = (p.tick as f64 / 10.0 + self.personality.hop_phase).fract();
            if phase < 0.5 {
                d.input.jump = true;
            }
        }
        // Speed below 1 walks instead of sprinting, slower still under 0.5.
        if speed < 0.5 && self.rng.chance(1.0 - speed * 2.0) {
            d.input.forward *= 0.0;
        }
    }

    /// Building: bridge, pillar or dig when the route is blocked. Fires through the normal block
    /// actions, so the server's protection still decides.
    pub fn build_if_blocked(&mut self, p: &Perception, d: &mut Decision, goal: Vec3, extra: &[String]) -> bool {
        let s = &self.settings;
        if !s.b("building") || !self.path.is_empty() {
            return false;
        }
        let tick = p.tick;
        if !self.ready("build", tick) {
            return false;
        }
        while self.built.front().is_some_and(|t| tick > *t + 1200) {
            self.built.pop_front();
        }
        if self.built.len() as i64 >= s.i("building-budget") {
            return false;
        }
        let here = self.body.pos;
        let feet = here.block();
        let dir = (goal - here).horizontal();
        let (dx, dz) = if dir.x.abs() > dir.z.abs() {
            (dir.x.signum() as i32, 0)
        } else {
            (0, dir.z.signum() as i32)
        };
        let inv = &p.me.inventory;
        let block_slot = inv.find(true, |st| crate::items::is_building_block(&st.id));
        let cooldown = s.i("building-cooldown").max(0) as u64;
        // Pillar up toward a target above and out of reach.
        if s.b("building-pillar") && goal.y - here.y > 2.5 && here.horizontal_distance(goal) < 3.0 && self.body.on_ground {
            let pillared = self.built.len() as i64;
            if let Some(slot) = block_slot {
                if pillared < s.i("building-max-pillar") {
                    d.actions.push(Action::SelectSlot(slot));
                    d.pitch = 90.0;
                    d.input.jump = true;
                    let under = [feet[0], feet[1] - 1, feet[2]];
                    d.actions.push(Action::UseOn { pos: under, face: 1, hand: Hand::Main });
                    self.built.push_back(tick);
                    self.cool("build", tick, cooldown.max(8));
                    d.mode = "pillar";
                    return true;
                }
            }
        }
        if s.b("building-clear-soft") {
            if let Some(nav::BuildStep::Dig { pos }) = nav::dig_step(p.world, feet, dx, dz, extra) {
                d.actions.push(Action::StartDig { pos, face: face_toward(dx, dz) });
                self.busy = Some(Busy::Digging { pos, until: tick + 10 });
                self.built.push_back(tick);
                self.cool("build", tick, cooldown);
                d.mode = "dig";
                return true;
            }
        }
        if s.b("building-bridge") {
            if let Some(nav::BuildStep::Place { pos, against }) = nav::bridge_step(p.world, feet, dx, dz) {
                if let Some(slot) = block_slot {
                    d.actions.push(Action::SelectSlot(slot));
                    d.input.sneak = true;
                    d.input.forward = 0.0;
                    let face = face_from(against, pos);
                    // Look at the side of the block we stand on, as a bridger does.
                    let target = Vec3::new(against[0] as f64 + 0.5 + dx as f64 * 0.5, against[1] as f64 + 0.5, against[2] as f64 + 0.5 + dz as f64 * 0.5);
                    self.look_at(d, target);
                    d.actions.push(Action::UseOn { pos: against, face, hand: Hand::Main });
                    self.built.push_back(tick);
                    self.cool("build", tick, cooldown);
                    d.mode = "bridge";
                    return true;
                } else if p.me.inventory.has("wind_charge") && self.ready("wc-bridge", tick) {
                    // No blocks: a wind charge at its feet to jump the gap.
                    if let Some(slot) = p.me.inventory.find(true, |st| st.is("wind_charge")) {
                        d.actions.push(Action::SelectSlot(slot));
                        d.pitch = 80.0;
                        d.input.jump = true;
                        d.actions.push(Action::Use(Hand::Main));
                        self.cool("wc-bridge", tick, 30);
                        return true;
                    }
                }
            }
        }
        false
    }

    /// One tick of the brain. `squad` is what the bots share.
    pub fn tick(&mut self, p: &Perception, squad: &mut squad::Squad) -> Decision {
        self.nav_left = squad::NAV_BUDGET_PER_TICK.saturating_sub(squad.nav_spent) as usize;
        self.nav_hard_left = squad::NAV_HARD_CAP_PER_TICK.saturating_sub(squad.nav_spent) as usize;
        self.nav_used = 0;
        prof_add(P_N_BOTS, 1);
        let d = self.tick_inner(p, squad);
        squad.nav_spent = squad.nav_spent.saturating_add(self.nav_used as u32);
        d
    }

    fn tick_inner(&mut self, p: &Perception, squad: &mut squad::Squad) -> Decision {
        let mut d = Decision {
            yaw: self.body.yaw,
            pitch: self.body.pitch,
            mode: "idle",
            ..Default::default()
        };
        if p.me.dead {
            d.actions.push(Action::Respawn);
            d.mode = "dead";
            return self.finish(p, d);
        }
        self.stuck_ticks = if self.body.pos.horizontal_distance(self.last_pos) < 0.02 { self.stuck_ticks + 1 } else { 0 };
        self.last_pos = self.body.pos;
        let mut t = std::time::Instant::now();
        self.take_events(p);
        lap(&mut t, P_EVENTS);
        if self.orders.paused || self.ghost {
            d.mode = if self.ghost { "ghost" } else { "paused" };
            return self.finish(p, d);
        }
        let now = p.me.inventory.held().map_or("", |s| s.id.as_str());
        if self.seen_held.as_deref() != Some(now) {
            // Melee already restarted the meter for a swap it chose (last_held); any other switch restarts it here.
            if self.seen_held.is_some() && self.last_held.as_deref() != Some(now) {
                self.fight.since_swing = 0;
                self.last_held = Some(now.to_owned());
            }
            self.seen_held = Some(now.to_owned());
        }
        self.fight.since_swing = self.fight.since_swing.saturating_add(1);

        // Survival first: the clutch moves win over everything.
        if survival::clutch(self, p, &mut d) {
            lap(&mut t, P_SURV);
            return self.finish(p, d);
        }
        if survival::web_escape(self, p, &mut d) || team::chorus_escape(self, p, &mut d) || team::fire_escape(self, p, &mut d) {
            lap(&mut t, P_SURV);
            return self.finish(p, d);
        }
        survival::totem(self, p, &mut d);
        if survival::continue_busy(self, p, &mut d) {
            lap(&mut t, P_SURV);
            return self.finish(p, d);
        }
        survival::auto_equip(self, p, &mut d);
        lap(&mut t, P_SURV);

        let t_target = std::time::Instant::now();
        let target = combat::choose_target(self, p, squad);
        prof_add(P_TARGET, t_target.elapsed().as_nanos() as u64);
        t = std::time::Instant::now();
        if survival::heal(self, p, &mut d, target.as_ref()) {
            lap(&mut t, P_HEAL);
            return self.finish(p, d);
        }
        if survival::eat(self, p, &mut d, target.as_ref()) {
            lap(&mut t, P_HEAL);
            return self.finish(p, d);
        }
        if survival::drain(self, p, &mut d) || team::heal_ally(self, p, &mut d, squad) {
            lap(&mut t, P_HEAL);
            return self.finish(p, d);
        }
        lap(&mut t, P_HEAL);
        if regear::regear(self, p, &mut d, target.as_ref(), squad) {
            lap(&mut t, P_REGEAR);
            return self.finish(p, d);
        }
        lap(&mut t, P_REGEAR);

        if survival::hide(self, p, &mut d) {
            lap(&mut t, P_ORDERS);
            return self.finish(p, d);
        }
        // Orders that outrank fighting.
        if let Some(r) = self.orders.retreat.clone() {
            if combat::retreat(self, p, &mut d, &r, target.as_ref()) {
                lap(&mut t, P_ORDERS);
                return self.finish(p, d);
            }
        }
        if self.orders.tether.is_some() && combat::tether(self, p, &mut d, squad) {
            lap(&mut t, P_ORDERS);
            return self.finish(p, d);
        }
        lap(&mut t, P_ORDERS);
        if let Some(tg) = target {
            combat::fight(self, p, &mut d, &tg, squad);
            lap(&mut t, P_FIGHT);
            return self.finish(p, d);
        }
        if matches!(self.busy, Some(Busy::Drawing { .. })) {
            // The target is gone: don't stand there with the bow drawn.
            d.actions.push(Action::Release);
            self.busy = None;
        }
        self.fight = FightMemory { strafe_sign: self.fight.strafe_sign, since_swing: self.fight.since_swing, ..Default::default() };

        if survival::buff(self, p, &mut d) || team::repair(self, p, &mut d) {
            lap(&mut t, P_POSITIONAL);
            return self.finish(p, d);
        }
        if combat::positional_orders(self, p, &mut d) {
            lap(&mut t, P_POSITIONAL);
            return self.finish(p, d);
        }
        if survival::after_fight(self, p, &mut d) {
            lap(&mut t, P_POSITIONAL);
            return self.finish(p, d);
        }
        lap(&mut t, P_POSITIONAL);
        combat::idle(self, p, &mut d);
        lap(&mut t, P_IDLE);
        self.finish(p, d)
    }

    fn take_events(&mut self, p: &Perception) {
        let tick = p.tick;
        for e in p.events {
            match e {
                // Only an entity hurts back (fall, fire and the void have no id): they are no one
                // to take revenge on and no part of a fight's tempo.
                Event::Hurt { by, .. } if *by > 0 => {
                    self.revenge = Some((*by, tick + 600));
                    self.known.insert(*by, tick);
                    self.fight.taken_streak += 1;
                    self.fight.taken_at = tick;
                    self.fight.landed_streak = 0;
                }
                Event::Heard { by, pos, .. } => {
                    let range = self.settings.f("hearing");
                    if *by != 0 && range > 0.0 && pos.distance(self.body.pos) <= range {
                        self.known.insert(*by, tick);
                    }
                }
                Event::Landed { .. } => {
                    self.fight.landed_streak += 1;
                    self.fight.landed_at = tick;
                    self.fight.taken_streak = 0;
                    self.fight.opened = true;
                }
                Event::Whiff { entity } => {
                    if self.fight.target == Some(*entity) && self.settings.b("whiff-punish") {
                        self.fight.punish_until = tick + 6;
                    }
                }
                Event::Died { entity } => {
                    if self.fight.target == Some(*entity) {
                        self.fight.target = None;
                    }
                    self.noticed.remove(entity);
                    self.known.remove(entity);
                    if self.revenge.is_some_and(|(r, _)| r == *entity) {
                        self.revenge = None;
                    }
                }
                _ => {}
            }
        }
        if self.revenge.is_some_and(|(_, until)| tick > until) {
            self.revenge = None;
        }
        // Streaks are of hits in a row: a quiet spell ends them.
        if tick > self.fight.taken_at + STREAK_TICKS {
            self.fight.taken_streak = 0;
        }
        if tick > self.fight.landed_at + STREAK_TICKS {
            self.fight.landed_streak = 0;
        }
        let memory = self.settings.i("detection-memory-ticks").max(0) as u64;
        self.known.retain(|_, t| tick <= *t + memory);
    }

    /// Runs the physics with the decided keys and releases actions whose ping has passed.
    fn finish(&mut self, p: &Perception, mut d: Decision) -> Decision {
        let tick = p.tick;
        let t_finish = std::time::Instant::now();
        // Hiding without a potion: crouched the whole time.
        if self.orders.hide && !self.hidden_with_potion && !matches!(d.mode, "fight") {
            d.input.sneak = true;
            d.input.sprint = false;
        }
        // Food: no sprinting at 6 or less, as vanilla.
        if p.me.food <= 6 {
            d.input.sprint = false;
        }
        // In water the bot swims up (hold jump), unless it sneaks to dive: it would sink and drown.
        if self.body.in_water && !d.input.sneak {
            d.input.jump = true;
        }
        self.body.yaw = d.yaw;
        self.body.pitch = d.pitch;
        self.body.speed_attr = crate::physics::WALK_SPEED
            * (1.0 + 0.2 * effect_level(p.me, "speed") as f64)
            * (1.0 - 0.15 * effect_level(p.me, "slowness") as f64).max(0.0);
        self.body.jump_boost = effect_level(p.me, "jump_boost");
        self.body.slow_falling = p.me.has_effect("slow_falling");
        self.body.levitation = effect_level(p.me, "levitation");
        if matches!(self.busy, Some(Busy::Using { .. })) {
            // Eating or drinking: no sprinting, slowed to a walk.
            d.input.sprint = false;
            d.input.forward *= 0.2;
            d.input.strafe *= 0.2;
        }
        let t_body = std::time::Instant::now();
        self.body.tick(&d.input, p.world);
        prof_add(P_BODY, t_body.elapsed().as_nanos() as u64);
        let mut late = Vec::new();
        while self.outbox.front().is_some_and(|(due, _)| *due <= tick) {
            late.push(self.outbox.pop_front().unwrap().1);
        }
        // Delayed attacks go out after this tick's slot changes.
        d.actions.extend(late);
        self.last_mode = d.mode;
        prof_add(P_FINISH, t_finish.elapsed().as_nanos() as u64);
        d
    }
}

/// Profiling (read and reset by the glue once a tick through `prof_take`): nanoseconds per section of one
/// bot's tick, summed over all bots (`P_BODY`..`P_EVENTS`; `P_WALK` contains `P_SEARCH`, `P_SMOOTH` and
/// `P_DIRECT`, and `P_FIGHT`, `P_ORDERS`, `P_IDLE` contain walks and sight lines), then plain counts.
pub const P_BODY: usize = 0;
pub const P_TARGET: usize = 1;
pub const P_SURV: usize = 2;
pub const P_HEAL: usize = 3;
pub const P_REGEAR: usize = 4;
pub const P_ORDERS: usize = 5;
pub const P_FIGHT: usize = 6;
pub const P_POSITIONAL: usize = 7;
pub const P_IDLE: usize = 8;
pub const P_FINISH: usize = 9;
pub const P_WALK: usize = 10;
pub const P_SEARCH: usize = 11;
pub const P_SMOOTH: usize = 12;
pub const P_SIGHT: usize = 13;
pub const P_DIRECT: usize = 14;
pub const P_EVENTS: usize = 15;
pub const P_N_SEARCH: usize = 16;
pub const P_N_NODES: usize = 17;
pub const P_N_FAILED: usize = 18;
pub const P_N_HOPELESS: usize = 19;
pub const P_N_STARVED: usize = 20;
pub const P_N_REPEAT: usize = 21;
pub const P_N_BOTS: usize = 22;
pub const P_N_DIRECT: usize = 23;
/// Per-bot totals the glue measures around the plugin's `think` (everything a bot's brain tick costs, so
/// `P_GLUE_BRAIN` minus the sections above is what is not in any of them), around its events lookup, and
/// around its engine move and its packets (`act`).
pub const P_GLUE_BRAIN: usize = 24;
pub const P_GLUE_EVENTS: usize = 25;
pub const P_GLUE_ENGINE: usize = 26;
pub const P_GLUE_ACT: usize = 27;
const PROF_LEN: usize = 28;
pub static PROF_NS: [std::sync::atomic::AtomicU64; PROF_LEN] = [const { std::sync::atomic::AtomicU64::new(0) }; PROF_LEN];
const PROF_NAMES: [&str; PROF_LEN] = [
    "horizonbot_body_us",
    "horizonbot_target_us",
    "horizonbot_surv_us",
    "horizonbot_heal_us",
    "horizonbot_regear_us",
    "horizonbot_orders_us",
    "horizonbot_fight_us",
    "horizonbot_positional_us",
    "horizonbot_idle_us",
    "horizonbot_finish_us",
    "horizonbot_walk_us",
    "horizonbot_search_us",
    "horizonbot_smooth_us",
    "horizonbot_sight_us",
    "horizonbot_direct_us",
    "horizonbot_takeevents_us",
    "horizonbot_n_searches",
    "horizonbot_n_search_nodes",
    "horizonbot_n_search_failed",
    "horizonbot_n_search_hopeless",
    "horizonbot_n_search_starved",
    "horizonbot_n_search_repeat",
    "horizonbot_n_bots_ticked",
    "horizonbot_n_direct_checks",
    "horizonbot_brain_us",
    "horizonbot_events_us",
    "horizonbot_engine_us",
    "horizonbot_act_us",
];

#[inline]
pub fn prof_add(i: usize, v: u64) {
    PROF_NS[i].fetch_add(v, std::sync::atomic::Ordering::Relaxed);
}

/// Everything counted since the last call, reset: nanosecond sections in microseconds (`_us`), counts as they are.
pub fn prof_take() -> Vec<(&'static str, u64)> {
    (0..PROF_LEN)
        .map(|i| {
            let v = PROF_NS[i].swap(0, std::sync::atomic::Ordering::Relaxed);
            (PROF_NAMES[i], if PROF_NAMES[i].ends_with("_us") { v / 1000 } else { v })
        })
        .collect()
}

/// Adds the time since `*t` to section `i` and restarts the lap.
#[inline]
fn lap(t: &mut std::time::Instant, i: usize) {
    let now = std::time::Instant::now();
    prof_add(i, now.duration_since(*t).as_nanos() as u64);
    *t = now;
}

pub fn effect_level(me: &SelfView, id: &str) -> u32 {
    me.effects.iter().find(|(e, _, _)| e == id).map_or(0, |(_, a, _)| a + 1)
}

/// The block face (vanilla's 3D data value) pointing from `from` to `to` (adjacent blocks).
pub fn face_from(from: [i32; 3], to: [i32; 3]) -> u8 {
    match (to[0] - from[0], to[1] - from[1], to[2] - from[2]) {
        (0, -1, 0) => 0,
        (0, 1, 0) => 1,
        (0, 0, -1) => 2,
        (0, 0, 1) => 3,
        (-1, 0, 0) => 4,
        _ => 5,
    }
}

fn face_toward(dx: i32, dz: i32) -> u8 {
    // The face of the dug block that looks back at the bot.
    face_from([dx, 0, dz], [0, 0, 0])
}

#[cfg(test)]
mod tests;
