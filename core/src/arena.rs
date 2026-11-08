//! The arena: two bots fight round after round where it was started; a report per side.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Side {
    pub kit: String,
    pub skill: Option<u8>,
    pub wins: u32,
    /// Ticks each win took.
    pub kill_ticks: Vec<u64>,
    pub hits: u32,
    pub damage: f64,
    pub swings: u32,
    pub crits: u32,
    pub smashes: u32,
    pub best_combo: u32,
    combo: u32,
    pub totems: u32,
    pub heals: u32,
    pub inventory_opens: u32,
}

impl Side {
    pub fn hit(&mut self, damage: f64, crit: bool, smash: bool) {
        self.hits += 1;
        self.damage += damage;
        self.crits += u32::from(crit);
        self.smashes += u32::from(smash);
        self.combo += 1;
        self.best_combo = self.best_combo.max(self.combo);
    }
    pub fn got_hit(&mut self) {
        self.combo = 0;
    }
    pub fn report(&self, name: &str, rounds: u32) -> String {
        let avg = if self.kill_ticks.is_empty() {
            "-".to_owned()
        } else {
            format!("{:.1}s", self.kill_ticks.iter().sum::<u64>() as f64 / self.kill_ticks.len() as f64 / 20.0)
        };
        let per_hit = if self.hits == 0 { 0.0 } else { self.damage / self.hits as f64 };
        let acc = if self.swings == 0 { 0.0 } else { self.hits as f64 * 100.0 / self.swings as f64 };
        format!(
            "{name} ({}): {}/{} wins, time to kill {avg}, {} hits, {per_hit:.1} per hit, {acc:.0}% of swings landed, {} crits, {} smashes, best combo {}, {} totems, {} heals, {} inventory opens",
            self.kit, self.wins, rounds, self.hits, self.crits, self.smashes, self.best_combo, self.totems, self.heals, self.inventory_opens
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Waiting this many ticks before the next round starts.
    Between(u64),
    Fighting { since: u64 },
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arena {
    pub world: String,
    pub center: crate::math::Vec3,
    pub rounds: u32,
    pub round: u32,
    pub a: Side,
    pub b: Side,
    pub phase: Phase,
    /// The two bots' names.
    pub bot_a: String,
    pub bot_b: String,
}

/// What the plugin must do for the arena now.
#[derive(Clone, Debug, PartialEq)]
pub enum ArenaStep {
    /// Reset both bots: full health, kits, placed 6 blocks apart, facing each other, fighting.
    StartRound,
    Report(Vec<String>),
}

impl Arena {
    pub fn new(world: &str, center: crate::math::Vec3, kit_a: &str, kit_b: &str, rounds: u32, skill_a: Option<u8>, skill_b: Option<u8>) -> Self {
        Arena {
            world: world.to_owned(),
            center,
            rounds,
            round: 0,
            a: Side { kit: kit_a.to_owned(), skill: skill_a, ..Default::default() },
            b: Side { kit: kit_b.to_owned(), skill: skill_b, ..Default::default() },
            phase: Phase::Between(20),
            bot_a: "ArenaA".into(),
            bot_b: "ArenaB".into(),
        }
    }

    /// Where each side starts a round.
    pub fn starts(&self) -> (crate::math::Vec3, crate::math::Vec3) {
        let off = crate::math::Vec3::new(3.0, 0.0, 0.0);
        (self.center - off, self.center + off)
    }

    pub fn tick(&mut self, tick: u64) -> Option<ArenaStep> {
        match self.phase {
            Phase::Between(0) => {
                if self.round >= self.rounds {
                    self.phase = Phase::Done;
                    return Some(ArenaStep::Report(self.report()));
                }
                self.round += 1;
                self.phase = Phase::Fighting { since: tick };
                Some(ArenaStep::StartRound)
            }
            Phase::Between(n) => {
                self.phase = Phase::Between(n - 1);
                None
            }
            _ => None,
        }
    }

    /// A side died: the other wins the round.
    pub fn died(&mut self, tick: u64, a_died: bool) {
        if let Phase::Fighting { since } = self.phase {
            let winner = if a_died { &mut self.b } else { &mut self.a };
            winner.wins += 1;
            winner.kill_ticks.push(tick.saturating_sub(since));
            self.phase = Phase::Between(60);
        }
    }

    pub fn report(&self) -> Vec<String> {
        vec![
            format!("Arena, {} of {} rounds:", self.round.min(self.rounds), self.rounds),
            self.a.report("A", self.round),
            self.b.report("B", self.round),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_then_a_report() {
        let mut a = Arena::new("world", crate::math::Vec3::ZERO, "iron", "diamond", 2, None, Some(80));
        let mut t = 0;
        let mut starts = 0;
        let mut report = None;
        while t < 1000 && report.is_none() {
            t += 1;
            match a.tick(t) {
                Some(ArenaStep::StartRound) => starts += 1,
                Some(ArenaStep::Report(r)) => report = Some(r),
                None => {}
            }
            if matches!(a.phase, Phase::Fighting { since } if t > since + 40) {
                a.died(t, starts == 1);
            }
        }
        assert_eq!(starts, 2);
        assert_eq!((a.a.wins, a.b.wins), (1, 1));
        assert!(report.unwrap()[1].contains("1/2 wins"));
    }
}
