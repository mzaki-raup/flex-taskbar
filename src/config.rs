//! Persisted configuration: one JSON file (`config.json`) in the portable data
//! folder, written atomically with a last-known-good `.bak` beside it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 1;

// RegisterHotKey modifier bits (MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN).
pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    /// `MOD_*` bits.
    pub modifiers: u32,
    /// Win32 virtual-key code.
    pub key: u32,
}

impl Hotkey {
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers & MOD_CONTROL != 0 {
            parts.push("Ctrl".to_string());
        }
        if self.modifiers & MOD_ALT != 0 {
            parts.push("Alt".to_string());
        }
        if self.modifiers & MOD_SHIFT != 0 {
            parts.push("Shift".to_string());
        }
        if self.modifiers & MOD_WIN != 0 {
            parts.push("Win".to_string());
        }
        parts.push(key_name(self.key));
        parts.join("+")
    }
}

fn key_name(vk: u32) -> String {
    match vk {
        0x20 => "Space".into(),
        0x0D => "Enter".into(),
        0x09 => "Tab".into(),
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(String::from).unwrap_or_default(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0xC0 => "`".into(),
        _ => format!("Key{vk:#04X}"),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Opens the search window. Ctrl+Alt+Space rather than Win+Space: Windows'
    /// input-language switcher usually owns Win+Space.
    pub search_hotkey: Option<Hotkey>,
    /// Pops the category menu at the mouse cursor.
    pub menu_hotkey: Option<Hotkey>,
    /// Opens the bar's *All* list with the keyboard in it.
    pub bar_hotkey: Option<Hotkey>,
    /// Switches to the next bar (see `bars`).
    pub next_bar_hotkey: Option<Hotkey>,
    pub max_recents: usize,
    pub show_recents_in_menu: bool,
    /// "All apps" in the menu is split into A–Z submenus above this many apps.
    pub group_all_apps_above: usize,
    /// The icon strip docked against the Windows taskbar.
    pub show_strip: bool,
    /// Reserve the strip's screen space (an AppBar), so maximized windows end
    /// above it instead of underneath.
    pub reserve_space: bool,
    /// Strip height in DIPs (48 matches the Windows 11 taskbar); its width
    /// when it stands on the left or right edge.
    pub strip_height: u32,
    /// Which screen edge the strip docks against.
    pub strip_edge: StripEdge,
    /// Milliseconds the pointer rests on a category icon before it opens.
    pub hover_delay_ms: u32,
    /// Rescan the app list by itself when apps are installed or removed.
    pub auto_rescan: bool,
    /// Clicking an app that has a window open switches to it (Shift+click
    /// starts another copy).
    pub switch_to_running: bool,
    /// Saved looks to switch between (Appearance window, *Saved looks…*).
    pub looks: Vec<crate::looks::Look>,
    /// Also list programs from package managers' folders (winget portable,
    /// Scoop, Chocolatey, npm, pip, pipx, Cargo, .NET tools, Go).
    pub package_apps: bool,
    /// How the All apps list is sorted, and what it hides.
    pub all_apps: crate::allview::AllAppsView,
    /// Theme, colours, transparency, border, corners and size of the strip.
    pub appearance: crate::appearance::Appearance,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            search_hotkey: Some(Hotkey { modifiers: MOD_CONTROL | MOD_ALT, key: 0x20 }),
            menu_hotkey: Some(Hotkey { modifiers: MOD_CONTROL | MOD_ALT, key: 0x4D }), // M
            bar_hotkey: Some(Hotkey { modifiers: MOD_CONTROL | MOD_ALT, key: 0x42 }),  // B
            next_bar_hotkey: Some(Hotkey { modifiers: MOD_CONTROL | MOD_ALT, key: 0x4E }), // N
            max_recents: 10,
            show_recents_in_menu: true,
            group_all_apps_above: 40,
            show_strip: true,
            reserve_space: true,
            strip_height: 48,
            strip_edge: StripEdge::Taskbar,
            hover_delay_ms: 100,
            auto_rescan: true,
            switch_to_running: true,
            looks: Vec::new(),
            package_apps: true,
            all_apps: crate::allview::AllAppsView::default(),
            appearance: crate::appearance::Appearance::default(),
        }
    }
}

/// Where the strip docks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StripEdge {
    /// The same edge as the Windows taskbar, next to it.
    Taskbar,
    Bottom,
    Top,
    Left,
    Right,
}

impl StripEdge {
    pub fn fixed(edge: crate::striplayout::Edge) -> StripEdge {
        use crate::striplayout::Edge;
        match edge {
            Edge::Bottom => StripEdge::Bottom,
            Edge::Top => StripEdge::Top,
            Edge::Left => StripEdge::Left,
            Edge::Right => StripEdge::Right,
        }
    }

    /// The edge to use, given the Windows taskbar's.
    pub fn resolve(self, taskbar: crate::striplayout::Edge) -> crate::striplayout::Edge {
        use crate::striplayout::Edge;
        match self {
            StripEdge::Taskbar => taskbar,
            StripEdge::Bottom => Edge::Bottom,
            StripEdge::Top => Edge::Top,
            StripEdge::Left => Edge::Left,
            StripEdge::Right => Edge::Right,
        }
    }
}

/// A category. Nesting depth is unbounded: subcategories live in `children`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Category {
    pub id: u64,
    pub name: String,
    /// File name inside the data folder's `icons\` directory.
    pub icon: Option<String>,
    /// App ids, in display order.
    pub apps: Vec<String>,
    pub children: Vec<Category>,
    /// Opens this category from anywhere.
    pub hotkey: Option<Hotkey>,
    /// A smart category: `apps` is filled from this rule, not by hand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smart: Option<crate::smart::Smart>,
}

/// An app the user added by hand (an exe, a shortcut, a URL, a `shell:` target,
/// or a browser web-app command line). Ids are `custom:<n>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct CustomApp {
    pub id: String,
    pub name: String,
    pub target: String,
    pub args: String,
    pub working_dir: String,
    pub run_as_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub settings: Settings,
    pub categories: Vec<Category>,
    pub custom_apps: Vec<CustomApp>,
    /// App id -> icon file name inside `icons\`.
    pub app_icons: BTreeMap<String, String>,
    /// Most recent first.
    pub recents: Vec<String>,
    /// Apps pinned to the icon strip.
    pub pinned: Vec<String>,
    /// Order of the strip's buttons, as keys: `cat:<id>` for a root category,
    /// the app id for a pinned app. Buttons missing from it follow, root
    /// categories first; keys that no longer exist are ignored.
    pub bar_order: Vec<String>,
    /// Root categories left off the bar in use.
    pub hidden_categories: Vec<u64>,
    /// Every bar, by name (see `bars`); the one in use is `bar`, and its
    /// buttons are the fields above.
    pub bars: Vec<crate::bars::Bar>,
    pub bar: String,
    /// When each app was first seen, for *Recently installed* (Unix
    /// seconds; 0 for apps that were there before anything was noted).
    pub first_seen: BTreeMap<String, u64>,
    /// How often each app was launched (or switched to), for *Most used*.
    pub launches: BTreeMap<String, u32>,
    pub next_id: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            categories: Vec::new(),
            custom_apps: Vec::new(),
            app_icons: BTreeMap::new(),
            recents: Vec::new(),
            pinned: Vec::new(),
            bar_order: Vec::new(),
            hidden_categories: Vec::new(),
            bars: Vec::new(),
            bar: String::new(),
            first_seen: BTreeMap::new(),
            launches: BTreeMap::new(),
            next_id: 1,
        }
    }
}

impl Config {
    pub fn alloc_id(&mut self) -> u64 {
        // Never hand out an id that a hand-edited or imported file already uses.
        let max_cat = crate::tree::max_id(&self.categories);
        let max_custom = self
            .custom_apps
            .iter()
            .filter_map(|a| a.id.strip_prefix("custom:")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max_cat + 1).max(max_custom + 1);
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn record_recent(&mut self, app_id: &str) {
        self.recents.retain(|r| r != app_id);
        self.recents.insert(0, app_id.to_string());
        self.recents.truncate(self.settings.max_recents.max(1));
        let n = self.launches.entry(app_id.to_string()).or_insert(0);
        *n = n.saturating_add(1);
    }

    /// Pins an app to the strip (at the end). Returns false if already pinned.
    pub fn pin(&mut self, app_id: &str) -> bool {
        if self.pinned.iter().any(|p| p == app_id) {
            return false;
        }
        self.pinned.push(app_id.to_string());
        true
    }

    /// Pins an app (or moves it, if already pinned) so it lands in front of
    /// the button now at `index` of the bar (`bar_keys().len()`: at the end).
    /// Returns false if nothing changed.
    pub fn pin_at(&mut self, app_id: &str, index: usize) -> bool {
        let keys = self.bar_keys();
        let index = index.min(keys.len());
        let newly = self.pin(app_id);
        match keys.iter().position(|k| k == app_id) {
            // Taking it out first shifts what follows it one place back.
            Some(pos) => self.move_bar_item(app_id, if pos < index { index - 1 } else { index }),
            None => {
                self.move_bar_item(app_id, index);
                newly
            }
        }
    }

    pub fn unpin(&mut self, app_id: &str) -> bool {
        let before = self.pinned.len();
        self.pinned.retain(|p| p != app_id);
        self.pinned.len() != before
    }

    /// The strip's buttons, left to right, as keys (see [`Config::bar_order`]).
    pub fn bar_keys(&self) -> Vec<String> {
        let roots: Vec<String> = self
            .categories
            .iter()
            .filter(|c| !self.hidden_categories.contains(&c.id))
            .map(|c| format!("cat:{}", c.id))
            .collect();
        let exists = |k: &String| roots.contains(k) || self.pinned.contains(k);
        let mut out: Vec<String> = Vec::with_capacity(roots.len() + self.pinned.len());
        for k in self.bar_order.iter().chain(&roots).chain(&self.pinned) {
            if exists(k) && !out.contains(k) {
                out.push(k.clone());
            }
        }
        out
    }

    /// Moves a strip button to position `to` (counted after it is taken
    /// out). Returns false if nothing changed.
    pub fn move_bar_item(&mut self, key: &str, to: usize) -> bool {
        let mut keys = self.bar_keys();
        let Some(pos) = keys.iter().position(|k| k == key) else { return false };
        let k = keys.remove(pos);
        let to = to.min(keys.len());
        keys.insert(to, k);
        let changed = pos != to;
        self.bar_order = keys;
        changed
    }

    /// Moves a strip button `delta` places left (negative) or right.
    pub fn move_bar_by(&mut self, key: &str, delta: isize) -> bool {
        let keys = self.bar_keys();
        let Some(pos) = keys.iter().position(|k| k == key) else { return false };
        let to = pos as isize + delta;
        if to < 0 || to as usize >= keys.len() {
            return false;
        }
        self.move_bar_item(key, to as usize)
    }

    pub fn custom_app(&self, id: &str) -> Option<&CustomApp> {
        self.custom_apps.iter().find(|a| a.id == id)
    }
}

/// How `load` obtained the configuration, so the UI can tell the user when a
/// file had to be recovered.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadOutcome {
    Loaded,
    FirstRun,
    /// `config.json` was unreadable; the backup was used. The broken file was kept
    /// under the given name.
    RestoredFromBackup(PathBuf),
    /// Both files were unreadable; defaults were used.
    ResetToDefaults(PathBuf),
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(data_dir: &Path) -> Store {
        Store { path: data_dir.join("config.json") }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn backup_path(&self) -> PathBuf {
        with_suffix(&self.path, ".bak")
    }

    pub fn load(&self) -> (Config, LoadOutcome) {
        let primary_exists = self.path.exists();
        if primary_exists && let Some(cfg) = read_config(&self.path) {
            return (cfg, LoadOutcome::Loaded);
        }

        let backup = read_config(&self.backup_path());
        if !primary_exists {
            return match backup {
                Some(cfg) => (cfg, LoadOutcome::RestoredFromBackup(self.backup_path())),
                None => (Config::default(), LoadOutcome::FirstRun),
            };
        }

        // Primary exists but is corrupt: keep it for inspection instead of
        // overwriting it on the next save.
        let kept = with_suffix(&self.path, &format!(".corrupt-{}", unix_time()));
        let _ = fs::rename(&self.path, &kept);
        match backup {
            Some(cfg) => (cfg, LoadOutcome::RestoredFromBackup(kept)),
            None => (Config::default(), LoadOutcome::ResetToDefaults(kept)),
        }
    }

    /// Atomic save: write `config.json.tmp`, flush it to disk, copy the current
    /// (known-good) file to `.bak`, then rename the temp file over the original.
    pub fn save(&self, cfg: &Config) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(cfg).map_err(std::io::Error::other)?;
        let tmp = with_suffix(&self.path, ".tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()?;
        }
        if read_config(&self.path).is_some() {
            fs::copy(&self.path, self.backup_path())?;
        }
        fs::rename(&tmp, &self.path)
    }

    /// `--reset`: move the current config aside (never delete it) so the next
    /// load starts from defaults.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn reset(&self) -> Option<PathBuf> {
        if !self.path.exists() {
            return None;
        }
        let kept = with_suffix(&self.path, &format!(".reset-{}", unix_time()));
        fs::rename(&self.path, &kept).ok()?;
        Some(kept)
    }
}

fn read_config(path: &Path) -> Option<Config> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice::<Config>(&bytes).ok()
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

pub fn unix_time() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Whether `name` is a bare file name, as the app stores for the pictures
/// it copies into its `icons` folder: no folders, drive or parent references,
/// so joining it to that folder can't reach outside it.
pub fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', '/', ':', '\0'])
        // Windows drops trailing dots and spaces: "..." would mean the folder.
        && !name.chars().all(|c| c == '.' || c == ' ')
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_file_names_only() {
        for ok in ["18dbae98d1de954c.png", "my icon.ico", "a.b.svg"] {
            assert!(is_plain_file_name(ok), "{ok}");
        }
        for bad in [
            "",
            ".",
            "..",
            "...",
            " ",
            "..\\..\\x.png",
            "../x.png",
            "sub\\x.png",
            "C:x.png",
            "C:\\x.png",
            "\\\\srv\\x",
            "a\0b",
        ] {
            assert!(!is_plain_file_name(bad), "{bad:?}");
        }
    }

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flex-cfg-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn first_run_gives_defaults() {
        let dir = temp_dir("first");
        let (cfg, outcome) = Store::new(&dir).load();
        assert_eq!(outcome, LoadOutcome::FirstRun);
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let store = Store::new(&dir);
        let mut cfg = Config::default();
        cfg.categories.push(Category { id: 5, name: "Dev".into(), ..Default::default() });
        store.save(&cfg).unwrap();
        let (loaded, outcome) = store.load();
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(loaded, cfg);
        assert!(!dir.join("config.json.tmp").exists());
    }

    #[test]
    fn corrupt_primary_restores_backup_and_keeps_broken_file() {
        let dir = temp_dir("corrupt");
        let store = Store::new(&dir);
        let mut cfg = Config::default();
        cfg.recents.push("a".into());
        store.save(&cfg).unwrap();
        cfg.recents.push("b".into());
        store.save(&cfg).unwrap(); // .bak now holds the first version
        fs::write(dir.join("config.json"), b"{ not json").unwrap();

        let (loaded, outcome) = store.load();
        assert_eq!(loaded.recents, vec!["a".to_string()]);
        match outcome {
            LoadOutcome::RestoredFromBackup(kept) => assert!(kept.exists()),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!dir.join("config.json").exists());
    }

    #[test]
    fn corrupt_file_is_never_copied_over_backup() {
        let dir = temp_dir("nobadbak");
        let store = Store::new(&dir);
        let cfg = Config::default();
        store.save(&cfg).unwrap();
        store.save(&cfg).unwrap();
        fs::write(dir.join("config.json"), b"garbage").unwrap();
        store.save(&cfg).unwrap();
        assert!(read_config(&dir.join("config.json.bak")).is_some());
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let cfg: Config = serde_json::from_str(r#"{"categories":[{"name":"X"}],"future":1}"#).unwrap();
        assert_eq!(cfg.categories[0].name, "X");
        assert_eq!(cfg.settings, Settings::default());
    }

    #[test]
    fn alloc_id_skips_ids_already_in_use() {
        let mut cfg = Config::default();
        cfg.categories.push(Category {
            id: 7,
            children: vec![Category { id: 12, ..Default::default() }],
            ..Default::default()
        });
        cfg.custom_apps.push(CustomApp { id: "custom:20".into(), ..Default::default() });
        assert_eq!(cfg.alloc_id(), 21);
        assert_eq!(cfg.alloc_id(), 22);
    }

    #[test]
    fn recents_are_deduplicated_and_capped() {
        let mut cfg = Config::default();
        cfg.settings.max_recents = 2;
        cfg.record_recent("a");
        cfg.record_recent("b");
        cfg.record_recent("a");
        cfg.record_recent("c");
        assert_eq!(cfg.recents, vec!["c".to_string(), "a".to_string()]);
    }

    #[test]
    fn pinning() {
        let mut cfg = Config::default();
        assert!(cfg.pin("a"));
        assert!(cfg.pin("b"));
        assert!(!cfg.pin("a"));
        assert!(cfg.unpin("b"));
        assert!(!cfg.unpin("b"));
        assert_eq!(cfg.pinned, vec!["a".to_string()]);
    }

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn bar_order() {
        let mut cfg = Config::default();
        for (id, name) in [(1, "A"), (2, "B")] {
            cfg.categories.push(Category { id, name: name.into(), ..Default::default() });
        }
        cfg.pin("x");
        cfg.pin("y");
        // Default: root categories, then pinned apps.
        assert_eq!(cfg.bar_keys(), keys(&["cat:1", "cat:2", "x", "y"]));
        // Apps and categories can be mixed freely.
        assert!(cfg.move_bar_item("y", 0));
        assert_eq!(cfg.bar_keys(), keys(&["y", "cat:1", "cat:2", "x"]));
        assert!(cfg.move_bar_item("cat:1", 3));
        assert_eq!(cfg.bar_keys(), keys(&["y", "cat:2", "x", "cat:1"]));
        assert!(!cfg.move_bar_item("cat:1", 9)); // already last
        assert!(cfg.move_bar_by("x", -1));
        assert!(!cfg.move_bar_by("y", -1));
        assert!(!cfg.move_bar_item("nope", 0));
        assert_eq!(cfg.bar_keys(), keys(&["y", "x", "cat:2", "cat:1"]));
        // New buttons join at the end; removed ones drop out.
        cfg.pin("z");
        cfg.unpin("x");
        cfg.categories.retain(|c| c.id != 2);
        assert_eq!(cfg.bar_keys(), keys(&["y", "cat:1", "z"]));
    }

    #[test]
    fn pinning_at_a_place() {
        let mut cfg = Config::default();
        cfg.categories.push(Category { id: 1, name: "A".into(), ..Default::default() });
        cfg.pin("x");
        // A new app lands in front of the button at the index.
        assert!(cfg.pin_at("n", 1));
        assert_eq!(cfg.bar_keys(), keys(&["cat:1", "n", "x"]));
        assert!(cfg.pin_at("m", 0));
        assert!(cfg.pin_at("e", 99));
        assert_eq!(cfg.bar_keys(), keys(&["m", "cat:1", "n", "x", "e"]));
        // A pinned one moves: forwards and backwards, counted before it moves.
        assert!(cfg.pin_at("m", 4));
        assert_eq!(cfg.bar_keys(), keys(&["cat:1", "n", "x", "m", "e"]));
        assert!(cfg.pin_at("e", 0));
        assert_eq!(cfg.bar_keys(), keys(&["e", "cat:1", "n", "x", "m"]));
        // Dropped either side of itself: nothing changes.
        assert!(!cfg.pin_at("n", 2));
        assert!(!cfg.pin_at("n", 3));
        assert_eq!(cfg.bar_keys(), keys(&["e", "cat:1", "n", "x", "m"]));
    }

    #[test]
    fn hotkey_description() {
        let hk = Hotkey { modifiers: MOD_CONTROL | MOD_ALT, key: 0x20 };
        assert_eq!(hk.describe(), "Ctrl+Alt+Space");
        assert_eq!(Hotkey { modifiers: MOD_SHIFT, key: 0x71 }.describe(), "Shift+F2");
    }
}
