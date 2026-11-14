//! Every behaviour setting, its default and limits, and the layering that decides which value a
//! bot reads: the config file (or the default), the active preset, the bot's faction, the bot's
//! own values. The difficulty knobs at -1 follow `skill` (or another setting) at whichever layer
//! it is set.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bool(bool),
    /// default, min, max
    Int(i64, i64, i64),
    /// default, min, max
    Num(f64, f64, f64),
    /// default, allowed values
    Choice(&'static str, &'static [&'static str]),
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// The short name `/horizonbot set` takes.
    pub key: &'static str,
    /// The group (`/horizonbot settings <group>`), also the config file's section.
    pub group: &'static str,
    /// The key inside the group's section of the config file.
    pub yaml: &'static str,
    pub kind: Kind,
}

/// Settings outside the main table.
const EXTRA: &[Spec] = &[
    Spec { key: "drop-items", group: "general", yaml: "drop-items", kind: Kind::Bool(true) },
    Spec { key: "heal-distance-min", group: "utility", yaml: "heal-distance-min", kind: Kind::Num(5.0, 0.0, 64.0) },
    Spec { key: "heal-distance", group: "utility", yaml: "heal-distance", kind: Kind::Num(12.0, 0.0, 64.0) },
    Spec { key: "heal-distance-max", group: "utility", yaml: "heal-distance-max", kind: Kind::Num(28.0, 0.0, 128.0) },
];

pub fn specs() -> impl Iterator<Item = &'static Spec> {
    crate::settings_table::SPECS.iter().chain(EXTRA.iter())
}

/// FNV-1a for the setting-name index: a lookup by name runs hundreds of times per bot per tick, and the
/// standard hasher (SipHash) costs about twice as much on these short keys.
#[derive(Clone, Copy)]
pub struct KeyHasher(u64);
impl Default for KeyHasher {
    fn default() -> Self {
        KeyHasher(0xcbf2_9ce4_8422_2325)
    }
}
impl std::hash::Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
type KeyMap = HashMap<&'static str, usize, std::hash::BuildHasherDefault<KeyHasher>>;

fn index() -> &'static KeyMap {
    static INDEX: OnceLock<KeyMap> = OnceLock::new();
    INDEX.get_or_init(|| specs().enumerate().map(|(i, s)| (s.key, i)).collect())
}

/// Every spec's default, in table order (what a snapshot starts from).
fn defaults() -> &'static Vec<Value> {
    static DEFAULTS: OnceLock<Vec<Value>> = OnceLock::new();
    DEFAULTS.get_or_init(|| all().iter().map(|s| s.default_value()).collect())
}

fn all() -> &'static Vec<&'static Spec> {
    static ALL: OnceLock<Vec<&'static Spec>> = OnceLock::new();
    ALL.get_or_init(|| specs().collect())
}

pub fn spec(key: &str) -> Option<&'static Spec> {
    index().get(key).map(|&i| all()[i])
}

/// The spec for a config-file location (`group`, `key in group`).
pub fn spec_at(group: &str, yaml: &str) -> Option<&'static Spec> {
    specs().find(|s| s.group == group && s.yaml == yaml)
}

pub fn groups() -> Vec<&'static str> {
    let mut g: Vec<&'static str> = Vec::new();
    for s in specs() {
        if !g.contains(&s.group) {
            g.push(s.group);
        }
    }
    g
}

/// The difficulty knobs (`/horizonbot difficulty`).
pub const KNOBS: &[&str] = &[
    "aim",
    "reaction",
    "crit-timing",
    "technique",
    "tracking",
    "clutch",
    "shield-disable-speed",
    "shield-charge",
    "strafe",
    "reach-flips",
    "potion-gap",
    "cart-skip",
    "elytra-mace-level",
    "stun-slam-chance",
];

/// The technique switches (`/horizonbot techniques`).
pub fn techniques() -> Vec<&'static Spec> {
    specs().filter(|s| s.group == "techniques").collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Num(f64),
    Text(String),
}

impl Value {
    pub fn render(&self) -> String {
        match self {
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Num(f) => crate::yaml::fmt_float(*f),
            Value::Text(t) => t.clone(),
        }
    }
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Int(i) => *i as f64,
            Value::Num(f) => *f,
            Value::Text(_) => 0.0,
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Num(f) => *f != 0.0,
            Value::Text(t) => t == "true",
        }
    }
    pub fn to_yaml(&self) -> crate::yaml::Yaml {
        use crate::yaml::Yaml;
        match self {
            Value::Bool(b) => Yaml::Bool(*b),
            Value::Int(i) => Yaml::Int(*i),
            Value::Num(f) => Yaml::Float(*f),
            Value::Text(t) => Yaml::Str(t.clone()),
        }
    }
    pub fn to_toml(&self) -> toml::Value {
        match self {
            Value::Bool(b) => toml::Value::Boolean(*b),
            Value::Int(i) => toml::Value::Integer(*i),
            Value::Num(f) => toml::Value::Float(*f),
            Value::Text(t) => toml::Value::String(t.clone()),
        }
    }
}

impl Spec {
    pub fn default_value(&self) -> Value {
        match self.kind {
            Kind::Bool(b) => Value::Bool(b),
            Kind::Int(d, _, _) => Value::Int(d),
            Kind::Num(d, _, _) => Value::Num(d),
            Kind::Choice(d, _) => Value::Text(d.to_owned()),
        }
    }

    /// Reads a value typed by a player (`/horizonbot set`), refusing one outside the limits.
    pub fn parse(&self, text: &str) -> Result<Value, String> {
        let t = text.trim();
        match self.kind {
            Kind::Bool(_) => match t.to_ascii_lowercase().as_str() {
                "true" | "on" | "yes" => Ok(Value::Bool(true)),
                "false" | "off" | "no" => Ok(Value::Bool(false)),
                _ => Err(format!("{} takes true or false", self.key)),
            },
            Kind::Int(_, lo, hi) => {
                let v: f64 = t.parse().map_err(|_| format!("{} takes a whole number", self.key))?;
                if v.fract() != 0.0 {
                    return Err(format!("{} takes a whole number", self.key));
                }
                let v = v as i64;
                if v < lo || v > hi {
                    return Err(format!("{} goes from {lo} to {hi}", self.key));
                }
                Ok(Value::Int(v))
            }
            Kind::Num(_, lo, hi) => {
                let v: f64 = t.parse().map_err(|_| format!("{} takes a number", self.key))?;
                if !v.is_finite() || v < lo || v > hi {
                    return Err(format!(
                        "{} goes from {} to {}",
                        self.key,
                        crate::yaml::fmt_float(lo),
                        crate::yaml::fmt_float(hi)
                    ));
                }
                Ok(Value::Num(v))
            }
            Kind::Choice(_, allowed) => {
                let v = t.to_ascii_lowercase();
                if allowed.contains(&v.as_str()) {
                    Ok(Value::Text(v))
                } else {
                    Err(format!("{} takes one of: {}", self.key, allowed.join(", ")))
                }
            }
        }
    }

    /// A value read from a file: numbers are clamped into the limits, a bad one is the default.
    pub fn coerce(&self, v: &Value) -> Value {
        match (self.kind, v) {
            (Kind::Bool(_), v) => Value::Bool(v.as_bool()),
            (Kind::Int(d, lo, hi), Value::Text(t)) => {
                Value::Int(t.parse::<i64>().map(|x| x.clamp(lo, hi)).unwrap_or(d))
            }
            (Kind::Int(_, lo, hi), v) => Value::Int((v.as_f64().round() as i64).clamp(lo, hi)),
            (Kind::Num(d, lo, hi), Value::Text(t)) => {
                Value::Num(t.parse::<f64>().map(|x| x.clamp(lo, hi)).unwrap_or(d))
            }
            (Kind::Num(_, lo, hi), v) => Value::Num(v.as_f64().clamp(lo, hi)),
            (Kind::Choice(d, allowed), v) => {
                let t = v.render().to_ascii_lowercase();
                Value::Text(if allowed.contains(&t.as_str()) { t } else { d.to_owned() })
            }
        }
    }
}

pub type Layer = BTreeMap<String, Value>;

/// The built-in presets.
pub fn builtin_presets() -> BTreeMap<String, Layer> {
    let mk = |pairs: &[(&str, Value)]| -> Layer {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    };
    use Value::{Bool as B, Int as I, Num as N};
    let mut p = BTreeMap::new();
    p.insert(
        "casual".into(),
        mk(&[
            ("skill", I(20)),
            ("miss-chance", I(34)),
            ("mistake-chance", I(40)),
            ("attack-cooldown", I(20)),
            ("reaction-ticks", I(70)),
            ("ping-millis", I(90)),
            ("crits", B(false)),
            ("pearls", B(false)),
            ("cobwebs", B(false)),
            ("buff-potions", B(false)),
            ("mace", B(false)),
        ]),
    );
    p.insert(
        "easy".into(),
        mk(&[
            ("skill", I(10)),
            ("aim", I(12)),
            ("reaction", I(8)),
            ("crit-timing", I(8)),
            ("technique", I(6)),
            ("tracking", I(12)),
            ("clutch", I(5)),
            ("shield-disable-speed", I(8)),
            ("shield-charge", I(100)),
            ("strafe", I(6)),
            ("reach-flips", I(8)),
            ("potion-gap", N(4.0)),
            ("cart-skip", I(70)),
            ("elytra-mace-level", I(1)),
            ("stun-slam-chance", I(15)),
            ("miss-chance", I(28)),
            ("mistake-chance", I(33)),
            ("web-escape-delay", I(34)),
            ("drain-delay", I(34)),
            ("attack-cooldown", I(18)),
            ("crit-chain", B(false)),
            ("hit-select", B(false)),
            ("mirror-punish", B(false)),
            ("cobwebs", B(false)),
            ("pearls", B(false)),
            ("shield-disable", B(false)),
            ("buff-potions", B(false)),
            ("healing", Value::Text("old".into())),
            ("combo-answers", B(false)),
            ("read-opponent", B(false)),
            ("shield-game", B(false)),
            ("elytra-mace", B(false)),
            ("mace-pearl-launch", B(false)),
            ("cart-pvp", B(false)),
        ]),
    );
    p.insert(
        "normal".into(),
        mk(&[
            ("skill", I(45)),
            ("aim", I(44)),
            ("reaction", I(38)),
            ("crit-timing", I(40)),
            ("technique", I(36)),
            ("tracking", I(42)),
            ("clutch", I(38)),
            ("shield-disable-speed", I(40)),
            ("shield-charge", I(85)),
            ("strafe", I(16)),
            ("reach-flips", I(4)),
            ("potion-gap", N(2.5)),
            ("cart-skip", I(45)),
            ("elytra-mace-level", I(2)),
            ("stun-slam-chance", I(35)),
            ("miss-chance", I(14)),
            ("mistake-chance", I(18)),
            ("web-escape-delay", I(17)),
            ("drain-delay", I(20)),
            ("attack-cooldown", I(14)),
            ("crit-chain", B(false)),
            ("hit-select", B(true)),
            ("jump-reset", B(false)),
            ("mirror-punish", B(false)),
            ("cobwebs", B(true)),
            ("pearls", B(true)),
            ("shield-disable", B(true)),
            ("shield-disable-chance", I(65)),
            ("buff-potions", B(true)),
            ("healing", Value::Text("advanced".into())),
            ("combo-answers", B(true)),
            ("read-opponent", B(false)),
            ("shield-game", B(true)),
            ("elytra-mace", B(false)),
            ("mace-pearl-launch", B(false)),
            ("cart-pvp", B(false)),
        ]),
    );
    p.insert(
        "hard".into(),
        mk(&[
            ("skill", I(62)),
            ("aim", I(64)),
            ("reaction", I(58)),
            ("crit-timing", I(62)),
            ("technique", I(58)),
            ("tracking", I(64)),
            ("clutch", I(62)),
            ("shield-disable-speed", I(64)),
            ("shield-charge", I(65)),
            ("strafe", I(24)),
            ("reach-flips", I(3)),
            ("potion-gap", N(2.0)),
            ("cart-skip", I(28)),
            ("elytra-mace-level", I(3)),
            ("stun-slam-chance", I(58)),
            ("miss-chance", I(8)),
            ("mistake-chance", I(10)),
            ("web-escape-delay", I(11)),
            ("drain-delay", I(12)),
            ("attack-cooldown", I(13)),
            ("crit-chain", B(true)),
            ("hit-select", B(true)),
            ("jump-reset", B(true)),
            ("mirror-punish", B(true)),
            ("cobwebs", B(true)),
            ("web-prediction", B(true)),
            ("head-web", B(true)),
            ("pearls", B(true)),
            ("shield-disable", B(true)),
            ("shield-disable-chance", I(80)),
            ("buff-potions", B(true)),
            ("healing", Value::Text("advanced".into())),
            ("combo-answers", B(true)),
            ("read-opponent", B(true)),
            ("shield-game", B(true)),
            ("damage-model", B(true)),
            ("edge-play", B(true)),
            ("elytra-mace", B(true)),
            ("mace-pearl-launch", B(true)),
            ("cart-pvp", B(false)),
        ]),
    );
    p.insert(
        "expert".into(),
        mk(&[
            ("skill", I(78)),
            ("aim", I(80)),
            ("reaction", I(78)),
            ("crit-timing", I(78)),
            ("technique", I(76)),
            ("tracking", I(78)),
            ("clutch", I(82)),
            ("shield-disable-speed", I(82)),
            ("shield-charge", I(45)),
            ("strafe", I(32)),
            ("reach-flips", I(2)),
            ("potion-gap", N(1.5)),
            ("cart-skip", I(12)),
            ("elytra-mace-level", I(4)),
            ("stun-slam-chance", I(80)),
            ("miss-chance", I(5)),
            ("mistake-chance", I(6)),
            ("web-escape-delay", I(7)),
            ("drain-delay", I(7)),
            ("attack-cooldown", I(12)),
            ("ping-millis", I(30)),
            ("crit-chain", B(true)),
            ("hit-select", B(true)),
            ("jump-reset", B(true)),
            ("mirror-punish", B(true)),
            ("d-tap", B(true)),
            ("whiff-punish", B(true)),
            ("cobwebs", B(true)),
            ("web-prediction", B(true)),
            ("head-web", B(true)),
            ("hit-web", B(true)),
            ("pearls", B(true)),
            ("shield-disable", B(true)),
            ("shield-disable-chance", I(92)),
            ("buff-potions", B(true)),
            ("healing", Value::Text("advanced".into())),
            ("combo-answers", B(true)),
            ("read-opponent", B(true)),
            ("shield-game", B(true)),
            ("damage-model", B(true)),
            ("edge-play", B(true)),
            ("elytra-mace", B(true)),
            ("elytra-chain", B(true)),
            ("mace-pearl-launch", B(true)),
            ("attribute-swap", B(true)),
            ("cart-pvp", B(true)),
            ("detap-breach", B(true)),
        ]),
    );
    p.insert(
        "sweaty".into(),
        mk(&[
            ("skill", I(90)),
            ("miss-chance", I(2)),
            ("mistake-chance", I(3)),
            ("attack-cooldown", I(10)),
            ("ping-millis", I(20)),
            ("reaction-ticks", I(6)),
            ("crits", B(true)),
            ("shield-disable", B(true)),
            ("buff-potions", B(true)),
            ("cobwebs", B(true)),
            ("mace-pearl-launch", B(true)),
            ("attribute-swap", B(true)),
            ("elytra-mace", B(true)),
            ("cart-pvp", B(true)),
            ("wc-reset", B(true)),
            ("spear-double-pop", B(true)),
        ]),
    );
    p.insert(
        "passive".into(),
        mk(&[
            ("auto-target", B(false)),
            ("wander", B(false)),
            ("bot-vs-bot", B(false)),
            ("revenge", B(true)),
            ("retreat-health", N(9.0)),
        ]),
    );
    p
}

/// `/horizonbot difficulty preset <level>`: every knob at once.
pub fn difficulty_preset(level: &str) -> Option<Layer> {
    let n = match level {
        "easy" => 12,
        "medium" => 38,
        "hard" => 58,
        "expert" => 78,
        "professional" => 92,
        "perfect" => 100,
        _ => return None,
    };
    let mut l = Layer::new();
    for k in ["aim", "reaction", "crit-timing", "technique", "tracking", "clutch", "shield-disable-speed"] {
        l.insert(k.into(), Value::Int(n));
    }
    l.insert("elytra-mace-level".into(), Value::Int(((n as f64) / 20.0).ceil().clamp(1.0, 5.0) as i64));
    l.insert("reach-flips".into(), Value::Int(((100 - n) / 25).clamp(0, 10)));
    Some(l)
}

pub const DIFFICULTY_LEVELS: &[&str] = &["easy", "medium", "hard", "expert", "professional", "perfect"];

/// All layers, as the plugin keeps them.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// What the config file says (missing keys: the defaults).
    pub base: Layer,
    pub presets: BTreeMap<String, Layer>,
    pub active_preset: Option<String>,
    /// Faction name -> its own values (from `factions.yml`).
    pub factions: BTreeMap<String, Layer>,
    /// Bot name (lower case) -> its own values (`bot-settings.yml`).
    pub bots: BTreeMap<String, Layer>,
    /// Bumped on every change, so cached snapshots know to refresh.
    pub generation: u64,
}

impl Settings {
    pub fn new() -> Self {
        Settings {
            presets: builtin_presets(),
            ..Default::default()
        }
    }

    fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// The value as one bot reads it (`faction`/`bot` may be None).
    pub fn get(&self, key: &str, faction: Option<&str>, bot: Option<&str>) -> Value {
        let Some(spec) = spec(key) else {
            return Value::Bool(false);
        };
        let layers = self.layers(faction, bot);
        for layer in layers.iter().rev() {
            if let Some(v) = layer.get(key) {
                return spec.coerce(v);
            }
        }
        spec.default_value()
    }

    fn layers<'a>(&'a self, faction: Option<&str>, bot: Option<&str>) -> Vec<&'a Layer> {
        let mut l = vec![&self.base];
        if let Some(p) = self.active_preset.as_ref().and_then(|p| self.presets.get(p)) {
            l.push(p);
        }
        if let Some(f) = faction.and_then(|f| self.factions.get(f)) {
            l.push(f);
        }
        if let Some(b) = bot.and_then(|b| self.bots.get(&b.to_ascii_lowercase())) {
            l.push(b);
        }
        l
    }

    /// Whether the value comes from the file's defaults (not written there).
    pub fn is_default(&self, key: &str) -> bool {
        !self.base.contains_key(key)
    }

    /// `/horizonbot set`: writes the base layer. Returns a warning when the active preset hides it.
    pub fn set_global(&mut self, key: &str, text: &str) -> Result<Option<String>, String> {
        let spec = spec(key).ok_or_else(|| format!("No setting called {key}"))?;
        let v = spec.parse(text)?;
        self.base.insert(key.to_owned(), v);
        self.bump();
        let hidden = self
            .active_preset
            .as_ref()
            .and_then(|p| self.presets.get(p))
            .is_some_and(|p| p.contains_key(key));
        Ok(hidden.then(|| {
            format!(
                "Saved, but the active preset {} sets {key} too and still wins",
                self.active_preset.clone().unwrap_or_default()
            )
        }))
    }

    pub fn set_faction(&mut self, faction: &str, key: &str, text: &str) -> Result<(), String> {
        let spec = spec(key).ok_or_else(|| format!("No setting called {key}"))?;
        let v = spec.parse(text)?;
        self.factions.entry(faction.to_owned()).or_default().insert(key.to_owned(), v);
        self.bump();
        Ok(())
    }

    pub fn clear_faction_key(&mut self, faction: &str, key: &str) {
        if let Some(l) = self.factions.get_mut(faction) {
            l.remove(key);
        }
        self.bump();
    }

    pub fn set_bot(&mut self, bot: &str, key: &str, text: &str) -> Result<(), String> {
        let spec = spec(key).ok_or_else(|| format!("No setting called {key}"))?;
        let v = spec.parse(text)?;
        self.bots
            .entry(bot.to_ascii_lowercase())
            .or_default()
            .insert(key.to_owned(), v);
        self.bump();
        Ok(())
    }

    pub fn clear_bot_keys(&mut self, bot: &str, keys: &[&str]) {
        if let Some(l) = self.bots.get_mut(&bot.to_ascii_lowercase()) {
            for k in keys {
                l.remove(*k);
            }
        }
        self.bump();
    }

    pub fn apply_preset(&mut self, name: &str) -> Result<(), String> {
        if name == "off" {
            self.active_preset = None;
        } else if self.presets.contains_key(name) {
            self.active_preset = Some(name.to_owned());
        } else {
            return Err(format!("No preset called {name}"));
        }
        self.bump();
        Ok(())
    }

    /// `/horizonbot preset save`: everything in force now (base + active preset), non-default only.
    pub fn save_preset(&mut self, name: &str) {
        let mut layer = Layer::new();
        for s in specs() {
            let v = self.get(s.key, None, None);
            if v != s.default_value() {
                layer.insert(s.key.to_owned(), v);
            }
        }
        self.presets.insert(name.to_owned(), layer);
        self.bump();
    }

    pub fn delete_preset(&mut self, name: &str) -> bool {
        let gone = self.presets.remove(name).is_some();
        if self.active_preset.as_deref() == Some(name) {
            self.active_preset = None;
        }
        self.bump();
        gone
    }

    /// The resolved values for one bot, typed and indexed for its tick.
    pub fn snapshot(&self, faction: Option<&str>, bot: Option<&str>) -> Snapshot {
        // Same answer as `get` for every key (the highest layer holding the key wins, coerced; else the
        // default), but the layers are listed once and each layer's own entries are applied in order, not
        // four map searches and two allocations per key.
        let mut values = defaults().clone();
        let layers = self.layers(faction, bot);
        for layer in layers {
            for (k, v) in layer {
                if let Some(&i) = index().get(k.as_str()) {
                    values[i] = all()[i].coerce(v);
                }
            }
        }
        Snapshot {
            values: std::sync::Arc::new(values),
            generation: self.generation,
        }
    }
}

/// One bot's resolved settings.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Shared: a snapshot is cloned by the brain many times a tick (`let st = bot.settings.clone()`), which
    /// must cost a counter bump, not a copy of 230 values. `set` copies on write.
    values: std::sync::Arc<Vec<Value>>,
    pub generation: u64,
}

impl Default for Snapshot {
    fn default() -> Self {
        Settings::new().snapshot(None, None)
    }
}

impl Snapshot {
    fn v(&self, key: &str) -> &Value {
        match index().get(key) {
            Some(&i) => &self.values[i],
            None => {
                debug_assert!(false, "unknown setting {key}");
                &Value::Bool(false)
            }
        }
    }
    pub fn b(&self, key: &str) -> bool {
        self.v(key).as_bool()
    }
    pub fn f(&self, key: &str) -> f64 {
        self.v(key).as_f64()
    }
    pub fn i(&self, key: &str) -> i64 {
        self.v(key).as_f64() as i64
    }
    pub fn s(&self, key: &str) -> String {
        self.v(key).render()
    }
    /// A probability setting given in percent, as 0..1.
    pub fn pct(&self, key: &str) -> f64 {
        (self.f(key) / 100.0).clamp(0.0, 1.0)
    }

    /// A difficulty knob, 0..1, with -1 following its parent (`skill`, except that
    /// `clutch` follows 100 - mistake-chance, `shield-disable-speed` follows `reaction`).
    pub fn knob(&self, key: &str) -> f64 {
        let raw = self.f(key);
        if raw >= 0.0 {
            return (raw / 100.0).clamp(0.0, 1.0);
        }
        match key {
            "clutch" => (1.0 - self.pct("mistake-chance")).clamp(0.0, 1.0),
            "shield-disable-speed" => self.knob("reaction"),
            _ => (self.f("skill") / 100.0).clamp(0.0, 1.0),
        }
    }

    /// The elytra-mace level 1..5 (0 follows skill).
    pub fn elytra_level(&self) -> u8 {
        let l = self.i("elytra-mace-level");
        if l > 0 {
            l.clamp(1, 5) as u8
        } else {
            (1.0 + self.knob("technique") * 4.0).round().clamp(1.0, 5.0) as u8
        }
    }

    /// Overrides one value in this snapshot only (a bot's skill spread inside its faction).
    pub fn set(&mut self, key: &str, v: Value) {
        if let (Some(&i), Some(spec)) = (index().get(key), spec(key)) {
            std::sync::Arc::make_mut(&mut self.values)[i] = spec.coerce(&v);
        }
    }

    pub fn all(&self) -> impl Iterator<Item = (&'static Spec, &Value)> {
        all().iter().copied().zip(self.values.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_complete_and_unique() {
        let n = specs().count();
        assert!(n >= 215, "{n}");
        assert_eq!(index().len(), n, "duplicate keys");
        for s in specs() {
            // Every default passes its own limits.
            assert_eq!(s.coerce(&s.default_value()), s.default_value(), "{}", s.key);
        }
    }

    #[test]
    fn layers_resolve_most_specific_first() {
        let mut st = Settings::new();
        assert_eq!(st.get("skill", None, None), Value::Int(45));
        st.set_global("skill", "60").unwrap();
        assert_eq!(st.get("skill", Some("rookies"), Some("Bob")), Value::Int(60));
        st.apply_preset("sweaty").unwrap();
        assert_eq!(st.get("skill", None, None), Value::Int(90));
        let warn = st.set_global("skill", "70").unwrap();
        assert!(warn.is_some());
        st.set_faction("rookies", "skill", "20").unwrap();
        assert_eq!(st.get("skill", Some("rookies"), None), Value::Int(20));
        st.set_bot("Bob", "skill", "99").unwrap();
        assert_eq!(st.get("skill", Some("rookies"), Some("bob")), Value::Int(99));
        st.apply_preset("off").unwrap();
        assert_eq!(st.get("skill", None, None), Value::Int(70));
    }

    #[test]
    fn limits_are_enforced() {
        let mut st = Settings::new();
        assert!(st.set_global("skill", "101").is_err());
        assert!(st.set_global("melee-range", "0.5").is_err());
        assert!(st.set_global("crits", "maybe").is_err());
        assert!(st.set_global("stun-slam-variant", "easy").is_ok());
        assert!(st.set_global("stun-slam-variant", "hard").is_err());
        assert!(st.set_global("nonsense", "1").is_err());
    }

    #[test]
    fn knobs_follow_their_parents() {
        let mut st = Settings::new();
        st.set_global("skill", "80").unwrap();
        st.set_global("mistake-chance", "20").unwrap();
        let s = st.snapshot(None, None);
        assert!((s.knob("aim") - 0.8).abs() < 1e-9);
        assert!((s.knob("clutch") - 0.8).abs() < 1e-9);
        st.set_global("reaction", "40").unwrap();
        let s = st.snapshot(None, None);
        assert!((s.knob("shield-disable-speed") - 0.4).abs() < 1e-9);
    }

    #[test]
    fn saving_a_preset_keeps_what_is_in_force() {
        let mut st = Settings::new();
        st.set_global("pearls", "false").unwrap();
        st.apply_preset("sweaty").unwrap();
        st.save_preset("tournament");
        let t = &st.presets["tournament"];
        assert_eq!(t.get("pearls"), Some(&Value::Bool(false)));
        assert_eq!(t.get("skill"), Some(&Value::Int(90)));
        assert!(!t.contains_key("melee-range"));
    }

    #[test]
    fn difficulty_presets_cover_all_levels() {
        for l in DIFFICULTY_LEVELS {
            assert!(difficulty_preset(l).is_some());
        }
        assert!(difficulty_preset("insane").is_none());
    }
}
