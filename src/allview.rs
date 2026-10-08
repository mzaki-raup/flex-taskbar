#![cfg_attr(not(windows), allow(dead_code))]

//! How the All apps list is sorted and filtered: by name, type, category or
//! recent use, with app types and categories that can be hidden. Pure, so it
//! can be unit-tested off Windows; the flyout turns the lines into rows.

use crate::appkind::AppKind;
use serde::{Deserialize, Serialize};

/// The coarse kinds shown and hidden together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindGroup {
    Desktop,
    Store,
    Web,
    Package,
    Custom,
}

impl KindGroup {
    pub const ALL: [KindGroup; 5] =
        [KindGroup::Desktop, KindGroup::Store, KindGroup::Web, KindGroup::Package, KindGroup::Custom];

    pub fn of(kind: AppKind) -> KindGroup {
        match kind {
            AppKind::Desktop => KindGroup::Desktop,
            AppKind::Store => KindGroup::Store,
            AppKind::ChromeWebApp | AppKind::EdgeWebApp | AppKind::WebApp => KindGroup::Web,
            AppKind::Package(_) => KindGroup::Package,
            AppKind::Custom => KindGroup::Custom,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            KindGroup::Desktop => "Desktop programs",
            KindGroup::Store => "Store apps",
            KindGroup::Web => "Web apps (Chrome, Edge…)",
            KindGroup::Package => "Package managers (winget, Scoop, npm…)",
            KindGroup::Custom => "Custom apps",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AllSort {
    /// A to Z.
    Name,
    /// Z to A.
    NameDesc,
    /// Grouped by type.
    Type,
    /// Grouped by root category.
    Category,
    /// Recently used first.
    Recent,
}

impl AllSort {
    pub const ALL: [AllSort; 5] = [AllSort::Name, AllSort::NameDesc, AllSort::Type, AllSort::Category, AllSort::Recent];

    pub fn title(self) -> &'static str {
        match self {
            AllSort::Name => "Name (A–Z)",
            AllSort::NameDesc => "Name (Z–A)",
            AllSort::Type => "Type",
            AllSort::Category => "Category",
            AllSort::Recent => "Recently used",
        }
    }

    /// Short form for the sort button.
    pub fn short(self) -> &'static str {
        match self {
            AllSort::Name => "A–Z",
            AllSort::NameDesc => "Z–A",
            AllSort::Type => "Type",
            AllSort::Category => "Category",
            AllSort::Recent => "Recent",
        }
    }
}

/// The All list's settings (`settings.all_apps`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AllAppsView {
    pub sort: AllSort,
    pub hidden_kinds: Vec<KindGroup>,
    /// Root categories whose apps are hidden.
    pub hidden_categories: Vec<u64>,
    /// Hide apps that aren't in any category.
    pub hide_uncategorized: bool,
}

impl Default for AllAppsView {
    fn default() -> Self {
        AllAppsView {
            sort: AllSort::Name,
            hidden_kinds: Vec::new(),
            hidden_categories: Vec::new(),
            hide_uncategorized: false,
        }
    }
}

impl AllAppsView {
    /// Whether anything is hidden (shown on the button).
    pub fn filtering(&self) -> bool {
        !self.hidden_kinds.is_empty() || !self.hidden_categories.is_empty() || self.hide_uncategorized
    }

    pub fn toggle_kind(&mut self, k: KindGroup) {
        toggle(&mut self.hidden_kinds, k);
    }

    pub fn toggle_category(&mut self, id: u64) {
        toggle(&mut self.hidden_categories, id);
    }

    pub fn show_all(&mut self) {
        self.hidden_kinds.clear();
        self.hidden_categories.clear();
        self.hide_uncategorized = false;
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, v: T) {
    match list.iter().position(|x| *x == v) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(v),
    }
}

/// One app, as far as sorting and filtering go.
pub struct Entry<'a> {
    pub name: &'a str,
    pub group: KindGroup,
    /// Root categories it is filed under (anywhere in their subtree).
    pub roots: Vec<u64>,
    /// Position in the recents list, if used recently.
    pub recent: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Header(String),
    /// Index into the entries.
    App(usize),
}

fn shown(e: &Entry, v: &AllAppsView) -> bool {
    if v.hidden_kinds.contains(&e.group) {
        return false;
    }
    if e.roots.is_empty() { !v.hide_uncategorized } else { e.roots.iter().any(|r| !v.hidden_categories.contains(r)) }
}

/// The list's lines: apps that aren't hidden, in the chosen order, with
/// headings when grouped. `roots` are the root categories in tree order.
pub fn lines(entries: &[Entry], v: &AllAppsView, roots: &[(u64, String)]) -> Vec<Line> {
    let mut idx: Vec<usize> = (0..entries.len()).filter(|&i| shown(&entries[i], v)).collect();
    // Each name lowercased once, not on every comparison.
    idx.sort_by_cached_key(|&i| (entries[i].name.to_lowercase(), i));
    let apps = |list: &[usize]| list.iter().map(|&i| Line::App(i)).collect::<Vec<_>>();
    let mut out = Vec::new();
    let group = |title: &str, list: Vec<usize>, out: &mut Vec<Line>| {
        if !list.is_empty() {
            out.push(Line::Header(title.to_string()));
            out.extend(apps(&list));
        }
    };
    match v.sort {
        AllSort::Name => out = apps(&idx),
        AllSort::NameDesc => {
            idx.reverse();
            out = apps(&idx);
        }
        AllSort::Type => {
            for k in KindGroup::ALL {
                let list: Vec<usize> = idx.iter().copied().filter(|&i| entries[i].group == k).collect();
                group(k.title(), list, &mut out);
            }
        }
        AllSort::Category => {
            // An app filed under several root categories shows under each.
            for (id, name) in roots {
                if v.hidden_categories.contains(id) {
                    continue;
                }
                let list: Vec<usize> = idx.iter().copied().filter(|&i| entries[i].roots.contains(id)).collect();
                group(name, list, &mut out);
            }
            let rest: Vec<usize> = idx.iter().copied().filter(|&i| entries[i].roots.is_empty()).collect();
            group("Not in a category", rest, &mut out);
        }
        AllSort::Recent => {
            let mut recent: Vec<usize> = idx.iter().copied().filter(|&i| entries[i].recent.is_some()).collect();
            recent.sort_by_key(|&i| entries[i].recent);
            let rest: Vec<usize> = idx.iter().copied().filter(|&i| entries[i].recent.is_none()).collect();
            group("Recently used", recent, &mut out);
            group("Everything else", rest, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e<'a>(name: &'a str, group: KindGroup, roots: &[u64], recent: Option<usize>) -> Entry<'a> {
        Entry { name, group, roots: roots.to_vec(), recent }
    }

    fn sample() -> Vec<Entry<'static>> {
        vec![
            e("notepad", KindGroup::Desktop, &[1], None),
            e("Calculator", KindGroup::Store, &[], Some(1)),
            e("YouTube", KindGroup::Web, &[2], Some(0)),
            e("7z", KindGroup::Package, &[1, 2], None),
            e("Backup", KindGroup::Custom, &[], None),
        ]
    }

    fn roots() -> Vec<(u64, String)> {
        vec![(1, "Tools".into()), (2, "Media".into())]
    }

    fn names(entries: &[Entry], ls: &[Line]) -> Vec<String> {
        ls.iter()
            .map(|l| match l {
                Line::Header(h) => format!("# {h}"),
                Line::App(i) => entries[*i].name.to_string(),
            })
            .collect()
    }

    #[test]
    fn by_name() {
        let es = sample();
        let v = AllAppsView::default();
        assert_eq!(names(&es, &lines(&es, &v, &roots())), ["7z", "Backup", "Calculator", "notepad", "YouTube"]);
        let v = AllAppsView { sort: AllSort::NameDesc, ..Default::default() };
        assert_eq!(names(&es, &lines(&es, &v, &roots())), ["YouTube", "notepad", "Calculator", "Backup", "7z"]);
    }

    #[test]
    fn by_type_and_recent() {
        let es = sample();
        let v = AllAppsView { sort: AllSort::Type, ..Default::default() };
        let got = names(&es, &lines(&es, &v, &roots()));
        assert_eq!(got[0], "# Desktop programs");
        assert_eq!(got[1], "notepad");
        assert_eq!(got.iter().filter(|s| s.starts_with('#')).count(), 5);
        let v = AllAppsView { sort: AllSort::Recent, ..Default::default() };
        assert_eq!(
            names(&es, &lines(&es, &v, &roots())),
            ["# Recently used", "YouTube", "Calculator", "# Everything else", "7z", "Backup", "notepad"]
        );
    }

    #[test]
    fn by_category() {
        let es = sample();
        let v = AllAppsView { sort: AllSort::Category, ..Default::default() };
        assert_eq!(
            names(&es, &lines(&es, &v, &roots())),
            ["# Tools", "7z", "notepad", "# Media", "7z", "YouTube", "# Not in a category", "Backup", "Calculator"]
        );
    }

    #[test]
    fn hiding() {
        let es = sample();
        let mut v = AllAppsView::default();
        v.toggle_kind(KindGroup::Web);
        v.toggle_kind(KindGroup::Custom);
        assert!(v.filtering());
        assert_eq!(names(&es, &lines(&es, &v, &roots())), ["7z", "Calculator", "notepad"]);
        // Hiding a category hides apps only filed there; 7z is also in Media.
        v.show_all();
        v.toggle_category(1);
        assert_eq!(names(&es, &lines(&es, &v, &roots())), ["7z", "Backup", "Calculator", "YouTube"]);
        v.hide_uncategorized = true;
        assert_eq!(names(&es, &lines(&es, &v, &roots())), ["7z", "YouTube"]);
        // Grouped by category, a hidden category has no section.
        v.sort = AllSort::Category;
        assert_eq!(names(&es, &lines(&es, &v, &roots())), ["# Media", "7z", "YouTube"]);
        v.toggle_category(1);
        assert!(!v.hidden_categories.contains(&1));
        v.show_all();
        assert!(!v.filtering());
    }

    #[test]
    fn groups_of_kinds() {
        assert_eq!(KindGroup::of(AppKind::EdgeWebApp), KindGroup::Web);
        assert_eq!(KindGroup::of(AppKind::Package(crate::pkgsources::Manager::Scoop)), KindGroup::Package);
        let json = serde_json::to_string(&AllAppsView { sort: AllSort::Type, ..Default::default() }).unwrap();
        assert!(json.contains("\"sort\":\"type\""));
    }
}
