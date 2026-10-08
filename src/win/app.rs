//! The running app: global state, the hidden main window (tray icon, hotkeys,
//! background-task results) and the message loop.
//!
//! State lives in one thread-local `RefCell`. Win32 re-enters window procedures
//! whenever a call pumps messages (menus, message boxes, dialogs) or sends
//! notifications (tree/list updates), so the rule throughout is: borrow state in
//! short `with(...)` calls, and never call into a window while holding it.

use super::catalog::{self, Catalog, ShellApp, Source as AppSource};
use super::icons::{self, Source as IconSource};
use super::launch::{self, Target};
use super::ui::{self, wide};
use super::{Args, Command, MAIN_CLASS, autostart, manager, menu, paths, searchwin, strip, supervisor, theme};
use crate::appkind::AppKind;
use crate::config::{Config, Hotkey, LoadOutcome, Store};
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_BAR_CLASSES, ICC_HOTKEY_CLASS, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, ICC_TREEVIEW_CLASSES,
    INITCOMMONCONTROLSEX, InitCommonControlsEx,
};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NIN_SELECT, NOTIFY_ICON_INFOTIP_FLAGS, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
    ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, HICON, IMAGE_ICON,
    IsDialogMessageW, KillTimer, LR_DEFAULTCOLOR, LoadImageW, MSG, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SM_CXSMICON, SW_SHOWNORMAL, SetTimer, TranslateMessage, WM_APP, WM_CONTEXTMENU, WM_DESTROY,
    WM_ENDSESSION, WM_HOTKEY, WM_QUERYENDSESSION, WM_SETTINGCHANGE, WM_SYSCOLORCHANGE, WM_TIMER, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};
use windows::core::{PCWSTR, w};

pub const WM_APP_COMMAND: u32 = WM_APP + 1;
const WM_APP_TRAY: u32 = WM_APP + 2;
pub const WM_APP_ICONS: u32 = WM_APP + 3;
const WM_APP_SCAN_DONE: u32 = WM_APP + 4;
const WM_APP_ERROR: u32 = WM_APP + 5;
/// From `watch`: something was installed or removed.
const WM_APP_APPS_CHANGED: u32 = WM_APP + 6;
/// Rescan once changes have settled.
const TIMER_RESCAN: usize = 7;

/// NIN_SELECT | NINF_KEY: the tray icon was activated with the keyboard.
const NIN_KEYSELECT: u32 = NIN_SELECT | 0x1;

const HOTKEY_SEARCH: i32 = 1;
const HOTKEY_MENU: i32 = 2;
const HOTKEY_BAR: i32 = 3;
const HOTKEY_NEXT_BAR: i32 = 4;
/// Category hotkeys are numbered from here.
const HOTKEY_CATEGORY: i32 = 100;

thread_local! {
    /// The category hotkeys registered now: (hotkey id, category id).
    static CATEGORY_HOTKEYS: std::cell::RefCell<Vec<(i32, u64)>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub const FOLDER_ICON: &str = "folder";

pub struct State {
    pub cfg: Config,
    store: Store,
    shell_apps: Vec<ShellApp>,
    pub catalog: Catalog,
    /// Keyed by app id, `cat:<id>`, or [`FOLDER_ICON`].
    icons: HashMap<String, HBITMAP>,
    pub icon_size: i32,
    pub main: HWND,
    pub scanning: bool,
    /// Apps changed during a scan: scan again when it finishes.
    pub rescan_pending: bool,
    save_error_shown: bool,
}

impl State {
    /// Icon lookup for code that already holds the state.
    pub fn icon_of(&self, key: &str) -> Option<HBITMAP> {
        self.icons.get(key).copied()
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    /// Windows that want Tab/Enter dialog navigation in the message loop.
    static DIALOGS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f` with the app state. Must not be nested; see the module comment.
pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(s.borrow_mut().as_mut().expect("app state not initialized")))
}

/// [`with`], or `None` while the state is already in use. For code that can
/// be called back from outside at any moment (a screen reader's question),
/// which must never panic.
pub fn try_with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

pub fn register_dialog(hwnd: HWND, add: bool) {
    DIALOGS.with(|d| {
        let mut d = d.borrow_mut();
        d.retain(|h| *h != hwnd);
        if add {
            d.push(hwnd);
        }
    });
}

pub fn main_hwnd() -> HWND {
    with(|s| s.main)
}

pub fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default() }
}

pub fn app_icon(size: i32) -> HICON {
    unsafe {
        // MAKEINTRESOURCE(1): the icon resource id from assets/app.rc, not a real pointer.
        #[allow(clippy::manual_dangling_ptr)]
        let id = PCWSTR(1 as *const u16);
        LoadImageW(Some(instance()), id, IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_default()
    }
}

thread_local! {
    /// Started by the supervisor (which can start it again).
    static SUPERVISED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Starts FlexTaskbar again without saving (a backup's settings were just
/// written to disk). Without a supervisor it just exits after saying so.
pub fn restart() {
    if !SUPERVISED.with(|s| s.get()) {
        ui::info(None, "The backup was restored. Start FlexTaskbar again to use it.");
        std::process::exit(super::EXIT_CLEAN);
    }
    strip::destroy();
    remove_tray_icon(main_hwnd());
    std::process::exit(super::EXIT_RESTART);
}

pub fn run(args: &Args) -> i32 {
    SUPERVISED.with(|s| s.set(args.worker));
    super::diagnostics::start();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES
                | ICC_LISTVIEW_CLASSES
                | ICC_TREEVIEW_CLASSES
                | ICC_HOTKEY_CLASS
                | ICC_BAR_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);
    }
    theme::allow_dark_menus();

    let paths = paths::get();
    let store = Store::new(&paths.data);
    let reset_backup = if args.reset { store.reset() } else { None };
    let (cfg, outcome) = store.load();
    let shell_apps = catalog::load_cache(&paths.apps_cache());
    let catalog = Catalog::build(&shell_apps, &cfg.custom_apps);
    let icon_size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) }.max(16);

    let main = match create_main_window() {
        Some(h) => h,
        None => return 2,
    };

    let first_run = matches!(outcome, LoadOutcome::FirstRun) && cfg.categories.is_empty();
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            cfg,
            store,
            shell_apps,
            catalog,
            icons: HashMap::new(),
            icon_size,
            main,
            scanning: false,
            rescan_pending: false,
            save_error_shown: false,
        })
    });

    if matches!(outcome, LoadOutcome::RestoredFromBackup(_) | LoadOutcome::ResetToDefaults(_)) {
        // Write the recovered settings back right away, so the next start is clean.
        save();
    }
    // Smart categories as of the last app list (the scan below updates them).
    refill_smart(false);
    theme::set_mode(with(|s| s.cfg.settings.appearance.theme));
    theme::allow_dark_for_window(main);
    add_tray_icon(main);
    strip::apply_settings();
    let hotkey_problems = register_hotkeys();
    load_folder_icon();
    request_all_icons();
    start_scan();
    super::watch::start(main, WM_APP_APPS_CHANGED);
    super::running::start(main);

    // Tell the user about anything that happened on the way up.
    let mut notes = Vec::new();
    if args.restarted {
        notes.push("FlexTaskbar closed unexpectedly and was restarted.".to_string());
    }
    match &outcome {
        LoadOutcome::RestoredFromBackup(kept) => notes.push(format!(
            "Your settings file was damaged and has been restored from its backup. The damaged copy was kept as {}.",
            file_name(kept)
        )),
        LoadOutcome::ResetToDefaults(kept) => notes.push(format!(
            "Your settings file was damaged and no backup was usable, so defaults were loaded. The damaged copy was kept as {}.",
            file_name(kept)
        )),
        _ => {}
    }
    if let Some(kept) = reset_backup {
        notes.push(format!("Configuration reset. The previous one was kept as {}.", file_name(&kept)));
    }
    notes.extend(hotkey_problems);
    if !notes.is_empty() {
        balloon(&notes.join("\n"), true);
    }

    match args.command {
        Some(cmd) => run_command(cmd),
        None if first_run => manager::show(),
        None => {}
    }

    message_loop()
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn message_loop() -> i32 {
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let handled = DIALOGS.with(|d| {
                let dialogs = d.borrow().clone();
                dialogs.into_iter().any(|h| IsDialogMessageW(h, &msg).as_bool())
            });
            if !handled {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        // WM_QUIT only comes from Exit (the main window being destroyed).
        let _ = msg;
        super::EXIT_CLEAN
    }
}

fn create_main_window() -> Option<HWND> {
    let class = wide(MAIN_CLASS);
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(main_proc),
            hInstance: instance(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        // A real (hidden) top-level window rather than a message-only one:
        // message-only windows never receive the TaskbarCreated broadcast.
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            PCWSTR(class.as_ptr()),
            w!("FlexTaskbar"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance()),
            None,
        )
        .ok()
    }
}

fn taskbar_created_msg() -> u32 {
    use std::sync::OnceLock;
    static MSG: OnceLock<u32> = OnceLock::new();
    *MSG.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg != 0 && msg == super::running::shell_message() {
        super::running::window_event(hwnd);
        return LRESULT(0);
    }
    match msg {
        WM_APP_COMMAND => {
            if let Some(cmd) = Command::from_usize(wparam.0) {
                run_command(cmd);
            }
            LRESULT(0)
        }
        WM_APP_TRAY => {
            let event = ui::loword(lparam.0 as usize) as u32;
            if event == NIN_SELECT || event == NIN_KEYSELECT || event == WM_CONTEXTMENU {
                show_menu();
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wparam.0 as i32 {
                HOTKEY_SEARCH => searchwin::toggle(),
                HOTKEY_MENU => show_menu(),
                HOTKEY_BAR => super::flyout::keyboard_open(),
                HOTKEY_NEXT_BAR => next_bar(),
                n if n >= HOTKEY_CATEGORY => {
                    let cat = CATEGORY_HOTKEYS.with(|c| c.borrow().iter().find(|(h, _)| *h == n).map(|(_, c)| *c));
                    if let Some(cat) = cat {
                        open_category_by_hotkey(cat);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_APP_ICONS => {
            let batch = unsafe { Box::from_raw(lparam.0 as *mut Vec<(String, isize)>) };
            let keys = with(|s| {
                let mut keys = Vec::with_capacity(batch.len());
                for (key, raw) in batch.into_iter() {
                    let bmp = HBITMAP(raw as *mut _);
                    // Never replace a bitmap here: an open menu may be drawing it.
                    if s.icons.contains_key(&key) {
                        icons::free(bmp);
                    } else {
                        s.icons.insert(key.clone(), bmp);
                        keys.push(key);
                    }
                }
                keys
            });
            searchwin::icons_arrived(&keys);
            manager::icons_arrived(&keys);
            LRESULT(0)
        }
        WM_APP_SCAN_DONE => {
            let result = unsafe { Box::from_raw(lparam.0 as *mut Result<Vec<ShellApp>, String>) };
            finish_scan(*result);
            if with(|s| std::mem::take(&mut s.rescan_pending)) {
                unsafe {
                    SetTimer(Some(hwnd), TIMER_RESCAN, crate::pkgsources::SETTLE_MS, None);
                }
            }
            LRESULT(0)
        }
        WM_APP_APPS_CHANGED => {
            // Installers write many files: rescan once it has been quiet a moment.
            if with(|s| s.cfg.settings.auto_rescan) {
                unsafe {
                    SetTimer(Some(hwnd), TIMER_RESCAN, crate::pkgsources::SETTLE_MS, None);
                }
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == super::running::TIMER_REFRESH || wparam.0 == super::running::TIMER_LAUNCHED => {
            super::running::timer(hwnd, wparam.0);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_RESCAN => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_RESCAN);
            }
            request_rescan();
            LRESULT(0)
        }
        WM_APP_ERROR => {
            let text = unsafe { Box::from_raw(lparam.0 as *mut String) };
            ui::error(None, &text);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            let area = if lparam.0 != 0 {
                unsafe { PCWSTR(lparam.0 as *const u16).to_string().unwrap_or_default() }
            } else {
                String::new()
            };
            // High contrast turned on or off, or Animation effects changed.
            const SPI_SETHIGHCONTRAST: usize = 0x0043;
            const SPI_SETCLIENTAREAANIMATION: usize = 0x1043;
            if area == "ImmersiveColorSet" {
                theme::forget_cached();
                theme::allow_dark_menus();
                searchwin::theme_changed();
                strip::theme_changed();
                super::panel::theme_changed();
            } else if wparam.0 == SPI_SETHIGHCONTRAST || wparam.0 == SPI_SETCLIENTAREAANIMATION {
                accessibility_changed();
            }
            LRESULT(0)
        }
        WM_SYSCOLORCHANGE => {
            accessibility_changed();
            LRESULT(0)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                // Logoff/shutdown: settings are already saved after every change.
                strip::destroy();
                remove_tray_icon(hwnd);
                std::process::exit(super::EXIT_CLEAN);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if msg == taskbar_created_msg() && msg != 0 => {
            // Explorer restarted: the tray icon and AppBar are gone; re-add them.
            add_tray_icon(hwnd);
            strip::shell_restarted();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn run_command(cmd: Command) {
    match cmd {
        Command::Manage => manager::show(),
        Command::Search => searchwin::show(),
        Command::Menu => show_menu(),
        Command::Bar => super::flyout::keyboard_open(),
        Command::Diagnostics => super::diagnostics::show(),
        Command::Exit => exit(),
    }
}

pub fn exit() {
    searchwin::destroy();
    manager::destroy();
    strip::destroy();
    let main = main_hwnd();
    unsafe {
        let _ = DestroyWindow(main);
    }
}

pub fn show_menu() {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    if let Some(action) = menu::track(main_hwnd(), pt) {
        perform(action);
    }
}

pub fn perform(action: menu::Action) {
    match action {
        menu::Action::Launch(id) => launch_app(&id),
        menu::Action::Search => searchwin::show(),
        menu::Action::Manage => manager::show(),
        menu::Action::Appearance => super::appearancewin::show(),
        menu::Action::Arrange => super::arrangewin::show(),
        menu::Action::Rescan => start_scan(),
        menu::Action::ToggleAutostart => toggle_autostart(None),
        menu::Action::ToggleStrip => set_strip(None),
        menu::Action::OpenDataFolder => open_data_folder(),
        menu::Action::Diagnostics => super::diagnostics::show(),
        menu::Action::SwitchBar(name) => switch_bar(&name),
        menu::Action::NextBar => next_bar(),
        menu::Action::NewBar { copy } => new_bar(copy),
        menu::Action::RenameBar => rename_bar(),
        menu::Action::DeleteBar => delete_bar(),
        menu::Action::ShowCategory(id) => {
            if with(|s| crate::bars::show_category(&mut s.cfg, id, true)) {
                save();
            }
        }
        menu::Action::Exit => exit(),
    }
}

/// Windows' contrast theme, system colours or animation setting changed:
/// everything drawn by FlexTaskbar follows.
fn accessibility_changed() {
    theme::forget_cached();
    searchwin::theme_changed();
    strip::apply_appearance();
    super::panel::theme_changed();
}

// ---------------------------------------------------------------- bars

/// Puts the bar called `name` in place of the one in use.
pub fn switch_bar(name: &str) {
    super::flyout::close();
    if with(|s| crate::bars::switch(&mut s.cfg, name)) {
        save();
    }
}

/// The next bar (the *Next bar* hotkey).
fn next_bar() {
    if let Some(name) = with(|s| crate::bars::next(&s.cfg)) {
        switch_bar(&name);
    }
}

fn new_bar(copy: bool) {
    let label = if copy { "Name of the copy" } else { "Name of the new bar" };
    let Some(name) = super::prompt::ask(main_hwnd(), "New bar", label, if copy { "" } else { "New bar" }) else {
        return;
    };
    super::flyout::close();
    with(|s| crate::bars::add(&mut s.cfg, &name, copy));
    save();
    // The Next bar hotkey is only taken once there are two bars.
    report_hotkeys(register_hotkeys());
}

fn rename_bar() {
    let cur = with(|s| crate::bars::current(&s.cfg));
    let Some(name) = super::prompt::ask(main_hwnd(), "Rename bar", "Name of this bar", &cur) else { return };
    if with(|s| crate::bars::rename(&mut s.cfg, &name)).is_some() {
        save();
    }
}

fn delete_bar() {
    let cur = with(|s| crate::bars::current(&s.cfg));
    let text = format!(
        "Delete the bar “{cur}”?\n\nIts pinned apps and order are forgotten; the apps and categories themselves stay."
    );
    if !ui::confirm(None, &text) {
        return;
    }
    super::flyout::close();
    if with(|s| crate::bars::delete_current(&mut s.cfg)).is_some() {
        save();
        register_hotkeys();
    }
}

/// Tells the user about hotkeys another program already owns.
fn report_hotkeys(problems: Vec<String>) {
    if !problems.is_empty() {
        balloon(&problems.join("\n"), true);
    }
}

/// Shows/hides the strip (`None` toggles) and optionally changes whether it
/// reserves screen space.
pub fn set_strip(show: Option<bool>) {
    with(|s| s.cfg.settings.show_strip = show.unwrap_or(!s.cfg.settings.show_strip));
    save();
    strip::apply_settings();
    manager::settings_changed();
}

pub fn toggle_autostart(owner: Option<HWND>) {
    let enable = !autostart::is_enabled();
    if let Err(e) = autostart::set_enabled(enable) {
        ui::error(owner, &format!("Couldn't change the startup setting.\n\n{e}"));
    }
}

pub fn open_data_folder() {
    let dir = wide(&paths::get().data.display().to_string());
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(dir.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

// ---------------------------------------------------------------- persistence

/// Saves the configuration. A failure is reported once per session rather than
/// on every change.
pub fn save() {
    let failed = with(|s| match s.store.save(&s.cfg) {
        Ok(()) => None,
        Err(e) if !s.save_error_shown => {
            s.save_error_shown = true;
            Some(format!("Settings couldn't be saved to {}:\n{e}", s.store.path().display()))
        }
        Err(_) => None,
    });
    if let Some(text) = failed {
        supervisor::log(&text);
        balloon(&text, true);
    }
    // Every change goes through here, so the strip always shows the saved state.
    strip::refresh();
    super::arrangewin::bar_changed();
}

/// The folder a pinned app opens as a flyout, if it is one.
pub fn folder_of(id: &str) -> Option<std::path::PathBuf> {
    with(|s| s.catalog.get(id).and_then(|a| a.folder.clone()))
}

/// Fills the smart categories from their rules (see `smart`). With
/// `new_list`, the app list itself changed: first notes the apps it hasn't
/// seen before, and forgets the ones gone. Returns whether the settings
/// changed (the caller saves).
pub fn refill_smart(new_list: bool) -> bool {
    with(|s| {
        let cfg = &mut s.cfg;
        let ids: Vec<&str> = s.catalog.apps.iter().map(|a| a.id.as_str()).collect();
        let now = crate::config::unix_time();
        let mut changed = false;
        if new_list {
            changed |= crate::smart::note_seen(&mut cfg.first_seen, &ids, now);
            changed |= crate::smart::forget_launches(&mut cfg.launches, &ids);
        }
        let facts: Vec<crate::smart::Facts> = s
            .catalog
            .apps
            .iter()
            .map(|a| crate::smart::Facts {
                id: &a.id,
                name: &a.name,
                file: &a.file_hint,
                kind: crate::allview::KindGroup::of(a.kind),
            })
            .collect();
        let history = crate::smart::History { first_seen: &cfg.first_seen, launches: &cfg.launches };
        changed | crate::smart::refill(&mut cfg.categories, &facts, &history, now)
    })
}

/// Whether category `id` is a smart one (it fills itself).
pub fn is_smart(id: u64) -> bool {
    with(|s| crate::tree::find(&s.cfg.categories, id).is_some_and(|c| c.smart.is_some()))
}

/// Rebuilds the catalog after custom apps changed.
pub fn rebuild_catalog() {
    with(|s| s.catalog = Catalog::build(&s.shell_apps, &s.cfg.custom_apps));
    if refill_smart(true) {
        save();
    }
    // Apps came or went: which of them are running may have changed.
    super::running::refresh();
}

// ---------------------------------------------------------------- launching

/// A `.cmd`/`.bat` launcher, or a program whose header says it is a console
/// one.
fn is_console_program(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".cmd") || lower.ends_with(".bat") {
        return true;
    }
    let mut head = [0u8; 1024];
    let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    crate::pkgsources::pe_is_console(&head[..n]) == Some(true)
}

pub fn launch_app(id: &str) {
    // Already open: switch to it instead (Shift+click starts another copy).
    if super::running::switch_to(id) {
        with(|s| s.cfg.record_recent(id));
        refill_smart(false);
        save();
        return;
    }
    let target = with(|s| {
        let entry = s.catalog.get(id)?;
        let target = match &entry.source {
            AppSource::Shell(pn) => Target::Shell(pn.clone()),
            // A package manager's command-line tool opens in a console that
            // stays open (it would otherwise flash and close).
            AppSource::File(path) if matches!(entry.kind, AppKind::Package(_)) && is_console_program(path) => {
                match crate::pkgsources::console_args(path) {
                    Some(args) => {
                        Target::Custom { target: "%ComSpec%".into(), args, dir: "%USERPROFILE%".into(), admin: false }
                    }
                    // A path cmd can't be given safely: start it directly.
                    None => {
                        Target::Custom { target: path.clone(), args: String::new(), dir: String::new(), admin: false }
                    }
                }
            }
            AppSource::File(path) => {
                Target::Custom { target: path.clone(), args: String::new(), dir: String::new(), admin: false }
            }
            AppSource::Custom => {
                let c = s.cfg.custom_app(id)?;
                Target::Custom {
                    target: c.target.clone(),
                    args: c.args.clone(),
                    dir: c.working_dir.clone(),
                    admin: c.run_as_admin,
                }
            }
        };
        s.cfg.record_recent(id);
        Some(target)
    });
    let Some(target) = target else {
        ui::warn(None, "That app is no longer installed. Use Rescan apps, or remove it from the category.");
        return;
    };
    refill_smart(false);
    save();
    super::running::launched(main_hwnd());
    let main_raw = main_hwnd().0 as isize;
    launch::spawn(target, move |msg| unsafe {
        let ptr = Box::into_raw(Box::new(msg));
        let hwnd = HWND(main_raw as *mut _);
        if windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(hwnd),
            WM_APP_ERROR,
            WPARAM(0),
            LPARAM(ptr as isize),
        )
        .is_err()
        {
            drop(Box::from_raw(ptr));
        }
    });
}

// ---------------------------------------------------------------- scanning

/// Rescans now, or as soon as the scan in progress has finished.
pub fn request_rescan() {
    if with(|s| s.scanning) {
        with(|s| s.rescan_pending = true);
    } else {
        start_scan();
    }
}

pub fn start_scan() {
    let already = with(|s| std::mem::replace(&mut s.scanning, true));
    if already {
        return;
    }
    let main_raw = main_hwnd().0 as isize;
    let packages = with(|s| s.cfg.settings.package_apps);
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let result = catalog::scan(packages).map_err(|e| e.message());
        let ptr = Box::into_raw(Box::new(result));
        unsafe {
            let hwnd = HWND(main_raw as *mut _);
            if windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(hwnd),
                WM_APP_SCAN_DONE,
                WPARAM(0),
                LPARAM(ptr as isize),
            )
            .is_err()
            {
                drop(Box::from_raw(ptr));
            }
        }
    });
    manager::scan_state_changed();
}

fn finish_scan(result: Result<Vec<ShellApp>, String>) {
    match result {
        Ok(apps) => {
            catalog::save_cache(&paths::get().apps_cache(), &apps);
            with(|s| {
                s.scanning = false;
                s.shell_apps = apps;
                s.catalog = Catalog::build(&s.shell_apps, &s.cfg.custom_apps);
            });
            if refill_smart(true) {
                save();
            }
            request_all_icons();
        }
        Err(e) => {
            with(|s| s.scanning = false);
            supervisor::log(&format!("app scan failed: {e}"));
        }
    }
    searchwin::catalog_changed();
    manager::catalog_changed();
    manager::scan_state_changed();
    strip::refresh();
}

// ---------------------------------------------------------------- icons

pub fn icon(key: &str) -> Option<HBITMAP> {
    with(|s| s.icons.get(key).copied())
}

fn icon_source(s: &State, app_id: &str) -> Option<IconSource> {
    if let Some(file) = s.cfg.app_icons.get(app_id) {
        return paths::get().icon_file(file).map(IconSource::Image);
    }
    match &s.catalog.get(app_id)?.source {
        AppSource::Shell(pn) => Some(IconSource::Shell(pn.clone())),
        AppSource::File(path) => Some(IconSource::Path(path.clone())),
        AppSource::Custom => {
            let c = s.cfg.custom_app(app_id)?;
            Some(IconSource::Path(c.target.clone()))
        }
    }
}

/// Icon source for any cache key: an app id, `cat:<id>` (custom image or the
/// folder icon), or [`FOLDER_ICON`].
pub fn icon_source_for(s: &State, key: &str) -> Option<IconSource> {
    // A file or folder shown in a pinned folder's flyout.
    if let Some(path) = key.strip_prefix("path:") {
        return Some(IconSource::Path(path.to_string()));
    }
    if key == FOLDER_ICON {
        return Some(IconSource::Path(paths::get().data.display().to_string()));
    }
    if let Some(cat) = key.strip_prefix("cat:") {
        let id: u64 = cat.parse().ok()?;
        return match crate::tree::find(&s.cfg.categories, id)?.icon.as_ref() {
            Some(f) => paths::get().icon_file(f).map(IconSource::Image),
            None => icon_source_for(s, FOLDER_ICON),
        };
    }
    icon_source(s, key)
}

/// Starts background loading for every app and category icon not yet cached.
pub fn request_all_icons() {
    let (requests, size, main) = with(|s| {
        let mut req: Vec<(String, IconSource)> = Vec::new();
        for a in &s.catalog.apps {
            if !s.icons.contains_key(&a.id)
                && let Some(src) = icon_source(s, &a.id)
            {
                req.push((a.id.clone(), src));
            }
        }
        crate::tree::walk(&s.cfg.categories, &mut |c, _| {
            let key = format!("cat:{}", c.id);
            if let Some(file) = &c.icon
                && !s.icons.contains_key(&key)
                && let Some(path) = paths::get().icon_file(file)
            {
                req.push((key, IconSource::Image(path)));
            }
        });
        // Recently used apps first: they're the likeliest to be shown next.
        let recents = s.cfg.recents.clone();
        req.sort_by_key(|(k, _)| recents.iter().position(|r| r == k).unwrap_or(usize::MAX));
        (req, s.icon_size, s.main)
    });
    if !requests.is_empty() {
        icons::load_async(requests, size, main, WM_APP_ICONS);
    }
}

fn load_folder_icon() {
    let size = with(|s| s.icon_size);
    let folder = paths::get().data.display().to_string();
    if let Some(bmp) = icons::load(&IconSource::Path(folder), size) {
        with(|s| s.icons.insert(FOLDER_ICON.to_string(), bmp));
    }
}

/// Reloads one icon right away (after the user picked or cleared a custom one).
/// Safe to replace here: this only runs from the manager, never while a menu is open.
pub fn reload_icon(key: &str) {
    let (source, size) = with(|s| {
        let src = if let Some(cat) = key.strip_prefix("cat:") {
            let id: u64 = cat.parse().unwrap_or(0);
            crate::tree::find(&s.cfg.categories, id)
                .and_then(|c| c.icon.clone())
                .and_then(|f| paths::get().icon_file(&f))
                .map(IconSource::Image)
        } else {
            icon_source(s, key)
        };
        (src, s.icon_size)
    });
    let bmp = source.and_then(|src| icons::load(&src, size));
    with(|s| {
        let old = match bmp {
            Some(b) => s.icons.insert(key.to_string(), b),
            None => s.icons.remove(key),
        };
        if let Some(old) = old {
            icons::free(old);
        }
    });
    let keys = vec![key.to_string()];
    searchwin::icons_changed(&keys);
    manager::icons_changed(&keys);
    strip::icon_changed(key);
}

// ---------------------------------------------------------------- hotkeys

/// (Re)registers the hotkeys. Returns a message per hotkey that another
/// program already owns.
pub fn register_hotkeys() -> Vec<String> {
    let (main, search, menu, bar, next) = with(|s| {
        let st = &s.cfg.settings;
        // With only one bar there is nothing to switch to: leave the keys
        // to other programs until a second bar is made.
        let several = crate::bars::names(&s.cfg).len() > 1;
        (s.main, st.search_hotkey, st.menu_hotkey, st.bar_hotkey, st.next_bar_hotkey.filter(|_| several))
    });
    let mut problems = Vec::new();
    for (id, hk, what) in [
        (HOTKEY_SEARCH, search, "search"),
        (HOTKEY_MENU, menu, "menu"),
        (HOTKEY_BAR, bar, "bar"),
        (HOTKEY_NEXT_BAR, next, "next bar"),
    ] {
        unsafe {
            let _ = UnregisterHotKey(Some(main), id);
        }
        if let Some(hk) = hk
            && !register(main, id, hk)
        {
            problems.push(format!("The {what} hotkey {} is already used by another program.", hk.describe()));
        }
    }
    // Each category with a hotkey, at any depth.
    for (hid, _) in CATEGORY_HOTKEYS.with(|c| std::mem::take(&mut *c.borrow_mut())) {
        unsafe {
            let _ = UnregisterHotKey(Some(main), hid);
        }
    }
    let mut wanted: Vec<(u64, String, Hotkey)> = Vec::new();
    with(|s| {
        crate::tree::walk(&s.cfg.categories, &mut |c, _| {
            if let Some(hk) = c.hotkey {
                wanted.push((c.id, c.name.clone(), hk));
            }
        })
    });
    for (i, (cat, name, hk)) in wanted.into_iter().enumerate() {
        let hid = HOTKEY_CATEGORY + i as i32;
        if register(main, hid, hk) {
            CATEGORY_HOTKEYS.with(|c| c.borrow_mut().push((hid, cat)));
        } else {
            problems.push(format!("The hotkey {} for “{name}” is already used by another program.", hk.describe()));
        }
    }
    problems
}

/// A category's hotkey: its flyout when it has a button on the bar, or
/// else its menu at the pointer.
fn open_category_by_hotkey(id: u64) {
    if let Some((hit, _)) = strip::flyout_buttons().into_iter().find(|(_, o)| *o == strip::Opens::Category(id)) {
        super::flyout::keyboard_open_category(hit, id);
        return;
    }
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    if let Some(action) = menu::track_category(main_hwnd(), pt, id) {
        perform(action);
    }
}

fn register(hwnd: HWND, id: i32, hk: Hotkey) -> bool {
    unsafe { RegisterHotKey(Some(hwnd), id, HOT_KEY_MODIFIERS(hk.modifiers) | MOD_NOREPEAT, hk.key).is_ok() }
}

// ---------------------------------------------------------------- tray

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    nid
}

fn copy_into(dst: &mut [u16], text: &str) {
    let w: Vec<u16> = text.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

fn add_tray_icon(hwnd: HWND) {
    let size = with(|s| s.icon_size);
    let mut nid = tray_data(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_APP_TRAY;
    nid.hIcon = app_icon(size);
    copy_into(&mut nid.szTip, "FlexTaskbar");
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

fn remove_tray_icon(hwnd: HWND) {
    let nid = tray_data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

/// Shows a tray notification.
pub fn balloon(text: &str, warning: bool) {
    let mut nid = tray_data(main_hwnd());
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO } | NOTIFY_ICON_INFOTIP_FLAGS(0);
    copy_into(&mut nid.szInfoTitle, "FlexTaskbar");
    copy_into(&mut nid.szInfo, text);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}
