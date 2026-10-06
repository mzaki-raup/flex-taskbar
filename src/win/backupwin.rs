//! *Back up settings…* and *Restore…* in the Manage window: one `.zip` with
//! `config.json` and the pictures in `icons\` (see `crate::backup`).
//!
//! Restoring checks the whole file first (names, sizes, checksums, and that
//! the settings parse), keeps the current settings as
//! `config.json.before-restore-<time>`, writes the backup's files into the
//! data folder and starts FlexTaskbar again so everything uses them.

use super::{app, paths, ui};
use crate::backup::{self, Entry, Kind};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::core::{PCWSTR, PWSTR};

const FILTER: &str = "FlexTaskbar backups (*.zip)\0*.zip\0All files\0*.*\0\0";

fn dialog(owner: HWND, save: bool, initial: &str) -> Option<PathBuf> {
    let filter: Vec<u16> = FILTER.encode_utf16().collect();
    let ext: Vec<u16> = "zip\0".encode_utf16().collect();
    let mut file = vec![0u16; 1024];
    for (i, c) in initial.encode_utf16().take(1000).enumerate() {
        file[i] = c;
    }
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrDefExt: PCWSTR(ext.as_ptr()),
        Flags: if save { OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST } else { OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST },
        ..Default::default()
    };
    let ok = unsafe { if save { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) } }.as_bool();
    ok.then(|| PathBuf::from(ui::from_wide(&file)))
}

/// Today as `YYYY-MM-DD`, for the suggested file name.
fn today() -> String {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
}

/// Writes `data` to `path` through a temporary file, so a failure never
/// leaves half a file behind.
fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp-flex");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// *Back up settings…*: saves the settings and pictures to a zip.
pub fn back_up(owner: HWND) {
    // Whatever is on screen is what gets backed up.
    app::save();
    let p = paths::get();
    let config = match std::fs::read(p.data.join("config.json")) {
        Ok(c) => c,
        Err(e) => {
            ui::error(Some(owner), &format!("The settings couldn't be read: {e}"));
            return;
        }
    };
    let mut entries = vec![Entry { name: "config.json".into(), data: config }];
    let mut pictures: Vec<(String, PathBuf)> = std::fs::read_dir(&p.icons)
        .map(|r| {
            r.flatten()
                .filter(|e| e.path().is_file())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    crate::config::is_plain_file_name(&name).then(|| (name, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    pictures.sort();
    for (name, path) in pictures {
        // Pictures are capped at 8 MB when chosen; skip anything odd.
        if std::fs::metadata(&path).is_ok_and(|m| m.len() as usize <= backup::MAX_PICTURE)
            && let Ok(data) = std::fs::read(&path)
        {
            entries.push(Entry { name: format!("icons/{name}"), data });
        }
    }
    let Some(target) = dialog(owner, true, &format!("FlexTaskbar backup {}.zip", today())) else { return };
    match write_atomic(&target, &backup::write_zip(&entries)) {
        Ok(()) => ui::info(
            Some(owner),
            &format!(
                "Backed up your settings and {} picture(s) to:\n{}\n\nUse Restore… on any PC to bring them back.",
                entries.len() - 1,
                target.display()
            ),
        ),
        Err(e) => ui::error(Some(owner), &format!("The backup couldn't be written: {e}")),
    }
}

/// *Restore…*: replaces the settings and pictures with a backup's, then
/// starts FlexTaskbar again.
pub fn restore(owner: HWND) {
    let Some(source) = dialog(owner, false, "") else { return };
    // Never read more than a backup can hold.
    let too_big = std::fs::metadata(&source).map(|m| m.len() as usize > backup::MAX_TOTAL + (1 << 20)).unwrap_or(true);
    if too_big {
        ui::error(Some(owner), "That file is too large to be a FlexTaskbar backup.");
        return;
    }
    let bytes = match std::fs::read(&source) {
        Ok(b) => b,
        Err(e) => {
            ui::error(Some(owner), &format!("The backup couldn't be read: {e}"));
            return;
        }
    };
    let files = match backup::read_zip(&bytes) {
        Ok(f) => f,
        Err(e) => {
            ui::error(Some(owner), &e);
            return;
        }
    };
    // The settings must load before anything is replaced.
    let config = files.iter().find(|(k, _)| *k == Kind::Config).map(|(_, d)| d.clone()).unwrap_or_default();
    if serde_json::from_slice::<crate::config::Config>(&config).is_err() {
        ui::error(Some(owner), "The settings in this backup couldn't be read, so nothing was changed.");
        return;
    }
    let pictures = files.iter().filter(|(k, _)| matches!(k, Kind::Picture(_))).count();
    if !ui::confirm(
        Some(owner),
        &format!(
            "Restore the backup from\n{}\n\nIts categories, pinned apps, settings and {pictures} picture(s) replace \
             the current ones. The current settings are kept as config.json.before-restore-… in the data folder.\n\n\
             FlexTaskbar starts again to finish.",
            source.display()
        ),
    ) {
        return;
    }
    let p = paths::get();
    let current = p.data.join("config.json");
    let keep = p.data.join(format!("config.json.before-restore-{}", crate::config::unix_time()));
    if current.exists()
        && let Err(e) = std::fs::copy(&current, &keep)
    {
        ui::error(Some(owner), &format!("The current settings couldn't be kept, so nothing was changed: {e}"));
        return;
    }
    for (kind, data) in &files {
        if let Kind::Picture(name) = kind
            && let Some(path) = p.icon_file(name)
            && let Err(e) = write_atomic(&path, data)
        {
            ui::error(
                Some(owner),
                &format!("A picture couldn't be written ({name}): {e}\n\nNothing else was changed."),
            );
            return;
        }
    }
    if let Err(e) = write_atomic(&current, &config) {
        ui::error(Some(owner), &format!("The settings couldn't be written: {e}\n\nThe old ones are unchanged."));
        return;
    }
    app::restart();
}
