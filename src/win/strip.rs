//! The icon strip: a bar docked against an edge of the primary monitor — by
//! default the Windows taskbar's, next to it — styled after the original
//! FlexTaskbar bar:
//!
//! - start (left, or top on a side edge): **All** (every app, as a list);
//! - centre: the root categories (marked ▾) and pinned apps;
//! - end: **Link** (add an app, file, URL or web app) and ⚙ (settings).
//!
//! On the left or right edge the bar stands upright and everything runs top
//! to bottom. Dragging the bar by an empty spot moves it to another edge.
//!
//! Resting the pointer on a category opens its flyout (see `flyout`). Pinned
//! apps launch with a click. Categories and apps can be dragged along the bar
//! to rearrange them (the order is `Config::bar_order`). The look — theme, colours, transparency, border,
//! corners, floating margin, width, sizes — comes from the Appearance
//! settings. The bar is a per-pixel-alpha layered window drawn with `canvas`.
//!
//! By default it reserves its screen space as an AppBar, so maximized windows
//! stop above it, and it hides while a full-screen app is in front.

use super::app;
use super::canvas::{self, Canvas};
use super::flyout;
use super::menu;
use super::theme;
use super::ui::{self, scale, wide};
use crate::appearance::{Appearance, Colors, DockWidth};
use crate::config::CustomApp;
use crate::striplayout::{self, BarLayout, Edge, Hit, Metrics, Slot};
use resvg::tiny_skia::Pixmap;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject, GetMonitorInfoW, HFONT, HGDIOBJ, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::Controls::{
    TTF_SUBCLASS, TTM_ADDTOOLW, TTM_DELTOOLW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
    ABN_FULLSCREENAPP, ABN_POSCHANGED, ABN_STATECHANGE, APPBARDATA, DragAcceptFiles, DragFinish, DragQueryFileW, HDROP,
    SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos, GetSystemMetrics,
    HWND_TOPMOST, InsertMenuW, KillTimer, MA_NOACTIVATE, MF_BYPOSITION, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    PostMessageW, RegisterClassW, SM_CXDRAG, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SendMessageW,
    SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD,
    TPM_RIGHTALIGN, TPM_RIGHTBUTTON, TPM_TOPALIGN, TRACK_POPUP_MENU_FLAGS, TrackPopupMenuEx, WINDOW_STYLE, WM_APP,
    WM_CAPTURECHANGED, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_DROPFILES, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_ACCEPTFILES, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, PWSTR, w};

const WM_APP_APPBAR: u32 = WM_APP + 20;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_HOVER: usize = 1;

const LEFT_ALL: usize = 0;
const RIGHT_LINK: usize = 0;
const RIGHT_SETTINGS: usize = 1;

#[derive(Clone, PartialEq)]
pub enum Item {
    Category(u64),
    App(String),
}

struct Strip {
    hwnd: HWND,
    tooltip: HWND,
    items: Vec<Item>,
    names: Vec<String>,
    layout: BarLayout,
    /// Bar-sized icons, keyed like the app's icon cache.
    icons: HashMap<String, Option<Pixmap>>,
    look: Appearance,
    colors: Colors,
    font: HFONT,
    dpi: u32,
    /// Window rectangle on screen.
    win: RECT,
    hover: Option<Hit>,
    /// The button whose flyout is open.
    open: Option<Hit>,
    tracking_leave: bool,
    reserved: bool,
    edge: Edge,
    tools: usize,
    /// Left button held on an item: (item index, position along the bar
    /// where it went down).
    press: Option<(usize, i32)>,
    /// Item being dragged, and the pointer's position along the bar.
    drag: Option<(usize, i32)>,
    /// Left button held on an empty spot: where it went down (client), and
    /// whether the bar is being dragged to another edge.
    bar_press: Option<(i32, i32)>,
    moving: bool,
}

thread_local! {
    static STRIP: RefCell<Option<Strip>> = const { RefCell::new(None) };
    static REPOSITIONING: Cell<bool> = const { Cell::new(false) };
}

fn hwnd() -> Option<HWND> {
    STRIP.with(|s| s.borrow().as_ref().map(|s| s.hwnd))
}

fn with<R>(f: impl FnOnce(&mut Strip) -> R) -> Option<R> {
    STRIP.with(|s| s.borrow_mut().as_mut().map(f))
}

/// The appearance settings and the colours they resolve to right now.
pub fn current_look() -> (Appearance, Colors) {
    let look = app::with(|s| s.cfg.settings.appearance.clamped());
    let colors = look.colors(theme::is_dark_cached());
    (look, colors)
}

// ---------------------------------------------------------------- lifecycle

/// Creates or removes the strip to match the settings.
pub fn apply_settings() {
    let show = app::with(|s| s.cfg.settings.show_strip);
    match (show, hwnd().is_some()) {
        (true, false) => create(),
        (false, true) => destroy(),
        (true, true) => apply_appearance(),
        (false, false) => {}
    }
}

/// The appearance (or bar height / reservation) changed: rebuild everything.
pub fn apply_appearance() {
    if hwnd().is_none() {
        return;
    }
    flyout::close();
    let (look, colors) = current_look();
    with(|s| {
        if s.look.icon_size != look.icon_size {
            s.icons.clear();
        }
        s.look = look;
        s.colors = colors;
    });
    reposition();
}

fn create() {
    let class = wide("FlexTaskbar.Strip");
    let (look, colors) = current_look();
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
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_ACCEPTFILES | WS_EX_LAYERED,
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
    STRIP.with(|s| {
        *s.borrow_mut() = Some(Strip {
            hwnd,
            tooltip,
            items: Vec::new(),
            names: Vec::new(),
            layout: striplayout::bar_layout(0, &empty_metrics(), 0, false),
            icons: HashMap::new(),
            look,
            colors,
            font: canvas::font(scale(13, dpi), false),
            dpi,
            win: RECT::default(),
            hover: None,
            open: None,
            tracking_leave: false,
            reserved: false,
            edge: Edge::Bottom,
            tools: 0,
            press: None,
            drag: None,
            bar_press: None,
            moving: false,
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
    flyout::close();
    let taken = STRIP.with(|s| s.borrow_mut().take());
    if let Some(s) = taken {
        unsafe {
            let mut abd = appbar_data(s.hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut abd);
            let _ = DestroyWindow(s.hwnd);
            let _ = DeleteObject(HGDIOBJ(s.font.0));
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
    let (_, colors) = current_look();
    if with(|s| s.colors = colors).is_some() {
        render();
    }
    flyout::close();
}

/// An icon was replaced (custom icon picked or reset).
pub fn icon_changed(key: &str) {
    with(|s| s.icons.remove(key));
    flyout::icon_changed(key);
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
        for key in s.cfg.bar_keys() {
            if let Some(id) = key.strip_prefix("cat:").and_then(|id| id.parse::<u64>().ok()) {
                if let Some(c) = s.cfg.categories.iter().find(|c| c.id == id) {
                    items.push(Item::Category(id));
                    names.push(c.name.clone());
                }
            } else if let Some(a) = s.catalog.get(&key) {
                items.push(Item::App(key));
                names.push(a.name.clone());
            }
        }
        let sources: Vec<_> = items.iter().map(|it| (key_of(it), app::icon_source_for(s, &key_of(it)))).collect();
        (items, names, sources)
    });
    let (size, missing) = with(|st| {
        let size = scale(st.look.icon_size as i32, st.dpi);
        let missing: Vec<_> = sources.into_iter().filter(|(k, _)| !st.icons.contains_key(k)).collect();
        (size, missing)
    })
    .unwrap_or_default();
    // A handful of icons, from Windows' own icon cache.
    let loaded: Vec<_> =
        missing.into_iter().map(|(k, src)| (k, src.and_then(|src| canvas::icon_pixmap(&src, size)))).collect();
    with(|st| {
        st.icons.extend(loaded);
        st.items = items;
        st.names = names;
    });
    relayout();
    flyout::refresh();
}

pub fn key_of(item: &Item) -> String {
    match item {
        Item::Category(id) => format!("cat:{id}"),
        Item::App(id) => id.clone(),
    }
}

fn empty_metrics() -> Metrics {
    Metrics { pad: 0, item_w: 1, gap: 0, left: Vec::new(), right: Vec::new() }
}

fn metrics(s: &Strip) -> Metrics {
    let d = s.dpi;
    let (all, link) = if s.edge.vertical() {
        // Upright: each button is a square-ish cell; Link shows just its glyph.
        (scale(32, d), scale(36, d))
    } else {
        (
            canvas::measure("All", s.font).0 + scale(20, d),
            scale(16 + 6, d) + canvas::measure("Link", s.font).0 + scale(20, d),
        )
    };
    let settings = scale(36, d);
    Metrics {
        pad: scale(8, d),
        item_w: scale(s.look.icon_size as i32 + 14, d),
        gap: scale(4, d),
        left: vec![all],
        right: vec![link, settings],
    }
}

/// Margin around the bar inside the window, in pixels.
fn margin(s: &Strip) -> i32 {
    scale(s.look.margin as i32, s.dpi)
}

/// The bar's thickness across its length (its height, or its width when it
/// stands on a side edge), in pixels.
fn thickness(s: &Strip) -> i32 {
    (if s.edge.vertical() { ui::rect_w(&s.win) } else { ui::rect_h(&s.win) }) - 2 * margin(s)
}

/// The bar's length, along which its buttons run.
fn length(s: &Strip) -> i32 {
    (if s.edge.vertical() { ui::rect_h(&s.win) } else { ui::rect_w(&s.win) }) - 2 * margin(s)
}

/// A client-area point's position along the bar, and across it.
fn along(s: &Strip, x: i32, y: i32) -> i32 {
    if s.edge.vertical() { y } else { x }
}

fn across(s: &Strip, x: i32, y: i32) -> i32 {
    if s.edge.vertical() { x } else { y }
}

/// Client rectangle of a slot, the bar's full thickness.
fn slot_rect(s: &Strip, slot: Slot) -> RECT {
    let (m, t) = (margin(s), thickness(s));
    if s.edge.vertical() {
        RECT { left: m, top: m + slot.x, right: m + t, bottom: m + slot.right() }
    } else {
        RECT { left: m + slot.x, top: m, right: m + slot.right(), bottom: m + t }
    }
}

fn relayout() {
    let Some(h) = hwnd() else { return };
    let tools = with(|s| {
        let len = length(s);
        s.layout = striplayout::bar_layout(len, &metrics(s), s.items.len(), s.look.dock_width == DockWidth::Fit);
        let mut tools: Vec<(String, Slot)> = vec![
            ("All apps".into(), s.layout.left[LEFT_ALL]),
            ("Add an app, file, website or web app".into(), s.layout.right[RIGHT_LINK]),
            ("Settings".into(), s.layout.right[RIGHT_SETTINGS]),
        ];
        // Categories open on hover, so only apps get a name tooltip.
        for (i, slot) in s.layout.items.iter().enumerate() {
            if matches!(s.items[i], Item::App(_)) {
                tools.push((s.names[i].clone(), *slot));
            }
        }
        let tools: Vec<(String, RECT)> = tools.into_iter().map(|(n, slot)| (n, slot_rect(s, slot))).collect();
        (s.tooltip, std::mem::replace(&mut s.tools, tools.len()), tools)
    });
    if let Some((tooltip, old_count, tools)) = tools {
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
            for (id, (name, rect)) in tools.iter().enumerate() {
                let mut text = wide(name);
                let ti = TTTOOLINFOW {
                    cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                    uFlags: TTF_SUBCLASS,
                    hwnd: h,
                    uId: id + 1,
                    rect: *rect,
                    lpszText: PWSTR(text.as_mut_ptr()),
                    ..Default::default()
                };
                SendMessageW(tooltip, TTM_ADDTOOLW, Some(WPARAM(0)), Some(LPARAM(&ti as *const _ as isize)));
            }
        }
    }
    render();
}

/// The button under a client-area point.
fn hit_at(x: i32, y: i32) -> Option<Hit> {
    STRIP.with(|s| {
        let b = s.borrow();
        let s = b.as_ref()?;
        let m = margin(s);
        let a = across(s, x, y);
        if a < m || a >= m + thickness(s) {
            return None;
        }
        striplayout::hit(&s.layout, along(s, x, y) - m)
    })
}

fn slot_of(s: &Strip, hit: Hit) -> Option<Slot> {
    match hit {
        Hit::Left(i) => s.layout.left.get(i).copied(),
        Hit::Item(i) => s.layout.items.get(i).copied(),
        Hit::Right(i) => s.layout.right.get(i).copied(),
    }
}

/// Screen rectangle of a button (flyouts open from it).
pub fn button_rect(hit: Hit) -> Option<RECT> {
    STRIP.with(|s| {
        let b = s.borrow();
        let s = b.as_ref()?;
        let rc = slot_rect(s, slot_of(s, hit)?);
        Some(RECT {
            left: s.win.left + rc.left,
            top: s.win.top + rc.top,
            right: s.win.left + rc.right,
            bottom: s.win.top + rc.bottom,
        })
    })
}

/// The strip's DPI (96 if it isn't shown).
pub fn dpi() -> u32 {
    with(|s| s.dpi).unwrap_or(96)
}

/// How deep categories may nest so every level of flyouts fits on the
/// primary monitor, given where the bar is (see `striplayout::max_levels`).
pub fn max_levels() -> usize {
    let (mon, _) = primary_monitor();
    let (edge, dpi, height) = {
        let st = with(|s| (s.edge, s.dpi));
        let (setting, height) = app::with(|s| (s.cfg.settings.strip_edge, s.cfg.settings.strip_height));
        match st {
            Some((edge, dpi)) => (edge, dpi, height),
            None => (setting.resolve(Edge::Bottom), 96, height),
        }
    };
    let bar = scale(height.clamp(24, 96) as i32, dpi);
    striplayout::max_levels(edge, (ui::rect_w(&mon), ui::rect_h(&mon)), bar, dpi)
}

/// The edge the strip is docked against (flyouts open away from it).
pub fn edge() -> Edge {
    with(|s| s.edge).unwrap_or(Edge::Bottom)
}

pub fn item(hit: Hit) -> Option<Item> {
    match hit {
        Hit::Item(i) => STRIP.with(|s| s.borrow().as_ref().and_then(|s| s.items.get(i).cloned())),
        _ => None,
    }
}

/// The flyout tells the strip which button it hangs from (drawn pressed).
pub fn set_open(hit: Option<Hit>) {
    if with(|s| std::mem::replace(&mut s.open, hit) != hit).unwrap_or(false) {
        render();
    }
}

/// Whether the pointer is over a given button.
pub fn pointer_over(hit: Hit) -> bool {
    let Some(rc) = button_rect(hit) else { return false };
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    pt.x >= rc.left && pt.x < rc.right && pt.y >= rc.top && pt.y < rc.bottom
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

fn abe(edge: Edge) -> u32 {
    match edge {
        Edge::Bottom => ABE_BOTTOM,
        Edge::Top => ABE_TOP,
        Edge::Left => ABE_LEFT,
        Edge::Right => ABE_RIGHT,
    }
}

/// `rect` cut down to a band `thickness` deep along `edge`.
fn band(rect: RECT, edge: Edge, thickness: i32) -> RECT {
    match edge {
        Edge::Bottom => RECT { top: rect.bottom - thickness, ..rect },
        Edge::Top => RECT { bottom: rect.top + thickness, ..rect },
        Edge::Left => RECT { right: rect.left + thickness, ..rect },
        Edge::Right => RECT { left: rect.right - thickness, ..rect },
    }
}

/// The Windows taskbar's edge of the primary monitor.
fn taskbar_edge(h: HWND) -> Edge {
    let mut tb = appbar_data(h);
    let edge = unsafe { if SHAppBarMessage(ABM_GETTASKBARPOS, &mut tb) != 0 { tb.uEdge } else { ABE_BOTTOM } };
    match edge {
        ABE_TOP => Edge::Top,
        ABE_LEFT => Edge::Left,
        ABE_RIGHT => Edge::Right,
        _ => Edge::Bottom,
    }
}

/// Docks the strip against its edge of the primary monitor.
fn reposition() {
    let Some(h) = hwnd() else { return };
    if REPOSITIONING.with(|r| r.replace(true)) {
        return; // our own ABM_SETPOS can notify us again
    }
    let (size_dip, reserve, setting) = app::with(|s| {
        (s.cfg.settings.strip_height.clamp(24, 96), s.cfg.settings.reserve_space, s.cfg.settings.strip_edge)
    });
    let dpi = ui::dpi_of(h);
    let margin_dip = with(|s| s.look.margin).unwrap_or(0) as i32;
    // The window is the bar plus its floating margin on every side.
    let thickness = scale(size_dip as i32, dpi) + 2 * scale(margin_dip, dpi);
    let (monitor, work) = primary_monitor();
    let mut abd = appbar_data(h);
    let edge = setting.resolve(taskbar_edge(h));

    let was_reserved = with(|s| std::mem::replace(&mut s.reserved, reserve)).unwrap_or(false);
    let rect = if reserve {
        abd.uEdge = abe(edge);
        abd.rc = band(monitor, edge, thickness);
        unsafe {
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            // QUERYPOS moved the proposed edge past other bars; restore our size.
            abd.rc = band(abd.rc, edge, thickness);
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
        band(work, edge, thickness)
    };
    let dpi_changed = with(|s| {
        s.edge = edge;
        s.win = rect;
        let changed = std::mem::replace(&mut s.dpi, dpi) != dpi;
        if changed {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(s.font.0));
            }
            s.font = canvas::font(scale(13, dpi), false);
            s.icons.clear();
        }
        changed
    })
    .unwrap_or(false);
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
    let icons_missing = with(|s| s.icons.is_empty() && !s.items.is_empty()).unwrap_or(false);
    if dpi_changed || icons_missing {
        refresh();
    } else {
        relayout();
    }
}

// ---------------------------------------------------------------- dragging

/// Which item each visible slot shows: the bar's order, or while dragging,
/// the order it would have if dropped now.
fn display_order(s: &Strip) -> Vec<usize> {
    let mut order: Vec<usize> = (0..s.items.len()).collect();
    if let Some((from, pos)) = s.drag {
        order.remove(from);
        order.insert(striplayout::drop_index(&s.layout.items, pos - margin(s)).min(order.len()), from);
    }
    order
}

/// Finishes a drag: the item moves to where it was dropped. Dropping it away
/// from the bar cancels.
fn drop_item(from: usize, x: i32, y: i32) {
    let target = with(|s| {
        let m = margin(s);
        let t = thickness(s);
        let a = across(s, x, y);
        let over_bar = a >= -t && a < 2 * m + 2 * t;
        let key = s.items.get(from).map(key_of)?;
        over_bar.then(|| (key, striplayout::drop_index(&s.layout.items, along(s, x, y) - m)))
    })
    .flatten();
    match target {
        Some((key, to)) if app::with(|s| s.cfg.move_bar_item(&key, to)) => {
            app::save();
        }
        _ => render(),
    }
}

/// While the bar itself is dragged: dock it against the edge nearest the
/// pointer (as the Windows taskbar does). Saved when the button is released.
fn move_bar() {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let mon = primary_monitor().0;
    let edge = striplayout::edge_for_point((mon.left, mon.top, mon.right, mon.bottom), pt.x, pt.y);
    if with(|s| s.edge != edge).unwrap_or(false) {
        flyout::close();
        app::with(|s| s.cfg.settings.strip_edge = crate::config::StripEdge::fixed(edge));
        reposition();
    }
}

// ---------------------------------------------------------------- drawing

fn render() {
    STRIP.with(|cell| {
        let b = cell.borrow();
        let Some(s) = b.as_ref() else { return };
        let (w, h) = (ui::rect_w(&s.win), ui::rect_h(&s.win));
        let Some(mut cv) = Canvas::new(w, h) else { return };
        let d = s.dpi;
        let c = &s.colors;
        let vertical = s.edge.vertical();
        let m = margin(s) as f32;
        let bar = slot_rect(s, s.layout.bar);
        let rf = |rc: &RECT| (rc.left as f32, rc.top as f32, ui::rect_w(rc) as f32, ui::rect_h(rc) as f32);
        let radius = scale(s.look.corner_radius as i32, d) as f32;
        let border = if s.look.border_width > 0 { (scale(s.look.border_width as i32, d) as f32).max(1.0) } else { 0.0 };

        let (bx, by, bw, bh) = rf(&bar);
        cv.fill_round_rect(bx, by, bw, bh, radius, c.background);
        let docked_flush = radius == 0.0 && m == 0.0 && s.look.dock_width == DockWidth::Full;
        if border > 0.0 {
            if docked_flush {
                // Like the Windows taskbar: just a line on the side facing the desktop.
                match s.edge {
                    Edge::Bottom => cv.fill_round_rect(bx, by, bw, border, 0.0, c.border),
                    Edge::Top => cv.fill_round_rect(bx, by + bh - border, bw, border, 0.0, c.border),
                    Edge::Left => cv.fill_round_rect(bx + bw - border, by, border, bh, 0.0, c.border),
                    Edge::Right => cv.fill_round_rect(bx, by, border, bh, 0.0, c.border),
                }
            } else {
                cv.stroke_round_rect(bx, by, bw, bh, radius, border, c.border);
            }
        }

        let inset = scale(5, d) as f32;
        let r4 = scale(4, d) as f32;
        // A button's highlight: the slot, inset across the bar.
        let cell = |rc: &RECT| {
            let (x, y, w, h) = rf(rc);
            if vertical { (x + inset, y, w - 2.0 * inset, h) } else { (x, y + inset, w, h - 2.0 * inset) }
        };
        let draw_state = |cv: &mut Canvas, hit: Hit, slot: Slot| {
            let open = s.open == Some(hit);
            let hover = s.hover == Some(hit);
            let rc = slot_rect(s, slot);
            if open || hover {
                let fill = if open { c.pressed } else { c.hover };
                let (x, y, w, h) = cell(&rc);
                cv.fill_round_rect(x, y, w, h, r4, fill);
            }
            if open {
                // Accent pill on the screen-edge side of the button whose flyout is open.
                let long = scale(16, d) as f32;
                let short = scale(3, d) as f32;
                let (x, y, w, h) = cell(&rc);
                let (px, py, pw, ph) = match s.edge {
                    Edge::Bottom => (x + (w - long) / 2.0, y + h - short / 2.0, long, short),
                    Edge::Top => (x + (w - long) / 2.0, y - short / 2.0, long, short),
                    Edge::Left => (x - short / 2.0, y + (h - long) / 2.0, short, long),
                    Edge::Right => (x + w - short / 2.0, y + (h - long) / 2.0, short, long),
                };
                cv.fill_round_rect(px, py, pw, ph, short / 2.0, c.accent);
            }
        };
        let centre = |rc: &RECT| ((rc.left + rc.right) as f32 / 2.0, (rc.top + rc.bottom) as f32 / 2.0);

        // Start: All.
        let all = s.layout.left[LEFT_ALL];
        draw_state(&mut cv, Hit::Left(LEFT_ALL), all);
        cv.text("All", slot_rect(s, all), s.font, c.text, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        // Centre: categories and pinned apps.
        let size = scale(s.look.icon_size as i32, d);
        // While dragging, the others make room where the dragged one would land.
        let order = display_order(s);
        let draw_item = |cv: &mut Canvas, i: usize, rc: RECT| {
            let (cx, cy) = centre(&rc);
            let x = (cx - size as f32 / 2.0) as i32;
            let y = (cy - size as f32 / 2.0) as i32;
            let is_cat = matches!(s.items[i], Item::Category(_));
            match s.icons.get(&key_of(&s.items[i])).and_then(|p| p.as_ref()) {
                Some(img) => cv.image(img, x, y, size, 1.0),
                None if is_cat => cv.folder(x as f32, y as f32, size as f32, c.accent),
                None => {
                    let letter: String =
                        s.names[i].chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
                    let rc = RECT { left: x, top: y, right: x + size, bottom: y + size };
                    cv.text(&letter, rc, s.font, c.text, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                }
            }
            if is_cat {
                let cs = scale(7, d) as f32;
                cv.chevron(
                    x as f32 + size as f32 + scale(2, d) as f32,
                    y as f32 + size as f32 - cs / 2.0,
                    cs,
                    c.subtle,
                );
            }
        };
        for (pos, slot) in s.layout.items.iter().enumerate() {
            let i = order[pos];
            let rc = slot_rect(s, *slot);
            if s.drag.is_some_and(|(from, _)| from == i) {
                // The drop spot.
                let (x, y, w, h) = cell(&rc);
                cv.fill_round_rect(x, y, w, h, r4, c.hover);
                continue;
            }
            if s.drag.is_none() {
                draw_state(&mut cv, Hit::Item(pos), *slot);
            }
            draw_item(&mut cv, i, rc);
        }
        if let Some((from, p)) = s.drag
            && let Some(slot) = s.layout.items.first()
        {
            // The dragged button follows the pointer.
            let start = (p - m as i32 - slot.w / 2).clamp(s.layout.bar.x, s.layout.bar.right() - slot.w);
            let rc = slot_rect(s, Slot { x: start, w: slot.w });
            let (x, y, w, h) = cell(&rc);
            cv.fill_round_rect(x, y, w, h, r4, c.pressed);
            draw_item(&mut cv, from, rc);
        }

        // End: Link, settings.
        let link = s.layout.right[RIGHT_LINK];
        draw_state(&mut cv, Hit::Right(RIGHT_LINK), link);
        let glyph = scale(16, d) as f32;
        let lrc = slot_rect(s, link);
        if vertical {
            let (cx, cy) = centre(&lrc);
            cv.link(cx, cy, glyph, c.text);
        } else {
            let lx = lrc.left as f32 + scale(10, d) as f32;
            cv.link(lx + glyph / 2.0, centre(&lrc).1, glyph, c.text);
            let rc = RECT { left: (lx + glyph) as i32 + scale(6, d), ..lrc };
            cv.text("Link", rc, s.font, c.text, DT_VCENTER | DT_SINGLELINE);
        }
        let gear = s.layout.right[RIGHT_SETTINGS];
        draw_state(&mut cv, Hit::Right(RIGHT_SETTINGS), gear);
        let (gx, gy) = centre(&slot_rect(s, gear));
        cv.gear(gx, gy, scale(8, d) as f32, c.text);

        cv.present(s.hwnd, s.win.left, s.win.top);
    });
}

// ---------------------------------------------------------------- actions

fn click(hit: Hit) {
    match hit {
        Hit::Left(LEFT_ALL) => flyout::toggle_all(hit),
        Hit::Item(_) => match item(hit) {
            Some(Item::Category(id)) => flyout::open_category(id, hit),
            Some(Item::App(id)) => {
                flyout::close();
                app::launch_app(&id);
            }
            None => {}
        },
        Hit::Right(RIGHT_LINK) => {
            flyout::close();
            add_link();
        }
        Hit::Right(_) => {
            flyout::close();
            super::manager::show();
        }
        Hit::Left(_) => {}
    }
}

/// "Link": add an app, file, URL or web app and pin it to the strip.
fn add_link() {
    let Some(h) = hwnd() else { return };
    let Some(fields) = super::appdialog::edit(h, &CustomApp::default(), "Add to the strip") else { return };
    let id = app::with(|s| {
        let id = format!("custom:{}", s.cfg.alloc_id());
        s.cfg.custom_apps.push(CustomApp { id: id.clone(), ..fields });
        s.cfg.pin(&id);
        id
    });
    app::rebuild_catalog();
    app::reload_icon(&id);
    app::save();
    super::searchwin::catalog_changed();
    super::manager::catalog_changed();
}

/// Popup menus open away from the strip's edge.
fn menu_align(edge: Edge) -> TRACK_POPUP_MENU_FLAGS {
    match edge {
        Edge::Bottom => TPM_BOTTOMALIGN,
        Edge::Top => TPM_TOPALIGN,
        Edge::Left => TPM_LEFTALIGN,
        Edge::Right => TPM_RIGHTALIGN,
    }
}

/// Right-click menu for one button; the full menu elsewhere.
fn context_menu(hit: Option<Hit>) {
    // A pending hover would otherwise open a flyout over the menu.
    if let Some(h) = hwnd() {
        unsafe {
            let _ = KillTimer(Some(h), TIMER_HOVER);
        }
    }
    flyout::close();
    let item = hit.and_then(item);
    let Some(item) = item else {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        if let Some(action) = menu::track(app::main_hwnd(), pt) {
            app::perform(action);
        }
        return;
    };
    const LEFT: usize = 1;
    const RIGHT: usize = 2;
    const UNPIN: usize = 3;
    const MANAGE: usize = 4;
    const LOOK: usize = 5;
    const HIDE: usize = 6;
    const ARRANGE: usize = 7;
    let key = key_of(&item);
    let (can_left, can_right) = app::with(|s| {
        let keys = s.cfg.bar_keys();
        let pos = keys.iter().position(|k| *k == key).unwrap_or(0);
        (pos > 0, pos + 1 < keys.len())
    });
    let chosen = unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        let mut pos = 0;
        let mut add = |id: usize, text: &str, enabled: bool| {
            let t = wide(text);
            let flags = if enabled { MF_BYPOSITION | MF_STRING } else { MF_BYPOSITION | MF_STRING | MF_GRAYED };
            let _ = InsertMenuW(menu, pos, flags, id, PCWSTR(t.as_ptr()));
            pos += 1;
        };
        let (back, forward) = if edge().vertical() { ("Move up", "Move down") } else { ("Move left", "Move right") };
        add(LEFT, back, can_left);
        add(RIGHT, forward, can_right);
        if matches!(item, Item::App(_)) {
            add(UNPIN, "Unpin from strip", true);
        }
        let _ = InsertMenuW(menu, pos, MF_BYPOSITION | MF_SEPARATOR, 0, PCWSTR::null());
        pos += 1;
        let mut add = |id: usize, text: &str| {
            let t = wide(text);
            let _ = InsertMenuW(menu, pos, MF_BYPOSITION | MF_STRING, id, PCWSTR(t.as_ptr()));
            pos += 1;
        };
        add(ARRANGE, "Arrange the bar…");
        add(MANAGE, "Manage categories…");
        add(LOOK, "Appearance…");
        add(HIDE, "Hide icon strip");
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let owner = app::main_hwnd();
        let _ = SetForegroundWindow(owner);
        let id =
            TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON | menu_align(edge())).0, pt.x, pt.y, owner, None).0
                as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        id
    };
    match chosen {
        LEFT | RIGHT => {
            let delta = if chosen == LEFT { -1 } else { 1 };
            app::with(|s| s.cfg.move_bar_by(&key, delta));
            app::save();
        }
        UNPIN => {
            if let Item::App(id) = &item {
                app::with(|s| s.cfg.unpin(id));
                app::save();
            }
        }
        MANAGE => match &item {
            Item::Category(id) => super::manager::show_category(*id),
            Item::App(_) => super::manager::show(),
        },
        LOOK => super::appearancewin::show(),
        ARRANGE => super::arrangewin::show(),
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

fn mouse_xy(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN => {
            let (x, y) = mouse_xy(lparam);
            match hit_at(x, y) {
                // An item: maybe the start of dragging it along the bar.
                Some(Hit::Item(i)) => {
                    with(|s| s.press = Some((i, along(s, x, y))));
                }
                Some(_) => return LRESULT(0),
                // An empty spot: maybe the start of dragging the bar to another edge.
                None => {
                    with(|s| s.bar_press = Some((x, y)));
                }
            }
            unsafe {
                SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = mouse_xy(lparam);
            let threshold = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(2);
            // Dragging the bar to another edge?
            let moving = with(|s| match s.bar_press {
                Some((x0, y0)) if s.moving || (x - x0).abs() >= threshold || (y - y0).abs() >= threshold => {
                    s.moving = true;
                    s.hover = None;
                    true
                }
                _ => false,
            })
            .unwrap_or(false);
            if moving {
                unsafe {
                    if let Ok(cur) = windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                        None,
                        windows::Win32::UI::WindowsAndMessaging::IDC_SIZEALL,
                    ) {
                        windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(cur));
                    }
                }
                move_bar();
                return LRESULT(0);
            }
            // Dragging an item along the bar?
            let dragging = with(|s| {
                let pos = along(s, x, y);
                if let Some((from, p0)) = s.press
                    && s.drag.is_none()
                    && (pos - p0).abs() >= threshold
                {
                    s.drag = Some((from, pos));
                    s.hover = None;
                    return Some(true);
                }
                s.drag.as_mut().map(|d| {
                    d.1 = pos;
                    false
                })
            })
            .flatten();
            if let Some(started) = dragging {
                if started {
                    flyout::close();
                    unsafe {
                        let _ = KillTimer(Some(hwnd), TIMER_HOVER);
                    }
                }
                render();
                return LRESULT(0);
            }
            let under = hit_at(x, y);
            let (changed, start_leave) = with(|s| {
                let changed = s.hover != under;
                s.hover = under;
                (changed, !std::mem::replace(&mut s.tracking_leave, true))
            })
            .unwrap_or((false, false));
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
                render();
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER_HOVER);
                }
                match under.and_then(|h| item(h).map(|it| (h, it))) {
                    // Categories open on hover (immediately if another flyout is
                    // already open, so moving along the bar switches between them).
                    Some((h, Item::Category(id))) => {
                        if flyout::is_open() {
                            flyout::open_category(id, h);
                        } else {
                            let ms = app::with(|s| s.cfg.settings.hover_delay_ms);
                            unsafe {
                                SetTimer(Some(hwnd), TIMER_HOVER, ms.max(1), None);
                            }
                        }
                    }
                    _ => flyout::pointer_left_anchor(),
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|s| {
                s.hover = None;
                s.tracking_leave = false;
            });
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HOVER);
            }
            render();
            flyout::pointer_left_anchor();
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_HOVER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HOVER);
            }
            if let Some(h) = with(|s| s.hover).flatten()
                && let Some(Item::Category(id)) = item(h)
                && pointer_over(h)
            {
                flyout::open_category(id, h);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = mouse_xy(lparam);
            let (press, drag, bar_press, moved) =
                with(|s| (s.press.take(), s.drag.take(), s.bar_press.take(), std::mem::take(&mut s.moving)))
                    .unwrap_or_default();
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HOVER);
                if press.is_some() || bar_press.is_some() {
                    let _ = ReleaseCapture();
                }
            }
            if moved {
                app::save();
                super::appearancewin::settings_changed();
            } else if let Some((from, _)) = drag {
                drop_item(from, x, y);
            } else if let Some(h) = hit_at(x, y) {
                click(h);
            } else {
                flyout::close();
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            // Capture lost mid-drag (another window took it): cancel.
            let moved = with(|s| {
                s.bar_press = None;
                std::mem::take(&mut s.moving)
            })
            .unwrap_or(false);
            if moved {
                app::save();
                super::appearancewin::settings_changed();
            }
            if with(|s| s.press.take().is_some() | s.drag.take().is_some()).unwrap_or(false) {
                render();
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (x, y) = mouse_xy(lparam);
            context_menu(hit_at(x, y));
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
                    flyout::close();
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
