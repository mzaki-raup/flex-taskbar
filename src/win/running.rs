//! Which apps have windows open, for the bar's and flyouts' running marks
//! and for switching to an app instead of starting it again.
//!
//! The main window registers as a shell hook window, so Windows tells it
//! when a top-level window is created, destroyed, activated or retitled (the
//! same notifications the taskbar gets). A short while after the last one
//! the windows are listed again; nothing runs while nothing changes. Each
//! process's program path and AppUserModelID are looked up once and kept
//! while it runs.

use super::ui::wide;
use super::{app, flyout, strip};
use crate::running::{self, Running, WindowInfo};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, KillTimer, RegisterShellHookWindow, RegisterWindowMessageW,
    SW_MINIMIZE, SW_RESTORE, SetForegroundWindow, SetTimer, ShowWindow, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PCWSTR, PWSTR};

/// Timer on the main window: list the windows again.
pub const TIMER_REFRESH: usize = 8;
/// Timer on the main window: look again after starting an app.
pub const TIMER_LAUNCHED: usize = 9;
/// Wait this long after the last window event (they come in bursts).
const SETTLE_MS: u32 = 150;

struct State {
    shell_msg: u32,
    running: Running,
    /// The ids of the apps that are running, worked out once per change so
    /// drawing only looks them up.
    ids: HashSet<String>,
    /// Program path and AppUserModelID by process id.
    processes: HashMap<u32, (Option<String>, Option<String>)>,
    /// A window's own AppUserModelID, by window, while it is open.
    window_ids: HashMap<isize, Option<String>>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Starts listening for windows coming and going.
pub fn start(main: HWND) {
    let name = wide("SHELLHOOK");
    let shell_msg = unsafe { RegisterWindowMessageW(PCWSTR(name.as_ptr())) };
    unsafe {
        let _ = RegisterShellHookWindow(main);
    }
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            shell_msg,
            running: Running::default(),
            ids: HashSet::new(),
            processes: HashMap::new(),
            window_ids: HashMap::new(),
        })
    });
    refresh();
}

/// The registered shell hook message (0 before [`start`]).
pub fn shell_message() -> u32 {
    STATE.with(|s| s.borrow().as_ref().map(|s| s.shell_msg).unwrap_or(0))
}

/// A window event: list the windows again shortly.
pub fn window_event(main: HWND) {
    unsafe {
        SetTimer(Some(main), TIMER_REFRESH, SETTLE_MS, None);
    }
}

/// An app was just started: look again once its window has had time to
/// appear (a second check catches slow starters).
pub fn launched(main: HWND) {
    unsafe {
        SetTimer(Some(main), TIMER_LAUNCHED, 1500, None);
    }
}

/// The refresh timer fired.
pub fn timer(main: HWND, id: usize) {
    unsafe {
        let _ = KillTimer(Some(main), id);
    }
    refresh();
}

/// Lists the taskbar windows and, if anything changed, redraws the marks.
pub fn refresh() {
    if STATE.with(|s| s.borrow().is_none()) {
        return; // not started yet
    }
    let mut list: Vec<HWND> = Vec::new();
    unsafe extern "system" fn each(h: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        list.push(h);
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    let own = std::process::id();
    let mut seen: Vec<u32> = Vec::new();
    let mut windows: Vec<(isize, Vec<String>)> = Vec::new();
    for h in list {
        if !is_task_window(h) {
            continue;
        }
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(h, Some(&mut pid));
        }
        if pid == 0 || pid == own {
            continue;
        }
        seen.push(pid);
        let (exe, process_id) = process_info(pid);
        // A window can carry its own id (web apps, Store app frames).
        let key = h.0 as isize;
        let own_id = match STATE.with(|s| s.borrow().as_ref().and_then(|st| st.window_ids.get(&key).cloned())) {
            Some(id) => id,
            None => {
                let id = window_aumid(h);
                STATE.with(|s| s.borrow_mut().as_mut().map(|st| st.window_ids.insert(key, id.clone())));
                id
            }
        };
        let aumid = own_id.or(process_id);
        windows.push((h.0 as isize, running::window_keys(aumid.as_deref(), exe.as_deref())));
    }
    let new = Running::new(&windows);
    let ids: HashSet<String> = app::with(|s| {
        s.catalog.apps.iter().filter(|a| !a.keys.is_empty() && new.is_running(&a.keys)).map(|a| a.id.clone()).collect()
    });
    let changed = STATE.with(|s| {
        let mut b = s.borrow_mut();
        let Some(st) = b.as_mut() else { return false };
        // Forget processes that have no windows any more.
        st.processes.retain(|pid, _| seen.contains(pid));
        st.window_ids.retain(|h, _| windows.iter().any(|(w, _)| w == h));
        let changed = ids != st.ids;
        st.running = new;
        st.ids = ids;
        changed
    });
    if changed {
        strip::refresh();
        flyout::refresh();
    }
}

fn is_task_window(h: HWND) -> bool {
    unsafe {
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
        running::is_task_window(&WindowInfo {
            visible: IsWindowVisible(h).as_bool(),
            cloaked: cloaked != 0,
            tool: ex & WS_EX_TOOLWINDOW.0 != 0,
            app_window: ex & WS_EX_APPWINDOW.0 != 0,
            owned: GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()),
            has_title: GetWindowTextLengthW(h) > 0,
        })
    }
}

/// A process's program path and (for packaged apps) AppUserModelID, looked
/// up once while it runs.
fn process_info(pid: u32) -> (Option<String>, Option<String>) {
    if let Some(info) = STATE.with(|s| s.borrow().as_ref().and_then(|s| s.processes.get(&pid).cloned())) {
        return info;
    }
    let info = unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let mut buf = vec![0u16; 1024];
                let mut len = buf.len() as u32;
                let exe = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                    .ok()
                    .map(|_| String::from_utf16_lossy(&buf[..len as usize]));
                let aumid = process_aumid(h);
                let _ = CloseHandle(h);
                (exe, aumid)
            }
            Err(_) => (None, None),
        }
    };
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.processes.insert(pid, info.clone());
        }
    });
    info
}

/// A packaged (Store) app process's AppUserModelID. Looked up at run time:
/// `GetApplicationUserModelId` is missing on some systems (Wine).
fn process_aumid(process: windows::Win32::Foundation::HANDLE) -> Option<String> {
    type GetId = unsafe extern "system" fn(windows::Win32::Foundation::HANDLE, *mut u32, PWSTR) -> u32;
    static FUNC: std::sync::OnceLock<Option<GetId>> = std::sync::OnceLock::new();
    let f = FUNC.get_or_init(|| unsafe {
        use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        let k = GetModuleHandleW(windows::core::w!("kernel32.dll")).ok()?;
        GetProcAddress(k, windows::core::s!("GetApplicationUserModelId")).map(|p| std::mem::transmute::<_, GetId>(p))
    });
    let f = (*f)?;
    let mut id = vec![0u16; 256];
    let mut len = id.len() as u32;
    // 0 is ERROR_SUCCESS; a process that isn't packaged fails.
    (unsafe { f(process, &mut len, PWSTR(id.as_mut_ptr())) } == 0)
        .then(|| String::from_utf16_lossy(&id[..len.saturating_sub(1) as usize]))
        .filter(|s| !s.is_empty())
}

/// The AppUserModelID a window sets for itself, if any.
fn window_aumid(h: HWND) -> Option<String> {
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(h).ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let p = PropVariantToStringAlloc(&value).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        s.filter(|s| !s.is_empty())
    }
}

/// Whether the app `id` has a window open (cheap: drawn every frame).
pub fn is_running(id: &str) -> bool {
    STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.ids.contains(id)))
}

fn still_open(h: isize) -> bool {
    let h = HWND(h as *mut _);
    unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(h)).as_bool() && is_task_window(h) }
}

/// A click on the app `id`: switches to its window when it is running
/// (cycling or minimising like the taskbar), unless Shift is held. Returns
/// whether it did; `false` means start the app.
pub fn switch_to(id: &str) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
    if !app::with(|s| s.cfg.settings.switch_to_running) {
        return false;
    }
    let keys = app::with(|s| s.catalog.get(id).map(|a| a.keys.clone())).unwrap_or_default();
    let mut windows = STATE.with(|s| s.borrow().as_ref().map(|st| st.running.windows(&keys)).unwrap_or_default());
    // Windows that have closed since the last look don't count.
    let stale = windows.iter().any(|&h| !still_open(h));
    if stale {
        refresh();
        windows = STATE.with(|s| s.borrow().as_ref().map(|st| st.running.windows(&keys)).unwrap_or_default());
    }
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    let foreground = unsafe { GetForegroundWindow() }.0 as isize;
    match running::click(&windows, foreground, shift) {
        running::Click::Launch => false,
        running::Click::Activate(h) => {
            let h = HWND(h as *mut _);
            unsafe {
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                let _ = SetForegroundWindow(h);
            }
            true
        }
        running::Click::Minimize(h) => {
            unsafe {
                let _ = ShowWindow(HWND(h as *mut _), SW_MINIMIZE);
            }
            true
        }
    }
}
