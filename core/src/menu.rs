//! The menus: chest screens built from plain items, one click per change. The hub leads to the
//! settings (by group, paged), the techniques, the bots, the factions, the kits and the presets.
//! A settings page can show the values of one bot or one faction; a click changes the value at
//! that layer (left: on / up / next, right: off / down / back).

use crate::command::Scope;
use crate::settings::{self, Kind, Snapshot};

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub item: String,
    pub count: u32,
    pub name: String,
    pub lore: Vec<String>,
}

impl MenuItem {
    fn new(item: &str, name: impl Into<String>) -> Self {
        MenuItem { item: crate::items::full(item), count: 1, name: name.into(), lore: Vec::new() }
    }
    fn lore(mut self, l: impl Into<String>) -> Self {
        self.lore.push(l.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Hub,
    Settings { scope: Scope, group: Option<String>, page: usize },
    Techniques { scope: Scope },
    Bots { page: usize },
    Factions,
    Kits,
    Presets,
}

/// What a slot does when clicked.
#[derive(Clone, Debug, PartialEq)]
pub enum Click {
    Nothing,
    Open(Page),
    /// A `/horizonbot` line run as the viewer.
    Command(String),
    /// Change one setting at a layer: (scope, key, left click).
    Setting(Scope, &'static str, bool),
}

pub struct Menu {
    pub title: String,
    pub rows: u8,
    pub items: Vec<Option<MenuItem>>,
    pub clicks: Vec<Click>,
}

impl Menu {
    fn new(title: impl Into<String>, rows: u8) -> Self {
        let n = rows as usize * 9;
        Menu { title: title.into(), rows, items: vec![None; n], clicks: vec![Click::Nothing; n] }
    }
    fn put(&mut self, slot: usize, item: MenuItem, click: Click) {
        if slot < self.items.len() {
            self.items[slot] = Some(item);
            self.clicks[slot] = click;
        }
    }
    fn back(&mut self, to: Page) {
        let s = self.items.len() - 9;
        self.put(s, MenuItem::new("arrow", "Back"), Click::Open(to));
    }
}

/// What a menu needs to know about the plugin.
pub struct View<'a> {
    pub snapshot: &'a dyn Fn(&Scope) -> Snapshot,
    pub bots: Vec<(String, bool, Option<String>)>,
    pub factions: Vec<(String, usize)>,
    pub kits: Vec<String>,
    pub presets: Vec<String>,
    pub active_preset: Option<String>,
}

fn scope_name(s: &Scope) -> String {
    match s {
        Scope::Global => "everyone".into(),
        Scope::Bot(b) => format!("bot {b}"),
        Scope::Faction(f) => format!("faction {f}"),
    }
}

fn setting_item(spec: &settings::Spec, snap: &Snapshot) -> MenuItem {
    let v = snap.s(spec.key);
    let item = match spec.kind {
        Kind::Bool(_) => {
            if snap.b(spec.key) {
                "lime_dye"
            } else {
                "gray_dye"
            }
        }
        Kind::Int(..) | Kind::Num(..) => "paper",
        Kind::Choice(..) => "book",
    };
    let limits = match spec.kind {
        Kind::Bool(_) => "left: on, right: off".to_owned(),
        Kind::Int(_, lo, hi) => format!("{lo} to {hi}; left: up, right: down"),
        Kind::Num(_, lo, hi) => format!("{lo} to {hi}; left: up, right: down"),
        Kind::Choice(_, c) => format!("{}; left: next, right: back", c.join(" / ")),
    };
    MenuItem::new(item, format!("{} = {v}", spec.key)).lore(spec.group).lore(limits)
}

pub fn build(page: &Page, view: &View) -> Menu {
    match page {
        Page::Hub => {
            let mut m = Menu::new("HorizonBot", 3);
            m.put(10, MenuItem::new("player_head", "Bots").lore(format!("{} spawned", view.bots.len())), Click::Open(Page::Bots { page: 0 }));
            m.put(11, MenuItem::new("comparator", "Settings"), Click::Open(Page::Settings { scope: Scope::Global, group: None, page: 0 }));
            m.put(12, MenuItem::new("mace", "Techniques"), Click::Open(Page::Techniques { scope: Scope::Global }));
            m.put(14, MenuItem::new("white_banner", "Factions").lore(format!("{}", view.factions.len())), Click::Open(Page::Factions));
            m.put(15, MenuItem::new("chest", "Kits").lore(format!("{}", view.kits.len())), Click::Open(Page::Kits));
            m.put(
                16,
                MenuItem::new("enchanted_book", "Presets").lore(format!("active: {}", view.active_preset.clone().unwrap_or_else(|| "none".into()))),
                Click::Open(Page::Presets),
            );
            m
        }
        Page::Settings { scope, group, page } => {
            let snap = (view.snapshot)(scope);
            match group {
                None => {
                    let mut m = Menu::new(format!("Settings: {}", scope_name(scope)), 4);
                    for (i, g) in settings::groups().into_iter().enumerate() {
                        let n = settings::specs().filter(|s| s.group == g).count();
                        m.put(
                            i,
                            MenuItem::new("book", g).lore(format!("{n} settings")),
                            Click::Open(Page::Settings { scope: scope.clone(), group: Some(g.to_owned()), page: 0 }),
                        );
                    }
                    m.back(Page::Hub);
                    m
                }
                Some(g) => {
                    let specs: Vec<&settings::Spec> = settings::specs().filter(|s| s.group == g.as_str()).collect();
                    let per = 45;
                    let pages = specs.len().div_ceil(per).max(1);
                    let p = (*page).min(pages - 1);
                    let mut m = Menu::new(format!("{g} ({}/{pages}): {}", p + 1, scope_name(scope)), 6);
                    for (i, s) in specs.iter().skip(p * per).take(per).enumerate() {
                        m.put(i, setting_item(s, &snap), Click::Setting(scope.clone(), s.key, true));
                    }
                    if p > 0 {
                        m.put(48, MenuItem::new("arrow", "Previous page"), Click::Open(Page::Settings { scope: scope.clone(), group: Some(g.clone()), page: p - 1 }));
                    }
                    if p + 1 < pages {
                        m.put(50, MenuItem::new("arrow", "Next page"), Click::Open(Page::Settings { scope: scope.clone(), group: Some(g.clone()), page: p + 1 }));
                    }
                    m.back(Page::Settings { scope: scope.clone(), group: None, page: 0 });
                    m
                }
            }
        }
        Page::Techniques { scope } => {
            let snap = (view.snapshot)(scope);
            let mut m = Menu::new(format!("Techniques: {}", scope_name(scope)), 5);
            for (i, s) in settings::techniques().into_iter().enumerate() {
                m.put(i, setting_item(s, &snap), Click::Setting(scope.clone(), s.key, true));
            }
            m.back(Page::Hub);
            m
        }
        Page::Bots { page } => {
            let per = 45;
            let pages = view.bots.len().div_ceil(per).max(1);
            let p = (*page).min(pages - 1);
            let mut m = Menu::new(format!("Bots ({}/{pages})", p + 1), 6);
            for (i, (name, paused, faction)) in view.bots.iter().skip(p * per).take(per).enumerate() {
                let item = MenuItem::new(if *paused { "skeleton_skull" } else { "player_head" }, name.clone())
                    .lore(faction.clone().map_or("no faction".into(), |f| format!("faction {f}")))
                    .lore(if *paused { "paused: click to resume" } else { "click to pause" });
                let cmd = if *paused { format!("resume {name}") } else { format!("pause {name}") };
                m.put(i, item, Click::Command(cmd));
            }
            if p > 0 {
                m.put(48, MenuItem::new("arrow", "Previous page"), Click::Open(Page::Bots { page: p - 1 }));
            }
            if p + 1 < pages {
                m.put(50, MenuItem::new("arrow", "Next page"), Click::Open(Page::Bots { page: p + 1 }));
            }
            m.put(53, MenuItem::new("barrier", "Remove all bots"), Click::Command("removeall".into()));
            m.back(Page::Hub);
            m
        }
        Page::Factions => {
            let mut m = Menu::new("Factions", 6);
            for (i, (f, n)) in view.factions.iter().take(45).enumerate() {
                m.put(
                    i,
                    MenuItem::new("white_banner", f.clone()).lore(format!("{n} members")).lore("click: its settings"),
                    Click::Open(Page::Settings { scope: Scope::Faction(f.clone()), group: None, page: 0 }),
                );
            }
            m.back(Page::Hub);
            m
        }
        Page::Kits => {
            let mut m = Menu::new("Kits: click to take one", 6);
            for (i, k) in view.kits.iter().take(45).enumerate() {
                m.put(i, MenuItem::new("chest", k.clone()), Click::Command(format!("kit give {k} {{viewer}}")));
            }
            m.back(Page::Hub);
            m
        }
        Page::Presets => {
            let mut m = Menu::new("Presets", 3);
            for (i, p) in view.presets.iter().take(18).enumerate() {
                let on = view.active_preset.as_deref() == Some(p.as_str());
                m.put(i, MenuItem::new(if on { "enchanted_book" } else { "book" }, p.clone()).lore(if on { "active" } else { "click to apply" }), Click::Command(format!("preset {p}")));
            }
            m.put(22, MenuItem::new("barrier", "Preset off"), Click::Command("preset off".into()));
            m.back(Page::Hub);
            m
        }
    }
}

/// The new value a click gives a setting: left = on / up / next, right = off / down / back.
pub fn stepped(spec: &settings::Spec, snap: &Snapshot, left: bool) -> String {
    match spec.kind {
        Kind::Bool(_) => left.to_string(),
        Kind::Int(_, lo, hi) => {
            let step = ((hi - lo) / 20).max(1);
            let v = snap.i(spec.key) + if left { step } else { -step };
            v.clamp(lo, hi).to_string()
        }
        Kind::Num(_, lo, hi) => {
            let step = ((hi - lo) / 20.0).max(0.1);
            let v = snap.f(spec.key) + if left { step } else { -step };
            format!("{:.2}", v.clamp(lo, hi))
        }
        Kind::Choice(_, c) => {
            let cur = snap.s(spec.key);
            let i = c.iter().position(|x| *x == cur).unwrap_or(0);
            let n = c.len();
            c[if left { (i + 1) % n } else { (i + n - 1) % n }].to_owned()
        }
    }
}

/// Parses a page id (`hub`, `settings:<scope>:<group>`, the short forms).
pub fn parse_page(s: &str) -> Page {
    let mut parts = s.split(':');
    match parts.next() {
        Some("settings") => Page::Settings { scope: Scope::Global, group: parts.nth(1).filter(|g| !g.is_empty()).map(str::to_owned), page: 0 },
        Some("faction-settings") => Page::Settings { scope: Scope::Faction(parts.next().unwrap_or("").to_owned()), group: None, page: 0 },
        Some("techniques") => Page::Techniques { scope: Scope::Global },
        Some("bots") => Page::Bots { page: 0 },
        Some("factions") => Page::Factions,
        Some("kits") => Page::Kits,
        Some("presets") => Page::Presets,
        _ => Page::Hub,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View<'static> {
        View {
            snapshot: &|_| crate::settings::Settings::new().snapshot(None, None),
            bots: vec![("Marco".into(), false, None), ("Polo".into(), true, Some("red".into()))],
            factions: vec![("red".into(), 1)],
            kits: vec!["iron".into()],
            presets: vec!["sweaty".into()],
            active_preset: None,
        }
    }

    #[test]
    fn the_hub_leads_everywhere() {
        let m = build(&Page::Hub, &view());
        assert!(m.clicks.contains(&Click::Open(Page::Bots { page: 0 })));
        assert!(m.clicks.contains(&Click::Open(Page::Presets)));
    }

    #[test]
    fn every_group_fits_its_pages() {
        let v = view();
        for g in settings::groups() {
            let m = build(&Page::Settings { scope: Scope::Global, group: Some(g.into()), page: 0 }, &v);
            let shown = m.clicks.iter().filter(|c| matches!(c, Click::Setting(..))).count();
            let total = settings::specs().filter(|s| s.group == g).count();
            assert_eq!(shown, total.min(45), "{g}");
        }
        let m = build(&Page::Bots { page: 0 }, &v);
        assert_eq!(m.clicks[1], Click::Command("resume Polo".into()));
    }

    #[test]
    fn clicks_step_values() {
        let snap = crate::settings::Settings::new().snapshot(None, None);
        let skill = settings::spec("skill").unwrap();
        assert_eq!(stepped(skill, &snap, true), "50");
        assert_eq!(stepped(skill, &snap, false), "40");
        let crits = settings::spec("crits").unwrap();
        assert_eq!(stepped(crits, &snap, false), "false");
        let ps = settings::spec("playstyle").unwrap();
        assert_eq!(stepped(ps, &snap, true), "aggressive");
        assert_eq!(stepped(ps, &snap, false), "adaptive");
    }
}
