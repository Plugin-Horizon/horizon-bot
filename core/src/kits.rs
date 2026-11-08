//! Kits: saved inventories (hotbar, bag, armour, off hand, ender chest) in `kits.yml`, in the
//! layout a Paper server writes them (`==: org.bukkit.inventory.ItemStack`, `DataVersion`, `id`,
//! `count`, `components` as text), so the same file serves both. Also what a bot restocks to.

use crate::items::Stack;
use crate::yaml::Yaml;
use std::collections::BTreeMap;

/// One item as the file holds it. Unknown keys are kept as they were.
#[derive(Clone, Debug, PartialEq)]
pub struct KitItem {
    pub id: String,
    pub count: u32,
    /// Component id -> its value as text (SNBT/JSON, as Paper writes it).
    pub components: Vec<(String, String)>,
    /// The rest of the serialised map (`==`, `DataVersion`, `schema_version`, ...).
    pub extra: Vec<(String, Yaml)>,
}

impl KitItem {
    pub fn new(id: &str, count: u32) -> Self {
        KitItem {
            id: crate::items::full(id),
            count,
            components: Vec::new(),
            extra: vec![("==".into(), Yaml::Str("org.bukkit.inventory.ItemStack".into()))],
        }
    }

    pub fn from_yaml(v: &Yaml) -> Option<Self> {
        let m = v.as_map()?;
        let id = v.get("id")?.as_text()?;
        let count = v.get("count").and_then(Yaml::as_i64).unwrap_or(1).clamp(1, 99) as u32;
        let components = v
            .get("components")
            .and_then(Yaml::as_map)
            .map(|c| c.iter().filter_map(|(k, v)| Some((k.clone(), v.as_text()?))).collect())
            .unwrap_or_default();
        let extra = m
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "id" | "count" | "components"))
            .cloned()
            .collect();
        Some(KitItem { id, count, components, extra })
    }

    pub fn to_yaml(&self) -> Yaml {
        let mut v = Yaml::map();
        let mut extra = self.extra.iter();
        // `==` and `DataVersion` first, as Paper writes them.
        for (k, x) in extra.by_ref() {
            v.set(k, x.clone());
            if k == "DataVersion" {
                break;
            }
        }
        v.set("id", Yaml::Str(self.id.clone()));
        v.set("count", Yaml::Int(i64::from(self.count)));
        if !self.components.is_empty() {
            let mut c = Yaml::map();
            for (k, x) in &self.components {
                c.set(k, Yaml::Str(x.clone()));
            }
            v.set("components", c);
        }
        for (k, x) in extra {
            v.set(k, x.clone());
        }
        v
    }

    pub fn component(&self, id: &str) -> Option<&str> {
        self.components.iter().find(|(k, _)| k == id).map(|(_, v)| v.as_str())
    }

    /// The brain's view: id, count, enchantments, potion.
    pub fn to_stack(&self) -> Stack {
        let mut s = Stack::new(&self.id, self.count);
        if let Some(e) = self.component("minecraft:enchantments") {
            s.enchants = parse_levels(e);
        }
        if let Some(d) = self.component("minecraft:damage") {
            s.damage = d.trim().parse().unwrap_or(0);
        }
        if let Some(p) = self.component("minecraft:potion_contents") {
            s.potion = parse_potion(p);
        }
        s
    }

    /// What it counts as when restocking: the item, and the potion for potions.
    pub fn restock_key(&self) -> String {
        match self.component("minecraft:potion_contents").and_then(parse_potion) {
            Some(p) => format!("{}#{p}", self.id),
            None => self.id.clone(),
        }
    }
}

/// `{"minecraft:sharpness":5,"minecraft:unbreaking":3}` (or the SNBT form with unquoted keys).
pub fn parse_levels(text: &str) -> Vec<(String, u32)> {
    let inner = text.trim().trim_start_matches('{').trim_end_matches('}');
    // Paper 1.21+ may nest them under "levels".
    let inner = inner
        .find("levels")
        .and_then(|i| inner[i..].find('{').map(|j| &inner[i + j + 1..]))
        .map(|s| s.trim_end_matches('}'))
        .unwrap_or(inner);
    inner
        .split(',')
        .filter_map(|kv| {
            let (k, v) = kv.rsplit_once(':')?;
            let k = k.trim().trim_matches('"').trim_matches('\'');
            let v: u32 = v.trim().trim_end_matches(['b', 's', 'i']).parse().ok()?;
            Some((crate::items::full(k), v))
        })
        .collect()
}

/// `{potion:"minecraft:strong_healing"}`, `{"potion":"minecraft:swiftness"}` or a bare id.
pub fn parse_potion(text: &str) -> Option<String> {
    let t = text.trim();
    if !t.starts_with('{') {
        let id = t.trim_matches('"');
        return (!id.is_empty()).then(|| crate::items::full(id));
    }
    let at = t.find("potion")?;
    let rest = &t[at + "potion".len()..];
    let rest = rest.trim_start_matches(['"', '\'']).trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.trim_start_matches(['"', '\'']);
    let end = rest.find(['"', '\'', ',', '}']).unwrap_or(rest.len());
    let id = &rest[..end];
    (!id.is_empty()).then(|| crate::items::full(id))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kit {
    /// The player's inventory contents as Paper lists them: 0..9 hotbar, 9..36 bag, then armour
    /// (boots first), off hand and the newer body slots. Missing entries are empty.
    pub contents: Vec<Option<KitItem>>,
    /// Boots, leggings, chestplate, helmet.
    pub armour: Vec<Option<KitItem>>,
    pub offhand: Option<KitItem>,
    /// The kit editor's second page.
    pub enderchest: Vec<Option<KitItem>>,
}

fn items(v: Option<&Yaml>) -> Vec<Option<KitItem>> {
    v.and_then(Yaml::as_seq)
        .map(|s| s.iter().map(KitItem::from_yaml).collect())
        .unwrap_or_default()
}

fn items_yaml(v: &[Option<KitItem>]) -> Yaml {
    Yaml::Seq(v.iter().map(|i| i.as_ref().map_or(Yaml::Null, KitItem::to_yaml)).collect())
}

impl Kit {
    pub fn from_yaml(v: &Yaml) -> Self {
        Kit {
            contents: items(v.get("contents")),
            armour: items(v.get("armour")),
            offhand: v.get("offhand").and_then(KitItem::from_yaml),
            enderchest: items(v.get("enderchest")),
        }
    }

    pub fn to_yaml(&self) -> Yaml {
        let mut v = Yaml::map();
        v.set("contents", items_yaml(&self.contents));
        v.set("armour", items_yaml(&self.armour));
        v.set("offhand", self.offhand.as_ref().map_or(Yaml::Null, KitItem::to_yaml));
        if !self.enderchest.is_empty() {
            v.set("enderchest", items_yaml(&self.enderchest));
        }
        v
    }

    /// The 36 storage slots (hotbar first).
    pub fn storage(&self) -> Vec<Option<KitItem>> {
        let mut s: Vec<Option<KitItem>> = self.contents.iter().take(36).cloned().collect();
        s.resize(36, None);
        s
    }

    /// Armour by slot, boots first, from `armour` or the content's armour entries.
    pub fn armour_slots(&self) -> [Option<KitItem>; 4] {
        let mut a: [Option<KitItem>; 4] = Default::default();
        for i in 0..4 {
            a[i] = self
                .armour
                .get(i)
                .cloned()
                .flatten()
                .or_else(|| self.contents.get(36 + i).cloned().flatten());
        }
        a
    }

    pub fn offhand_item(&self) -> Option<KitItem> {
        self.offhand.clone().or_else(|| self.contents.get(40).cloned().flatten())
    }

    pub fn is_empty(&self) -> bool {
        self.contents.iter().all(Option::is_none)
            && self.armour.iter().all(Option::is_none)
            && self.offhand.is_none()
    }

    /// Every item it holds, worn and carried (not the ender chest).
    pub fn all_items(&self) -> Vec<KitItem> {
        let mut v: Vec<KitItem> = self.storage().into_iter().flatten().collect();
        v.extend(self.armour_slots().into_iter().flatten());
        v.extend(self.offhand_item());
        v
    }

    /// The counts a regear tops a bot back up to: what the kit had, and never below the
    /// standing minimums (4 totems, 16 pearls, 8 golden apples, 4 healing potions).
    pub fn restock_targets(&self) -> BTreeMap<String, u32> {
        let mut t: BTreeMap<String, u32> = BTreeMap::new();
        for i in self.all_items() {
            *t.entry(i.restock_key()).or_default() += i.count;
        }
        for (key, min) in [
            ("minecraft:totem_of_undying", 4),
            ("minecraft:ender_pearl", 16),
            ("minecraft:golden_apple", 8),
        ] {
            let e = t.entry(key.to_owned()).or_default();
            *e = (*e).max(min);
        }
        let healing: u32 = t
            .iter()
            .filter(|(k, _)| k.contains("potion#") && k.ends_with("healing"))
            .map(|(_, v)| *v)
            .sum();
        if healing < 4 {
            *t.entry("minecraft:splash_potion#minecraft:strong_healing".into()).or_default() += 4 - healing;
        }
        t
    }
}

/// What a bot is short of, against what it carries now (`restock_key` -> count).
pub fn shortfall(targets: &BTreeMap<String, u32>, carried: &BTreeMap<String, u32>) -> Vec<(String, u32)> {
    targets
        .iter()
        .filter_map(|(k, want)| {
            let have = carried.get(k).copied().unwrap_or(0);
            (have < *want).then(|| (k.clone(), want - have))
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct Kits {
    pub kits: BTreeMap<String, Kit>,
}

impl Kits {
    pub fn load(doc: &Yaml) -> Self {
        let mut kits = BTreeMap::new();
        if let Some(m) = doc.get("kits").and_then(Yaml::as_map) {
            for (name, v) in m {
                kits.insert(name.clone(), Kit::from_yaml(v));
            }
        }
        Kits { kits }
    }

    pub fn save(&self) -> Yaml {
        let mut m = Yaml::map();
        for (name, k) in &self.kits {
            m.set(name, k.to_yaml());
        }
        let mut doc = Yaml::map();
        doc.set("kits", m);
        doc
    }

    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), String> {
        if self.kits.contains_key(new) {
            return Err(format!("A kit called {new} exists already"));
        }
        let k = self.kits.remove(old).ok_or_else(|| format!("No kit called {old}"))?;
        self.kits.insert(new.to_owned(), k);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KITS: &str = "\
kits:
  iron:
    contents:
    - ==: org.bukkit.inventory.ItemStack
      DataVersion: 4790
      id: minecraft:iron_sword
      count: 1
      components:
        minecraft:enchantments: '{\"minecraft:sharpness\":2}'
      schema_version: 1
    - null
    - ==: org.bukkit.inventory.ItemStack
      DataVersion: 4790
      id: minecraft:splash_potion
      count: 3
      components:
        minecraft:potion_contents: '{potion:\"minecraft:strong_healing\"}'
      schema_version: 1
    armour:
    - ==: org.bukkit.inventory.ItemStack
      DataVersion: 4790
      id: minecraft:iron_boots
      count: 1
      schema_version: 1
    - null
    - null
    - null
    offhand: null
";

    #[test]
    fn reads_paper_kits_and_writes_them_back() {
        let doc = crate::yaml::parse(KITS).unwrap();
        let kits = Kits::load(&doc);
        let k = &kits.kits["iron"];
        let sword = k.contents[0].as_ref().unwrap();
        assert_eq!(sword.id, "minecraft:iron_sword");
        assert_eq!(sword.to_stack().enchant("sharpness"), 2);
        assert_eq!(k.contents[2].as_ref().unwrap().to_stack().potion.as_deref(), Some("minecraft:strong_healing"));
        assert_eq!(k.armour_slots()[0].as_ref().unwrap().id, "minecraft:iron_boots");
        let text = crate::yaml::dump(&kits.save());
        assert!(text.contains("==: org.bukkit.inventory.ItemStack"));
        let again = Kits::load(&crate::yaml::parse(&text).unwrap());
        assert_eq!(again.kits, kits.kits);
        // Key order as Paper's: == and DataVersion before id.
        let i = text.find("DataVersion").unwrap();
        assert!(i < text.find("id: minecraft:iron_sword").unwrap());
    }

    #[test]
    fn restock_targets_keep_the_minimums() {
        let doc = crate::yaml::parse(KITS).unwrap();
        let kits = Kits::load(&doc);
        let t = kits.kits["iron"].restock_targets();
        assert_eq!(t["minecraft:totem_of_undying"], 4);
        assert_eq!(t["minecraft:splash_potion#minecraft:strong_healing"], 4);
        let mut have = BTreeMap::new();
        have.insert("minecraft:ender_pearl".to_owned(), 10);
        let s = shortfall(&t, &have);
        assert!(s.contains(&("minecraft:ender_pearl".into(), 6)));
    }

    #[test]
    fn enchantment_text_forms() {
        assert_eq!(parse_levels("{\"minecraft:sharpness\":5}"), vec![("minecraft:sharpness".into(), 5)]);
        assert_eq!(
            parse_levels("{levels:{\"minecraft:breach\":4,\"minecraft:density\":5}}"),
            vec![("minecraft:breach".into(), 4), ("minecraft:density".into(), 5)]
        );
        assert_eq!(parse_potion("{\"potion\":\"minecraft:swiftness\"}").as_deref(), Some("minecraft:swiftness"));
    }
}
