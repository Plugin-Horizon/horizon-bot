//! Scripts (`scripts.yml`): command lines saved under a name, `{1}`..`{9}` filled from the run's
//! arguments, `wait <ticks>` pausing without blocking the server, one script at a time.

use crate::yaml::Yaml;
use std::collections::{BTreeMap, VecDeque};

pub const MAX_LINES: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct Scripts {
    pub scripts: BTreeMap<String, Vec<String>>,
}

impl Scripts {
    pub fn load(doc: &Yaml) -> Self {
        let mut scripts = BTreeMap::new();
        if let Some(m) = doc.get("scripts").and_then(Yaml::as_map) {
            for (name, v) in m {
                scripts.insert(name.clone(), v.strings());
            }
        }
        Scripts { scripts }
    }

    pub fn save(&self) -> Yaml {
        let mut m = Yaml::map();
        for (name, lines) in &self.scripts {
            m.set(name, Yaml::str_seq(lines.iter().cloned()));
        }
        let mut doc = Yaml::map();
        doc.set("scripts", m);
        doc
    }

    pub fn create(&mut self, name: &str) -> Result<(), String> {
        if self.scripts.contains_key(name) {
            return Err(format!("Script {name} exists already"));
        }
        self.scripts.insert(name.to_owned(), Vec::new());
        Ok(())
    }

    pub fn add(&mut self, name: &str, line: &str) -> Result<usize, String> {
        let s = self.scripts.get_mut(name).ok_or_else(|| format!("No script called {name}"))?;
        if s.len() >= MAX_LINES {
            return Err(format!("A script has at most {MAX_LINES} lines"));
        }
        s.push(line.trim().trim_start_matches('/').to_owned());
        Ok(s.len())
    }

    pub fn remove_line(&mut self, name: &str, line: usize) -> Result<String, String> {
        let s = self.scripts.get_mut(name).ok_or_else(|| format!("No script called {name}"))?;
        if line == 0 || line > s.len() {
            return Err(format!("Script {name} has lines 1 to {}", s.len()));
        }
        Ok(s.remove(line - 1))
    }
}

/// Replaces `{1}`..`{9}` with the arguments; a missing one is left empty.
pub fn fill(line: &str, args: &[String]) -> String {
    let mut out = line.to_owned();
    for i in 1..=9 {
        out = out.replace(&format!("{{{i}}}"), args.get(i - 1).map(String::as_str).unwrap_or(""));
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Who runs a script: the lines run as them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sender {
    Console,
    Player(u128),
}

#[derive(Clone, Debug)]
struct Run {
    name: String,
    sender: Sender,
    lines: Vec<String>,
    next: usize,
    wait: u32,
    /// The bots this run's last spawn made (`latest` inside the script).
    latest: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Runner {
    current: Option<Run>,
    queue: VecDeque<Run>,
}

/// One line for the plugin to execute now.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub sender: Sender,
    pub line: String,
    /// What `latest` means for this line.
    pub latest: Vec<String>,
}

impl Runner {
    pub fn start(&mut self, scripts: &Scripts, name: &str, args: &[String], sender: Sender) -> Result<bool, String> {
        let lines = scripts.scripts.get(name).ok_or_else(|| format!("No script called {name}"))?;
        let lines = lines
            .iter()
            .map(|l| l.trim().trim_start_matches('/').to_owned())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| fill(&l, args))
            .collect();
        let run = Run {
            name: name.to_owned(),
            sender,
            lines,
            next: 0,
            wait: 0,
            latest: Vec::new(),
        };
        if self.current.is_some() {
            self.queue.push_back(run);
            Ok(false)
        } else {
            self.current = Some(run);
            Ok(true)
        }
    }

    pub fn running(&self) -> Option<&str> {
        self.current.as_ref().map(|r| r.name.as_str())
    }

    /// `/horizonbot reload` stops everything.
    pub fn stop_all(&mut self) {
        self.current = None;
        self.queue.clear();
    }

    /// The bots a `spawn` line of the current run just made.
    pub fn note_spawned(&mut self, names: Vec<String>) {
        if let Some(r) = self.current.as_mut() {
            r.latest = names;
        }
    }

    /// The lines due this tick, up to the next `wait`.
    pub fn tick(&mut self) -> Vec<Step> {
        let mut out = Vec::new();
        loop {
            let Some(run) = self.current.as_mut() else {
                match self.queue.pop_front() {
                    Some(r) => {
                        self.current = Some(r);
                        continue;
                    }
                    None => return out,
                }
            };
            if run.wait > 0 {
                run.wait -= 1;
                return out;
            }
            let Some(line) = run.lines.get(run.next).cloned() else {
                self.current = None;
                continue;
            };
            run.next += 1;
            let mut words = line.split_whitespace();
            if words.next() == Some("wait") {
                run.wait = words.next().and_then(|t| t.parse().ok()).unwrap_or(20);
                // The line after a wait waits a full wait, including this tick.
                run.wait = run.wait.saturating_sub(1);
                return out;
            }
            out.push(Step {
                sender: run.sender.clone(),
                line,
                latest: run.latest.clone(),
            });
            // A spawn's bots are what `latest` means from the next line on: let the
            // plugin report them before running further lines.
            if out.last().is_some_and(|s| is_spawn(&s.line)) {
                return out;
            }
        }
    }
}

fn is_spawn(line: &str) -> bool {
    let mut w = line.split_whitespace();
    matches!(w.next(), Some("horizonbot" | "hbot" | "bot")) && w.next() == Some("spawn")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raid() -> Scripts {
        let mut s = Scripts::default();
        s.create("raid").unwrap();
        for l in [
            "horizonbot spawn 20",
            "horizonbot faction join {1} latest",
            "/horizonbot kit givefaction {2} {1}",
            "# a comment",
            "wait 3",
            "horizonbot faction hostile {1} player",
        ] {
            s.add("raid", l).unwrap();
        }
        s
    }

    #[test]
    fn runs_lines_with_waits_and_latest() {
        let s = raid();
        let mut r = Runner::default();
        assert!(r.start(&s, "raid", &["bandits".into(), "iron".into()], Sender::Console).unwrap());
        let t1 = r.tick();
        assert_eq!(t1.len(), 1);
        assert_eq!(t1[0].line, "horizonbot spawn 20");
        r.note_spawned(vec!["a".into(), "b".into()]);
        let t2 = r.tick();
        assert_eq!(t2[0].line, "horizonbot faction join bandits latest");
        assert_eq!(t2[0].latest, vec!["a", "b"]);
        assert_eq!(t2[1].line, "horizonbot kit givefaction iron bandits");
        assert_eq!(t2.len(), 2);
        assert!(r.tick().is_empty());
        assert!(r.tick().is_empty());
        let t = r.tick();
        assert_eq!(t[0].line, "horizonbot faction hostile bandits player");
        assert!(r.running().is_none() || r.tick().is_empty());
    }

    #[test]
    fn one_script_at_a_time() {
        let mut s = raid();
        s.create("hello").unwrap();
        s.add("hello", "say hi").unwrap();
        let mut r = Runner::default();
        r.start(&s, "raid", &[], Sender::Console).unwrap();
        assert!(!r.start(&s, "hello", &[], Sender::Player(7)).unwrap());
        let mut lines = Vec::new();
        for _ in 0..20 {
            for st in r.tick() {
                lines.push(st.line);
            }
        }
        assert_eq!(lines.last().map(String::as_str), Some("say hi"));
    }

    #[test]
    fn limits_and_the_file() {
        let mut s = Scripts::default();
        s.create("x").unwrap();
        for i in 0..MAX_LINES {
            s.add("x", &format!("say {i}")).unwrap();
        }
        assert!(s.add("x", "say too many").is_err());
        let src = "scripts:\n  raiders:\n  - horizonbot set wander false\n  - horizonbot spawn 24\n";
        let l = Scripts::load(&crate::yaml::parse(src).unwrap());
        assert_eq!(l.scripts["raiders"].len(), 2);
    }
}
