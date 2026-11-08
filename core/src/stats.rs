//! The scoreboard of fights with a bot on at least one side (`stats.yml`, keyed by UUID).

use crate::yaml::Yaml;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub name: String,
    pub bot: bool,
    pub kills: u64,
    pub deaths: u64,
    /// Melee swings aimed at someone.
    pub swings: u64,
    /// Melee hits that landed.
    pub landed: u64,
    pub shot_hits: u64,
    pub dealt: f64,
    pub taken: f64,
}

impl Record {
    pub fn kd(&self) -> f64 {
        self.kills as f64 / (self.deaths.max(1)) as f64
    }
    pub fn accuracy(&self) -> f64 {
        if self.swings == 0 {
            0.0
        } else {
            self.landed as f64 / self.swings as f64
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Top {
    Kills,
    Kd,
    Accuracy,
    Damage,
}

impl Top {
    pub fn parse(s: &str) -> Option<Top> {
        Some(match s {
            "kills" => Top::Kills,
            "kd" => Top::Kd,
            "accuracy" => Top::Accuracy,
            "damage" => Top::Damage,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// UUID as text (with dashes) -> record.
    pub records: BTreeMap<String, Record>,
    pub dirty: bool,
}

pub fn uuid_text(u: u128) -> String {
    let h = format!("{u:032x}");
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub fn parse_uuid(s: &str) -> Option<u128> {
    let h: String = s.chars().filter(|c| *c != '-').collect();
    if h.len() != 32 {
        return None;
    }
    u128::from_str_radix(&h, 16).ok()
}

impl Stats {
    fn rec(&mut self, uuid: u128, name: &str, bot: bool) -> &mut Record {
        self.dirty = true;
        let r = self.records.entry(uuid_text(uuid)).or_default();
        r.name = name.to_owned();
        r.bot = bot;
        r
    }

    /// A melee swing by `who` at someone in a bot fight.
    pub fn swing(&mut self, uuid: u128, name: &str, bot: bool, landed: bool) {
        let r = self.rec(uuid, name, bot);
        r.swings += 1;
        if landed {
            r.landed += 1;
        }
    }

    pub fn shot_hit(&mut self, uuid: u128, name: &str, bot: bool) {
        self.rec(uuid, name, bot).shot_hits += 1;
    }

    pub fn damage(&mut self, from: (u128, &str, bool), to: (u128, &str, bool), amount: f64) {
        self.rec(from.0, from.1, from.2).dealt += amount;
        self.rec(to.0, to.1, to.2).taken += amount;
    }

    pub fn kill(&mut self, killer: Option<(u128, &str, bool)>, victim: (u128, &str, bool)) {
        if let Some(k) = killer {
            self.rec(k.0, k.1, k.2).kills += 1;
        }
        self.rec(victim.0, victim.1, victim.2).deaths += 1;
    }

    pub fn find(&self, name: &str) -> Option<&Record> {
        self.records.values().find(|r| r.name.eq_ignore_ascii_case(name))
    }

    pub fn reset(&mut self, name: &str) -> usize {
        let before = self.records.len();
        if name == "all" {
            self.records.clear();
        } else {
            self.records.retain(|_, r| !r.name.eq_ignore_ascii_case(name));
        }
        self.dirty = true;
        before - self.records.len()
    }

    pub fn top(&self, by: Top, n: usize) -> Vec<&Record> {
        let mut v: Vec<&Record> = self.records.values().collect();
        let key = |r: &Record| match by {
            Top::Kills => r.kills as f64,
            Top::Kd => r.kd(),
            Top::Accuracy => r.accuracy(),
            Top::Damage => r.dealt,
        };
        v.sort_by(|a, b| key(b).partial_cmp(&key(a)).unwrap_or(std::cmp::Ordering::Equal).then(a.name.cmp(&b.name)));
        v.truncate(n);
        v
    }

    pub fn load(doc: &Yaml) -> Self {
        let mut s = Stats::default();
        if let Some(m) = doc.as_map() {
            for (uuid, v) in m {
                let n = |k: &str| v.get(k).and_then(Yaml::as_i64).unwrap_or(0).max(0) as u64;
                let f = |k: &str| v.get(k).and_then(Yaml::as_f64).unwrap_or(0.0);
                s.records.insert(
                    uuid.clone(),
                    Record {
                        name: v.get("name").and_then(Yaml::as_text).unwrap_or_default(),
                        bot: v.get("bot").and_then(Yaml::as_bool).unwrap_or(false),
                        kills: n("kills"),
                        deaths: n("deaths"),
                        swings: n("swings"),
                        landed: n("landed"),
                        shot_hits: n("shot-hits"),
                        dealt: f("dealt"),
                        taken: f("taken"),
                    },
                );
            }
        }
        s
    }

    pub fn save(&self) -> Yaml {
        let mut doc = Yaml::map();
        for (uuid, r) in &self.records {
            let mut v = Yaml::map();
            v.set("name", Yaml::Str(r.name.clone()));
            v.set("bot", Yaml::Bool(r.bot));
            v.set("kills", Yaml::Int(r.kills as i64));
            v.set("deaths", Yaml::Int(r.deaths as i64));
            v.set("swings", Yaml::Int(r.swings as i64));
            v.set("landed", Yaml::Int(r.landed as i64));
            v.set("shot-hits", Yaml::Int(r.shot_hits as i64));
            v.set("dealt", Yaml::Float(r.dealt));
            v.set("taken", Yaml::Float(r.taken));
            doc.set(uuid, v);
        }
        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_ranks() {
        let mut s = Stats::default();
        s.swing(1, "Bot1", true, true);
        s.swing(1, "Bot1", true, false);
        s.damage((1, "Bot1", true), (2, "Steve", false), 6.5);
        s.kill(Some((1, "Bot1", true)), (2, "Steve", false));
        let b = s.find("bot1").unwrap();
        assert_eq!((b.kills, b.swings, b.landed), (1, 2, 1));
        assert_eq!(s.top(Top::Kills, 10)[0].name, "Bot1");
        let back = Stats::load(&crate::yaml::parse(&crate::yaml::dump(&s.save())).unwrap());
        assert_eq!(back.records, s.records);
        assert_eq!(s.reset("all"), 2);
    }

    #[test]
    fn uuids_round_trip() {
        let u = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdefu128;
        assert_eq!(parse_uuid(&uuid_text(u)), Some(u));
        assert_eq!(uuid_text(u), "01234567-89ab-cdef-0123-456789abcdef");
    }
}
