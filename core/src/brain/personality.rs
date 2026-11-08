//! Each bot's own habits, from its name, and the playstyles.

use crate::math::{name_hash, Rng};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playstyle {
    Default,
    Aggressive,
    Safe,
    Defensive,
    Balanced,
    Adaptive,
}

impl Playstyle {
    pub fn parse(s: &str) -> Playstyle {
        match s {
            "aggressive" => Playstyle::Aggressive,
            "safe" => Playstyle::Safe,
            "defensive" => Playstyle::Defensive,
            "balanced" => Playstyle::Balanced,
            "adaptive" => Playstyle::Adaptive,
            _ => Playstyle::Default,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Playstyle::Default => "default",
            Playstyle::Aggressive => "aggressive",
            Playstyle::Safe => "safe",
            Playstyle::Defensive => "defensive",
            Playstyle::Balanced => "balanced",
            Playstyle::Adaptive => "adaptive",
        }
    }
}

/// How a playstyle shifts the bot's thresholds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Temper {
    /// Added to the retreat/heal health thresholds (half-hearts).
    pub heal_shift: f64,
    /// Multiplies how far it chases.
    pub chase: f64,
    /// Extra chance to raise the shield.
    pub block: f64,
    /// Whether it backs off to reset at all.
    pub resets: bool,
    /// Heals where it stands instead of backing off.
    pub heal_in_place: bool,
}

impl Temper {
    /// `winning` is the health race (-1 losing .. 1 winning) for adaptive bots.
    pub fn of(style: Playstyle, winning: f64) -> Temper {
        let base = Temper { heal_shift: 0.0, chase: 1.0, block: 0.0, resets: true, heal_in_place: false };
        match style {
            Playstyle::Default => base,
            Playstyle::Aggressive => Temper { heal_shift: -2.0, chase: 1.5, ..base },
            Playstyle::Safe => Temper { heal_shift: 3.0, chase: 0.8, block: 0.1, ..base },
            Playstyle::Defensive => Temper { heal_shift: 1.0, chase: 0.25, block: 0.25, resets: false, heal_in_place: true },
            Playstyle::Balanced => Temper { heal_shift: 1.0, chase: 1.1, block: 0.05, ..base },
            Playstyle::Adaptive => {
                if winning >= 0.0 {
                    Temper::of(Playstyle::Aggressive, 0.0)
                } else {
                    Temper::of(Playstyle::Safe, 0.0)
                }
            }
        }
    }
}

/// Habits, stable per name.
#[derive(Clone, Debug, PartialEq)]
pub struct Personality {
    /// How often it hops when it doesn't need to (0..1).
    pub jumpy: f64,
    /// Strafe width multiplier (0.6..1.5).
    pub strafe_width: f64,
    /// Preferred fighting distance offset (-0.4..0.4 blocks).
    pub range_bias: f64,
    /// Rush (> 0) or turtle (< 0), -1..1.
    pub rush: f64,
    /// Sprint-comboer (true) or crit-hunter.
    pub comboer: bool,
    /// Spam-clicks air now and then (only below skill 45).
    pub spam_clicker: bool,
    /// How brave in a losing army (0..1).
    pub bravery: f64,
    /// Skill offset inside its faction (-1..1, times skill-spread).
    pub skill_offset: f64,
    /// Locks in rather than panicking at low health (0..1, raised by skill).
    pub nerve: f64,
    /// Which way it likes to circle first.
    pub circle_left: bool,
    /// Its rhythm for bunny hops (0..1).
    pub hop_phase: f64,
}

impl Personality {
    pub fn from_name(name: &str, skill: f64) -> Personality {
        let mut r = Rng::new(name_hash(name));
        Personality {
            jumpy: r.unit() * 0.6,
            strafe_width: 0.6 + r.unit() * 0.9,
            range_bias: r.signed() * 0.4,
            rush: r.signed(),
            comboer: r.chance(0.5),
            spam_clicker: skill < 45.0 && r.chance(0.35),
            bravery: r.unit(),
            skill_offset: r.signed(),
            nerve: (r.unit() * 0.6 + skill / 100.0 * 0.6).min(1.0),
            circle_left: r.chance(0.5),
            hop_phase: r.unit(),
        }
    }

    /// A plain personality (`personality` off): everyone the same.
    pub fn plain() -> Personality {
        Personality {
            jumpy: 0.1,
            strafe_width: 1.0,
            range_bias: 0.0,
            rush: 0.0,
            comboer: false,
            spam_clicker: false,
            bravery: 0.5,
            skill_offset: 0.0,
            nerve: 0.5,
            circle_left: true,
            hop_phase: 0.0,
        }
    }
}

/// randomplaystyle: one style by the configured weights.
pub fn roll_playstyle(weights: &[(String, f64)], rng: &mut Rng) -> Playstyle {
    let w: Vec<f64> = weights.iter().map(|(_, v)| *v).collect();
    rng.weighted(&w).map_or(Playstyle::Default, |i| Playstyle::parse(&weights[i].0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn habits_are_stable_per_name() {
        assert_eq!(Personality::from_name("Marco", 50.0), Personality::from_name("marco", 50.0));
        assert_ne!(Personality::from_name("Marco", 50.0), Personality::from_name("Polo", 50.0));
        assert!(!Personality::from_name("x", 80.0).spam_clicker);
    }

    #[test]
    fn adaptive_follows_the_race() {
        assert_eq!(Temper::of(Playstyle::Adaptive, 0.5), Temper::of(Playstyle::Aggressive, 0.0));
        assert_eq!(Temper::of(Playstyle::Adaptive, -0.5), Temper::of(Playstyle::Safe, 0.0));
        assert!(!Temper::of(Playstyle::Defensive, 0.0).resets);
    }
}
