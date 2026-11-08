//! `bots.yml`: the bots that were online when the server stopped (brought back with
//! `bots-relog`) and the history of every bot name used (for `clearplayerdata`), and
//! `bot-settings.yml`: each bot's own setting values, kept by name.
//!
//! The per-bot entry layout is described in STATUS.md.

use crate::math::Vec3;
use crate::settings::Layer;
use crate::yaml::Yaml;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct SavedBot {
    pub name: String,
    pub world: String,
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// The account whose skin it wears (None: its own name, or a random one from the pool).
    pub skin_from: Option<String>,
    pub owner: Option<String>,
    pub paused: bool,
    /// The kit it was given last (what it restocks to).
    pub kit: Option<String>,
    pub playstyle: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BotStore {
    pub bots: Vec<SavedBot>,
    pub history: Vec<String>,
}

impl BotStore {
    pub fn remember(&mut self, name: &str) {
        if !self.history.iter().any(|h| h.eq_ignore_ascii_case(name)) {
            self.history.push(name.to_owned());
        }
    }

    pub fn load(doc: &Yaml) -> Self {
        let bots = doc
            .get("bots")
            .and_then(Yaml::as_seq)
            .map(|s| {
                s.iter()
                    .filter_map(|b| {
                        Some(SavedBot {
                            name: b.get("name")?.as_text()?,
                            world: b.get("world").and_then(Yaml::as_text).unwrap_or_else(|| "world".into()),
                            pos: Vec3::new(
                                b.get("x").and_then(Yaml::as_f64).unwrap_or(0.0),
                                b.get("y").and_then(Yaml::as_f64).unwrap_or(64.0),
                                b.get("z").and_then(Yaml::as_f64).unwrap_or(0.0),
                            ),
                            yaw: b.get("yaw").and_then(Yaml::as_f64).unwrap_or(0.0) as f32,
                            pitch: b.get("pitch").and_then(Yaml::as_f64).unwrap_or(0.0) as f32,
                            skin_from: b.get("skin").and_then(Yaml::as_text),
                            owner: b.get("owner").and_then(Yaml::as_text),
                            paused: b.get("paused").and_then(Yaml::as_bool).unwrap_or(false),
                            kit: b.get("kit").and_then(Yaml::as_text),
                            playstyle: b.get("playstyle").and_then(Yaml::as_text),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        BotStore {
            bots,
            history: doc.get("history").map(Yaml::strings).unwrap_or_default(),
        }
    }

    pub fn save(&self) -> Yaml {
        let mut doc = Yaml::map();
        doc.set(
            "bots",
            Yaml::Seq(
                self.bots
                    .iter()
                    .map(|b| {
                        let mut y = Yaml::map();
                        y.set("name", Yaml::Str(b.name.clone()));
                        y.set("world", Yaml::Str(b.world.clone()));
                        y.set("x", Yaml::Float(b.pos.x));
                        y.set("y", Yaml::Float(b.pos.y));
                        y.set("z", Yaml::Float(b.pos.z));
                        y.set("yaw", Yaml::Float(f64::from(b.yaw)));
                        y.set("pitch", Yaml::Float(f64::from(b.pitch)));
                        if let Some(s) = &b.skin_from {
                            y.set("skin", Yaml::Str(s.clone()));
                        }
                        if let Some(o) = &b.owner {
                            y.set("owner", Yaml::Str(o.clone()));
                        }
                        if b.paused {
                            y.set("paused", Yaml::Bool(true));
                        }
                        if let Some(k) = &b.kit {
                            y.set("kit", Yaml::Str(k.clone()));
                        }
                        if let Some(p) = &b.playstyle {
                            y.set("playstyle", Yaml::Str(p.clone()));
                        }
                        y
                    })
                    .collect(),
            ),
        );
        doc.set("history", Yaml::str_seq(self.history.iter().cloned()));
        doc
    }
}

pub fn load_bot_settings(doc: &Yaml) -> BTreeMap<String, Layer> {
    let mut out = BTreeMap::new();
    // Under `bots:` or straight at the top.
    if let Some(m) = doc.get("bots").and_then(Yaml::as_map).or_else(|| doc.as_map()) {
        for (name, v) in m {
            let mut l = Layer::new();
            for (k, x) in v.as_map().unwrap_or(&[]) {
                if let (Some(spec), Some(val)) = (crate::settings::spec(k), crate::factions::yaml_value(x)) {
                    l.insert(k.clone(), spec.coerce(&val));
                }
            }
            out.insert(name.to_ascii_lowercase(), l);
        }
    }
    out
}

pub fn save_bot_settings(bots: &BTreeMap<String, Layer>) -> Yaml {
    let mut m = Yaml::map();
    for (name, l) in bots {
        if l.is_empty() {
            continue;
        }
        let mut v = Yaml::map();
        for (k, x) in l {
            v.set(k, x.to_yaml());
        }
        m.set(name, v);
    }
    let mut doc = Yaml::map();
    doc.set("bots", m);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_empty_bots_file() {
        let s = BotStore::load(&crate::yaml::parse("bots: []\nhistory:\n- Alpha\n- Bravo\n").unwrap());
        assert!(s.bots.is_empty());
        assert_eq!(s.history, vec!["Alpha", "Bravo"]);
    }

    #[test]
    fn round_trips() {
        let mut s = BotStore::default();
        s.bots.push(SavedBot {
            name: "Marco".into(),
            world: "world".into(),
            pos: Vec3::new(1.5, 64.0, -3.25),
            yaw: 90.0,
            pitch: 0.0,
            skin_from: Some("Notch".into()),
            owner: None,
            paused: true,
            kit: Some("iron".into()),
            playstyle: None,
        });
        s.remember("Marco");
        s.remember("marco");
        let back = BotStore::load(&crate::yaml::parse(&crate::yaml::dump(&s.save())).unwrap());
        assert_eq!(back.bots, s.bots);
        assert_eq!(back.history, vec!["Marco"]);
    }
}
