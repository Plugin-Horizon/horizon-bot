//! What a bot knows about items: weapons, armour, food, potions and the tools of its tricks.
//! Numbers are vanilla 26.x's (attack damage and speed, armour points and toughness).

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    pub id: String,
    pub count: u32,
    /// Durability used.
    pub damage: u32,
    pub max_damage: u32,
    pub enchants: Vec<(String, u32)>,
    /// `minecraft:potion_contents`' potion (`minecraft:strong_healing`).
    pub potion: Option<String>,
}

impl Stack {
    pub fn new(id: &str, count: u32) -> Self {
        Stack {
            id: full(id),
            count,
            max_damage: max_damage(&full(id)),
            ..Default::default()
        }
    }
    pub fn short(&self) -> &str {
        self.id.strip_prefix("minecraft:").unwrap_or(&self.id)
    }
    pub fn enchant(&self, id: &str) -> u32 {
        let id = full(id);
        self.enchants.iter().find(|(e, _)| *e == id).map_or(0, |(_, l)| *l)
    }
    pub fn with_enchant(mut self, id: &str, level: u32) -> Self {
        self.enchants.push((full(id), level));
        self
    }
    pub fn with_potion(mut self, potion: &str) -> Self {
        self.potion = Some(full(potion));
        self
    }
    pub fn durability_left(&self) -> f64 {
        if self.max_damage == 0 {
            1.0
        } else {
            1.0 - self.damage as f64 / self.max_damage as f64
        }
    }
    pub fn is(&self, id: &str) -> bool {
        self.short() == id.strip_prefix("minecraft:").unwrap_or(id)
    }
}

pub fn full(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

fn short(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponKind {
    Sword,
    Axe,
    Mace,
    Spear,
    Trident,
    Hand,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weapon {
    pub kind: WeaponKind,
    /// Attack damage with the item (fist 1).
    pub damage: f64,
    /// Attacks per second.
    pub speed: f64,
}

impl Weapon {
    /// Ticks for a full charge (vanilla's attack strength delay).
    pub fn cooldown_ticks(&self) -> f64 {
        20.0 / self.speed
    }
}

fn tier(id: &str) -> Option<(&str, &str)> {
    let s = short(id);
    for t in ["wooden", "stone", "copper", "iron", "golden", "diamond", "netherite"] {
        if let Some(rest) = s.strip_prefix(t) {
            return Some((t, rest.trim_start_matches('_')));
        }
    }
    None
}

pub fn weapon(id: &str) -> Weapon {
    let s = short(id);
    let w = |kind, damage, speed| Weapon { kind, damage, speed };
    if s == "mace" {
        return w(WeaponKind::Mace, 6.0, 0.6);
    }
    if s == "trident" {
        return w(WeaponKind::Trident, 9.0, 1.1);
    }
    if let Some((t, tool)) = tier(s) {
        let base = match t {
            "wooden" | "golden" => 0.0,
            "stone" => 1.0,
            "copper" => 1.0,
            "iron" => 2.0,
            "diamond" => 3.0,
            _ => 4.0,
        };
        match tool {
            "sword" => return w(WeaponKind::Sword, 4.0 + base, 1.6),
            "axe" => {
                let (d, sp) = match t {
                    "wooden" | "golden" => (7.0, 0.8),
                    "stone" => (9.0, 0.8),
                    "copper" => (9.0, 0.8),
                    "iron" => (9.0, 0.9),
                    "diamond" => (9.0, 1.0),
                    _ => (10.0, 1.0),
                };
                return w(WeaponKind::Axe, d, sp);
            }
            "spear" => return w(WeaponKind::Spear, 2.0 + base, 1.0),
            "pickaxe" => return w(WeaponKind::Other, 2.0 + base, 1.2),
            "shovel" => return w(WeaponKind::Other, 2.5 + base, 1.0),
            "hoe" => return w(WeaponKind::Other, 1.0, 1.0 + base),
            _ => {}
        }
    }
    w(WeaponKind::Hand, 1.0, 4.0)
}

/// Melee damage of a stack against bare skin (enchantments: Sharpness).
pub fn melee_damage(stack: Option<&Stack>) -> f64 {
    let Some(s) = stack else { return 1.0 };
    let w = weapon(&s.id);
    let sharp = s.enchant("sharpness");
    w.damage + if sharp > 0 { 0.5 * sharp as f64 + 0.5 } else { 0.0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArmorSlot {
    Feet = 0,
    Legs = 1,
    Chest = 2,
    Head = 3,
}

pub fn armor(id: &str) -> Option<(ArmorSlot, f64, f64)> {
    let s = short(id);
    if s == "elytra" {
        return Some((ArmorSlot::Chest, 0.0, 0.0));
    }
    if s == "turtle_helmet" {
        return Some((ArmorSlot::Head, 2.0, 0.0));
    }
    if s == "carved_pumpkin" || s.ends_with("_head") || s.ends_with("_skull") {
        return None;
    }
    let (mat, piece) = s.rsplit_once('_')?;
    let slot = match piece {
        "helmet" => ArmorSlot::Head,
        "chestplate" => ArmorSlot::Chest,
        "leggings" => ArmorSlot::Legs,
        "boots" => ArmorSlot::Feet,
        _ => return None,
    };
    let (pts, tough): ([f64; 4], f64) = match mat {
        "leather" => ([1.0, 2.0, 3.0, 1.0], 0.0),
        "chainmail" => ([1.0, 4.0, 5.0, 2.0], 0.0),
        "copper" => ([1.0, 3.0, 4.0, 2.0], 0.0),
        "golden" => ([1.0, 3.0, 5.0, 2.0], 0.0),
        "iron" => ([2.0, 5.0, 6.0, 2.0], 0.0),
        "diamond" => ([3.0, 6.0, 8.0, 3.0], 2.0),
        "netherite" => ([3.0, 6.0, 8.0, 3.0], 3.0),
        _ => return None,
    };
    Some((slot, pts[slot as usize], tough))
}

/// An armour piece's worth for auto-equip: points, toughness, Protection.
pub fn armor_score(s: &Stack) -> f64 {
    match armor(&s.id) {
        Some((_, p, t)) => p * 10.0 + t * 4.0 + s.enchant("protection") as f64 * 3.0 + s.enchant("blast_protection") as f64
            + s.durability_left(),
        None => -1.0,
    }
}

/// Vanilla's damage after armour (`CombatRules.getDamageAfterAbsorb`) and Protection.
pub fn damage_after_armor(damage: f64, armor: f64, toughness: f64, protection_levels: u32) -> f64 {
    let t = 2.0 + toughness / 4.0;
    let a = (armor - damage / t).max(armor * 0.2).min(20.0);
    let after = damage * (1.0 - a / 25.0);
    let epf = (protection_levels as f64).min(20.0);
    after * (1.0 - epf / 25.0)
}

pub fn food(id: &str) -> Option<(u32, f64)> {
    Some(match short(id) {
        "golden_apple" => (4, 9.6),
        "enchanted_golden_apple" => (4, 9.6),
        "golden_carrot" => (6, 14.4),
        "cooked_beef" | "cooked_porkchop" => (8, 12.8),
        "cooked_mutton" => (6, 9.6),
        "cooked_chicken" => (6, 7.2),
        "cooked_salmon" => (6, 9.6),
        "cooked_cod" => (5, 6.0),
        "bread" => (5, 6.0),
        "baked_potato" => (5, 6.0),
        "apple" => (4, 2.4),
        "carrot" => (3, 3.6),
        "melon_slice" => (2, 1.2),
        "sweet_berries" => (2, 0.4),
        "pumpkin_pie" => (8, 4.8),
        "mushroom_stew" | "beetroot_soup" | "rabbit_stew" => (6, 7.2),
        "cookie" => (2, 0.4),
        "dried_kelp" => (1, 0.6),
        "chorus_fruit" => (4, 2.4),
        _ => return None,
    })
}

/// Food worth eating in a fight (gapples are healing, not food).
pub fn is_plain_food(id: &str) -> bool {
    food(id).is_some() && !matches!(short(id), "golden_apple" | "enchanted_golden_apple" | "chorus_fruit")
}

pub fn is_healing_potion(s: &Stack) -> bool {
    matches!(s.short(), "potion" | "splash_potion" | "lingering_potion")
        && s.potion.as_deref().is_some_and(|p| p.ends_with("healing") || p.ends_with("regeneration"))
}

/// Buff potions a bot drinks before a fight.
pub fn buff_effect(s: &Stack) -> Option<&'static str> {
    if s.short() != "potion" {
        return None;
    }
    let p = s.potion.as_deref()?;
    let p = short(p);
    let p = p.trim_start_matches("long_").trim_start_matches("strong_");
    Some(match p {
        "strength" => "strength",
        "swiftness" => "speed",
        "fire_resistance" => "fire_resistance",
        "turtle_master" => "resistance",
        "invisibility" => "invisibility",
        "slow_falling" => "slow_falling",
        _ => return None,
    })
}

/// Splash potions a bot throws at an enemy.
pub fn is_offensive_splash(s: &Stack) -> bool {
    matches!(s.short(), "splash_potion" | "lingering_potion")
        && s.potion.as_deref().is_some_and(|p| {
            let p = short(p).trim_start_matches("long_").trim_start_matches("strong_");
            matches!(p, "weakness" | "slowness" | "harming" | "poison")
        })
}

pub fn is_invisibility(s: &Stack) -> bool {
    s.potion.as_deref().is_some_and(|p| p.ends_with("invisibility"))
}

pub fn max_damage(id: &str) -> u32 {
    let s = short(id);
    if s == "mace" {
        return 500;
    }
    if s == "trident" {
        return 250;
    }
    if s == "shield" {
        return 336;
    }
    if s == "elytra" {
        return 432;
    }
    if s == "bow" {
        return 384;
    }
    if s == "crossbow" {
        return 465;
    }
    if s == "fishing_rod" {
        return 64;
    }
    if let Some((t, tool)) = tier(s) {
        let tool_d = match t {
            "wooden" => 59,
            "stone" => 131,
            "copper" => 190,
            "iron" => 250,
            "golden" => 32,
            "diamond" => 1561,
            _ => 2031,
        };
        if matches!(tool, "sword" | "axe" | "pickaxe" | "shovel" | "hoe" | "spear") {
            return tool_d;
        }
    }
    if let Some((slot, _, _)) = armor(id) {
        let mul = match short(id).split('_').next().unwrap_or("") {
            "leather" => 5,
            "chainmail" | "iron" => 15,
            "copper" => 11,
            "golden" => 7,
            "diamond" => 33,
            "netherite" => 37,
            _ => 0,
        };
        let base = [13, 15, 16, 11][slot as usize];
        return base * mul;
    }
    0
}

/// Blocks a bot may place to bridge or pillar (anything plain and solid).
pub fn is_building_block(id: &str) -> bool {
    let s = short(id);
    matches!(
        s,
        "cobblestone" | "stone" | "dirt" | "netherrack" | "cobbled_deepslate" | "end_stone" | "sandstone"
            | "oak_planks" | "spruce_planks" | "birch_planks" | "obsidian" | "blackstone" | "deepslate"
            | "andesite" | "diorite" | "granite" | "tuff" | "basalt" | "stone_bricks" | "mud_bricks"
    ) || s.ends_with("_planks")
        || s.ends_with("_wool")
        || s.ends_with("_concrete")
        || s.ends_with("_terracotta")
}

/// Items that break a fall without water (`block-clutch`).
pub fn is_clutch_block(id: &str) -> bool {
    matches!(short(id), "slime_block" | "cobweb" | "powder_snow_bucket" | "hay_block" | "twisting_vines" | "scaffolding")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapons_and_armour() {
        assert_eq!(weapon("minecraft:diamond_sword").damage, 7.0);
        assert_eq!(weapon("netherite_axe").kind, WeaponKind::Axe);
        assert_eq!(weapon("netherite_axe").damage, 10.0);
        assert_eq!(weapon("mace").kind, WeaponKind::Mace);
        assert_eq!(weapon("stick").kind, WeaponKind::Hand);
        assert_eq!(armor("diamond_chestplate"), Some((ArmorSlot::Chest, 8.0, 2.0)));
        assert_eq!(armor("netherite_boots"), Some((ArmorSlot::Feet, 3.0, 3.0)));
        assert_eq!(max_damage("diamond_chestplate"), 528);
        assert_eq!(max_damage("netherite_helmet"), 407);
        let s = Stack::new("diamond_sword", 1).with_enchant("sharpness", 5);
        assert_eq!(melee_damage(Some(&s)), 10.0);
    }

    #[test]
    fn armour_reduces_damage_as_vanilla() {
        // Full diamond: 20 points, 8 toughness, a 10 damage hit.
        let d = damage_after_armor(10.0, 20.0, 8.0, 0);
        assert!((d - 3.0).abs() < 0.01, "{d}");
        assert_eq!(damage_after_armor(10.0, 0.0, 0.0, 0), 10.0);
    }

    #[test]
    fn potions_are_recognised() {
        let h = Stack::new("splash_potion", 1).with_potion("strong_healing");
        assert!(is_healing_potion(&h));
        let w = Stack::new("splash_potion", 1).with_potion("weakness");
        assert!(is_offensive_splash(&w));
        let s = Stack::new("potion", 1).with_potion("long_strength");
        assert_eq!(buff_effect(&s), Some("strength"));
        assert!(is_plain_food("cooked_beef"));
        assert!(!is_plain_food("golden_apple"));
    }
}
