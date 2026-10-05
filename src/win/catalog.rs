//! The list of launchable apps: everything in the shell's Apps folder (the same
//! list as Start's "All apps" — desktop programs, Store apps, and browser web
//! apps installed from Chrome/Edge) plus the user's custom apps.
//!
//! If the Apps folder can't be enumerated, the Start Menu shortcut folders are
//! scanned instead, so the launcher still has something to show.

use crate::config::CustomApp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, KF_FLAG_DEFAULT, SHGetKnownFolderItem, SIGDN,
    SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::core::Result;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShellApp {
    /// Parsing name inside the Apps folder: an AppUserModelID for Store/web
    /// apps, or a known-folder-relative path for desktop programs.
    pub parsing_name: String,
    pub name: String,
    /// True for the Start Menu fallback: `parsing_name` is then a file path.
    #[serde(default)]
    pub file: bool,
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
                source: if s.file {
                    Source::File(s.parsing_name.clone())
                } else {
                    Source::Shell(s.parsing_name.clone())
                },
            });
        }
        for c in custom {
            apps.push(AppEntry {
                id: c.id.clone(),
                name: c.name.clone(),
                file_hint: file_hint(&c.target),
                source: Source::Custom,
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

/// Enumerates the Apps folder, falling back to the Start Menu folders. Needs COM
/// initialized on the calling thread.
pub fn scan() -> Result<Vec<ShellApp>> {
    match scan_apps_folder() {
        Ok(apps) if !apps.is_empty() => Ok(apps),
        Ok(_) => Ok(scan_start_menu()),
        Err(e) => {
            let apps = scan_start_menu();
            if apps.is_empty() { Err(e) } else { Ok(apps) }
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
                    out.push(ShellApp { parsing_name, name, file: false });
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
            out.push(ShellApp { parsing_name: path.display().to_string(), name, file: true });
        }
    }
    out
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
