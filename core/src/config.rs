//! The config file, `config/horizonbot-common.toml`: every setting in TOML, in sections
//! (`[combat] skill = 45`, `[ranged] enabled = true`, `[presets.sweaty]`...).
//! `/horizonbot set` writes into it keeping its comments.

use crate::settings::{self, Layer, Value};
use crate::skins::RandomSkins;
use std::collections::BTreeMap;

pub const DEFAULT_TOML: &str = include_str!("../default.toml");

#[derive(Clone, Debug)]
pub struct Config {
    pub language: String,
    pub base: Layer,
    pub presets: BTreeMap<String, Layer>,
    pub active_preset: Option<String>,
    /// randomplaystyle's odds.
    pub playstyle_weights: Vec<(String, f64)>,
    pub holding_world: String,
    pub skins_random: RandomSkins,
    pub built_in_pool: bool,
    pub pool: Vec<String>,
    pub name_tables: BTreeMap<String, Vec<String>>,
    pub extra_breakable: Vec<String>,
    /// `[horizon] max-bots`: the most bots at once (spawns past it are refused); 0 = no limit.
    pub max_bots: u32,
    /// `[horizon] bot-view-distance`: the view distance a bot asks the server for, in chunks.
    pub bot_view_distance: u8,
    /// `[horizon] joins-per-tick`: the most bots let in per server tick (a big spawn trickles in).
    pub joins_per_tick: u32,
}

pub const DEFAULT_MAX_BOTS: u32 = 0;
pub const DEFAULT_BOT_VIEW_DISTANCE: u8 = 2;
pub const DEFAULT_JOINS_PER_TICK: u32 = 10;

/// The wall time (milliseconds) the joins of one tick may use before the rest wait for the next tick.
pub const JOIN_BUDGET_MS: f64 = 8.0;

/// Whether one more queued bot may join in this tick: `per_tick` is the most a tick lets in, `joined`
/// how many this tick has let in, `spent_ms` the wall time those joins used. The budget is checked
/// after each join, so a tick's joins stop soon after they went over it; the first join of a tick
/// is always let in (a spawn always moves on, however slow the server).
pub fn may_join_more(per_tick: u32, joined: u32, spent_ms: f64) -> bool {
    joined < per_tick && (joined == 0 || spent_ms <= JOIN_BUDGET_MS)
}

/// The most bots a tick lets in, given how long the last server tick took. The cost of a join shows
/// up in the tick AFTER it was admitted (the hub handles it in its next poll), so the pace follows the
/// tick time: `per_tick` while the server keeps up, half of it once a tick ran over 60 ms, a fifth
/// over 150 ms; never less than one, so a spawn always moves on.
pub fn joins_for_tick(per_tick: u32, last_tick_ms: f64) -> u32 {
    if last_tick_ms <= 60.0 {
        per_tick
    } else if last_tick_ms <= 150.0 {
        (per_tick / 2).max(1)
    } else {
        (per_tick / 5).max(1)
    }
}

/// How many ticks a teleport waits for a bot that is still queued to be placed, `queued` bots being
/// ahead of it: each tick lets in at least one, and a bot is placed a few ticks after it joined.
pub fn placement_wait_ticks(queued: usize) -> u32 {
    200u32.saturating_add(u32::try_from(queued).unwrap_or(u32::MAX))
}

pub const PLAYSTYLES: &[&str] = &["default", "aggressive", "safe", "defensive", "balanced", "adaptive"];

pub fn default_weights() -> Vec<(String, f64)> {
    [("default", 25.0), ("aggressive", 20.0), ("safe", 15.0), ("defensive", 15.0), ("balanced", 15.0), ("adaptive", 10.0)]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), *v))
        .collect()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            language: "en".into(),
            base: Layer::new(),
            presets: settings::builtin_presets(),
            active_preset: None,
            playstyle_weights: default_weights(),
            holding_world: "world".into(),
            skins_random: RandomSkins::Missing,
            built_in_pool: true,
            pool: Vec::new(),
            name_tables: BTreeMap::new(),
            extra_breakable: Vec::new(),
            max_bots: DEFAULT_MAX_BOTS,
            bot_view_distance: DEFAULT_BOT_VIEW_DISTANCE,
            joins_per_tick: DEFAULT_JOINS_PER_TICK,
        }
    }
}

fn toml_value(v: &toml::Value) -> Option<Value> {
    Some(match v {
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Integer(i) => Value::Int(*i),
        toml::Value::Float(f) => Value::Num(*f),
        toml::Value::String(s) => Value::Text(s.clone()),
        _ => return None,
    })
}

fn strings_toml(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

impl Config {
    /// Reads the TOML text. Unknown keys are ignored; values outside their limits are clamped.
    pub fn parse(text: &str) -> Result<Config, String> {
        let root: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let mut c = Config::default();
        for (group, section) in &root {
            let Some(t) = section.as_table() else { continue };
            for (key, v) in t {
                if let (Some(spec), Some(val)) = (settings::spec_at(group, key), toml_value(v)) {
                    c.base.insert(spec.key.to_owned(), spec.coerce(&val));
                }
            }
        }
        if let Some(p) = root.get("plugin").and_then(|v| v.as_table()) {
            if let Some(l) = p.get("language").and_then(|v| v.as_str()) {
                c.language = l.to_owned();
            }
        }
        if let Some(h) = root.get("horizon").and_then(|v| v.as_table()) {
            if let Some(n) = h.get("max-bots").and_then(|v| v.as_integer()) {
                c.max_bots = n.clamp(0, u32::MAX as i64) as u32;
            }
            if let Some(n) = h.get("bot-view-distance").and_then(|v| v.as_integer()) {
                c.bot_view_distance = n.clamp(2, 32) as u8;
            }
            if let Some(n) = h.get("joins-per-tick").and_then(|v| v.as_integer()) {
                c.joins_per_tick = n.clamp(1, 1000) as u32;
            }
        }
        c.active_preset = root
            .get("active-preset")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if let Some(ps) = root.get("presets").and_then(|v| v.as_table()) {
            for (name, p) in ps {
                let mut layer = Layer::new();
                for (k, v) in p.as_table().into_iter().flatten() {
                    if let (Some(spec), Some(val)) = (settings::spec(k), toml_value(v)) {
                        layer.insert(k.clone(), spec.coerce(&val));
                    }
                }
                c.presets.insert(name.clone(), layer);
            }
        }
        if let Some(w) = root.get("playstyle-weights").and_then(|v| v.as_table()) {
            c.playstyle_weights = PLAYSTYLES
                .iter()
                .map(|s| {
                    let def = default_weights().into_iter().find(|(k, _)| k == s).map_or(0.0, |(_, v)| v);
                    let v = w
                        .get(*s)
                        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                        .unwrap_or(def);
                    ((*s).to_owned(), v.max(0.0))
                })
                .collect();
        }
        if let Some(h) = root.get("companions").and_then(|v| v.get("holding-world")).and_then(|v| v.as_str()) {
            c.holding_world = h.to_owned();
        }
        if let Some(s) = root.get("skins").and_then(|v| v.as_table()) {
            if let Some(r) = s.get("random") {
                c.skins_random = RandomSkins::parse(&r.as_str().map(str::to_owned).unwrap_or_else(|| r.to_string()));
            }
            if let Some(b) = s.get("built-in-pool").and_then(|v| v.as_bool()) {
                c.built_in_pool = b;
            }
            c.pool = strings_toml(s.get("pool"));
        }
        if let Some(t) = root.get("name-tables").and_then(|v| v.as_table()) {
            for (name, v) in t {
                c.name_tables.insert(name.clone(), strings_toml(Some(v)));
            }
        }
        c.extra_breakable = strings_toml(root.get("building").and_then(|b| b.get("extra-breakable")));
        Ok(c)
    }

    pub fn settings(&self) -> settings::Settings {
        let mut s = settings::Settings::new();
        s.base = self.base.clone();
        s.presets = self.presets.clone();
        s.active_preset = self.active_preset.clone();
        s
    }
}

/// Writes one setting into the config text, keeping everything else (comments too).
pub fn write_setting(text: &str, key: &str, value: &Value) -> Result<String, String> {
    let spec = settings::spec(key).ok_or_else(|| format!("No setting called {key}"))?;
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    if doc.get(spec.group).is_none() {
        doc[spec.group] = toml_edit::table();
    }
    doc[spec.group][spec.yaml] = edit_value(value);
    Ok(doc.to_string())
}

fn edit_value(v: &Value) -> toml_edit::Item {
    match v {
        Value::Bool(b) => toml_edit::value(*b),
        Value::Int(i) => toml_edit::value(*i),
        Value::Num(f) => toml_edit::value(*f),
        Value::Text(t) => toml_edit::value(t.as_str()),
    }
}

/// Writes the presets and the active one (preset save/delete/apply).
pub fn write_presets(text: &str, presets: &BTreeMap<String, Layer>, active: Option<&str>) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    doc["active-preset"] = toml_edit::value(active.unwrap_or(""));
    let mut ps = toml_edit::Table::new();
    ps.set_implicit(true);
    for (name, layer) in presets {
        let mut t = toml_edit::Table::new();
        for (k, v) in layer {
            t[k.as_str()] = edit_value(v);
        }
        ps[name.as_str()] = toml_edit::Item::Table(t);
    }
    doc["presets"] = toml_edit::Item::Table(ps);
    Ok(doc.to_string())
}

pub fn write_weights(text: &str, weights: &[(String, f64)]) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.to_string())?;
    let mut t = toml_edit::Table::new();
    for (k, v) in weights {
        t[k.as_str()] = toml_edit::value(*v);
    }
    doc["playstyle-weights"] = toml_edit::Item::Table(t);
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_parses_with_the_defaults() {
        let c = Config::parse(DEFAULT_TOML).unwrap();
        let s = c.settings().snapshot(None, None);
        assert_eq!(s.i("skill"), 45);
        assert_eq!(s.f("melee-range"), 2.8);
        assert!(s.b("ranged"));
        assert_eq!(s.f("detection-sneaking"), 0.65);
        assert!(c.presets.contains_key("sweaty"));
        assert_eq!(c.name_tables["default"].len(), 8);
    }

    #[test]
    fn the_file_agrees_with_the_settings_table() {
        let c = Config::parse(DEFAULT_TOML).unwrap();
        for sp in settings::specs() {
            if let Some(v) = c.base.get(sp.key) {
                assert_eq!(sp.coerce(v), sp.default_value(), "{}", sp.key);
            }
        }
        assert!(c.base.len() > 200, "{}", c.base.len());
        for (name, layer) in &c.presets {
            for k in layer.keys() {
                assert!(settings::spec(k).is_some(), "preset {name}: unknown setting {k}");
            }
        }
        for (name, layer) in &settings::builtin_presets() {
            for k in layer.keys() {
                assert!(settings::spec(k).is_some(), "built-in preset {name}: unknown setting {k}");
            }
        }
    }

    #[test]
    fn set_keeps_comments() {
        let text = "# top\n[combat]\n# the dial\nskill = 50\n";
        let out = write_setting(text, "skill", &Value::Int(80)).unwrap();
        assert!(out.contains("# the dial"));
        assert_eq!(Config::parse(&out).unwrap().base.get("skill"), Some(&Value::Int(80)));
        let out = write_setting(&out, "ranged-min-range", &Value::Num(5.0)).unwrap();
        assert!(out.contains("[ranged]"));
        let mut p = BTreeMap::new();
        let mut l = Layer::new();
        l.insert("skill".into(), Value::Int(1));
        p.insert("x".into(), l);
        let out = write_presets(&out, &p, Some("x")).unwrap();
        let c = Config::parse(&out).unwrap();
        assert_eq!(c.active_preset.as_deref(), Some("x"));
        assert_eq!(c.presets["x"].get("skill"), Some(&Value::Int(1)));
    }

    #[test]
    fn joins_stop_at_the_budget_but_never_below_one() {
        // The first join of a tick is always let in, whatever it cost.
        assert!(may_join_more(10, 0, 500.0));
        assert!(may_join_more(10, 3, JOIN_BUDGET_MS));
        assert!(!may_join_more(10, 3, JOIN_BUDGET_MS + 0.5));
        // The per-tick cap holds however cheap the joins were.
        assert!(may_join_more(10, 9, 0.0));
        assert!(!may_join_more(10, 10, 0.0));
        assert!(!may_join_more(1, 1, 0.0));
        assert_eq!(placement_wait_ticks(0), 200);
        assert_eq!(placement_wait_ticks(50), 250);
    }

    #[test]
    fn the_join_pace_follows_the_tick_time() {
        assert_eq!(joins_for_tick(10, 50.0), 10);
        assert_eq!(joins_for_tick(10, 60.0), 10);
        assert_eq!(joins_for_tick(10, 100.0), 5);
        assert_eq!(joins_for_tick(10, 400.0), 2);
        assert_eq!(joins_for_tick(1, 400.0), 1);
        assert_eq!(joins_for_tick(3, 100.0), 1);
    }
}
