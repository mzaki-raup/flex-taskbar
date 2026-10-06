//! The list of launchable apps: everything in the shell's Apps folder (the same
//! list as Start's "All apps" — desktop programs, Store apps, and browser web
//! apps installed from Chrome/Edge) plus the user's custom apps.
//!
//! Each entry's kind (Store app, Chrome or Edge web app…) comes from
//! `appkind`, from how the shell names it.
//!
//! If the Apps folder can't be enumerated, the Start Menu shortcut folders are
//! scanned instead, so the launcher still has something to show.

use crate::appkind::{self, AppKind};
use crate::config::CustomApp;
use crate::{pkgsources, running};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use windows::Win32::Storage::EnhancedStorage::PKEY_Link_TargetParsingPath;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2, KF_FLAG_DEFAULT,
    SHGetKnownFolderItem, SHGetKnownFolderPath, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::core::{Interface, Result};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShellApp {
    /// Parsing name inside the Apps folder: an AppUserModelID for Store/web
    /// apps, or a known-folder-relative path for desktop programs.
    pub parsing_name: String,
    pub name: String,
    /// True for the Start Menu fallback: `parsing_name` is then a file path.
    #[serde(default)]
    pub file: bool,
    /// The program it starts, when the shell knows (a shortcut's target, or
    /// a desktop program's path): recognises its windows as running.
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Source {
    /// Parsing name inside shell:AppsFolder.
    Shell(String),
    /// A shortcut or program file (Start Menu fallback).
    File(String),
    Custom,
}

#[derive(Clone, Debug)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    /// File name or AUMID, searched as a secondary field.
    pub file_hint: String,
    pub source: Source,
    /// Desktop program, Store app, Chrome/Edge web app or custom.
    pub kind: AppKind,
    /// What its windows are recognised by (see `running::app_keys`).
    pub keys: Vec<String>,
}

#[derive(Default)]
pub struct Catalog {
    pub apps: Vec<AppEntry>,
    index: HashMap<String, usize>,
}

impl Catalog {
    pub fn build(shell: &[ShellApp], custom: &[CustomApp]) -> Catalog {
        let mut apps: Vec<AppEntry> = Vec::with_capacity(shell.len() + custom.len());
        for s in shell {
            apps.push(AppEntry {
                id: s.parsing_name.clone(),
                name: s.name.clone(),
                file_hint: file_hint(&s.parsing_name),
                kind: if s.file {
                    appkind::classify_file(&s.parsing_name)
                } else {
                    appkind::classify_shell(&s.parsing_name)
                },
                source: if s.file {
                    Source::File(s.parsing_name.clone())
                } else {
                    Source::Shell(s.parsing_name.clone())
                },
                keys: if s.file {
                    running::app_keys(None, Some(&s.parsing_name))
                } else {
                    running::app_keys(Some(&s.parsing_name), s.target.as_deref())
                },
            });
        }
        for c in custom {
            apps.push(AppEntry {
                id: c.id.clone(),
                name: c.name.clone(),
                file_hint: file_hint(&c.target),
                kind: appkind::classify_custom(&c.target, &c.args),
                source: Source::Custom,
                keys: running::app_keys(None, Some(&super::launch::expand(c.target.trim()))),
            });
        }
        apps.sort_by_cached_key(|a| a.name.to_lowercase());
        let mut index = HashMap::with_capacity(apps.len());
        for (i, a) in apps.iter().enumerate() {
            index.entry(a.id.clone()).or_insert(i);
        }
        Catalog { apps, index }
    }

    pub fn get(&self, id: &str) -> Option<&AppEntry> {
        self.index.get(id).map(|&i| &self.apps[i])
    }
}

fn file_hint(target: &str) -> String {
    target.rsplit(['\\', '/']).next().unwrap_or(target).to_string()
}

/// Enumerates the Apps folder, falling back to the Start Menu folders, and
/// (with `packages`) adds programs from package managers' folders. Needs COM
/// initialized on the calling thread.
pub fn scan(packages: bool) -> Result<Vec<ShellApp>> {
    let (mut apps, err) = match scan_apps_folder() {
        Ok(apps) if !apps.is_empty() => (apps, None),
        Ok(_) => (scan_start_menu(), None),
        Err(e) => (scan_start_menu(), Some(e)),
    };
    if packages {
        add_package_apps(&mut apps);
    }
    // Nothing found and the Apps folder failed: keep the last list instead.
    match err {
        Some(e) if apps.is_empty() => Err(e),
        _ => Ok(apps),
    }
}

/// Programs in winget's, Scoop's, Chocolatey's, npm's, pip's… folders that
/// aren't already in the list under the same name.
fn add_package_apps(apps: &mut Vec<ShellApp>) {
    let stem = |s: &str| {
        let f = file_hint(s).to_lowercase();
        f.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(f)
    };
    let mut known: HashSet<String> = apps.iter().flat_map(|a| [a.name.to_lowercase(), stem(&a.parsing_name)]).collect();
    let subdirs = |root: &Path| -> Vec<std::path::PathBuf> {
        std::fs::read_dir(root)
            .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default()
    };
    for (manager, dir) in pkgsources::tool_dirs(|v| std::env::var(v).ok(), subdirs) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut found: Vec<ShellApp> = entries
            .flatten()
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                if !pkgsources::wanted(manager, &file) {
                    return None;
                }
                let name = pkgsources::display_name(&file);
                Some(ShellApp { parsing_name: e.path().display().to_string(), name, file: true, target: None })
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        for app in found {
            if known.insert(app.name.to_lowercase()) {
                apps.push(app);
            }
        }
    }
}

fn scan_apps_folder() -> Result<Vec<ShellApp>> {
    let mut out = Vec::new();
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
        loop {
            let mut batch: [Option<IShellItem>; 32] = Default::default();
            let mut fetched = 0u32;
            let hr = items.Next(&mut batch, Some(&mut fetched));
            for item in batch.iter().take(fetched as usize).flatten() {
                let (Some(name), Some(parsing_name)) =
                    (display_name(item, SIGDN_NORMALDISPLAY), display_name(item, SIGDN_PARENTRELATIVEPARSING))
                else {
                    continue;
                };
                if !name.is_empty() && !parsing_name.is_empty() {
                    let target = link_target(item).or_else(|| known_folder_path(&parsing_name));
                    out.push(ShellApp { parsing_name, name, file: false, target });
                }
            }
            if hr.is_err() || (fetched as usize) < batch.len() {
                break; // a short batch (S_FALSE) means the end of the enumeration
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|a| seen.insert(a.parsing_name.clone()));
    Ok(out)
}

fn scan_start_menu() -> Vec<ShellApp> {
    let mut roots = Vec::new();
    for (var, sub) in [
        ("ProgramData", "Microsoft\\Windows\\Start Menu\\Programs"),
        ("APPDATA", "Microsoft\\Windows\\Start Menu\\Programs"),
    ] {
        if let Some(base) = std::env::var_os(var) {
            roots.push(std::path::PathBuf::from(base).join(sub));
        }
    }
    let mut out = Vec::new();
    let mut stack = roots;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !matches!(ext.as_str(), "lnk" | "url" | "exe") {
                continue;
            }
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if name.is_empty() || name.to_lowercase().starts_with("uninstall") {
                continue;
            }
            out.push(ShellApp { parsing_name: path.display().to_string(), name, file: true, target: None });
        }
    }
    out
}

/// A shortcut's target program, as the Apps folder reports it.
fn link_target(item: &IShellItem) -> Option<String> {
    unsafe {
        let item2: IShellItem2 = item.cast().ok()?;
        let p = item2.GetString(&PKEY_Link_TargetParsingPath).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.filter(|s| !s.is_empty())
    }
}

/// A desktop program's path from a parsing name like
/// `{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\notepad.exe` (a known folder,
/// then the rest of the path).
fn known_folder_path(parsing_name: &str) -> Option<String> {
    let rest = parsing_name.strip_prefix('{')?;
    let (guid, tail) = rest.split_once("}\\")?;
    let guid = windows::core::GUID::try_from(guid).ok()?;
    unsafe {
        let p = SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None).ok()?;
        let base = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        Some(format!("{}\\{tail}", base?))
    }
}

fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    unsafe {
        let p = item.GetDisplayName(kind).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

pub fn load_cache(path: &Path) -> Vec<ShellApp> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_cache(path: &Path, apps: &[ShellApp]) {
    if let Ok(json) = serde_json::to_vec(apps) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}
