//! What the bots share: relations, the pile-on count per enemy, the squad-wide cobweb clock,
//! whose turn it is to regear, and each faction's morale.

use crate::factions::{Factions, Relation};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub struct Squad {
    pub factions: Factions,
    pub tick: u64,
    /// No bot webs before this tick (`cobweb-global-cooldown`).
    pub web_ready_at: u64,
    /// Bots on each enemy last tick and this one.
    last_on: BTreeMap<i32, u32>,
    now_on: BTreeMap<i32, u32>,
    /// Factions with a member off regearing.
    pub regearing: BTreeSet<String>,
    /// Faction -> (members at the fight's start, members now, enemies near).
    pub morale: BTreeMap<String, (u32, u32, u32)>,
    /// The target a faction's members are mostly on (focus fire).
    pub focus: BTreeMap<String, i32>,
    /// A low ally to peel for: faction -> (ally entity id, attacker id).
    pub peel: BTreeMap<String, (i32, i32)>,
    /// Tethered targets (lower case): the escort's shared warning state.
    pub tethers: BTreeMap<String, TetherState>,
    /// Path-search nodes expanded by all bots this tick (see `NAV_BUDGET_PER_TICK`).
    pub nav_spent: u32,
}

/// The most path-search node expansions all bots together may spend in one server tick. A bot that
/// finds the budget used up keeps its old path (or walks straight) and searches in a later tick.
///
/// Measured with 500 bots: 8000 nodes were ~96 000 block reads (~12 per node), 10-20 ms of every tick. 3000 is about
/// 3-4 ms; the bots that miss out keep walking toward the goal and search a few ticks later.
pub const NAV_BUDGET_PER_TICK: u32 = 3000;
/// What the small searches of long-starved bots (`NAV_STARVED_TICKS`) may bring the tick's total to, at most.
pub const NAV_HARD_CAP_PER_TICK: u32 = 4500;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TetherState {
    pub warned_at: Option<u64>,
    pub hostile: bool,
}

impl Squad {
    /// Call once per server tick before the bots think.
    pub fn begin_tick(&mut self, tick: u64) {
        self.tick = tick;
        self.last_on = std::mem::take(&mut self.now_on);
        self.factions.tick_calls();
        self.peel.clear();
        self.nav_spent = 0;
    }

    pub fn relation(&self, a: Option<&str>, b: Option<&str>) -> Relation {
        self.factions.relation(a, b)
    }

    /// Bots that were on this enemy last tick (minus the asking bot if it was one).
    pub fn pile(&self, enemy: i32) -> u32 {
        self.last_on.get(&enemy).copied().unwrap_or(0)
    }

    pub fn engage(&mut self, enemy: i32) {
        *self.now_on.entry(enemy).or_default() += 1;
    }

    /// A faction whose side is outnumbered and has lost members breaks; the less brave first.
    pub fn breaking(&self, faction: &str, bravery: f64, leader_near: bool) -> bool {
        let Some(&(start, now, enemies)) = self.morale.get(faction) else {
            return false;
        };
        if start == 0 || now >= start || enemies <= now {
            return false;
        }
        let lost = 1.0 - now as f64 / start as f64;
        let outnumbered = enemies as f64 / now.max(1) as f64;
        let pressure = lost * outnumbered.min(3.0);
        let hold = if leader_near { 0.4 } else { 0.0 };
        pressure > 0.5 + bravery * 0.6 + hold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piles_count_last_tick() {
        let mut s = Squad::default();
        s.begin_tick(1);
        s.engage(7);
        s.engage(7);
        assert_eq!(s.pile(7), 0);
        s.begin_tick(2);
        assert_eq!(s.pile(7), 2);
    }

    #[test]
    fn nav_budget_resets_each_tick() {
        let mut s = Squad::default();
        s.begin_tick(1);
        s.nav_spent = 5000;
        s.begin_tick(2);
        assert_eq!(s.nav_spent, 0);
    }

    #[test]
    fn morale_breaks_when_outnumbered_and_losing() {
        let mut s = Squad::default();
        s.morale.insert("red".into(), (10, 4, 9));
        assert!(s.breaking("red", 0.1, false));
        assert!(!s.breaking("red", 0.1, true) || !s.breaking("red", 0.9, true));
        s.morale.insert("blue".into(), (10, 10, 3));
        assert!(!s.breaking("blue", 0.0, false));
    }
}
