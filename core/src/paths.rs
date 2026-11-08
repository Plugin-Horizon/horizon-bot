//! Patrol routes (`paths.yml`): points walked in order and back again, with console commands that
//! run when a bot arrives at a point.

use crate::math::Vec3;
use crate::yaml::Yaml;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub world: String,
    pub pos: Vec3,
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub name: String,
    pub waypoints: Vec<Waypoint>,
}

/// Where a patrolling bot is on its route.
#[derive(Clone, Debug, PartialEq)]
pub struct Patrol {
    pub path: String,
    pub index: usize,
    pub forward: bool,
}

impl Patrol {
    pub fn new(path: &str) -> Self {
        Patrol {
            path: path.to_owned(),
            index: 0,
            forward: true,
        }
    }

    /// Moves to the next point; at either end the bot turns back instead of jumping.
    pub fn advance(&mut self, len: usize) {
        if len <= 1 {
            self.index = 0;
            return;
        }
        if self.forward {
            if self.index + 1 >= len {
                self.forward = false;
                self.index = len - 2;
            } else {
                self.index += 1;
            }
        } else if self.index == 0 {
            self.forward = true;
            self.index = 1;
        } else {
            self.index -= 1;
        }
    }
}

/// `{bot}`, `{path}`, `{index}`, `{x}`, `{y}`, `{z}`, `{world}` in a waypoint's command.
pub fn fill(command: &str, bot: &str, path: &str, index: usize, w: &Waypoint) -> String {
    command
        .replace("{bot}", bot)
        .replace("{path}", path)
        .replace("{index}", &(index + 1).to_string())
        .replace("{x}", &format!("{}", w.pos.x.floor() as i64))
        .replace("{y}", &format!("{}", w.pos.y.floor() as i64))
        .replace("{z}", &format!("{}", w.pos.z.floor() as i64))
        .replace("{world}", &w.world)
}

#[derive(Clone, Debug, Default)]
pub struct Paths {
    pub paths: BTreeMap<String, Path>,
}

impl Paths {
    pub fn create(&mut self, name: &str) -> Result<(), String> {
        if self.paths.contains_key(name) {
            return Err(format!("Route {name} exists already"));
        }
        self.paths.insert(
            name.to_owned(),
            Path {
                name: name.to_owned(),
                waypoints: Vec::new(),
            },
        );
        Ok(())
    }

    pub fn add(&mut self, name: &str, world: &str, pos: Vec3) -> Result<usize, String> {
        let p = self.paths.get_mut(name).ok_or_else(|| format!("No route called {name}"))?;
        p.waypoints.push(Waypoint {
            world: world.to_owned(),
            pos,
            commands: Vec::new(),
        });
        Ok(p.waypoints.len())
    }

    /// `index` is 1-based as `path list` shows it.
    pub fn remove(&mut self, name: &str, index: usize) -> Result<(), String> {
        let p = self.paths.get_mut(name).ok_or_else(|| format!("No route called {name}"))?;
        if index == 0 || index > p.waypoints.len() {
            return Err(format!("Route {name} has points 1 to {}", p.waypoints.len()));
        }
        p.waypoints.remove(index - 1);
        Ok(())
    }

    pub fn waypoint_mut(&mut self, name: &str, index: usize) -> Result<&mut Waypoint, String> {
        let p = self.paths.get_mut(name).ok_or_else(|| format!("No route called {name}"))?;
        let n = p.waypoints.len();
        p.waypoints
            .get_mut(index.wrapping_sub(1))
            .ok_or_else(|| format!("Route {name} has points 1 to {n}"))
    }

    pub fn load(doc: &Yaml) -> Self {
        let mut out = Paths::default();
        if let Some(m) = doc.get("paths").and_then(Yaml::as_map) {
            for (key, v) in m {
                let name = v.get("name").and_then(Yaml::as_text).unwrap_or_else(|| key.clone());
                let waypoints = v
                    .get("waypoints")
                    .and_then(Yaml::as_seq)
                    .map(|s| {
                        s.iter()
                            .filter_map(|w| {
                                Some(Waypoint {
                                    world: w.get("world").and_then(Yaml::as_text).unwrap_or_else(|| "world".into()),
                                    pos: Vec3::new(
                                        w.get("x")?.as_f64()?,
                                        w.get("y")?.as_f64()?,
                                        w.get("z")?.as_f64()?,
                                    ),
                                    commands: w.get("commands").map(Yaml::strings).unwrap_or_default(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.paths.insert(key.clone(), Path { name, waypoints });
            }
        }
        out
    }

    pub fn save(&self) -> Yaml {
        let mut m = Yaml::map();
        for (key, p) in &self.paths {
            let mut v = Yaml::map();
            v.set("name", Yaml::Str(p.name.clone()));
            v.set(
                "waypoints",
                Yaml::Seq(
                    p.waypoints
                        .iter()
                        .map(|w| {
                            let mut y = Yaml::map();
                            y.set("world", Yaml::Str(w.world.clone()));
                            y.set("x", Yaml::Float(w.pos.x));
                            y.set("y", Yaml::Float(w.pos.y));
                            y.set("z", Yaml::Float(w.pos.z));
                            if !w.commands.is_empty() {
                                y.set("commands", Yaml::str_seq(w.commands.iter().cloned()));
                            }
                            y
                        })
                        .collect(),
                ),
            );
            m.set(key, v);
        }
        let mut doc = Yaml::map();
        doc.set("paths", m);
        doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patrols_turn_back_at_the_ends() {
        let mut p = Patrol::new("walls");
        let seen: Vec<usize> = (0..7)
            .map(|_| {
                p.advance(3);
                p.index
            })
            .collect();
        assert_eq!(seen, vec![1, 2, 1, 0, 1, 2, 1]);
    }

    #[test]
    fn reads_the_plugins_file_and_fills_placeholders() {
        let src = "paths:\n  '1':\n    name: '1'\n    waypoints:\n    - world: world\n      x: -200.5\n      y: 95.0\n      z: -381.8\n";
        let p = Paths::load(&crate::yaml::parse(src).unwrap());
        let w = &p.paths["1"].waypoints[0];
        assert_eq!(w.pos.x, -200.5);
        assert_eq!(fill("say {bot} at {x} {y} {z} in {world} ({index})", "Marco", "1", 0, w), "say Marco at -201 95 -382 in world (1)");
        let back = Paths::load(&crate::yaml::parse(&crate::yaml::dump(&p.save())).unwrap());
        assert_eq!(back.paths, p.paths);
    }
}
