//! Notices when apps are installed or removed, so the app list can rescan by
//! itself: a background thread waits on change notifications for
//!
//! - the Start Menu folders (per user and for all users), where installers,
//!   winget, Chocolatey, UniGetUI and Chrome/Edge web apps put shortcuts;
//! - `%LOCALAPPDATA%\Packages`, where every Store / modern app gets a folder;
//! - the package managers' program folders (see `pkgsources`).
//!
//! It posts `WM_APP_APPS_CHANGED` to the main window, which waits for things
//! to settle and then rescans. Folders that appear later (a package manager
//! installed after FlexTaskbar started) are picked up every few minutes.

use super::ui::wide;
use crate::pkgsources;
use std::path::PathBuf;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM};
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FindCloseChangeNotification,
    FindFirstChangeNotificationW, FindNextChangeNotification,
};
use windows::Win32::System::Threading::WaitForMultipleObjects;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::PCWSTR;

/// How often to look again for folders that didn't exist yet.
const REFRESH_MS: u32 = 5 * 60 * 1000;
/// WaitForMultipleObjects' limit.
const MAX_HANDLES: usize = 64;

/// (folder, include subfolders, what to watch).
fn folders() -> Vec<(PathBuf, bool, FILE_NOTIFY_CHANGE)> {
    let env = |v: &str| std::env::var_os(v).map(PathBuf::from);
    let mut out = Vec::new();
    let names = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME;
    for base in [env("APPDATA"), env("ProgramData")].into_iter().flatten() {
        out.push((base.join("Microsoft\\Windows\\Start Menu\\Programs"), true, names));
    }
    // Only package folders appearing or going: apps write inside theirs all the time.
    if let Some(local) = env("LOCALAPPDATA") {
        out.push((local.join("Packages"), false, FILE_NOTIFY_CHANGE_DIR_NAME));
    }
    let subdirs = |root: &std::path::Path| -> Vec<PathBuf> {
        std::fs::read_dir(root)
            .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default()
    };
    for (_, dir) in pkgsources::tool_dirs(|v| std::env::var(v).ok(), subdirs) {
        out.push((dir, false, FILE_NOTIFY_CHANGE_FILE_NAME));
    }
    out.retain(|(d, ..)| d.is_dir());
    out.truncate(MAX_HANDLES);
    out
}

/// Starts watching; changes are posted to `hwnd` as `msg`.
pub fn start(hwnd: HWND, msg: u32) {
    let raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        let hwnd = HWND(raw as *mut _);
        loop {
            let mut handles: Vec<HANDLE> = Vec::new();
            for (dir, subtree, filter) in folders() {
                let w = wide(&dir.display().to_string());
                if let Ok(h) = unsafe { FindFirstChangeNotificationW(PCWSTR(w.as_ptr()), subtree, filter) } {
                    handles.push(h);
                }
            }
            if handles.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(REFRESH_MS as u64));
                continue;
            }
            loop {
                let r = unsafe { WaitForMultipleObjects(&handles, false, REFRESH_MS) };
                if r == WAIT_TIMEOUT {
                    break; // look for new folders
                }
                let i = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
                if i >= handles.len() {
                    break; // a handle went bad: start over
                }
                unsafe {
                    let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
                    if FindNextChangeNotification(handles[i]).is_err() {
                        break;
                    }
                }
            }
            for h in handles {
                unsafe {
                    let _ = FindCloseChangeNotification(h);
                }
            }
        }
    });
}
