//! Factions and overfactions: members (bots and players, by name), war, peace and alliances,
//! leaders and their calls to arms, standing formations, uniforms and random kit tables. Kept in
//! `factions.yml` in YAML.

use crate::settings::{Layer, Value};
use crate::yaml::Yaml;
use std::collections::BTreeMap;

/// The faction every real player is put in when they join without one.
pub const PLAYER_FACTION: &str = "player";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Faction {
    pub members: Vec<String>,
    pub enemies: Vec<String>,
    pub allies: Vec<String>,
    pub peace: Vec<String>,
    pub leader: Option<String>,
    pub formation: Option<(String, Option<u32>)>,
    /// A uniform: every bot in this account's skin.
    pub skin: Option<String>,
    /// Kit name -> weight (0..1000).
    pub kits: Vec<(String, u32)>,
    pub settings: Layer,
    /// The playstyle set for the whole faction.
    pub playstyle: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overfaction {
    pub factions: Vec<String>,
    pub enemies: Vec<String>,
    pub peace: Vec<String>,
    /// Not kept over a restart.
    pub leader: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Same,
    Allied,
    War,
    Peace,
    Neutral,
}

#[derive(Clone, Debug, Default)]
pub struct Factions {
    pub factions: BTreeMap<String, Faction>,
    pub overfactions: BTreeMap<String, Overfaction>,
    /// Member name (lower case) -> faction.
    index: BTreeMap<String, String>,
    /// Active leader calls: (faction, target name lower case) -> ticks left.
    pub calls: BTreeMap<(String, String), u32>,
}

fn lc(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn push_unique(v: &mut Vec<String>, s: &str) {
    if !v.iter().any(|x| x.eq_ignore_ascii_case(s)) {
        v.push(s.to_owned());
    }
}

fn remove(v: &mut Vec<String>, s: &str) {
    v.retain(|x| !x.eq_ignore_ascii_case(s));
}

impl Factions {
    pub fn reindex(&mut self) {
        self.index.clear();
        for (name, f) in &self.factions {
            for m in &f.members {
                self.index.insert(lc(m), name.clone());
            }
        }
    }

    pub fn faction_of(&self, member: &str) -> Option<&str> {
        // (No factions, the common case with hundreds of bots: no key to build.)
        if self.index.is_empty() {
            return None;
        }
        self.index.get(&lc(member)).map(String::as_str)
    }

    pub fn overfaction_of(&self, faction: &str) -> Option<&str> {
        self.overfactions
            .iter()
            .find(|(_, o)| o.factions.iter().any(|f| f == faction))
            .map(|(n, _)| n.as_str())
    }

    pub fn create(&mut self, name: &str) -> Result<(), String> {
        let name = lc(name);
        if !valid_name(&name) {
            return Err("Faction names are letters, digits, _ and -".into());
        }
        if self.factions.contains_key(&name) {
            return Err(format!("Faction {name} already exists"));
        }
        self.factions.insert(name, Faction::default());
        Ok(())
    }

    pub fn disband(&mut self, name: &str) -> Result<(), String> {
        let name = lc(name);
        if self.factions.remove(&name).is_none() {
            return Err(format!("No faction called {name}"));
        }
        for f in self.factions.values_mut() {
            remove(&mut f.enemies, &name);
            remove(&mut f.allies, &name);
            remove(&mut f.peace, &name);
        }
        for o in self.overfactions.values_mut() {
            remove(&mut o.factions, &name);
            remove(&mut o.enemies, &name);
            remove(&mut o.peace, &name);
        }
        self.calls.retain(|(f, _), _| *f != name);
        self.reindex();
        Ok(())
    }

    /// Everyone is in one faction at a time: joining leaves the old one.
    pub fn join(&mut self, faction: &str, member: &str) -> Result<(), String> {
        let faction = lc(faction);
        if !self.factions.contains_key(&faction) {
            return Err(format!("No faction called {faction}"));
        }
        self.leave(member);
        let f = self.factions.get_mut(&faction).unwrap();
        push_unique(&mut f.members, member);
        self.index.insert(lc(member), faction);
        Ok(())
    }

    pub fn leave(&mut self, member: &str) -> Option<String> {
        let old = self.index.remove(&lc(member))?;
        if let Some(f) = self.factions.get_mut(&old) {
            remove(&mut f.members, member);
            if f.leader.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(member)) {
                f.leader = None;
            }
        }
        Some(old)
    }

    /// A real player joining: into `player` unless they are in a faction already.
    pub fn player_joined(&mut self, name: &str) {
        if self.faction_of(name).is_some() {
            return;
        }
        if !self.factions.contains_key(PLAYER_FACTION) {
            self.factions.insert(PLAYER_FACTION.into(), Faction::default());
        }
        let _ = self.join(PLAYER_FACTION, name);
    }

    fn pair(&mut self, a: &str, b: &str) -> Result<(String, String), String> {
        let (a, b) = (lc(a), lc(b));
        if a == b {
            return Err("A faction can't do that with itself".into());
        }
        for n in [&a, &b] {
            if !self.factions.contains_key(n) {
                return Err(format!("No faction called {n}"));
            }
        }
        Ok((a, b))
    }

    /// War: ends any alliance or peace.
    pub fn hostile(&mut self, a: &str, b: &str) -> Result<(), String> {
        let (a, b) = self.pair(a, b)?;
        for (x, y) in [(&a, &b), (&b, &a)] {
            let f = self.factions.get_mut(x).unwrap();
            remove(&mut f.allies, y);
            remove(&mut f.peace, y);
            push_unique(&mut f.enemies, y);
        }
        Ok(())
    }

    pub fn peace(&mut self, a: &str, b: &str) -> Result<(), String> {
        let (a, b) = self.pair(a, b)?;
        for (x, y) in [(&a, &b), (&b, &a)] {
            let f = self.factions.get_mut(x).unwrap();
            remove(&mut f.enemies, y);
            push_unique(&mut f.peace, y);
        }
        Ok(())
    }

    pub fn ally(&mut self, a: &str, b: &str) -> Result<(), String> {
        let (a, b) = self.pair(a, b)?;
        for (x, y) in [(&a, &b), (&b, &a)] {
            let f = self.factions.get_mut(x).unwrap();
            remove(&mut f.enemies, y);
            remove(&mut f.peace, y);
            push_unique(&mut f.allies, y);
        }
        Ok(())
    }

    pub fn unally(&mut self, a: &str, b: &str) -> Result<(), String> {
        let (a, b) = self.pair(a, b)?;
        for (x, y) in [(&a, &b), (&b, &a)] {
            remove(&mut self.factions.get_mut(x).unwrap().allies, y);
        }
        Ok(())
    }

    /// The relation between two factions (None = no faction), overfactions included.
    pub fn relation(&self, a: Option<&str>, b: Option<&str>) -> Relation {
        let (Some(a), Some(b)) = (a, b) else {
            return Relation::Neutral;
        };
        if a == b {
            return Relation::Same;
        }
        let (oa, ob) = (self.overfaction_of(a), self.overfaction_of(b));
        if oa.is_some() && oa == ob {
            return Relation::Allied;
        }
        let fa = self.factions.get(a);
        let has = |list: Option<&Vec<String>>, n: &str| list.is_some_and(|l| l.iter().any(|x| x == n));
        if has(fa.map(|f| &f.allies), b) {
            return Relation::Allied;
        }
        if has(fa.map(|f| &f.enemies), b) {
            return Relation::War;
        }
        if has(fa.map(|f| &f.peace), b) {
            return Relation::Peace;
        }
        // Coalition-level relations: overfaction vs overfaction, or vs a single faction.
        let o_rel = |o: Option<&str>, other_f: &str, other_o: Option<&str>| -> Option<Relation> {
            let of = self.overfactions.get(o?)?;
            let hit = |l: &Vec<String>| {
                l.iter().any(|x| x == other_f || other_o.is_some_and(|oo| oo == x))
            };
            if hit(&of.enemies) {
                Some(Relation::War)
            } else if hit(&of.peace) {
                Some(Relation::Peace)
            } else {
                None
            }
        };
        o_rel(oa, b, ob)
            .or_else(|| o_rel(ob, a, oa))
            .unwrap_or(Relation::Neutral)
    }

    pub fn set_leader(&mut self, faction: &str, leader: Option<&str>) -> Result<(), String> {
        let faction = lc(faction);
        if !self.factions.contains_key(&faction) {
            return Err(format!("No faction called {faction}"));
        }
        if let Some(l) = leader {
            // The leader is always a member.
            self.join(&faction, l)?;
        }
        self.factions.get_mut(&faction).unwrap().leader = leader.map(str::to_owned);
        Ok(())
    }

    pub fn leader_of(&self, faction: &str) -> Option<&str> {
        self.factions.get(faction)?.leader.as_deref()
    }

    /// The leader of `faction` swung at, or was hit by, `target`: the faction turns on them
    /// (allies of the faction are never rallied against).
    pub fn leader_call(&mut self, faction: &str, target: &str, target_faction: Option<&str>, seconds: u32) {
        if seconds == 0 {
            return;
        }
        if matches!(self.relation(Some(faction), target_faction), Relation::Allied | Relation::Same) {
            return;
        }
        self.calls.insert((faction.to_owned(), lc(target)), seconds * 20);
    }

    pub fn called_on(&self, faction: &str, target: &str) -> bool {
        self.calls.contains_key(&(faction.to_owned(), lc(target)))
    }

    pub fn tick_calls(&mut self) {
        self.calls.retain(|_, t| {
            *t = t.saturating_sub(1);
            *t > 0
        });
    }

    pub fn set_kit_weight(&mut self, faction: &str, kit: &str, weight: u32) -> Result<(), String> {
        let f = self
            .factions
            .get_mut(&lc(faction))
            .ok_or_else(|| format!("No faction called {faction}"))?;
        f.kits.retain(|(k, _)| k != kit);
        if weight > 0 {
            f.kits.push((kit.to_owned(), weight.min(1000)));
        }
        Ok(())
    }

    /// Rolls a kit from the faction's table.
    pub fn roll_kit(&self, faction: &str, rng: &mut crate::math::Rng) -> Option<String> {
        let f = self.factions.get(faction)?;
        let w: Vec<f64> = f.kits.iter().map(|(_, w)| *w as f64).collect();
        rng.weighted(&w).map(|i| f.kits[i].0.clone())
    }

    pub fn rename_kit(&mut self, old: &str, new: &str) {
        for f in self.factions.values_mut() {
            for (k, _) in f.kits.iter_mut() {
                if k == old {
                    *k = new.to_owned();
                }
            }
        }
    }

    // ---- overfactions ----

    pub fn of_create(&mut self, name: &str) -> Result<(), String> {
        let name = lc(name);
        if !valid_name(&name) {
            return Err("Overfaction names are letters, digits, _ and -".into());
        }
        if self.overfactions.contains_key(&name) {
            return Err(format!("Overfaction {name} already exists"));
        }
        self.overfactions.insert(name, Overfaction::default());
        Ok(())
    }

    pub fn of_disband(&mut self, name: &str) -> Result<(), String> {
        self.overfactions
            .remove(&lc(name))
            .map(|_| ())
            .ok_or_else(|| format!("No overfaction called {name}"))
    }

    pub fn of_add(&mut self, of: &str, faction: &str) -> Result<(), String> {
        let (of, faction) = (lc(of), lc(faction));
        if !self.factions.contains_key(&faction) {
            return Err(format!("No faction called {faction}"));
        }
        if !self.overfactions.contains_key(&of) {
            return Err(format!("No overfaction called {of}"));
        }
        self.of_remove(&faction);
        push_unique(&mut self.overfactions.get_mut(&of).unwrap().factions, &faction);
        Ok(())
    }

    pub fn of_remove(&mut self, faction: &str) -> bool {
        let mut any = false;
        for o in self.overfactions.values_mut() {
            let before = o.factions.len();
            remove(&mut o.factions, faction);
            any |= before != o.factions.len();
        }
        any
    }

    /// War or peace between a coalition and a coalition or a single faction.
    pub fn of_relation(&mut self, a: &str, b: &str, war: bool) -> Result<(), String> {
        let (a, b) = (lc(a), lc(b));
        if !self.overfactions.contains_key(&a) {
            return Err(format!("No overfaction called {a}"));
        }
        if !self.overfactions.contains_key(&b) && !self.factions.contains_key(&b) {
            return Err(format!("No overfaction or faction called {b}"));
        }
        let o = self.overfactions.get_mut(&a).unwrap();
        if war {
            remove(&mut o.peace, &b);
            push_unique(&mut o.enemies, &b);
        } else {
            remove(&mut o.enemies, &b);
            push_unique(&mut o.peace, &b);
        }
        if let Some(ob) = self.overfactions.get_mut(&b) {
            if war {
                remove(&mut ob.peace, &a);
                push_unique(&mut ob.enemies, &a);
            } else {
                remove(&mut ob.enemies, &a);
                push_unique(&mut ob.peace, &a);
            }
        }
        Ok(())
    }

    /// Every member name of the factions of a coalition.
    pub fn of_members(&self, of: &str) -> Vec<String> {
        let Some(o) = self.overfactions.get(&lc(of)) else {
            return Vec::new();
        };
        o.factions
            .iter()
            .filter_map(|f| self.factions.get(f))
            .flat_map(|f| f.members.iter().cloned())
            .collect()
    }

    // ---- the file ----

    pub fn load(doc: &Yaml) -> Self {
        let mut out = Factions::default();
        if let Some(fs) = doc.get("factions").and_then(Yaml::as_map) {
            for (name, v) in fs {
                let mut f = Faction {
                    members: v.get("members").map(Yaml::strings).unwrap_or_default(),
                    enemies: v.get("enemies").map(Yaml::strings).unwrap_or_default(),
                    allies: v.get("allies").map(Yaml::strings).unwrap_or_default(),
                    peace: v.get("peace").map(Yaml::strings).unwrap_or_default(),
                    leader: v.get("leader").and_then(Yaml::as_text),
                    skin: v.get("skin").and_then(Yaml::as_text),
                    playstyle: v.get("playstyle").and_then(Yaml::as_text),
                    ..Default::default()
                };
                if let Some(fm) = v.get("formation") {
                    if let Some(shape) = fm.get("shape").and_then(Yaml::as_text) {
                        let size = fm.get("size").and_then(Yaml::as_i64).map(|s| s.max(1) as u32);
                        f.formation = Some((shape, size));
                    }
                }
                if let Some(ks) = v.get("kits").and_then(Yaml::as_map) {
                    for (k, w) in ks {
                        if let Some(w) = w.as_i64() {
                            f.kits.push((k.clone(), w.clamp(0, 1000) as u32));
                        }
                    }
                }
                if let Some(ss) = v.get("settings").and_then(Yaml::as_map) {
                    for (k, sv) in ss {
                        if let Some(spec) = crate::settings::spec(k) {
                            if let Some(val) = yaml_value(sv) {
                                f.settings.insert(k.clone(), spec.coerce(&val));
                            }
                        }
                    }
                }
                out.factions.insert(name.clone(), f);
            }
        }
        if let Some(os) = doc.get("overfactions").and_then(Yaml::as_map) {
            for (name, v) in os {
                out.overfactions.insert(
                    name.clone(),
                    Overfaction {
                        factions: v.get("factions").map(Yaml::strings).unwrap_or_default(),
                        enemies: v.get("enemies").map(Yaml::strings).unwrap_or_default(),
                        peace: v.get("peace").map(Yaml::strings).unwrap_or_default(),
                        leader: None,
                    },
                );
            }
        }
        out.reindex();
        out
    }

    pub fn save(&self) -> Yaml {
        let mut fs = Yaml::map();
        for (name, f) in &self.factions {
            let mut v = Yaml::map();
            v.set("members", Yaml::str_seq(f.members.iter().cloned()));
            v.set("enemies", Yaml::str_seq(f.enemies.iter().cloned()));
            v.set("allies", Yaml::str_seq(f.allies.iter().cloned()));
            v.set("peace", Yaml::str_seq(f.peace.iter().cloned()));
            if let Some(l) = &f.leader {
                v.set("leader", Yaml::Str(l.clone()));
            }
            if let Some((shape, size)) = &f.formation {
                let mut fm = Yaml::map();
                fm.set("shape", Yaml::Str(shape.clone()));
                if let Some(s) = size {
                    fm.set("size", Yaml::Int(i64::from(*s)));
                }
                v.set("formation", fm);
            }
            if let Some(s) = &f.skin {
                v.set("skin", Yaml::Str(s.clone()));
            }
            if let Some(p) = &f.playstyle {
                v.set("playstyle", Yaml::Str(p.clone()));
            }
            if !f.kits.is_empty() {
                let mut ks = Yaml::map();
                for (k, w) in &f.kits {
                    ks.set(k, Yaml::Int(i64::from(*w)));
                }
                v.set("kits", ks);
            }
            if !f.settings.is_empty() {
                let mut ss = Yaml::map();
                for (k, val) in &f.settings {
                    ss.set(k, val.to_yaml());
                }
                v.set("settings", ss);
            }
            fs.set(name, v);
        }
        let mut doc = Yaml::map();
        doc.set("factions", fs);
        if !self.overfactions.is_empty() {
            let mut os = Yaml::map();
            for (name, o) in &self.overfactions {
                let mut v = Yaml::map();
                v.set("factions", Yaml::str_seq(o.factions.iter().cloned()));
                v.set("enemies", Yaml::str_seq(o.enemies.iter().cloned()));
                v.set("peace", Yaml::str_seq(o.peace.iter().cloned()));
                os.set(name, v);
            }
            doc.set("overfactions", os);
        }
        doc
    }

    /// The faction layers for the settings resolver.
    pub fn settings_layers(&self) -> BTreeMap<String, Layer> {
        self.factions
            .iter()
            .filter(|(_, f)| !f.settings.is_empty())
            .map(|(n, f)| (n.clone(), f.settings.clone()))
            .collect()
    }
}

pub fn yaml_value(v: &Yaml) -> Option<Value> {
    Some(match v {
        Yaml::Bool(b) => Value::Bool(*b),
        Yaml::Int(i) => Value::Int(*i),
        Yaml::Float(f) => Value::Num(*f),
        Yaml::Str(s) => Value::Text(s.clone()),
        _ => return None,
    })
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 32 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Factions {
        let mut f = Factions::default();
        f.create("bandits").unwrap();
        f.create("guards").unwrap();
        f
    }

    #[test]
    fn one_faction_at_a_time() {
        let mut f = two();
        f.join("bandits", "Bob").unwrap();
        f.join("guards", "bob").unwrap();
        assert_eq!(f.faction_of("BOB"), Some("guards"));
        assert!(f.factions["bandits"].members.is_empty());
    }

    #[test]
    fn war_and_peace_rule_each_other_out() {
        let mut f = two();
        f.peace("bandits", "guards").unwrap();
        assert_eq!(f.relation(Some("bandits"), Some("guards")), Relation::Peace);
        f.hostile("bandits", "guards").unwrap();
        assert_eq!(f.relation(Some("guards"), Some("bandits")), Relation::War);
        assert!(f.factions["bandits"].peace.is_empty());
        f.ally("bandits", "guards").unwrap();
        assert_eq!(f.relation(Some("bandits"), Some("guards")), Relation::Allied);
        assert!(f.factions["guards"].enemies.is_empty());
        assert!(f.hostile("bandits", "bandits").is_err());
    }

    #[test]
    fn overfactions_ally_their_members_and_carry_war() {
        let mut f = two();
        f.create("pirates").unwrap();
        f.create("knights").unwrap();
        f.of_create("north").unwrap();
        f.of_create("south").unwrap();
        f.of_add("north", "bandits").unwrap();
        f.of_add("north", "pirates").unwrap();
        f.of_add("south", "guards").unwrap();
        assert_eq!(f.relation(Some("bandits"), Some("pirates")), Relation::Allied);
        assert_eq!(f.relation(Some("bandits"), Some("guards")), Relation::Neutral);
        f.of_relation("north", "south", true).unwrap();
        assert_eq!(f.relation(Some("pirates"), Some("guards")), Relation::War);
        f.of_relation("north", "knights", true).unwrap();
        assert_eq!(f.relation(Some("knights"), Some("bandits")), Relation::War);
    }

    #[test]
    fn leader_calls_expire_and_spare_allies() {
        let mut f = two();
        f.set_leader("bandits", Some("Boss")).unwrap();
        assert_eq!(f.faction_of("boss"), Some("bandits"));
        f.leader_call("bandits", "Steve", Some("player"), 1);
        assert!(f.called_on("bandits", "steve"));
        for _ in 0..20 {
            f.tick_calls();
        }
        assert!(!f.called_on("bandits", "steve"));
        f.ally("bandits", "guards").unwrap();
        f.leader_call("bandits", "Gary", Some("guards"), 30);
        assert!(!f.called_on("bandits", "gary"));
    }

    #[test]
    fn the_file_round_trips() {
        let mut f = two();
        f.join("bandits", "b1").unwrap();
        f.hostile("bandits", "guards").unwrap();
        f.set_kit_weight("bandits", "iron", 30).unwrap();
        f.factions.get_mut("bandits").unwrap().formation = Some(("rows".into(), Some(4)));
        f.factions
            .get_mut("guards")
            .unwrap()
            .settings
            .insert("skill".into(), Value::Int(80));
        f.of_create("north").unwrap();
        f.of_add("north", "bandits").unwrap();
        let doc = f.save();
        let back = Factions::load(&crate::yaml::parse(&crate::yaml::dump(&doc)).unwrap());
        assert_eq!(back.factions, f.factions);
        assert_eq!(back.overfactions, f.overfactions);
        assert_eq!(back.faction_of("B1"), Some("bandits"));
    }

    #[test]
    fn players_land_in_the_player_faction_once() {
        let mut f = two();
        f.player_joined("Steve");
        assert_eq!(f.faction_of("steve"), Some(PLAYER_FACTION));
        f.join("guards", "Steve").unwrap();
        f.player_joined("Steve");
        assert_eq!(f.faction_of("steve"), Some("guards"));
    }
}
