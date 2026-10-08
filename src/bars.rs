#![cfg_attr(not(windows), allow(dead_code))]

//! Bar profiles ("Work", "Gaming"…): several bars, each with its own pinned
//! apps, order, and root categories left off it, switched from the menus or
//! a hotkey. The bar in use is always the one in `Config::pinned`,
//! `bar_order` and `hidden_categories`, so everything else keeps reading
//! those; `Config::bars` keeps every bar by name, and switching swaps them.
//! Pure, so it can be unit-tested off Windows.

use crate::config::Config;
use serde::{Deserialize, Serialize};

/// The name of the bar there is before any other is made.
pub const FIRST: &str = "Main";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bar {
    pub name: String,
    pub pinned: Vec<String>,
    pub bar_order: Vec<String>,
    /// Root categories not shown on this bar.
    pub hidden: Vec<u64>,
}

/// The bar in use's name.
pub fn current(cfg: &Config) -> String {
    if cfg.bar.trim().is_empty() { FIRST.to_string() } else { cfg.bar.clone() }
}

/// Every bar's name, in order (just the one in use until another is made).
pub fn names(cfg: &Config) -> Vec<String> {
    let cur = current(cfg);
    let mut v: Vec<String> = cfg.bars.iter().map(|b| b.name.clone()).collect();
    if !v.contains(&cur) {
        v.insert(0, cur);
    }
    v
}

/// Copies the bar in use into its entry in `bars` (adding it if missing).
fn store_current(cfg: &mut Config) {
    let bar = Bar {
        name: current(cfg),
        pinned: cfg.pinned.clone(),
        bar_order: cfg.bar_order.clone(),
        hidden: cfg.hidden_categories.clone(),
    };
    match cfg.bars.iter_mut().find(|b| b.name == bar.name) {
        Some(b) => *b = bar,
        None => cfg.bars.insert(0, bar),
    }
}

fn load(cfg: &mut Config, bar: Bar) {
    cfg.pinned = bar.pinned;
    cfg.bar_order = bar.bar_order;
    cfg.hidden_categories = bar.hidden;
    cfg.bar = bar.name;
}

/// Switches to the bar called `name`. Returns false if there is none, or it
/// is already in use.
pub fn switch(cfg: &mut Config, name: &str) -> bool {
    if current(cfg) == name {
        return false;
    }
    let Some(bar) = cfg.bars.iter().find(|b| b.name == name).cloned() else { return false };
    store_current(cfg);
    load(cfg, bar);
    true
}

/// The bar after the one in use (wrapping round), if there are several.
pub fn next(cfg: &Config) -> Option<String> {
    let names = names(cfg);
    let cur = current(cfg);
    let i = names.iter().position(|n| *n == cur)?;
    (names.len() > 1).then(|| names[(i + 1) % names.len()].clone())
}

/// Makes a new bar and switches to it: empty (just the root categories), or
/// a copy of the bar in use. Its name is `wanted`, made unique. Returns it.
pub fn add(cfg: &mut Config, wanted: &str, copy: bool) -> String {
    store_current(cfg);
    let taken = names(cfg);
    let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
    let name = crate::looks::unique_name(&taken, if wanted.trim().is_empty() { "New bar" } else { wanted });
    let bar = if copy {
        Bar {
            name: name.clone(),
            pinned: cfg.pinned.clone(),
            bar_order: cfg.bar_order.clone(),
            hidden: cfg.hidden_categories.clone(),
        }
    } else {
        Bar { name: name.clone(), ..Default::default() }
    };
    cfg.bars.push(bar.clone());
    load(cfg, bar);
    name
}

/// Renames the bar in use. Returns the name it got (made unique), or `None`
/// if it is blank or unchanged.
pub fn rename(cfg: &mut Config, wanted: &str) -> Option<String> {
    let cur = current(cfg);
    let wanted = crate::looks::tidy_name(wanted);
    if wanted.is_empty() || wanted == cur {
        return None;
    }
    store_current(cfg);
    let others: Vec<String> = names(cfg).into_iter().filter(|n| *n != cur).collect();
    let others: Vec<&str> = others.iter().map(String::as_str).collect();
    let name = crate::looks::unique_name(&others, &wanted);
    if let Some(b) = cfg.bars.iter_mut().find(|b| b.name == cur) {
        b.name = name.clone();
    }
    cfg.bar = name.clone();
    Some(name)
}

/// Deletes the bar in use and switches to the next one. The last bar can't
/// be deleted. Returns the bar now in use.
pub fn delete_current(cfg: &mut Config) -> Option<String> {
    let to = next(cfg)?;
    let cur = current(cfg);
    let bar = cfg.bars.iter().find(|b| b.name == to).cloned()?;
    cfg.bars.retain(|b| b.name != cur);
    load(cfg, bar);
    Some(to)
}

/// A custom app was deleted: off every bar, not just the one in use.
pub fn forget_app(cfg: &mut Config, id: &str) {
    cfg.unpin(id);
    for b in &mut cfg.bars {
        b.pinned.retain(|p| p != id);
        b.bar_order.retain(|p| p != id);
    }
}

/// Leaves a root category off the bar in use (`false`), or puts it back.
pub fn show_category(cfg: &mut Config, id: u64, shown: bool) -> bool {
    let hidden = cfg.hidden_categories.contains(&id);
    if shown == !hidden {
        return false;
    }
    if shown {
        cfg.hidden_categories.retain(|h| *h != id);
    } else {
        cfg.hidden_categories.push(id);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Category;

    fn cfg() -> Config {
        let mut c = Config::default();
        for (id, name) in [(1, "Dev"), (2, "Games")] {
            c.categories.push(Category { id, name: name.into(), ..Default::default() });
        }
        c.pin("code");
        c.pin("term");
        c
    }

    #[test]
    fn one_bar_to_begin_with() {
        let c = cfg();
        assert_eq!(current(&c), "Main");
        assert_eq!(names(&c), ["Main"]);
        assert_eq!(next(&c), None);
    }

    #[test]
    fn making_and_switching() {
        let mut c = cfg();
        // An empty bar: no pinned apps, every root category.
        assert_eq!(add(&mut c, "Gaming", false), "Gaming");
        assert_eq!(c.bar_keys(), ["cat:1", "cat:2"]);
        c.pin("steam");
        assert!(show_category(&mut c, 1, false));
        assert_eq!(c.bar_keys(), ["cat:2", "steam"]);
        assert_eq!(names(&c), ["Main", "Gaming"]);
        // Back to the first: as it was.
        assert!(switch(&mut c, "Main"));
        assert_eq!(c.bar_keys(), ["cat:1", "cat:2", "code", "term"]);
        assert!(!switch(&mut c, "Main"));
        assert!(!switch(&mut c, "Nope"));
        // And again: the second kept its own.
        assert_eq!(next(&c).as_deref(), Some("Gaming"));
        assert!(switch(&mut c, "Gaming"));
        assert_eq!(c.bar_keys(), ["cat:2", "steam"]);
        assert_eq!(next(&c).as_deref(), Some("Main"));
        // A copy starts like the one in use, under a unique name.
        assert_eq!(add(&mut c, "gaming", true), "gaming (2)");
        assert_eq!(c.bar_keys(), ["cat:2", "steam"]);
        assert_eq!(names(&c), ["Main", "Gaming", "gaming (2)"]);
        assert_eq!(add(&mut c, "  ", false), "New bar");
        // A deleted app leaves every bar.
        forget_app(&mut c, "steam");
        assert!(switch(&mut c, "Gaming"));
        assert_eq!(c.bar_keys(), ["cat:2"]);
        assert!(c.bars.iter().all(|b| !b.pinned.contains(&"steam".to_string())));
    }

    #[test]
    fn renaming_and_deleting() {
        let mut c = cfg();
        assert_eq!(rename(&mut c, " Work "), Some("Work".into()));
        assert_eq!(names(&c), ["Work"]);
        assert_eq!(rename(&mut c, "Work"), None);
        assert_eq!(rename(&mut c, ""), None);
        assert_eq!(rename(&mut c, "Work\u{202E}\n"), None); // tidied, it is the same name
        assert_eq!(add(&mut c, "Tab\tbar", false), "Tab bar");
        assert!(switch(&mut c, "Work"));
        c.bars.retain(|b| b.name != "Tab bar");
        // The last bar stays.
        assert_eq!(delete_current(&mut c), None);
        add(&mut c, "Play", false);
        c.pin("game");
        assert_eq!(rename(&mut c, "work"), Some("work (2)".into()));
        // Deleting the one in use goes to the next, as it was.
        assert_eq!(delete_current(&mut c), Some("Work".into()));
        assert_eq!(names(&c), ["Work"]);
        assert_eq!(c.bar_keys(), ["cat:1", "cat:2", "code", "term"]);
    }

    #[test]
    fn hiding_categories() {
        let mut c = cfg();
        assert!(show_category(&mut c, 2, false));
        assert!(!show_category(&mut c, 2, false));
        assert_eq!(c.bar_keys(), ["cat:1", "code", "term"]);
        assert!(show_category(&mut c, 2, true));
        assert!(!show_category(&mut c, 2, true));
        assert_eq!(c.bar_keys(), ["cat:1", "cat:2", "code", "term"]);
        // An older config without bars reads as one bar.
        let old: Config = serde_json::from_str(r#"{"pinned":["x"]}"#).unwrap();
        assert_eq!((names(&old), old.bar_keys()), (vec!["Main".to_string()], vec!["x".to_string()]));
    }
}
