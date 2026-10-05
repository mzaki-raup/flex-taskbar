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
    pub max_recents: usize,
    pub show_recents_in_menu: bool,
    /// "All apps" in the menu is split into A–Z submenus above this many apps.
    pub group_all_apps_above: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            search_hotkey: Some(Hotkey { modifiers: MOD_CONTROL | MOD_ALT, key: 0x20 }),
            menu_hotkey: Some(Hotkey { modifiers: MOD_CONTROL | MOD_ALT, key: 0x4D }), // M
            max_recents: 10,
            show_recents_in_menu: true,
            group_all_apps_above: 40,
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

#[cfg(test)]
mod tests {
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
    fn hotkey_description() {
        let hk = Hotkey { modifiers: MOD_CONTROL | MOD_ALT, key: 0x20 };
        assert_eq!(hk.describe(), "Ctrl+Alt+Space");
        assert_eq!(Hotkey { modifiers: MOD_SHIFT, key: 0x71 }.describe(), "Shift+F2");
    }
}
