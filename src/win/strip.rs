//! The icon strip: a horizontal bar docked against the Windows taskbar (just
//! above it, or below it when the taskbar is at the top of the screen).
//!
//! Left to right: the FlexTaskbar button (the full menu), then every root
//! category and every pinned app, centered like the Windows 11 taskbar.
//!
//! - Resting the pointer on a category opens its contents upward as a native
//!   menu; subcategories cascade on hover, at any depth.
//! - While a menu is open, moving along the strip switches to the category
//!   under the pointer, the way a menu bar works.
//! - Clicking a pinned app launches it.
//!
//! By default the strip reserves its screen space as an AppBar, so maximized
//! windows stop above it. It hides while a full-screen app is active.

use super::app;
use super::icons::{self, Source as IconSource};
use super::menu;
use super::theme;
use super::ui::{self, scale, wide};
use crate::config::CustomApp;
use crate::striplayout::{self, Layout};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Instant;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BLENDFUNCTION, BeginPaint, BitBlt, ClientToScreen, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateSolidBrush, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW,
    EndPaint, FillRect, GetMonitorInfoW, GetStockObject, HBITMAP, HGDIOBJ, InvalidateRect, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFO, MonitorFromPoint, NULL_PEN, PAINTSTRUCT, RoundRect, SRCCOPY, ScreenToClient, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Controls::{
    TTF_SUBCLASS, TTM_ADDTOOLW, TTM_DELTOOLW, TTM_POP, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_TOP, ABM_GETTASKBARPOS, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABN_FULLSCREENAPP,
    ABN_POSCHANGED, ABN_STATECHANGE, APPBARDATA, DragAcceptFiles, DragFinish, DragQueryFileW, HDROP, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreatePopupMenu, CreateWindowExW, DI_NORMAL, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, DrawIconEx, EndMenu, GetClientRect, GetCursorPos, GetWindowRect, HICON, HWND_TOPMOST, InsertMenuW,
    KillTimer, MA_NOACTIVATE, MF_BYPOSITION, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, MSGF_MENU, PostMessageW,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SendMessageW, SetForegroundWindow, SetTimer,
    SetWindowPos, SetWindowsHookExW, ShowWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TPM_TOPALIGN, TPM_VERNEGANIMATION, TPM_VERPOSANIMATION, TPMPARAMS, TrackPopupMenuEx, UnhookWindowsHookEx,
    WH_MSGFILTER, WINDOW_STYLE, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_DROPFILES, WM_ERASEBKGND, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NULL, WM_PAINT, WM_RBUTTONUP, WM_SIZE, WM_TIMER, WNDCLASSW, WS_EX_ACCEPTFILES,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, PWSTR, w};

const WM_APP_APPBAR: u32 = WM_APP + 20;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_HOVER: usize = 1;

/// A strip button: `None` is the FlexTaskbar button, `Some(i)` is item `i`.
type Button = Option<usize>;

#[derive(Clone, PartialEq)]
enum Item {
    Category(u64),
    App(String),
}

struct Strip {
    hwnd: HWND,
    tooltip: HWND,
    items: Vec<Item>,
    names: Vec<String>,
    layout: Layout,
    /// Strip-sized icons, keyed like the app's icon cache.
    icons: HashMap<String, Option<HBITMAP>>,
    launcher_icon: HICON,
    icon_size: i32,
    dpi: u32,
    hover: Option<Button>,
    /// The button whose menu is open.
    open: Option<Button>,
    /// Hovering this button won't open it again until the pointer leaves it
    /// (set right after its menu closed).
    suppress: Option<Button>,
    closed_at: Option<(Button, Instant)>,
    tracking_leave: bool,
    reserved: bool,
    edge: u32,
    dark: bool,
    tools: usize,
}

thread_local! {
    static STRIP: RefCell<Option<Strip>> = const { RefCell::new(None) };
    static REPOSITIONING: Cell<bool> = const { Cell::new(false) };
    // Menu-switching state, read by the message-filter hook while a menu is open.
    static HOOK_CURRENT: Cell<Option<Button>> = const { Cell::new(None) };
    static HOOK_SWITCH: Cell<Option<Button>> = const { Cell::new(None) };
}

fn hwnd() -> Option<HWND> {
    STRIP.with(|s| s.borrow().as_ref().map(|s| s.hwnd))
}

fn with<R>(f: impl FnOnce(&mut Strip) -> R) -> Option<R> {
    STRIP.with(|s| s.borrow_mut().as_mut().map(f))
}

// ---------------------------------------------------------------- lifecycle

/// Creates or removes the strip to match the settings.
pub fn apply_settings() {
    let (show, reserve) = app::with(|s| (s.cfg.settings.show_strip, s.cfg.settings.reserve_space));
    match (show, hwnd().is_some()) {
        (true, false) => create(),
        (false, true) => destroy(),
        (true, true) => {
            with(|s| s.reserved = reserve);
            reposition();
        }
        (false, false) => {}
    }
}

fn create() {
    let class = wide("FlexTaskbar.Strip");
    let hwnd = unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: app::instance(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                None,
                windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
            )
            .unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        // Start on the primary monitor so the window gets that monitor's DPI.
        let wa = primary_monitor().1;
        match CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_ACCEPTFILES,
            PCWSTR(class.as_ptr()),
            w!("FlexTaskbar strip"),
            WS_POPUP,
            wa.left,
            wa.bottom - 48,
            ui::rect_w(&wa),
            48,
            None,
            None,
            Some(app::instance()),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        }
    };
    let tooltip = ui::child_popup(hwnd, "tooltips_class32", WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX));
    let dpi = ui::dpi_of(hwnd);
    let reserve = app::with(|s| s.cfg.settings.reserve_space);
    let icon_size = scale(24, dpi);
    STRIP.with(|s| {
        *s.borrow_mut() = Some(Strip {
            hwnd,
            tooltip,
            items: Vec::new(),
            names: Vec::new(),
            layout: striplayout::layout(0, 0, 1, 0, 0),
            icons: HashMap::new(),
            launcher_icon: app::app_icon(icon_size),
            icon_size,
            dpi,
            hover: None,
            open: None,
            suppress: None,
            closed_at: None,
            tracking_leave: false,
            reserved: reserve,
            edge: ABE_BOTTOM,
            dark: theme::is_dark(),
            tools: 0,
        })
    });
    unsafe {
        DragAcceptFiles(hwnd, true);
        let mut abd = appbar_data(hwnd);
        SHAppBarMessage(ABM_NEW, &mut abd);
    }
    refresh();
    reposition();
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

/// Removes the strip, releasing its reserved screen space.
pub fn destroy() {
    let taken = STRIP.with(|s| s.borrow_mut().take());
    if let Some(s) = taken {
        unsafe {
            let mut abd = appbar_data(s.hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut abd);
            let _ = DestroyWindow(s.hwnd);
            let _ = DestroyIcon(s.launcher_icon);
        }
        for bmp in s.icons.into_values().flatten() {
            icons::free(bmp);
        }
    }
}

/// Explorer restarted: its AppBar list is gone, so register again.
pub fn shell_restarted() {
    if let Some(h) = hwnd() {
        unsafe {
            let mut abd = appbar_data(h);
            SHAppBarMessage(ABM_NEW, &mut abd);
        }
        reposition();
    }
}

pub fn theme_changed() {
    if with(|s| s.dark = theme::is_dark()).is_some() {
        invalidate();
    }
}

/// An icon was replaced (custom icon picked or reset).
pub fn icon_changed(key: &str) {
    let old = with(|s| s.icons.remove(key)).flatten().flatten();
    if let Some(bmp) = old {
        icons::free(bmp);
    }
    refresh();
}

// ---------------------------------------------------------------- items & layout

/// Rebuilds the buttons from the configuration (root categories, then pinned
/// apps that still exist).
pub fn refresh() {
    if hwnd().is_none() {
        return;
    }
    let (items, names, sources) = app::with(|s| {
        let mut items = Vec::new();
        let mut names = Vec::new();
        for c in &s.cfg.categories {
            items.push(Item::Category(c.id));
            names.push(c.name.clone());
        }
        for id in &s.cfg.pinned {
            if let Some(a) = s.catalog.get(id) {
                items.push(Item::App(id.clone()));
                names.push(a.name.clone());
            }
        }
        let sources: Vec<(String, Option<IconSource>)> = items
            .iter()
            .map(|it| {
                let key = key_of(it);
                let src = app::icon_source_for(s, &key);
                (key, src)
            })
            .collect();
        (items, names, sources)
    });
    // Load any icons not cached yet (a handful, from Windows' own icon cache).
    let (size, missing): (i32, Vec<(String, Option<IconSource>)>) = with(|st| {
        let missing = sources.into_iter().filter(|(k, _)| !st.icons.contains_key(k)).collect();
        (st.icon_size, missing)
    })
    .unwrap_or_default();
    let loaded: Vec<(String, Option<HBITMAP>)> =
        missing.into_iter().map(|(k, src)| (k, src.and_then(|src| icons::load(&src, size)))).collect();
    with(|st| {
        for (k, b) in loaded {
            st.icons.insert(k, b);
        }
        st.items = items;
        st.names = names;
    });
    relayout();
}

fn key_of(item: &Item) -> String {
    match item {
        Item::Category(id) => format!("cat:{id}"),
        Item::App(id) => id.clone(),
    }
}

fn relayout() {
    let Some(h) = hwnd() else { return };
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(h, &mut rc);
    }
    let tools = with(|s| {
        let d = s.dpi;
        s.layout = striplayout::layout(rc.right, scale(6, d), scale(44, d), scale(4, d), s.items.len());
        let mut tools = vec![("FlexTaskbar".to_string(), s.layout.launcher)];
        // Categories open on hover, so only apps get a name tooltip.
        for (i, slot) in s.layout.items.iter().enumerate() {
            if matches!(s.items[i], Item::App(_)) {
                tools.push((s.names[i].clone(), *slot));
            }
        }
        (s.tooltip, std::mem::replace(&mut s.tools, tools.len()), tools)
    });
    if let Some((tooltip, old_count, tools)) = tools {
        // Tooltips: one tool per button, rebuilt with the layout.
        unsafe {
            for id in 0..old_count {
                let ti = TTTOOLINFOW {
                    cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                    hwnd: h,
                    uId: id + 1,
                    ..Default::default()
                };
                SendMessageW(tooltip, TTM_DELTOOLW, Some(WPARAM(0)), Some(LPARAM(&ti as *const _ as isize)));
            }
            for (id, (name, slot)) in tools.iter().enumerate() {
                let mut text = wide(name);
                let ti = TTTOOLINFOW {
                    cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                    uFlags: TTF_SUBCLASS,
                    hwnd: h,
                    uId: id + 1,
                    rect: RECT { left: slot.x, top: 0, right: slot.x + slot.w, bottom: rc.bottom },
                    lpszText: PWSTR(text.as_mut_ptr()),
                    ..Default::default()
                };
                SendMessageW(tooltip, TTM_ADDTOOLW, Some(WPARAM(0)), Some(LPARAM(&ti as *const _ as isize)));
            }
        }
    }
    invalidate();
}

/// Tooltips off while a menu is open (so a name tip can't sit on top of the
/// menu), back on afterwards. Removing the tools is more reliable than
/// TTM_ACTIVATE across tooltip implementations.
fn tooltips(on: bool) {
    if on {
        relayout(); // re-adds the tools
        return;
    }
    let Some((h, tip, count)) = with(|s| (s.hwnd, s.tooltip, std::mem::replace(&mut s.tools, 0))) else { return };
    unsafe {
        SendMessageW(tip, TTM_POP, Some(WPARAM(0)), Some(LPARAM(0)));
        for id in 0..count {
            let ti = TTTOOLINFOW {
                cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                hwnd: h,
                uId: id + 1,
                ..Default::default()
            };
            SendMessageW(tip, TTM_DELTOOLW, Some(WPARAM(0)), Some(LPARAM(&ti as *const _ as isize)));
        }
    }
}

fn invalidate() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = InvalidateRect(Some(h), None, false);
        }
    }
}

fn button_at(x: i32) -> Option<Button> {
    STRIP.with(|s| s.borrow().as_ref().and_then(|s| striplayout::hit(&s.layout, x)))
}

fn is_menu_button(b: Button) -> bool {
    match b {
        None => true,
        Some(i) => {
            STRIP.with(|s| s.borrow().as_ref().is_some_and(|s| matches!(s.items.get(i), Some(Item::Category(_)))))
        }
    }
}

// ---------------------------------------------------------------- docking

fn appbar_data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: WM_APP_APPBAR,
        ..Default::default()
    }
}

/// (monitor rect, work area) of the primary monitor.
fn primary_monitor() -> (RECT, RECT) {
    unsafe {
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        (mi.rcMonitor, mi.rcWork)
    }
}

/// Docks the strip against the Windows taskbar's edge of the primary monitor.
fn reposition() {
    let Some(h) = hwnd() else { return };
    if REPOSITIONING.with(|r| r.replace(true)) {
        return; // our own ABM_SETPOS can notify us again
    }
    let (height_dip, reserve) =
        app::with(|s| (s.cfg.settings.strip_height.clamp(24, 96), s.cfg.settings.reserve_space));
    let dpi = ui::dpi_of(h);
    let height = scale(height_dip as i32, dpi);
    let (monitor, work) = primary_monitor();
    let mut abd = appbar_data(h);
    // The Windows taskbar's edge; a vertical taskbar still gets a bottom strip.
    let taskbar_edge = unsafe {
        let mut tb = appbar_data(h);
        if SHAppBarMessage(ABM_GETTASKBARPOS, &mut tb) != 0 { tb.uEdge } else { ABE_BOTTOM }
    };
    let edge = if taskbar_edge == ABE_TOP { ABE_TOP } else { ABE_BOTTOM };

    let was_reserved = with(|s| std::mem::replace(&mut s.reserved, reserve)).unwrap_or(false);
    let rect = if reserve {
        abd.uEdge = edge;
        abd.rc = monitor;
        if edge == ABE_TOP {
            abd.rc.bottom = abd.rc.top + height;
        } else {
            abd.rc.top = abd.rc.bottom - height;
        }
        unsafe {
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            // QUERYPOS moved the proposed edge past the taskbar; restore our height.
            if edge == ABE_TOP {
                abd.rc.bottom = abd.rc.top + height;
            } else {
                abd.rc.top = abd.rc.bottom - height;
            }
            SHAppBarMessage(ABM_SETPOS, &mut abd);
        }
        abd.rc
    } else {
        if was_reserved {
            // Give the reserved space back: re-register without a position.
            unsafe {
                SHAppBarMessage(ABM_REMOVE, &mut abd);
                SHAppBarMessage(ABM_NEW, &mut abd);
            }
        }
        let (_, work) = if was_reserved { primary_monitor() } else { (monitor, work) };
        if edge == ABE_TOP {
            RECT { bottom: work.top + height, ..work }
        } else {
            RECT { top: work.bottom - height, ..work }
        }
    };
    let icon_size = scale(24, dpi);
    let size_changed = with(|s| {
        s.edge = edge;
        s.dpi = dpi;
        std::mem::replace(&mut s.icon_size, icon_size) != icon_size
    })
    .unwrap_or(false);
    if size_changed {
        // DPI changed: icons must be reloaded at the new size.
        let old: Vec<HBITMAP> = with(|s| s.icons.drain().filter_map(|(_, b)| b).collect()).unwrap_or_default();
        for b in old {
            icons::free(b);
        }
    }
    unsafe {
        let _ = SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            rect.left,
            rect.top,
            ui::rect_w(&rect),
            ui::rect_h(&rect),
            SWP_NOACTIVATE,
        );
    }
    REPOSITIONING.with(|r| r.set(false));
    if size_changed {
        refresh();
    } else {
        relayout();
    }
}

// ---------------------------------------------------------------- painting

fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    STRIP.with(|cell| {
        let b = cell.borrow();
        let Some(s) = b.as_ref() else { return };
        unsafe {
            let mem = CreateCompatibleDC(Some(hdc));
            let bmp = CreateCompatibleBitmap(hdc, rc.right.max(1), rc.bottom.max(1));
            let old = SelectObject(mem, HGDIOBJ(bmp.0));

            let (bg, line, hl, text) = if s.dark {
                (theme::rgb(28, 28, 28), theme::rgb(58, 58, 58), theme::rgb(58, 58, 58), theme::rgb(240, 240, 240))
            } else {
                (
                    theme::rgb(238, 240, 243),
                    theme::rgb(210, 212, 216),
                    theme::rgb(222, 225, 230),
                    theme::rgb(30, 30, 30),
                )
            };
            let brush = CreateSolidBrush(bg);
            FillRect(mem, &rc, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            // Hairline on the side facing the desktop.
            let line_brush = CreateSolidBrush(line);
            let edge_rc = if s.edge == ABE_TOP { RECT { top: rc.bottom - 1, ..rc } } else { RECT { bottom: 1, ..rc } };
            FillRect(mem, &edge_rc, line_brush);
            let _ = DeleteObject(HGDIOBJ(line_brush.0));

            let pad = scale(5, s.dpi);
            let radius = scale(8, s.dpi);
            let draw_highlight = |slot: &striplayout::Slot| {
                let hb = CreateSolidBrush(hl);
                let old_b = SelectObject(mem, HGDIOBJ(hb.0));
                let old_p = SelectObject(mem, GetStockObject(NULL_PEN));
                let _ = RoundRect(mem, slot.x, pad, slot.x + slot.w, rc.bottom - pad + 1, radius, radius);
                SelectObject(mem, old_p);
                SelectObject(mem, old_b);
                let _ = DeleteObject(HGDIOBJ(hb.0));
            };
            let active = |b: Button| s.hover == Some(b) || s.open == Some(b);

            let size = s.icon_size;
            let y = (rc.bottom - size) / 2;
            let launcher = s.layout.launcher;
            if active(None) {
                draw_highlight(&launcher);
            }
            let _ = DrawIconEx(
                mem,
                launcher.x + (launcher.w - size) / 2,
                y,
                s.launcher_icon,
                size,
                size,
                0,
                None,
                DI_NORMAL,
            );

            let src = CreateCompatibleDC(Some(mem));
            for (i, slot) in s.layout.items.iter().enumerate() {
                if active(Some(i)) {
                    draw_highlight(slot);
                }
                let x = slot.x + (slot.w - size) / 2;
                match s.icons.get(&key_of(&s.items[i])).copied().flatten() {
                    Some(icon) => {
                        let prev = SelectObject(src, HGDIOBJ(icon.0));
                        let blend = BLENDFUNCTION {
                            BlendOp: AC_SRC_OVER as u8,
                            BlendFlags: 0,
                            SourceConstantAlpha: 255,
                            AlphaFormat: AC_SRC_ALPHA as u8,
                        };
                        let _ = AlphaBlend(mem, x, y, size, size, src, 0, 0, size, size, blend);
                        SelectObject(src, prev);
                    }
                    None => {
                        // No icon: show the first letter instead of an empty slot.
                        let letter: String =
                            s.names[i].chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                        let mut w = wide(&letter);
                        let len = w.len() - 1;
                        let mut r = RECT { left: x, top: y, right: x + size, bottom: y + size };
                        SetBkMode(mem, TRANSPARENT);
                        SetTextColor(mem, text);
                        DrawTextW(mem, &mut w[..len], &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                    }
                }
            }
            let _ = DeleteDC(src);

            let _ = BitBlt(hdc, 0, 0, rc.right, rc.bottom, Some(mem), 0, 0, SRCCOPY);
            SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem);
        }
    });
    unsafe {
        let _ = EndPaint(hwnd, &ps);
    }
}

// ---------------------------------------------------------------- menus

fn button_screen_rect(b: Button) -> Option<RECT> {
    let (h, slot) = STRIP.with(|s| {
        let st = s.borrow();
        let st = st.as_ref()?;
        let slot = match b {
            None => st.layout.launcher,
            Some(i) => *st.layout.items.get(i)?,
        };
        Some((st.hwnd, slot))
    })?;
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(h, &mut rc);
        let mut tl = POINT { x: slot.x, y: 0 };
        let mut br = POINT { x: slot.x + slot.w, y: rc.bottom };
        let _ = ClientToScreen(h, &mut tl);
        let _ = ClientToScreen(h, &mut br);
        Some(RECT { left: tl.x, top: tl.y, right: br.x, bottom: br.y })
    }
}

/// Opens a button's menu. While it is open, moving the pointer onto another
/// category (or the FlexTaskbar button) switches to that one's menu.
fn open_menu(first: Button) {
    let Some(strip_hwnd) = hwnd() else { return };
    let owner = app::main_hwnd();
    let mut current = first;
    loop {
        let built = match current {
            None => Some(menu::build_main()),
            Some(i) => match STRIP.with(|s| s.borrow().as_ref().and_then(|s| s.items.get(i).cloned())) {
                Some(Item::Category(id)) => menu::build_category(id),
                _ => None,
            },
        };
        let (Some(built), Some(btn)) = (built, button_screen_rect(current)) else { return };
        let mut strip_rc = RECT::default();
        unsafe {
            let _ = GetWindowRect(strip_hwnd, &mut strip_rc);
        }
        let top_edge = with(|s| s.edge == ABE_TOP).unwrap_or(false);
        let (y, flags) = if top_edge {
            (strip_rc.bottom, TPM_TOPALIGN | TPM_VERPOSANIMATION)
        } else {
            (strip_rc.top, TPM_BOTTOMALIGN | TPM_VERNEGANIMATION)
        };
        let params = TPMPARAMS { cbSize: std::mem::size_of::<TPMPARAMS>() as u32, rcExclude: strip_rc };

        tooltips(false);
        with(|s| s.open = Some(current));
        invalidate();
        HOOK_CURRENT.with(|c| c.set(Some(current)));
        HOOK_SWITCH.with(|c| c.set(None));
        let chosen = unsafe {
            let hook = SetWindowsHookExW(WH_MSGFILTER, Some(menu_filter), None, GetCurrentThreadId()).ok();
            let _ = SetForegroundWindow(owner);
            let id = TrackPopupMenuEx(
                built.menu,
                (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | flags).0,
                btn.left,
                y,
                owner,
                Some(&params),
            )
            .0 as usize;
            if let Some(hook) = hook {
                let _ = UnhookWindowsHookEx(hook);
            }
            let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
            id
        };
        HOOK_CURRENT.with(|c| c.set(None));
        tooltips(true);
        let switch = HOOK_SWITCH.with(|c| c.take());
        with(|s| {
            s.open = None;
            s.closed_at = Some((current, Instant::now()));
        });
        invalidate();

        if let Some(action) = built.finish(chosen) {
            app::perform(action);
            return;
        }
        match switch {
            Some(next) => current = next,
            None => {
                // Closed without choosing: don't pop the same menu straight back
                // open while the pointer is still resting on its button.
                let under = cursor_button();
                with(|s| {
                    s.suppress = under;
                    s.hover = under;
                });
                invalidate();
                return;
            }
        }
    }
}

fn cursor_button() -> Option<Button> {
    let h = hwnd()?;
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = ScreenToClient(h, &mut pt);
        let mut rc = RECT::default();
        let _ = GetClientRect(h, &mut rc);
        if pt.y < 0 || pt.y >= rc.bottom || pt.x < 0 || pt.x >= rc.right {
            return None;
        }
        button_at(pt.x)
    }
}

/// While a strip menu is open: if the pointer moves onto another menu button,
/// close this menu and remember which one to open next.
unsafe extern "system" fn menu_filter(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == MSGF_MENU as i32 && lparam.0 != 0 {
        let msg = unsafe { &*(lparam.0 as *const MSG) };
        if msg.message == WM_MOUSEMOVE
            && let (Some(h), Some(current)) = (hwnd(), HOOK_CURRENT.with(|c| c.get()))
        {
            let mut pt = msg.pt;
            unsafe {
                let _ = ScreenToClient(h, &mut pt);
            }
            let mut rc = RECT::default();
            unsafe {
                let _ = GetClientRect(h, &mut rc);
            }
            if pt.y >= 0
                && pt.y < rc.bottom
                && let Some(b) = button_at(pt.x)
                && b != current
                && is_menu_button(b)
            {
                HOOK_SWITCH.with(|c| c.set(Some(b)));
                unsafe {
                    let _ = EndMenu();
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Right-click menu for one button.
fn context_menu(b: Button) {
    let item = match b {
        None => None,
        Some(i) => STRIP.with(|s| s.borrow().as_ref().and_then(|s| s.items.get(i).cloned())),
    };
    let Some(item) = item else {
        open_menu(None); // the FlexTaskbar button or empty strip: the full menu
        return;
    };
    const LEFT: usize = 1;
    const RIGHT: usize = 2;
    const UNPIN: usize = 3;
    const MANAGE: usize = 4;
    const HIDE: usize = 5;
    let (can_left, can_right) = app::with(|s| match &item {
        Item::Category(id) => {
            let pos = s.cfg.categories.iter().position(|c| c.id == *id).unwrap_or(0);
            (pos > 0, pos + 1 < s.cfg.categories.len())
        }
        Item::App(id) => {
            let pos = s.cfg.pinned.iter().position(|p| p == id).unwrap_or(0);
            (pos > 0, pos + 1 < s.cfg.pinned.len())
        }
    });
    let chosen = unsafe {
        let m = CreatePopupMenu().unwrap_or_default();
        let add = |pos: u32, id: usize, text: &str, enabled: bool| {
            let t = wide(text);
            let flags = if enabled { MF_BYPOSITION | MF_STRING } else { MF_BYPOSITION | MF_STRING | MF_GRAYED };
            let _ = InsertMenuW(m, pos, flags, id, PCWSTR(t.as_ptr()));
        };
        add(0, LEFT, "Move left", can_left);
        add(1, RIGHT, "Move right", can_right);
        let _ = InsertMenuW(m, 2, MF_BYPOSITION | MF_SEPARATOR, 0, PCWSTR::null());
        let mut pos = 3;
        if matches!(item, Item::App(_)) {
            add(pos, UNPIN, "Unpin from strip", true);
            pos += 1;
        }
        add(pos, MANAGE, "Manage categories…", true);
        add(pos + 1, HIDE, "Hide icon strip", true);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let owner = app::main_hwnd();
        tooltips(false);
        let _ = SetForegroundWindow(owner);
        let id = TrackPopupMenuEx(m, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN).0, pt.x, pt.y, owner, None).0
            as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        tooltips(true);
        id
    };
    match chosen {
        LEFT | RIGHT => {
            let delta = if chosen == LEFT { -1 } else { 1 };
            app::with(|s| match &item {
                Item::Category(id) => crate::tree::move_sibling(&mut s.cfg.categories, *id, delta),
                Item::App(id) => s.cfg.move_pinned(id, delta),
            });
            app::save();
        }
        UNPIN => {
            if let Item::App(id) = &item {
                app::with(|s| s.cfg.unpin(id));
                app::save();
            }
        }
        MANAGE => super::manager::show(),
        HIDE => app::perform(menu::Action::ToggleStrip),
        _ => {}
    }
}

/// Files dropped on the strip become custom apps pinned to it.
fn on_drop(drop: HDROP) {
    let mut files = Vec::new();
    unsafe {
        let count = DragQueryFileW(drop, u32::MAX, None);
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(drop, i, Some(&mut buf));
            files.push(std::path::PathBuf::from(ui::from_wide(&buf)));
        }
        DragFinish(drop);
    }
    if files.is_empty() {
        return;
    }
    app::with(|s| {
        for f in &files {
            let id = format!("custom:{}", s.cfg.alloc_id());
            let name =
                f.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.display().to_string());
            s.cfg.custom_apps.push(CustomApp {
                id: id.clone(),
                name,
                target: f.display().to_string(),
                ..Default::default()
            });
            s.cfg.pin(&id);
        }
    });
    app::rebuild_catalog();
    app::request_all_icons();
    app::save();
    super::searchwin::catalog_changed();
    super::manager::catalog_changed();
}

// ---------------------------------------------------------------- window proc

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            relayout();
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let under = button_at(x);
            let (changed, start_leave, suppressed) = with(|s| {
                let changed = s.hover != under;
                s.hover = under;
                if s.suppress.is_some() && s.suppress != under {
                    s.suppress = None;
                }
                let start_leave = !std::mem::replace(&mut s.tracking_leave, true);
                (changed, start_leave, s.suppress == under && under.is_some())
            })
            .unwrap_or((false, false, false));
            if start_leave {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
            }
            if changed {
                invalidate();
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER_HOVER);
                }
                // Categories open on hover; the FlexTaskbar button and apps need a click.
                if let Some(Some(i)) = under
                    && !suppressed
                    && is_menu_button(Some(i))
                {
                    let ms = app::with(|s| s.cfg.settings.hover_delay_ms);
                    unsafe {
                        SetTimer(Some(hwnd), TIMER_HOVER, ms.max(1), None);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|s| {
                s.hover = None;
                s.suppress = None;
                s.tracking_leave = false;
            });
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HOVER);
            }
            invalidate();
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_HOVER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HOVER);
            }
            if let Some(b) = cursor_button() {
                let suppressed = with(|s| s.suppress == Some(b)).unwrap_or(false);
                if b.is_some() && is_menu_button(b) && !suppressed {
                    open_menu(b);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let Some(b) = button_at(x) else { return LRESULT(0) };
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HOVER);
            }
            // A click that just closed this button's menu shouldn't reopen it.
            let just_closed =
                with(|s| s.closed_at.is_some_and(|(cb, t)| cb == b && t.elapsed().as_millis() < 300)).unwrap_or(false);
            if just_closed {
                return LRESULT(0);
            }
            let item = b.and_then(|i| STRIP.with(|s| s.borrow().as_ref().and_then(|s| s.items.get(i).cloned())));
            match item {
                Some(Item::App(id)) => app::launch_app(&id),
                _ => open_menu(b),
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            context_menu(button_at(x).flatten());
            LRESULT(0)
        }
        WM_DROPFILES => {
            on_drop(HDROP(wparam.0 as *mut _));
            LRESULT(0)
        }
        WM_APP_APPBAR => {
            match wparam.0 as u32 {
                ABN_POSCHANGED | ABN_STATECHANGE => reposition(),
                ABN_FULLSCREENAPP => unsafe {
                    // Get out of the way of games and full-screen video.
                    let _ = ShowWindow(hwnd, if lparam.0 != 0 { SW_HIDE } else { SW_SHOWNOACTIVATE });
                },
                _ => {}
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            reposition();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
