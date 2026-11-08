//! Bot names and skins: generated names, name tables, the skin pool with its cache
//! (`skin-cache.yml`), and who wears what.

use crate::math::Rng;
use crate::yaml::Yaml;
use std::collections::{BTreeMap, BTreeSet};

/// A skin as the session server gives it: the textures property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skin {
    pub value: String,
    pub signature: String,
}

impl Skin {
    /// The textures JSON's skin URL (to tell two accounts with the same skin apart).
    pub fn texture_url(&self) -> Option<String> {
        let json = decode_base64(&self.value)?;
        let text = String::from_utf8(json).ok()?;
        let at = text.find("\"url\"")?;
        let rest = &text[at + 5..];
        let start = rest.find('"')? + 1;
        let end = rest[start..].find('"')? + start;
        Some(rest[start..end].to_owned())
    }
}

pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RandomSkins {
    /// Bots whose own name has no skin.
    Missing,
    /// Every bot.
    Always,
    Off,
}

impl RandomSkins {
    pub fn parse(s: &str) -> Self {
        match s {
            "always" => RandomSkins::Always,
            "off" | "false" => RandomSkins::Off,
            _ => RandomSkins::Missing,
        }
    }
}

/// The built-in pool: ordinary words and first names, most of them taken accounts.
pub const BUILT_IN_POOL: &[&str] = &[
    "Adam", "Alex", "Alice", "Amber", "Andy", "Anna", "Apple", "Arrow", "Ash", "Ava", "Bacon", "Bandit", "Basil",
    "Bear", "Ben", "Berry", "Blaze", "Bliss", "Bolt", "Brick", "Bruno", "Buddy", "Cake", "Carl", "Cedar", "Chloe",
    "Clara", "Cloud", "Coal", "Comet", "Cookie", "Copper", "Coral", "Daisy", "Dan", "Dash", "Dawn", "Delta", "Dino",
    "Drift", "Dusk", "Eagle", "Echo", "Eden", "Elsa", "Ember", "Emma", "Eva", "Falcon", "Felix", "Fern", "Finn",
    "Flint", "Fox", "Frost", "Gale", "George", "Ghost", "Grace", "Hank", "Harry", "Hawk", "Hazel", "Holly", "Honey",
    "Hugo", "Ice", "Iris", "Isaac", "Ivy", "Jack", "Jade", "Jake", "James", "Jay", "Jazz", "Jelly", "Jim", "Joe",
    "Juno", "Kate", "Kiwi", "Lars", "Lily", "Lime", "Lucy", "Luke", "Luna", "Mango", "Mark", "Max", "Maya", "Mia",
    "Mint", "Moss", "Nina", "Noah", "Nova", "Oak", "Olive", "Oscar", "Owl", "Pearl", "Pepper", "Pine", "Pixel",
    "Plum", "Quinn", "Rain", "Raven", "Ray", "Reed", "Rex", "Rose", "Ruby", "Rust", "Sage", "Sam", "Sky", "Snow",
    "Sofia", "Spark", "Stone", "Storm", "Sunny", "Tara", "Tiger", "Toby", "Tom", "Vera", "Wolf", "Zane", "Zoe",
];

/// The skin pool: names to look up, the cache of what came back, and the order bots get them.
#[derive(Clone, Debug, Default)]
pub struct SkinPool {
    /// Lower-case account name -> skin (or None: no skin, a default one).
    pub cache: BTreeMap<String, Option<Skin>>,
    /// Pool names in use order.
    pub names: Vec<String>,
    /// Skins already handed out in the current round (no repeats until all are used).
    used: BTreeSet<String>,
}

impl SkinPool {
    pub fn new(built_in: bool, extra: &[String]) -> Self {
        let mut names: Vec<String> = Vec::new();
        if built_in {
            names.extend(BUILT_IN_POOL.iter().map(|s| (*s).to_owned()));
        }
        for e in extra {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(e)) {
                names.push(e.clone());
            }
        }
        SkinPool {
            cache: BTreeMap::new(),
            names,
            used: BTreeSet::new(),
        }
    }

    /// Names still to look up (not in the cache).
    pub fn missing(&self) -> Vec<String> {
        self.names
            .iter()
            .filter(|n| !self.cache.contains_key(&n.to_ascii_lowercase()))
            .cloned()
            .collect()
    }

    pub fn put(&mut self, name: &str, skin: Option<Skin>) {
        self.cache.insert(name.to_ascii_lowercase(), skin);
    }

    pub fn get(&self, name: &str) -> Option<&Skin> {
        self.cache.get(&name.to_ascii_lowercase()).and_then(Option::as_ref)
    }

    /// The pool's usable skins: looked up, not default, and each texture once.
    pub fn usable(&self) -> Vec<(String, Skin)> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for n in &self.names {
            if let Some(s) = self.get(n) {
                let key = s.texture_url().unwrap_or_else(|| s.value.clone());
                if seen.insert(key) {
                    out.push((n.clone(), s.clone()));
                }
            }
        }
        out
    }

    /// The next random skin, going through every skin before any repeats.
    pub fn next_random(&mut self, rng: &mut Rng) -> Option<(String, Skin)> {
        let usable = self.usable();
        if usable.is_empty() {
            return None;
        }
        let fresh: Vec<&(String, Skin)> = usable.iter().filter(|(n, _)| !self.used.contains(n)).collect();
        let pick = if fresh.is_empty() {
            self.used.clear();
            rng.pick(&usable)?.clone()
        } else {
            (*rng.pick(&fresh)?).clone()
        };
        self.used.insert(pick.0.clone());
        Some(pick)
    }

    pub fn load_cache(&mut self, doc: &Yaml) {
        if let Some(m) = doc.get("skins").and_then(Yaml::as_map) {
            for (name, v) in m {
                let skin = match (v.get("value").and_then(Yaml::as_text), v.get("signature").and_then(Yaml::as_text)) {
                    (Some(value), Some(signature)) => Some(Skin { value, signature }),
                    _ => None,
                };
                self.cache.insert(name.to_ascii_lowercase(), skin);
            }
        }
    }

    pub fn save_cache(&self) -> Yaml {
        let mut m = Yaml::map();
        for (name, s) in &self.cache {
            let mut v = Yaml::map();
            if let Some(s) = s {
                v.set("value", Yaml::Str(s.value.clone()));
                v.set("signature", Yaml::Str(s.signature.clone()));
            } else {
                v.set("none", Yaml::Bool(true));
            }
            m.set(name, v);
        }
        let mut doc = Yaml::map();
        doc.set("skins", m);
        doc
    }
}

/// What a new bot wears: an explicit account, its own name's, or one from the pool.
#[derive(Clone, Debug, PartialEq)]
pub enum SkinChoice {
    /// No lookup at all (`spawn <name> none`): the default skin, or the pool if it has one now.
    None,
    /// This account's skin.
    From(String),
    /// Its own name's account, falling back to the pool by the `random` rule.
    Own,
}

pub fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

const STARTS: &[&str] = &[
    "Ka", "Zo", "Mi", "Ra", "Ve", "Lu", "To", "Fen", "Gri", "Bo", "Sa", "Ny", "Ar", "El", "Dra", "Qui", "Hex", "Jo",
    "Pa", "Tre", "Vi", "Wo", "Xa", "Yu", "Cro", "Sty", "Bla", "Kro", "Mor", "Sil",
];
const MIDS: &[&str] = &["", "ri", "na", "lo", "ve", "do", "ka", "mi", "ze", "ra", "th", "nd", "ck", "x", "rn"];
const ENDS: &[&str] = &[
    "", "o", "a", "us", "ix", "en", "er", "on", "y", "ie", "PvP", "_", "MC", "z", "ly", "Pro", "Gg", "ox",
];

/// A made-up player name, unique against `taken` (lower case).
pub fn generate_name(rng: &mut Rng, taken: &BTreeSet<String>) -> String {
    for attempt in 0..200 {
        let mut n = String::new();
        n.push_str(rng.pick(STARTS).unwrap());
        n.push_str(rng.pick(MIDS).unwrap());
        n.push_str(rng.pick(ENDS).unwrap());
        if rng.chance(0.45) || attempt > 50 {
            n.push_str(&rng.int(1, 999).to_string());
        }
        if n.ends_with('_') && rng.chance(0.5) {
            n.pop();
        }
        n.truncate(16);
        if valid_name(&n) && !taken.contains(&n.to_ascii_lowercase()) {
            return n;
        }
    }
    format!("Bot{}", rng.int(10000, 99999))
}

/// Up to `count` names from a table, skipping taken ones and never one twice.
pub fn from_table(table: &[String], count: usize, taken: &BTreeSet<String>, rng: &mut Rng) -> Vec<String> {
    let mut pool: Vec<&String> = table
        .iter()
        .filter(|n| valid_name(n) && !taken.contains(&n.to_ascii_lowercase()))
        .collect();
    let mut seen = BTreeSet::new();
    pool.retain(|n| seen.insert(n.to_ascii_lowercase()));
    rng.shuffle(&mut pool);
    pool.into_iter().take(count).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skin(url: &str) -> Skin {
        let json = format!("{{\"textures\":{{\"SKIN\":{{\"url\":\"{url}\"}}}}}}");
        Skin {
            value: encode(json.as_bytes()),
            signature: "sig".into(),
        }
    }

    fn encode(b: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in b.chunks(3) {
            let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= c.len() {
                    s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }

    #[test]
    fn a_round_uses_every_skin_once_and_skips_shared_textures() {
        let mut p = SkinPool::new(false, &["a".into(), "b".into(), "c".into(), "d".into()]);
        p.put("a", Some(skin("http://t/1")));
        p.put("b", Some(skin("http://t/2")));
        p.put("c", Some(skin("http://t/1")));
        p.put("d", None);
        assert_eq!(p.usable().len(), 2);
        let mut rng = Rng::new(3);
        let first: BTreeSet<String> = (0..2).map(|_| p.next_random(&mut rng).unwrap().0).collect();
        assert_eq!(first.len(), 2);
        assert!(p.next_random(&mut rng).is_some());
        let back = {
            let mut q = SkinPool::new(false, &[]);
            q.load_cache(&crate::yaml::parse(&crate::yaml::dump(&p.save_cache())).unwrap());
            q
        };
        assert_eq!(back.cache, p.cache);
    }

    #[test]
    fn names_are_valid_and_unique() {
        let mut rng = Rng::new(9);
        let mut taken = BTreeSet::new();
        for _ in 0..300 {
            let n = generate_name(&mut rng, &taken);
            assert!(valid_name(&n), "{n}");
            assert!(taken.insert(n.to_ascii_lowercase()), "{n}");
        }
        let table: Vec<String> = ["Alpha", "Bravo", "Charlie", "alpha"].iter().map(|s| s.to_string()).collect();
        let mut t = BTreeSet::new();
        t.insert("bravo".to_owned());
        let got = from_table(&table, 5, &t, &mut rng);
        assert_eq!(got.len(), 2);
        assert!(!got.iter().any(|g| g == "Bravo"));
    }
}
