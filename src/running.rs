#![cfg_attr(not(windows), allow(dead_code))]

//! Which apps are running: which windows count (the ones a taskbar would
//! show), how windows and apps are matched (by AppUserModelID, or by the
//! program's path), and what clicking a running app does. Pure, so it can
//! be unit-tested off Windows; `win::running` feeds it the windows.

use std::collections::HashMap;

/// What decides whether a window belongs on a taskbar.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowInfo {
    pub visible: bool,
    /// Hidden by DWM (on another virtual desktop, or a suspended Store app).
    pub cloaked: bool,
    /// `WS_EX_TOOLWINDOW`.
    pub tool: bool,
    /// `WS_EX_APPWINDOW`: shown even when owned.
    pub app_window: bool,
    /// Has an owner window (dialogs, palettes).
    pub owned: bool,
    pub has_title: bool,
}

/// The rule the Windows taskbar uses, near enough.
pub fn is_task_window(w: &WindowInfo) -> bool {
    w.visible && !w.cloaked && w.has_title && (w.app_window || (!w.tool && !w.owned))
}

/// Lowercased, with `/` as `\` and no surrounding quotes or spaces.
pub fn norm_path(p: &str) -> String {
    p.trim().trim_matches('"').replace('/', "\\").to_lowercase()
}

fn exe_key(path: &str) -> Option<String> {
    let p = norm_path(path);
    p.ends_with(".exe").then(|| format!("exe:{p}"))
}

fn id_key(aumid: &str) -> Option<String> {
    let a = aumid.trim().to_lowercase();
    (!a.is_empty()).then(|| format!("id:{a}"))
}

/// The keys an app is recognised by: its AppUserModelID (the Apps folder's
/// parsing name, for Store and web apps) and the program it starts.
pub fn app_keys(aumid: Option<&str>, exe: Option<&str>) -> Vec<String> {
    aumid.and_then(id_key).into_iter().chain(exe.and_then(exe_key)).collect()
}

/// The keys a window is recognised by. A window with its own
/// AppUserModelID is matched by that alone, so a Chrome web-app window
/// doesn't make the Chrome browser look open. Otherwise its program's full
/// path, and its bare file name (for apps whose target is just
/// `notepad.exe`, found on the PATH).
pub fn window_keys(aumid: Option<&str>, exe: Option<&str>) -> Vec<String> {
    match aumid.and_then(id_key) {
        Some(k) => vec![k],
        None => {
            let Some(full) = exe.and_then(exe_key) else { return Vec::new() };
            let bare = full.rsplit_once('\\').map(|(_, name)| format!("exe:{name}"));
            std::iter::once(full).chain(bare).collect()
        }
    }
}

/// The running windows by key, each list in Z order (front first).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Running {
    by_key: HashMap<String, Vec<isize>>,
}

impl Running {
    /// `windows`: (window handle, its keys), front to back.
    pub fn new(windows: &[(isize, Vec<String>)]) -> Running {
        let mut by_key: HashMap<String, Vec<isize>> = HashMap::new();
        for (hwnd, keys) in windows {
            for k in keys {
                by_key.entry(k.clone()).or_default().push(*hwnd);
            }
        }
        Running { by_key }
    }

    /// The app's windows, front first, without repeats.
    pub fn windows(&self, app_keys: &[String]) -> Vec<isize> {
        let mut out: Vec<isize> = Vec::new();
        for k in app_keys {
            for h in self.by_key.get(k).into_iter().flatten() {
                if !out.contains(h) {
                    out.push(*h);
                }
            }
        }
        out
    }

    pub fn is_running(&self, app_keys: &[String]) -> bool {
        app_keys.iter().any(|k| self.by_key.contains_key(k))
    }
}

/// What a click on a running app does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Not running (or Shift held): start it.
    Launch,
    /// Bring this window to the front (restoring it if minimised).
    Activate(isize),
    /// It is the only window and already in front: minimise it.
    Minimize(isize),
}

/// `windows`: the app's windows, front first; `foreground`: the window in
/// front. A click on an app that is in front cycles through its windows, or
/// minimises its only one, like the Windows taskbar.
pub fn click(windows: &[isize], foreground: isize, shift: bool) -> Click {
    if shift || windows.is_empty() {
        return Click::Launch;
    }
    match windows.iter().position(|&h| h == foreground) {
        None => Click::Activate(windows[0]),
        Some(_) if windows.len() == 1 => Click::Minimize(windows[0]),
        // Z order puts the one in front first, so the next is at the back:
        // the one that was in front longest ago.
        Some(i) => Click::Activate(windows[(i + windows.len() - 1) % windows.len()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_windows_count() {
        let normal = WindowInfo { visible: true, has_title: true, ..Default::default() };
        assert!(is_task_window(&normal));
        assert!(!is_task_window(&WindowInfo { visible: false, ..normal }));
        assert!(!is_task_window(&WindowInfo { cloaked: true, ..normal }));
        assert!(!is_task_window(&WindowInfo { has_title: false, ..normal }));
        assert!(!is_task_window(&WindowInfo { tool: true, ..normal }));
        assert!(!is_task_window(&WindowInfo { owned: true, ..normal }));
        // An owned window that asks to be on the taskbar.
        assert!(is_task_window(&WindowInfo { owned: true, app_window: true, ..normal }));
    }

    #[test]
    fn keys() {
        assert_eq!(norm_path(r#" "C:/Program Files/App/App.EXE" "#), r"c:\program files\app\app.exe");
        assert_eq!(
            app_keys(Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"), None),
            ["id:microsoft.windowscalculator_8wekyb3d8bbwe!app"]
        );
        assert_eq!(
            app_keys(Some("{GUID}\\notepad.exe"), Some(r"C:\Windows\notepad.exe")),
            ["id:{guid}\\notepad.exe", r"exe:c:\windows\notepad.exe"]
        );
        // Only programs count as a path key (not a shortcut or document).
        assert!(app_keys(None, Some(r"C:\x\readme.txt")).is_empty());
        // A window with its own id is matched by that alone.
        assert_eq!(window_keys(Some("Chrome._crx_abc"), Some(r"C:\Chrome\chrome.exe")), ["id:chrome._crx_abc"]);
        assert_eq!(window_keys(None, Some(r"C:\Chrome\chrome.exe")), [r"exe:c:\chrome\chrome.exe", "exe:chrome.exe"]);
        // An app started by bare name matches a window by its file name.
        let bare = app_keys(None, Some("Notepad.exe"));
        assert_eq!(bare, ["exe:notepad.exe"]);
        let r = Running::new(&[(9, window_keys(None, Some(r"C:\Windows\System32\notepad.exe")))]);
        assert!(r.is_running(&bare));
        // But a full path only matches that program, not one of the same name.
        assert!(!r.is_running(&app_keys(None, Some(r"D:\Tools\notepad.exe"))));
    }

    #[test]
    fn matching() {
        let r = Running::new(&[
            (1, window_keys(None, Some(r"C:\Windows\notepad.exe"))),
            (2, window_keys(Some("Chrome._crx_yt"), Some(r"C:\Chrome\chrome.exe"))),
            (3, window_keys(None, Some(r"C:\Windows\NOTEPAD.EXE"))),
        ]);
        let notepad = app_keys(Some("{x}\\notepad.exe"), Some(r"c:\windows\notepad.exe"));
        assert!(r.is_running(&notepad));
        assert_eq!(r.windows(&notepad), [1, 3]);
        let youtube = app_keys(Some("Chrome._crx_yt"), None);
        assert_eq!(r.windows(&youtube), [2]);
        // The browser itself isn't open: only a web app is.
        let chrome = app_keys(Some("Chrome"), Some(r"C:\Chrome\chrome.exe"));
        assert!(!r.is_running(&chrome));
        assert_eq!(Running::default().windows(&notepad), Vec::<isize>::new());
    }

    #[test]
    fn clicking() {
        assert_eq!(click(&[], 0, false), Click::Launch);
        assert_eq!(click(&[5], 9, true), Click::Launch); // Shift: another copy
        assert_eq!(click(&[5, 6], 9, false), Click::Activate(5));
        assert_eq!(click(&[5], 5, false), Click::Minimize(5));
        // In front with several windows: cycle to the one at the back.
        assert_eq!(click(&[5, 6, 7], 5, false), Click::Activate(7));
    }
}
