//! A bot's inventory as its client knows it, and the searches the brain makes in it.

use crate::items::{self, ArmorSlot, Stack, WeaponKind};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inventory {
    /// 0..9 hotbar, 9..36 bag.
    pub slots: Vec<Option<Stack>>,
    /// Feet, legs, chest, head.
    pub armor: [Option<Stack>; 4],
    pub offhand: Option<Stack>,
    pub selected: usize,
    pub ender: Vec<Option<Stack>>,
}

impl Inventory {
    pub fn new() -> Self {
        Inventory {
            slots: vec![None; 36],
            ender: vec![None; 27],
            ..Default::default()
        }
    }

    pub fn held(&self) -> Option<&Stack> {
        self.slots.get(self.selected).and_then(Option::as_ref)
    }

    pub fn held_is(&self, id: &str) -> bool {
        self.held().is_some_and(|s| s.is(id))
    }

    pub fn offhand_is(&self, id: &str) -> bool {
        self.offhand.as_ref().is_some_and(|s| s.is(id))
    }

    /// The first slot (hotbar first) whose stack passes `f`; `hotbar_only` limits to 0..9.
    pub fn find(&self, hotbar_only: bool, f: impl Fn(&Stack) -> bool) -> Option<usize> {
        let end = if hotbar_only { 9 } else { self.slots.len() };
        (0..end).find(|&i| self.slots[i].as_ref().is_some_and(&f))
    }

    pub fn find_id(&self, hotbar_only: bool, id: &str) -> Option<usize> {
        self.find(hotbar_only, |s| s.is(id))
    }

    pub fn count(&self, f: impl Fn(&Stack) -> bool) -> u32 {
        let carried: u32 = self.slots.iter().flatten().filter(|s| f(s)).map(|s| s.count).sum();
        carried + self.offhand.iter().filter(|s| f(s)).map(|s| s.count).sum::<u32>()
    }

    pub fn count_id(&self, id: &str) -> u32 {
        self.count(|s| s.is(id))
    }

    pub fn has(&self, id: &str) -> bool {
        self.count_id(id) > 0
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none) && self.armor.iter().all(Option::is_none) && self.offhand.is_none()
    }

    /// Armour points and toughness worn, and the total Protection levels.
    pub fn armor_value(&self) -> (f64, f64, u32) {
        let mut pts = 0.0;
        let mut tough = 0.0;
        let mut prot = 0;
        for s in self.armor.iter().flatten() {
            if let Some((_, p, t)) = items::armor(&s.id) {
                pts += p;
                tough += t;
            }
            prot += s.enchant("protection");
        }
        (pts, tough, prot)
    }

    /// The best melee weapon slot (by damage per hit, Sharpness counted).
    pub fn best_weapon(&self, hotbar_only: bool, kind: Option<WeaponKind>) -> Option<usize> {
        let end = if hotbar_only { 9 } else { self.slots.len() };
        (0..end)
            .filter(|&i| {
                self.slots[i].as_ref().is_some_and(|s| {
                    let w = items::weapon(&s.id);
                    w.kind != WeaponKind::Hand && w.kind != WeaponKind::Other && kind.is_none_or(|k| k == w.kind)
                })
            })
            .max_by(|&a, &b| {
                let da = items::melee_damage(self.slots[a].as_ref()) * items::weapon(&self.slots[a].as_ref().unwrap().id).speed.min(1.6);
                let db = items::melee_damage(self.slots[b].as_ref()) * items::weapon(&self.slots[b].as_ref().unwrap().id).speed.min(1.6);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// For each armour slot, a carried piece better than the worn one (slot index in `slots`).
    pub fn armor_upgrades(&self) -> Vec<(ArmorSlot, usize)> {
        let mut out = Vec::new();
        for slot in [ArmorSlot::Feet, ArmorSlot::Legs, ArmorSlot::Chest, ArmorSlot::Head] {
            let worn = self.armor[slot as usize].as_ref().map_or(-1.0, items::armor_score);
            let best = self
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
                .filter(|(_, s)| items::armor(&s.id).is_some_and(|(sl, _, _)| sl == slot))
                // An elytra is worn only for the elytra plays.
                .filter(|(_, s)| !s.is("elytra"))
                .max_by(|a, b| items::armor_score(a.1).partial_cmp(&items::armor_score(b.1)).unwrap_or(std::cmp::Ordering::Equal));
            if let Some((i, s)) = best {
                if items::armor_score(s) > worn + 0.5 {
                    out.push((slot, i));
                }
            }
        }
        out
    }

    /// A free hotbar slot, or the least useful one to swap into.
    pub fn hotbar_slot_for_swap(&self, keep: &[usize]) -> usize {
        if let Some(i) = (0..9).find(|i| self.slots[*i].is_none()) {
            return i;
        }
        (0..9).rev().find(|i| !keep.contains(i)).unwrap_or(8)
    }

    /// Everything carried, keyed as a regear counts it.
    pub fn restock_counts(&self) -> std::collections::BTreeMap<String, u32> {
        let mut m = std::collections::BTreeMap::new();
        for s in self.slots.iter().flatten().chain(self.offhand.iter()).chain(self.armor.iter().flatten()) {
            let key = match &s.potion {
                Some(p) => format!("{}#{p}", s.id),
                None => s.id.clone(),
            };
            *m.entry(key).or_default() += s.count;
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_best_gear() {
        let mut inv = Inventory::new();
        inv.slots[0] = Some(Stack::new("iron_sword", 1));
        inv.slots[3] = Some(Stack::new("diamond_sword", 1));
        inv.slots[20] = Some(Stack::new("netherite_sword", 1));
        assert_eq!(inv.best_weapon(true, None), Some(3));
        assert_eq!(inv.best_weapon(false, None), Some(20));
        inv.slots[10] = Some(Stack::new("diamond_chestplate", 1));
        inv.armor[ArmorSlot::Chest as usize] = Some(Stack::new("iron_chestplate", 1));
        assert_eq!(inv.armor_upgrades(), vec![(ArmorSlot::Chest, 10)]);
        inv.offhand = Some(Stack::new("totem_of_undying", 1));
        assert_eq!(inv.count_id("totem_of_undying"), 1);
    }
}
