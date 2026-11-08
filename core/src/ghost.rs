//! Ghosts: scenery players that replay a recorded path forever (redoing the recorded block
//! changes, undoing them before each lap), or stand still, stare at the nearest player and vanish
//! when someone comes close. They never fight, can't be hit and say their lines in turn.
//!
//! Recordings live in `ghost-recordings/<name>.txt`, one frame per line. The format is a plain
//! text list of frames.

use crate::math::{pitch_to, yaw_to, Vec3};

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub sneaking: bool,
}

/// A block change made while recording: what was there and what came.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockChange {
    pub frame: usize,
    pub pos: [i32; 3],
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recording {
    pub world: String,
    pub frames: Vec<Frame>,
    pub blocks: Vec<BlockChange>,
}

impl Recording {
    pub fn to_text(&self) -> String {
        let mut out = format!("world {}\n", self.world);
        for f in &self.frames {
            out.push_str(&format!(
                "f {:.4} {:.4} {:.4} {:.2} {:.2} {}\n",
                f.pos.x,
                f.pos.y,
                f.pos.z,
                f.yaw,
                f.pitch,
                u8::from(f.sneaking)
            ));
        }
        for b in &self.blocks {
            out.push_str(&format!("b {} {} {} {} {} {}\n", b.frame, b.pos[0], b.pos[1], b.pos[2], b.before, b.after));
        }
        out
    }

    pub fn from_text(text: &str) -> Result<Recording, String> {
        let mut r = Recording::default();
        for (n, line) in text.lines().enumerate() {
            let w: Vec<&str> = line.split_whitespace().collect();
            let bad = || format!("line {}: {line}", n + 1);
            match w.first().copied() {
                Some("world") => r.world = w.get(1).copied().unwrap_or("world").to_owned(),
                Some("f") if w.len() >= 6 => {
                    let num = |i: usize| w[i].parse::<f64>().map_err(|_| bad());
                    r.frames.push(Frame {
                        pos: Vec3::new(num(1)?, num(2)?, num(3)?),
                        yaw: num(4)? as f32,
                        pitch: num(5)? as f32,
                        sneaking: w.get(6) == Some(&"1"),
                    });
                }
                Some("b") if w.len() >= 7 => {
                    let int = |i: usize| w[i].parse::<i64>().map_err(|_| bad());
                    r.blocks.push(BlockChange {
                        frame: int(1)? as usize,
                        pos: [int(2)? as i32, int(3)? as i32, int(4)? as i32],
                        before: w[5].to_owned(),
                        after: w[6].to_owned(),
                    });
                }
                None => {}
                _ => return Err(bad()),
            }
        }
        Ok(r)
    }
}

/// Recording in progress for one player.
#[derive(Clone, Debug, Default)]
pub struct Recorder {
    pub name: String,
    pub rec: Recording,
}

impl Recorder {
    pub fn frame(&mut self, pos: Vec3, yaw: f32, pitch: f32, sneaking: bool) {
        self.rec.frames.push(Frame { pos, yaw, pitch, sneaking });
    }
    pub fn block(&mut self, pos: [i32; 3], before: &str, after: &str) {
        let frame = self.rec.frames.len().saturating_sub(1);
        self.rec.blocks.push(BlockChange { frame, pos, before: before.to_owned(), after: after.to_owned() });
    }
    /// The blocks to put back when recording stops, last change first.
    pub fn undo(&self) -> Vec<([i32; 3], String)> {
        self.rec.blocks.iter().rev().map(|b| (b.pos, b.before.clone())).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GhostKind {
    Replay { recording: String, frame: usize },
    Stare { pos: Vec3, spawned: u64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ghost {
    pub name: String,
    pub kind: GhostKind,
    pub lines: Vec<String>,
    pub next_line: usize,
    pub skin_from: Option<String>,
}

/// What a ghost does this tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GhostTick {
    pub pos: Option<Vec3>,
    pub yaw: f32,
    pub pitch: f32,
    pub sneaking: bool,
    /// Blocks to set now (redo or undo).
    pub blocks: Vec<([i32; 3], String)>,
    pub say: Option<String>,
    /// A staring ghost someone came close to: remove it.
    pub vanish: bool,
}

pub const SAY_EVERY: u64 = 120;
pub const VANISH_RADIUS: f64 = 6.0;
pub const VANISH_GRACE: u64 = 100;

impl Ghost {
    pub fn tick(&mut self, tick: u64, recordings: &std::collections::BTreeMap<String, Recording>, players: &[Vec3]) -> GhostTick {
        let mut out = GhostTick::default();
        if !self.lines.is_empty() && tick % SAY_EVERY == 0 {
            out.say = Some(self.lines[self.next_line % self.lines.len()].clone());
            self.next_line += 1;
        }
        match &mut self.kind {
            GhostKind::Replay { recording, frame } => {
                let Some(r) = recordings.get(recording.as_str()) else { return out };
                if r.frames.is_empty() {
                    return out;
                }
                if *frame >= r.frames.len() {
                    // Undo the lap's changes before starting over.
                    out.blocks.extend(r.blocks.iter().rev().map(|b| (b.pos, b.before.clone())));
                    *frame = 0;
                }
                let f = &r.frames[*frame];
                out.pos = Some(f.pos);
                out.yaw = f.yaw;
                out.pitch = f.pitch;
                out.sneaking = f.sneaking;
                out.blocks.extend(r.blocks.iter().filter(|b| b.frame == *frame).map(|b| (b.pos, b.after.clone())));
                *frame += 1;
            }
            GhostKind::Stare { pos, spawned } => {
                let eye = *pos + Vec3::new(0.0, 1.62, 0.0);
                let nearest = players
                    .iter()
                    .min_by(|a, b| a.distance(*pos).partial_cmp(&b.distance(*pos)).unwrap_or(std::cmp::Ordering::Equal));
                out.pos = Some(*pos);
                if let Some(n) = nearest {
                    let look = *n + Vec3::new(0.0, 1.62, 0.0);
                    out.yaw = yaw_to(eye, look);
                    out.pitch = pitch_to(eye, look);
                    if n.distance(*pos) < VANISH_RADIUS && tick > *spawned + VANISH_GRACE {
                        out.vanish = true;
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn lap() -> Recording {
        let mut r = Recorder { name: "lap".into(), ..Default::default() };
        r.rec.world = "world".into();
        for i in 0..3 {
            r.frame(Vec3::new(i as f64, 64.0, 0.0), 0.0, 0.0, false);
            if i == 1 {
                r.block([5, 64, 5], "minecraft:air", "minecraft:stone");
            }
        }
        r.rec
    }

    #[test]
    fn recordings_round_trip() {
        let r = lap();
        assert_eq!(Recording::from_text(&r.to_text()).unwrap(), r);
    }

    #[test]
    fn replays_loop_and_undo_their_blocks() {
        let mut recs = BTreeMap::new();
        recs.insert("lap".to_owned(), lap());
        let mut g = Ghost {
            name: "Wanderer".into(),
            kind: GhostKind::Replay { recording: "lap".into(), frame: 0 },
            lines: vec![],
            next_line: 0,
            skin_from: None,
        };
        let t: Vec<GhostTick> = (1..=5).map(|t| g.tick(t, &recs, &[])).collect();
        assert_eq!(t[1].blocks, vec![([5, 64, 5], "minecraft:stone".to_owned())]);
        assert_eq!(t[3].pos, Some(Vec3::new(0.0, 64.0, 0.0)));
        assert_eq!(t[3].blocks, vec![([5, 64, 5], "minecraft:air".to_owned())]);
    }

    #[test]
    fn staring_ghosts_vanish_after_the_grace() {
        let mut g = Ghost {
            name: "Watcher".into(),
            kind: GhostKind::Stare { pos: Vec3::new(0.0, 64.0, 0.0), spawned: 0 },
            lines: vec!["Who's there?".into(), "Leave now".into()],
            next_line: 0,
            skin_from: None,
        };
        let close = [Vec3::new(3.0, 64.0, 0.0)];
        assert!(!g.tick(50, &BTreeMap::new(), &close).vanish);
        assert!(g.tick(101, &BTreeMap::new(), &close).vanish);
        assert_eq!(g.tick(120, &BTreeMap::new(), &[]).say.as_deref(), Some("Who's there?"));
        assert_eq!(g.tick(240, &BTreeMap::new(), &[]).say.as_deref(), Some("Leave now"));
    }
}
