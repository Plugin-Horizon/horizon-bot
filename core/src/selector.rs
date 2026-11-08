//! Selectors: one bot, many, or all (`Marco`, `all`, `latest`, `faction <name>`,
//! `overfaction <name>`, `random[limit=5,nofaction=true]`, `list:a,b,c`), and targets
//! (`<player|bot|x y z>`).

use crate::math::Vec3;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RandomFilter {
    pub limit: usize,
    pub nofaction: Option<bool>,
    pub paused: Option<bool>,
    pub world: Option<String>,
    pub alreadyhaskit: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Selector {
    Name(String),
    All,
    Latest,
    Faction(String),
    Overfaction(String),
    Random(RandomFilter),
    List(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A player or a bot, by name.
    Entity(String),
    /// Coordinates; `relative[i]` for a `~` coordinate (added to the sender's position).
    Pos { pos: Vec3, relative: [bool; 3] },
}

impl Target {
    pub fn resolve_pos(&self, origin: Vec3) -> Option<Vec3> {
        match self {
            Target::Entity(_) => None,
            Target::Pos { pos, relative } => Some(Vec3::new(
                if relative[0] { origin.x + pos.x } else { pos.x },
                if relative[1] { origin.y + pos.y } else { pos.y },
                if relative[2] { origin.z + pos.z } else { pos.z },
            )),
        }
    }
}

/// Words left to read in a command line.
#[derive(Clone, Debug)]
pub struct Words<'a> {
    pub words: Vec<&'a str>,
    pub at: usize,
}

impl<'a> Words<'a> {
    pub fn new(line: &'a str) -> Self {
        Words {
            words: line.split_whitespace().collect(),
            at: 0,
        }
    }
    pub fn peek(&self) -> Option<&'a str> {
        self.words.get(self.at).copied()
    }
    pub fn peek_at(&self, n: usize) -> Option<&'a str> {
        self.words.get(self.at + n).copied()
    }
    pub fn next(&mut self) -> Option<&'a str> {
        let w = self.peek();
        if w.is_some() {
            self.at += 1;
        }
        w
    }
    pub fn need(&mut self, what: &str) -> Result<&'a str, String> {
        self.next().ok_or_else(|| format!("Missing {what}"))
    }
    pub fn left(&self) -> usize {
        self.words.len().saturating_sub(self.at)
    }
    pub fn rest(&mut self) -> String {
        let r = self.words[self.at.min(self.words.len())..].join(" ");
        self.at = self.words.len();
        r
    }
    pub fn eat(&mut self, word: &str) -> bool {
        if self.peek().is_some_and(|w| w.eq_ignore_ascii_case(word)) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    pub fn number(&mut self, what: &str) -> Result<f64, String> {
        let w = self.need(what)?;
        w.parse().map_err(|_| format!("{what} must be a number, not {w}"))
    }
    pub fn opt_number(&mut self) -> Option<f64> {
        let v = self.peek()?.parse().ok()?;
        self.at += 1;
        Some(v)
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

impl Selector {
    /// Reads one selector. `full` allows `latest`, `random[...]`, `overfaction` and `list:`;
    /// the short form (`guard`, `pause`, `tp`) takes a name, `all` or `faction <name>` only.
    pub fn read(w: &mut Words, full: bool) -> Result<Selector, String> {
        let first = w.need("bots")?;
        let lower = first.to_ascii_lowercase();
        match lower.as_str() {
            "all" | "*" => return Ok(Selector::All),
            "faction" | "f" => return Ok(Selector::Faction(w.need("faction name")?.to_ascii_lowercase())),
            _ => {}
        }
        if full {
            match lower.as_str() {
                "latest" => return Ok(Selector::Latest),
                "overfaction" | "of" => {
                    return Ok(Selector::Overfaction(w.need("overfaction name")?.to_ascii_lowercase()))
                }
                _ => {}
            }
            if let Some(list) = lower.strip_prefix("list:") {
                let _ = list;
                let names = first[5..].split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect();
                return Ok(Selector::List(names));
            }
            if lower == "random" {
                return Ok(Selector::Random(RandomFilter { limit: 1, ..Default::default() }));
            }
            if lower.starts_with("random[") {
                let mut text = first[7..].to_owned();
                while !text.contains(']') {
                    let more = w.next().ok_or("random[...] is missing its ]")?;
                    text.push(',');
                    text.push_str(more);
                }
                let inner = text.split(']').next().unwrap_or("");
                let mut f = RandomFilter { limit: 1, ..Default::default() };
                for kv in inner.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let (k, v) = kv.split_once('=').ok_or_else(|| format!("Bad filter {kv}"))?;
                    let (k, v) = (k.trim(), v.trim());
                    match k {
                        "limit" => f.limit = v.parse().map_err(|_| format!("limit must be a number, not {v}"))?,
                        "nofaction" => f.nofaction = Some(parse_bool(v).ok_or("nofaction is true or false")?),
                        "paused" => f.paused = Some(parse_bool(v).ok_or("paused is true or false")?),
                        "alreadyhaskit" => f.alreadyhaskit = Some(parse_bool(v).ok_or("alreadyhaskit is true or false")?),
                        "world" => f.world = Some(v.to_owned()),
                        other => return Err(format!("Unknown filter {other}")),
                    }
                }
                return Ok(Selector::Random(f));
            }
        }
        Ok(Selector::Name(first.to_owned()))
    }
}

/// Reads `<player|bot|x y z>`: three numbers (or `~`) make coordinates.
pub fn read_target(w: &mut Words) -> Result<Target, String> {
    let first = w.need("a player, a bot or x y z")?;
    let coord = |s: &str| -> Option<(f64, bool)> {
        if let Some(r) = s.strip_prefix('~') {
            Some((if r.is_empty() { 0.0 } else { r.parse().ok()? }, true))
        } else {
            s.parse().ok().map(|v| (v, false))
        }
    };
    if let Some(x) = coord(first) {
        let y = w.next().and_then(coord).ok_or("Coordinates need x y z")?;
        let z = w.next().and_then(coord).ok_or("Coordinates need x y z")?;
        return Ok(Target::Pos {
            pos: Vec3::new(x.0, y.0, z.0),
            relative: [x.1, y.1, z.1],
        });
    }
    Ok(Target::Entity(first.to_owned()))
}

/// What a selector picks from, for each bot.
pub struct Candidate<'a> {
    pub name: &'a str,
    pub faction: Option<&'a str>,
    pub paused: bool,
    pub world: &'a str,
    pub has_items: bool,
}

/// The names a selector picks. `latest` is the last spawn's bots; `of_factions` the factions of an
/// overfaction (looked up by the caller).
pub fn pick(
    sel: &Selector,
    bots: &[Candidate],
    latest: &[String],
    of_factions: &dyn Fn(&str) -> Vec<String>,
    rng: &mut crate::math::Rng,
) -> Vec<String> {
    let names = |it: &mut dyn Iterator<Item = &Candidate>| it.map(|c| c.name.to_owned()).collect::<Vec<_>>();
    match sel {
        Selector::Name(n) => names(&mut bots.iter().filter(|c| c.name.eq_ignore_ascii_case(n))),
        Selector::All => names(&mut bots.iter()),
        Selector::Latest => names(&mut bots.iter().filter(|c| latest.iter().any(|l| l.eq_ignore_ascii_case(c.name)))),
        Selector::Faction(f) => names(&mut bots.iter().filter(|c| c.faction == Some(f.as_str()))),
        Selector::Overfaction(o) => {
            let fs = of_factions(o);
            names(&mut bots.iter().filter(|c| c.faction.is_some_and(|f| fs.iter().any(|x| x == f))))
        }
        Selector::List(l) => names(&mut bots.iter().filter(|c| l.iter().any(|n| n.eq_ignore_ascii_case(c.name)))),
        Selector::Random(f) => {
            let mut pool: Vec<&Candidate> = bots
                .iter()
                .filter(|c| f.nofaction.is_none_or(|nf| nf == c.faction.is_none()))
                .filter(|c| f.paused.is_none_or(|p| p == c.paused))
                .filter(|c| {
                    f.world.as_ref().is_none_or(|w| {
                        w.eq_ignore_ascii_case(c.world) || crate::world::dimension_id(w) == crate::world::dimension_id(c.world)
                    })
                })
                .filter(|c| f.alreadyhaskit.is_none_or(|k| k == c.has_items))
                .collect();
            rng.shuffle(&mut pool);
            pool.into_iter().take(f.limit.max(1)).map(|c| c.name.to_owned()).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_form() {
        let mut w = Words::new("faction red Steve");
        assert_eq!(Selector::read(&mut w, true).unwrap(), Selector::Faction("red".into()));
        assert_eq!(w.next(), Some("Steve"));
        let mut w = Words::new("random[limit=5, nofaction=true] x");
        assert_eq!(
            Selector::read(&mut w, true).unwrap(),
            Selector::Random(RandomFilter { limit: 5, nofaction: Some(true), ..Default::default() })
        );
        assert_eq!(w.next(), Some("x"));
        let mut w = Words::new("list:a,b,c");
        assert_eq!(Selector::read(&mut w, true).unwrap(), Selector::List(vec!["a".into(), "b".into(), "c".into()]));
        let mut w = Words::new("latest");
        assert_eq!(Selector::read(&mut w, false).unwrap(), Selector::Name("latest".into()));
        let mut w = Words::new("of north");
        assert_eq!(Selector::read(&mut w, true).unwrap(), Selector::Overfaction("north".into()));
    }

    #[test]
    fn targets() {
        let mut w = Words::new("100 64 -250");
        assert_eq!(
            read_target(&mut w).unwrap(),
            Target::Pos { pos: Vec3::new(100.0, 64.0, -250.0), relative: [false; 3] }
        );
        let mut w = Words::new("~ ~5 ~-2");
        let t = read_target(&mut w).unwrap();
        assert_eq!(t.resolve_pos(Vec3::new(1.0, 2.0, 3.0)), Some(Vec3::new(1.0, 7.0, 1.0)));
        let mut w = Words::new("Steve 30");
        assert_eq!(read_target(&mut w).unwrap(), Target::Entity("Steve".into()));
    }

    #[test]
    fn random_filters_pick() {
        let bots = [
            Candidate { name: "a", faction: None, paused: false, world: "world", has_items: false },
            Candidate { name: "b", faction: Some("red"), paused: false, world: "world", has_items: true },
            Candidate { name: "c", faction: None, paused: true, world: "nether", has_items: false },
        ];
        let mut rng = crate::math::Rng::new(1);
        let none = |_: &str| Vec::new();
        let f = Selector::Random(RandomFilter { limit: 10, nofaction: Some(true), paused: Some(false), ..Default::default() });
        assert_eq!(pick(&f, &bots, &[], &none, &mut rng), vec!["a"]);
        assert_eq!(pick(&Selector::Faction("red".into()), &bots, &[], &none, &mut rng), vec!["b"]);
        assert_eq!(pick(&Selector::Latest, &bots, &["C".into()], &none, &mut rng), vec!["c"]);
        let of = |_: &str| vec!["red".to_owned()];
        assert_eq!(pick(&Selector::Overfaction("x".into()), &bots, &[], &of, &mut rng), vec!["b"]);
    }
}
