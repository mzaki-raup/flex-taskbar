//! The icon strip: a bar docked against the Windows taskbar (just above it, or
//! below it when the taskbar is at the top of the screen), styled after the
//! original FlexTaskbar bar:
//!
//! - left: **All** (every app, as a list);
//! - centre: the root categories (marked ▾) and pinned apps;
//! - right: **Link** (add an app, file, URL or web app) and ⚙ (settings).
//!
//! Resting the pointer on a category opens its flyout (see `flyout`). Pinned
//! apps launch with a click. The look — theme, colours, transparency, border,
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
use crate::striplayout::{self, BarLayout, Hit, Metrics, Slot};
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
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_TOP, ABM_GETTASKBARPOS, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABN_FULLSCREENAPP,
    ABN_POSCHANGED, ABN_STATECHANGE, APPBARDATA, DragAcceptFiles, DragFinish, DragQueryFileW, HDROP, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos, HWND_TOPMOST,
    InsertMenuW, KillTimer, MA_NOACTIVATE, MF_BYPOSITION, MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SendMessageW, SetForegroundWindow, SetTimer,
    SetWindowPos, ShowWindow, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WINDOW_STYLE, WM_APP,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_DROPFILES, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NULL, WM_RBUTTONUP,
    WM_TIMER, WNDCLASSW, WS_EX_ACCEPTFILES, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
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
    edge: u32,
    tools: usize,
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
            edge: ABE_BOTTOM,
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
    let all = canvas::measure("All", s.font).0 + scale(20, d);
    let link = scale(16 + 6, d) + canvas::measure("Link", s.font).0 + scale(20, d);
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

fn bar_height(s: &Strip) -> i32 {
    ui::rect_h(&s.win) - 2 * margin(s)
}

fn relayout() {
    let Some(h) = hwnd() else { return };
    let tools = with(|s| {
        let m = margin(s);
        let width = ui::rect_w(&s.win) - 2 * m;
        s.layout = striplayout::bar_layout(width, &metrics(s), s.items.len(), s.look.dock_width == DockWidth::Fit);
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
        let bottom = m + bar_height(s);
        (s.tooltip, std::mem::replace(&mut s.tools, tools.len()), tools, m, bottom)
    });
    if let Some((tooltip, old_count, tools, m, bottom)) = tools {
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
                    rect: RECT { left: slot.x + m, top: m, right: slot.right() + m, bottom },
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
        if y < m || y >= m + bar_height(s) {
            return None;
        }
        striplayout::hit(&s.layout, x - m)
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
        let slot = slot_of(s, hit)?;
        let m = margin(s);
        Some(RECT {
            left: s.win.left + m + slot.x,
            top: s.win.top + m,
            right: s.win.left + m + slot.right(),
            bottom: s.win.top + m + bar_height(s),
        })
    })
}

/// The strip's DPI (96 if it isn't shown).
pub fn dpi() -> u32 {
    with(|s| s.dpi).unwrap_or(96)
}

pub fn edge_is_top() -> bool {
    with(|s| s.edge == ABE_TOP).unwrap_or(false)
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

/// Docks the strip against the Windows taskbar's edge of the primary monitor.
fn reposition() {
    let Some(h) = hwnd() else { return };
    if REPOSITIONING.with(|r| r.replace(true)) {
        return; // our own ABM_SETPOS can notify us again
    }
    let (height_dip, reserve) =
        app::with(|s| (s.cfg.settings.strip_height.clamp(24, 96), s.cfg.settings.reserve_space));
    let dpi = ui::dpi_of(h);
    let margin_dip = with(|s| s.look.margin).unwrap_or(0) as i32;
    // The window is the bar plus its floating margin on every side.
    let height = scale(height_dip as i32, dpi) + 2 * scale(margin_dip, dpi);
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

// ---------------------------------------------------------------- drawing

fn render() {
    STRIP.with(|cell| {
        let b = cell.borrow();
        let Some(s) = b.as_ref() else { return };
        let (w, h) = (ui::rect_w(&s.win), ui::rect_h(&s.win));
        let Some(mut cv) = Canvas::new(w, h) else { return };
        let d = s.dpi;
        let c = &s.colors;
        let m = margin(s) as f32;
        let bh = bar_height(s) as f32;
        let bar = s.layout.bar;
        let (bx, bw) = (m + bar.x as f32, bar.w as f32);
        let radius = scale(s.look.corner_radius as i32, d) as f32;
        let border =
            scale(s.look.border_width as i32, d).min(if s.look.border_width > 0 { i32::MAX } else { 0 }) as f32;
        let border = if s.look.border_width > 0 { border.max(1.0) } else { 0.0 };

        cv.fill_round_rect(bx, m, bw, bh, radius, c.background);
        let docked_flush = radius == 0.0 && m == 0.0 && s.look.dock_width == DockWidth::Full;
        if border > 0.0 {
            if docked_flush {
                // Like the Windows taskbar: just a line on the side facing the desktop.
                let y = if s.edge == ABE_TOP { bh - border } else { 0.0 };
                cv.fill_round_rect(bx, y, bw, border, 0.0, c.border);
            } else {
                cv.stroke_round_rect(bx, m, bw, bh, radius, border, c.border);
            }
        }

        let inset = scale(5, d) as f32;
        let r4 = scale(4, d) as f32;
        let draw_state = |cv: &mut Canvas, hit: Hit, slot: Slot| {
            let open = s.open == Some(hit);
            let hover = s.hover == Some(hit);
            if open || hover {
                let fill = if open { c.pressed } else { c.hover };
                cv.fill_round_rect(m + slot.x as f32, m + inset, slot.w as f32, bh - 2.0 * inset, r4, fill);
            }
            if open {
                // Accent pill under the button whose flyout is open.
                let pw = scale(16, d) as f32;
                let ph = scale(3, d) as f32;
                let x = m + slot.x as f32 + (slot.w as f32 - pw) / 2.0;
                let y = if s.edge == ABE_TOP { m + inset - ph / 2.0 } else { m + bh - inset - ph / 2.0 };
                cv.fill_round_rect(x, y, pw, ph, ph / 2.0, c.accent);
            }
        };
        let text_rect = |slot: Slot| RECT {
            left: m as i32 + slot.x,
            top: m as i32,
            right: m as i32 + slot.right(),
            bottom: (m + bh) as i32,
        };

        // Left: All.
        let all = s.layout.left[LEFT_ALL];
        draw_state(&mut cv, Hit::Left(LEFT_ALL), all);
        cv.text("All", text_rect(all), s.font, c.text, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        // Centre: categories and pinned apps.
        let size = scale(s.look.icon_size as i32, d);
        let cy = m + bh / 2.0;
        for (i, slot) in s.layout.items.iter().enumerate() {
            draw_state(&mut cv, Hit::Item(i), *slot);
            let x = m as i32 + slot.x + (slot.w - size) / 2;
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
        }

        // Right: Link, settings.
        let link = s.layout.right[RIGHT_LINK];
        draw_state(&mut cv, Hit::Right(RIGHT_LINK), link);
        let glyph = scale(16, d) as f32;
        let lx = m + link.x as f32 + scale(10, d) as f32;
        cv.link(lx + glyph / 2.0, cy, glyph, c.text);
        let mut rc = text_rect(link);
        rc.left = (lx + glyph) as i32 + scale(6, d);
        cv.text("Link", rc, s.font, c.text, DT_VCENTER | DT_SINGLELINE);
        let gear = s.layout.right[RIGHT_SETTINGS];
        draw_state(&mut cv, Hit::Right(RIGHT_SETTINGS), gear);
        cv.gear(m + gear.x as f32 + gear.w as f32 / 2.0, cy, scale(8, d) as f32, c.text);

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

/// Right-click menu for one button; the full menu elsewhere.
fn context_menu(hit: Option<Hit>) {
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
        let menu = CreatePopupMenu().unwrap_or_default();
        let mut pos = 0;
        let mut add = |id: usize, text: &str, enabled: bool| {
            let t = wide(text);
            let flags = if enabled { MF_BYPOSITION | MF_STRING } else { MF_BYPOSITION | MF_STRING | MF_GRAYED };
            let _ = InsertMenuW(menu, pos, flags, id, PCWSTR(t.as_ptr()));
            pos += 1;
        };
        add(LEFT, "Move left", can_left);
        add(RIGHT, "Move right", can_right);
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
        add(MANAGE, "Manage categories…");
        add(LOOK, "Appearance…");
        add(HIDE, "Hide icon strip");
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let owner = app::main_hwnd();
        let _ = SetForegroundWindow(owner);
        let id = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN).0, pt.x, pt.y, owner, None)
            .0 as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
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
        MANAGE => match &item {
            Item::Category(id) => super::manager::show_category(*id),
            Item::App(_) => super::manager::show(),
        },
        LOOK => super::appearancewin::show(),
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
        WM_MOUSEMOVE => {
            let (x, y) = mouse_xy(lparam);
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
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_HOVER);
            }
            if let Some(h) = hit_at(x, y) {
                click(h);
            } else {
                flyout::close();
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
