#![cfg_attr(not(windows), allow(dead_code))]

//! Smart categories: categories that fill themselves — recently installed
//! apps, the most used ones, or every app matching a simple rule (its kind,
//! and words in its name or file). Their app lists are worked out from the
//! rule each time the app list or the launch counts change; they can't be
//! filled by hand, so nothing filed by hand ever moves. Pure, so it can be
//! unit-tested off Windows.

use crate::allview::KindGroup;
use crate::config::Category;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// At most this many apps in a smart category.
pub const MAX_APPS: usize = 48;

/// When more apps than this appear at once, they weren't just installed:
/// the list's source changed (the first scan, package managers' apps
/// turned on, the Start Menu used in place of the Apps folder…).
pub const BULK: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "fill", rename_all = "snake_case")]
pub enum Smart {
    /// Apps that turned up in the last `days` days, newest first.
    RecentlyInstalled { days: u32 },
    /// The `count` apps launched most often.
    MostUsed { count: usize },
    /// Every app of one of `kinds` (any kind when empty) whose name or file
    /// contains `contains` (ignoring case; anything when empty), by name.
    Matching {
        #[serde(default)]
        kinds: Vec<KindGroup>,
        #[serde(default)]
        contains: String,
    },
}

/// A kind, as said in the middle of a sentence.
pub fn noun(k: KindGroup) -> &'static str {
    match k {
        KindGroup::Desktop => "desktop programs",
        KindGroup::Store => "Store apps",
        KindGroup::Web => "web apps",
        KindGroup::Package => "package manager tools",
        KindGroup::Custom => "custom apps",
    }
}

/// What a smart category may look at for one app.
pub struct Facts<'a> {
    pub id: &'a str,
    pub name: &'a str,
    /// File name, path or AppUserModelID.
    pub file: &'a str,
    pub kind: KindGroup,
}

/// What FlexTaskbar remembers between runs (`Config::first_seen`,
/// `Config::launches`).
pub struct History<'a> {
    /// When each app was first seen (Unix seconds; 0: before anything was
    /// noted, so never "recent").
    pub first_seen: &'a BTreeMap<String, u64>,
    pub launches: &'a BTreeMap<String, u32>,
}

impl Smart {
    /// The ready-made ones offered for a new smart category, with a name.
    pub fn presets() -> Vec<(&'static str, Smart)> {
        vec![
            ("Recently installed", Smart::RecentlyInstalled { days: 14 }),
            ("Most used", Smart::MostUsed { count: 10 }),
            ("Package manager tools", Smart::Matching { kinds: vec![KindGroup::Package], contains: String::new() }),
            ("Web apps", Smart::Matching { kinds: vec![KindGroup::Web], contains: String::new() }),
            ("Store apps", Smart::Matching { kinds: vec![KindGroup::Store], contains: String::new() }),
        ]
    }

    /// One line saying what fills it.
    pub fn describe(&self) -> String {
        match self {
            Smart::RecentlyInstalled { days } => format!("Fills itself: apps installed in the last {days} days"),
            Smart::MostUsed { count } => format!("Fills itself: the {count} most used apps"),
            Smart::Matching { kinds, contains } => {
                let what = if kinds.is_empty() {
                    "all apps".to_string()
                } else {
                    kinds.iter().map(|k| noun(*k)).collect::<Vec<_>>().join(", ")
                };
                if contains.trim().is_empty() {
                    format!("Fills itself: {what}")
                } else {
                    format!("Fills itself: {what} with “{}” in the name", contains.trim())
                }
            }
        }
    }

    /// The apps it holds now, in order.
    pub fn fill(&self, apps: &[Facts], history: &History, now: u64) -> Vec<String> {
        let mut picked: Vec<&Facts> = match self {
            Smart::RecentlyInstalled { days } => {
                let since = now.saturating_sub(*days as u64 * 86_400);
                let seen = |a: &Facts| history.first_seen.get(a.id).copied().unwrap_or(0);
                let mut v: Vec<&Facts> = apps.iter().filter(|a| seen(a) > 0 && seen(a) >= since).collect();
                v.sort_by_cached_key(|a| (std::cmp::Reverse(seen(a)), a.name.to_lowercase()));
                v
            }
            Smart::MostUsed { count } => {
                let used = |a: &Facts| history.launches.get(a.id).copied().unwrap_or(0);
                let mut v: Vec<&Facts> = apps.iter().filter(|a| used(a) > 0).collect();
                v.sort_by_cached_key(|a| (std::cmp::Reverse(used(a)), a.name.to_lowercase()));
                v.truncate(*count);
                v
            }
            Smart::Matching { kinds, contains } => {
                let words = contains.trim().to_lowercase();
                let mut v: Vec<&Facts> = apps
                    .iter()
                    .filter(|a| kinds.is_empty() || kinds.contains(&a.kind))
                    .filter(|a| {
                        words.is_empty()
                            || a.name.to_lowercase().contains(&words)
                            || a.file.to_lowercase().contains(&words)
                    })
                    .collect();
                v.sort_by_cached_key(|a| a.name.to_lowercase());
                v
            }
        };
        picked.truncate(MAX_APPS);
        picked.into_iter().map(|a| a.id.to_string()).collect()
    }
}

/// Notes the apps in the list for the first time, at `now`, and forgets the
/// ones no longer there (so a reinstalled app counts as new again). A first
/// list, or a lot of apps at once, is noted as having always been there
/// (see [`BULK`]). Returns whether anything changed.
pub fn note_seen(first_seen: &mut BTreeMap<String, u64>, ids: &[&str], now: u64) -> bool {
    let before = first_seen.len();
    let listed: HashSet<&str> = ids.iter().copied().collect();
    first_seen.retain(|id, _| listed.contains(id.as_str()));
    let mut changed = first_seen.len() != before;
    let new: Vec<&str> = ids.iter().copied().filter(|id| !first_seen.contains_key(*id)).collect();
    let when = if before == 0 || new.len() > BULK { 0 } else { now };
    for id in new {
        first_seen.insert(id.to_string(), when);
        changed = true;
    }
    changed
}

/// Forgets the launch counts of apps no longer in the list.
pub fn forget_launches(launches: &mut BTreeMap<String, u32>, ids: &[&str]) -> bool {
    let before = launches.len();
    let listed: HashSet<&str> = ids.iter().copied().collect();
    launches.retain(|id, _| listed.contains(id.as_str()));
    launches.len() != before
}

/// Fills every smart category (at any depth) from its rule. Returns whether
/// any changed.
pub fn refill(cats: &mut [Category], apps: &[Facts], history: &History, now: u64) -> bool {
    let mut changed = false;
    for c in cats {
        if let Some(rule) = &c.smart {
            let apps = rule.fill(apps, history, now);
            if c.apps != apps {
                c.apps = apps;
                changed = true;
            }
        }
        changed |= refill(&mut c.children, apps, history, now);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    fn facts() -> Vec<Facts<'static>> {
        vec![
            Facts { id: "code", name: "Visual Studio Code", file: "Code.exe", kind: KindGroup::Desktop },
            Facts { id: "rg", name: "rg", file: r"C:\scoop\shims\rg.exe", kind: KindGroup::Package },
            Facts { id: "yt", name: "YouTube", file: "chrome-app", kind: KindGroup::Web },
            Facts { id: "np", name: "Notepad", file: "notepad.exe", kind: KindGroup::Desktop },
            Facts { id: "calc", name: "Calculator", file: "Microsoft.WindowsCalculator", kind: KindGroup::Store },
        ]
    }

    fn map<T: Copy>(v: &[(&str, T)]) -> BTreeMap<String, T> {
        v.iter().map(|(k, t)| (k.to_string(), *t)).collect()
    }

    #[test]
    fn recently_installed() {
        let now = 100 * DAY;
        let first_seen = map(&[("code", now - 2 * DAY), ("rg", now - DAY), ("yt", now - 30 * DAY), ("np", 0)]);
        let launches = BTreeMap::new();
        let h = History { first_seen: &first_seen, launches: &launches };
        // Newest first; too old, "always there" and never noted are left out.
        assert_eq!(Smart::RecentlyInstalled { days: 14 }.fill(&facts(), &h, now), ["rg", "code"]);
        assert_eq!(Smart::RecentlyInstalled { days: 60 }.fill(&facts(), &h, now), ["rg", "code", "yt"]);
    }

    #[test]
    fn most_used() {
        let first_seen = BTreeMap::new();
        let launches = map(&[("np", 9), ("code", 30), ("calc", 9), ("yt", 1), ("gone", 99)]);
        let h = History { first_seen: &first_seen, launches: &launches };
        // Most launches first, ties by name; never launched left out.
        assert_eq!(Smart::MostUsed { count: 3 }.fill(&facts(), &h, 0), ["code", "calc", "np"]);
        assert_eq!(Smart::MostUsed { count: 10 }.fill(&facts(), &h, 0), ["code", "calc", "np", "yt"]);
    }

    #[test]
    fn matching_rules() {
        let (a, b) = (BTreeMap::new(), BTreeMap::new());
        let h = History { first_seen: &a, launches: &b };
        let rule = |kinds: Vec<KindGroup>, contains: &str| Smart::Matching { kinds, contains: contains.into() };
        assert_eq!(rule(vec![KindGroup::Package], "").fill(&facts(), &h, 0), ["rg"]);
        assert_eq!(rule(vec![KindGroup::Desktop, KindGroup::Store], "").fill(&facts(), &h, 0), ["calc", "np", "code"]);
        // Words match the name or the file, ignoring case.
        assert_eq!(rule(vec![], " NOTE ").fill(&facts(), &h, 0), ["np"]);
        assert_eq!(rule(vec![], "scoop").fill(&facts(), &h, 0), ["rg"]);
        assert_eq!(rule(vec![KindGroup::Web], "note").fill(&facts(), &h, 0), Vec::<String>::new());
        // Nothing to go on: every app, capped.
        let many: Vec<String> = (0..60).map(|i| format!("app{i:02}")).collect();
        let many: Vec<Facts> =
            many.iter().map(|id| Facts { id, name: id, file: "", kind: KindGroup::Desktop }).collect();
        assert_eq!(rule(vec![], "").fill(&many, &h, 0).len(), MAX_APPS);
    }

    #[test]
    fn noting_apps() {
        let mut seen = BTreeMap::new();
        // The first list: always there.
        assert!(note_seen(&mut seen, &["a", "b"], 500));
        assert_eq!(seen, map(&[("a", 0), ("b", 0)]));
        // One more: new now. Nothing new: nothing changes.
        assert!(note_seen(&mut seen, &["a", "b", "c"], 600));
        assert_eq!(seen["c"], 600);
        assert!(!note_seen(&mut seen, &["a", "b", "c"], 700));
        // Gone: forgotten, so it is new again if it comes back.
        assert!(note_seen(&mut seen, &["a", "c"], 800));
        assert!(!seen.contains_key("b"));
        assert!(note_seen(&mut seen, &["a", "b", "c"], 900));
        assert_eq!(seen["b"], 900);
        // Many at once: the source changed, not installs.
        let ids: Vec<String> = (0..=BULK).map(|i| format!("pkg{i}")).collect();
        let mut all: Vec<&str> = vec!["a", "b", "c"];
        all.extend(ids.iter().map(String::as_str));
        assert!(note_seen(&mut seen, &all, 1000));
        assert_eq!(seen["pkg0"], 0);
        let mut launches = map(&[("a", 3), ("zz", 1)]);
        assert!(forget_launches(&mut launches, &all));
        assert!(!forget_launches(&mut launches, &all));
        assert_eq!(launches, map(&[("a", 3)]));
    }

    #[test]
    fn refilling_and_saving() {
        let (a, b) = (BTreeMap::new(), BTreeMap::new());
        let h = History { first_seen: &a, launches: &b };
        let web = Smart::Matching { kinds: vec![KindGroup::Web], contains: String::new() };
        let mut cats = vec![Category {
            id: 1,
            name: "Mine".into(),
            apps: vec!["np".into()],
            children: vec![Category { id: 2, smart: Some(web.clone()), ..Default::default() }],
            ..Default::default()
        }];
        assert!(refill(&mut cats, &facts(), &h, 0));
        // Hand-made ones are left alone; smart ones at any depth are filled.
        assert_eq!(cats[0].apps, ["np"]);
        assert_eq!(cats[0].children[0].apps, ["yt"]);
        assert!(!refill(&mut cats, &facts(), &h, 0));
        // The rule round-trips through the config file.
        let json = serde_json::to_string(&web).unwrap();
        assert_eq!(json, r#"{"fill":"matching","kinds":["web"],"contains":""}"#);
        assert_eq!(
            serde_json::from_str::<Smart>(r#"{"fill":"matching"}"#).unwrap(),
            Smart::Matching { kinds: vec![], contains: String::new() }
        );
        assert_eq!(Smart::RecentlyInstalled { days: 7 }.describe(), "Fills itself: apps installed in the last 7 days");
        assert_eq!(
            Smart::Matching { kinds: vec![KindGroup::Package], contains: "py".into() }.describe(),
            "Fills itself: package manager tools with “py” in the name"
        );
    }
}
