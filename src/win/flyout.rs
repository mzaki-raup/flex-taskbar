//! Flyouts from the icon strip, in the original FlexTaskbar style:
//!
//! - a **category** flyout: its subcategories and apps as tiles (large icon,
//!   name underneath, `flyout_columns` per row; subcategories first, marked
//!   with ▾). Resting the pointer on a subcategory opens *its* flyout beyond
//!   this one, the same way a category on the strip opens, at any depth;
//! - the **All** flyout: every app as a scrollable list.
//!
//! The open flyouts form a stack of levels: level 0 hangs off a strip button,
//! each further level off a subcategory tile of the level below. Every level
//! opens away from the strip's edge (above a bottom strip, to the right of a
//! left one, and so on). They close
//! shortly after the pointer has left all of them and the strip button. Each is
//! a per-pixel-alpha layered window drawn with `canvas`, using the strip's
//! appearance settings.

use super::app;
use super::canvas::{self, Canvas};
use super::strip;
use super::ui::{self, scale, wide};
use crate::appearance::{Appearance, Colors};
use crate::striplayout::{self, Edge, Hit};
use resvg::tiny_skia::Pixmap;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteObject, GetMonitorInfoW, HFONT, HGDIOBJ,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos, HWND_TOPMOST,
    InsertMenuW, KillTimer, MA_NOACTIVATE, MF_BYPOSITION, MF_STRING, PostMessageW, RegisterClassW, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenuEx, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NULL, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const WM_MOUSELEAVE: u32 = 0x02A3;
/// Timers, all on the level-0 window.
const TIMER_CLOSE: usize = 1;
const TIMER_HOVER: usize = 2;
const CLOSE_DELAY_MS: u32 = 300;
/// How long the pointer rests on a tile before the levels above follow it
/// (a subcategory opens; anything else closes what was open above).
const HOVER_DELAY_MS: u32 = 200;

#[derive(Clone, PartialEq)]
enum View {
    Category(u64),
    All { first_row: usize },
}

#[derive(Clone, PartialEq)]
enum Elem {
    Sub(u64),
    Tile(String),
    Row(String),
    /// Static text ("No apps in this category", the All header).
    Label,
}

struct Placed {
    rect: RECT,
    elem: Elem,
    text: String,
    icon: Option<String>,
}

struct Level {
    hwnd: HWND,
    view: View,
    /// For levels above 0: the index of the subcategory tile in the level
    /// below that opened this one.
    source: Option<usize>,
    elems: Vec<Placed>,
    hover: Option<usize>,
    /// Window rectangle on screen.
    win: RECT,
    tracking_leave: bool,
    /// Total rows in the All list, and how many fit.
    rows: (usize, usize),
}

struct Flyouts {
    levels: Vec<Level>,
    anchor: Hit,
    look: Appearance,
    colors: Colors,
    dpi: u32,
    font: HFONT,
    small: HFONT,
    /// Icons by (cache key, pixel size).
    icons: HashMap<(String, i32), Option<Pixmap>>,
}

thread_local! {
    static FLYOUT: RefCell<Option<Flyouts>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Flyouts) -> R) -> Option<R> {
    FLYOUT.with(|s| s.borrow_mut().as_mut().map(f))
}

pub fn is_open() -> bool {
    FLYOUT.with(|f| f.borrow().is_some())
}

/// The level-0 window, which owns the timers.
fn base() -> Option<HWND> {
    with(|f| f.levels.first().map(|l| l.hwnd)).flatten()
}

/// Which level `hwnd` is. Uses `try_borrow`: window messages can arrive while
/// the state is borrowed (for example while a window is being drawn).
fn level_of(hwnd: HWND) -> Option<usize> {
    FLYOUT.with(|f| f.try_borrow().ok()?.as_ref()?.levels.iter().position(|l| l.hwnd == hwnd))
}

// ---------------------------------------------------------------- opening

pub fn open_category(id: u64, anchor: Hit) {
    let already = with(|f| f.anchor == anchor && f.levels.first().is_some_and(|l| l.view == View::Category(id)));
    if already == Some(true) {
        cancel_close();
        return;
    }
    show(View::Category(id), anchor);
}

pub fn toggle_all(anchor: Hit) {
    if with(|f| f.anchor == anchor) == Some(true) {
        close();
    } else {
        show(View::All { first_row: 0 }, anchor);
    }
}

fn show(view: View, anchor: Hit) {
    if !is_open() {
        let (look, colors) = strip::current_look();
        let dpi = strip::dpi();
        FLYOUT.with(|f| {
            *f.borrow_mut() = Some(Flyouts {
                levels: Vec::new(),
                anchor,
                look,
                colors,
                dpi,
                font: canvas::font(scale(13, dpi), false),
                small: canvas::font(scale(11, dpi), false),
                icons: HashMap::new(),
            })
        });
    }
    truncate(0);
    with(|f| f.anchor = anchor);
    if !push_level(view, None) {
        close();
        return;
    }
    strip::set_open(Some(anchor));
    cancel_close();
    rebuild(0);
}

fn create_window() -> Option<HWND> {
    let class = wide("FlexTaskbar.Flyout");
    unsafe {
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
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            PCWSTR(class.as_ptr()),
            w!("FlexTaskbar flyout"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(app::instance()),
            None,
        )
        .ok()
    }
}

fn push_level(view: View, source: Option<usize>) -> bool {
    let Some(hwnd) = create_window() else { return false };
    with(|f| {
        f.levels.push(Level {
            hwnd,
            view,
            source,
            elems: Vec::new(),
            hover: None,
            win: RECT::default(),
            tracking_leave: false,
            rows: (0, 0),
        })
    });
    true
}

/// Closes every level from `keep` up, leaving `keep` levels open.
fn truncate(keep: usize) {
    let gone: Vec<HWND> =
        with(|f| f.levels.drain(keep.min(f.levels.len())..).map(|l| l.hwnd).collect()).unwrap_or_default();
    for h in gone {
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

pub fn close() {
    if let Some(base) = base() {
        unsafe {
            let _ = KillTimer(Some(base), TIMER_CLOSE);
            let _ = KillTimer(Some(base), TIMER_HOVER);
        }
    }
    truncate(0);
    if let Some(f) = FLYOUT.with(|f| f.borrow_mut().take()) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(f.font.0));
            let _ = DeleteObject(HGDIOBJ(f.small.0));
        }
        strip::set_open(None);
    }
}

/// The configuration changed: redraw the open flyouts' content.
pub fn refresh() {
    if !is_open() {
        return;
    }
    let (look, colors) = strip::current_look();
    with(|f| {
        f.look = look;
        f.colors = colors;
    });
    let n = with(|f| f.levels.len()).unwrap_or(0);
    for i in 0..n {
        if with(|f| i < f.levels.len()) != Some(true) {
            break;
        }
        rebuild(i);
    }
}

pub fn icon_changed(key: &str) {
    with(|f| f.icons.retain(|(k, _), _| k != key));
}

/// The pointer left the flyout's button on the strip: close soon, unless it
/// arrives in a flyout.
pub fn pointer_left_anchor() {
    start_close_timer();
}

fn start_close_timer() {
    if let Some(h) = base() {
        unsafe {
            SetTimer(Some(h), TIMER_CLOSE, CLOSE_DELAY_MS, None);
        }
    }
}

fn cancel_close() {
    if let Some(h) = base() {
        unsafe {
            let _ = KillTimer(Some(h), TIMER_CLOSE);
        }
    }
}

// ---------------------------------------------------------------- layout

fn icon_for(f: &Flyouts, key: &str, size: i32) -> Option<Pixmap> {
    f.icons.get(&(key.to_string(), size)).cloned().flatten()
}

/// Lays out level `idx`, sizes and positions its window, and draws it. A
/// category that no longer exists closes that level and those above it.
fn rebuild(idx: usize) {
    let Some((view, source)) = with(|f| f.levels.get(idx).map(|l| (l.view.clone(), l.source))).flatten() else {
        return;
    };
    // What this level hangs off, on screen.
    let anchor_rc = if idx == 0 {
        with(|f| f.anchor).and_then(strip::button_rect)
    } else {
        // The tile must still be this subcategory (the level below may
        // have been rebuilt).
        with(|f| {
            let below = &f.levels[idx - 1];
            source
                .and_then(|i| below.elems.get(i))
                .filter(|p| matches!((&p.elem, &view), (Elem::Sub(a), View::Category(b)) if a == b))
                .map(|p| (p.rect, below.win))
        })
        .flatten()
        .map(|(rc, win)| {
            // The tile's span along the bar, the flyout's across it, so the
            // next level opens beyond this one, lined up with the tile.
            if strip::edge().vertical() {
                RECT { left: win.left, top: win.top + rc.top, right: win.right, bottom: win.top + rc.bottom }
            } else {
                RECT { left: win.left + rc.left, top: win.top, right: win.left + rc.right, bottom: win.bottom }
            }
        })
    };
    let Some(anchor_rc) = anchor_rc else {
        if idx == 0 {
            close()
        } else {
            truncate(idx)
        }
        return;
    };

    struct Content {
        subs: Vec<(u64, String)>,
        tiles: Vec<(String, String)>,
        empty: bool,
        rows: Vec<(String, String)>,
    }
    let content = app::with(|s| match &view {
        View::Category(id) => crate::tree::find(&s.cfg.categories, *id).map(|c| Content {
            subs: c.children.iter().map(|ch| (ch.id, ch.name.clone())).collect(),
            tiles: c.apps.iter().filter_map(|a| s.catalog.get(a).map(|e| (a.clone(), e.name.clone()))).collect(),
            empty: c.children.is_empty() && c.apps.is_empty(),
            rows: Vec::new(),
        }),
        View::All { .. } => Some(Content {
            subs: Vec::new(),
            tiles: Vec::new(),
            empty: false,
            rows: s.catalog.apps.iter().map(|a| (a.id.clone(), a.name.clone())).collect(),
        }),
    });
    let Some(content) = content else {
        if idx == 0 {
            close()
        } else {
            truncate(idx)
        }
        return;
    };

    let Some((look, dpi, font)) = with(|f| (f.look.clone(), f.dpi, f.font)) else { return };
    let s = |v: i32| scale(v, dpi);
    let border = if look.flyout_border_width > 0 { s(look.flyout_border_width as i32).max(1) } else { 0 };
    let pad = s(4) + border;
    let mut elems: Vec<Placed> = Vec::new();
    let mut y = pad;
    let inner_w;
    let mut all_rows = None;

    match &view {
        View::Category(_) => {
            let tile = (s(84), s(76));
            let tm = s(2);
            let cols = (look.flyout_columns as usize).max(1);
            // Subcategories first, so they sit in the top row, nearest to
            // the flyouts they open above.
            let items: Vec<(Elem, String, String)> = content
                .subs
                .iter()
                .map(|(id, name)| (Elem::Sub(*id), name.clone(), format!("cat:{id}")))
                .chain(content.tiles.iter().map(|(id, name)| (Elem::Tile(id.clone()), name.clone(), id.clone())))
                .collect();
            let tiles_w = cols.min(items.len().max(1)) as i32 * (tile.0 + 2 * tm);
            inner_w = tiles_w.max(canvas::measure("No apps in this category", font).0 + s(16));
            let n = items.len();
            // The subcategories (first) sit in the row nearest where their
            // flyouts open: the top row for a bottom strip, the bottom row
            // (rows flipped) for a top strip. Side strips keep reading order.
            let rows = n.div_ceil(cols);
            let flip = strip::edge() == Edge::Top;
            for ((col, row), (elem, text, icon)) in striplayout::grid(n, cols).into_iter().zip(items) {
                let row = if flip { rows - 1 - row } else { row };
                let left = pad + col as i32 * (tile.0 + 2 * tm) + tm;
                let top = y + row as i32 * (tile.1 + 2 * tm) + tm;
                elems.push(Placed {
                    rect: RECT { left, top, right: left + tile.0, bottom: top + tile.1 },
                    elem,
                    text,
                    icon: Some(icon),
                });
            }
            if n > 0 {
                y += n.div_ceil(cols) as i32 * (tile.1 + 2 * tm);
            }
            if content.empty {
                elems.push(Placed {
                    rect: RECT { left: pad + s(4), top: y + s(4), right: pad + inner_w, bottom: y + s(24) },
                    elem: Elem::Label,
                    text: "No apps in this category".into(),
                    icon: None,
                });
                y += s(28);
            }
        }
        View::All { first_row } => {
            inner_w = s(280) - 2 * pad;
            elems.push(Placed {
                rect: RECT { left: pad + s(4), top: y + s(4), right: pad + inner_w, bottom: y + s(22) },
                elem: Elem::Label,
                text: if content.rows.is_empty() { "No apps found".into() } else { "All apps".into() },
                icon: None,
            });
            y += s(24);
            let row_h = s(28);
            let visible = ((s(480) - y - pad) / row_h).max(1) as usize;
            let first = (*first_row).min(content.rows.len().saturating_sub(visible));
            all_rows = Some((content.rows.len(), visible, first));
            for (id, name) in content.rows.iter().skip(first).take(visible) {
                elems.push(Placed {
                    rect: RECT { left: pad, top: y, right: pad + inner_w, bottom: y + row_h },
                    elem: Elem::Row(id.clone()),
                    text: name.clone(),
                    icon: Some(id.clone()),
                });
                y += row_h;
            }
        }
    }
    let w = inner_w + 2 * pad;
    let h = y + pad;

    // Away from the strip's edge (above a bottom strip, right of a left one…),
    // kept on the monitor.
    let mon = unsafe {
        let m = MonitorFromPoint(POINT { x: anchor_rc.left, y: anchor_rc.top }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(m, &mut mi);
        mi.rcMonitor
    };
    let (x, y) = striplayout::flyout_origin(
        strip::edge(),
        (anchor_rc.left, anchor_rc.top, anchor_rc.right, anchor_rc.bottom),
        (w, h),
        (mon.left, mon.top, mon.right, mon.bottom),
        s(4),
    );
    let win = RECT { left: x, top: y, right: x + w, bottom: y + h };

    // Icons for everything on show, loaded once per size.
    let wanted: Vec<(String, i32)> = elems
        .iter()
        .filter_map(|p| {
            let size = match p.elem {
                Elem::Tile(_) | Elem::Sub(_) => s(look.icon_size as i32),
                Elem::Row(_) => s(20),
                _ => s(16),
            };
            p.icon.clone().map(|k| (k, size))
        })
        .collect();
    let missing: Vec<(String, i32)> =
        with(|f| wanted.into_iter().filter(|k| !f.icons.contains_key(k)).collect()).unwrap_or_default();
    if !missing.is_empty() {
        let sources: Vec<_> = app::with(|st| missing.iter().map(|(k, _)| app::icon_source_for(st, k)).collect());
        let loaded: Vec<_> = missing
            .into_iter()
            .zip(sources)
            .map(|((k, size), src)| ((k, size), src.and_then(|src| canvas::icon_pixmap(&src, size))))
            .collect();
        with(|f| f.icons.extend(loaded));
    }

    let hwnd = with(|f| {
        let l = &mut f.levels[idx];
        l.elems = elems;
        l.win = win;
        if let Some((total, visible, first)) = all_rows {
            l.rows = (total, visible);
            l.view = View::All { first_row: first };
        }
        if l.hover.is_some_and(|i| i >= l.elems.len()) {
            l.hover = None;
        }
        l.hwnd
    });
    if let Some(hw) = hwnd {
        unsafe {
            let _ = SetWindowPos(hw, Some(HWND_TOPMOST), win.left, win.top, w, h, SWP_NOACTIVATE);
            let _ = ShowWindow(hw, SW_SHOWNOACTIVATE);
        }
    }
    render(idx);
}

// ---------------------------------------------------------------- drawing

fn render(idx: usize) {
    FLYOUT.with(|cell| {
        let b = cell.borrow();
        let Some(f) = b.as_ref() else { return };
        let Some(l) = f.levels.get(idx) else { return };
        // The tile whose flyout is open above this one stays highlighted.
        let open_child = f.levels.get(idx + 1).and_then(|c| c.source);
        let (w, h) = (ui::rect_w(&l.win), ui::rect_h(&l.win));
        let Some(mut cv) = Canvas::new(w, h) else { return };
        let d = f.dpi;
        let s = |v: i32| scale(v, d);
        let c = f.colors;
        let radius = s(f.look.flyout_corner_radius as i32) as f32;
        let border =
            if f.look.flyout_border_width > 0 { s(f.look.flyout_border_width as i32).max(1) as f32 } else { 0.0 };
        cv.fill_round_rect(0.0, 0.0, w as f32, h as f32, radius, c.background);
        cv.stroke_round_rect(0.0, 0.0, w as f32, h as f32, radius, border, c.flyout_border);

        let r4 = s(4) as f32;
        for (i, p) in l.elems.iter().enumerate() {
            let rc = p.rect;
            let fill = if open_child == Some(i) {
                Some(c.pressed)
            } else if l.hover == Some(i) && p.elem != Elem::Label {
                Some(c.hover)
            } else {
                None
            };
            if let Some(fill) = fill {
                cv.fill_round_rect(
                    rc.left as f32,
                    rc.top as f32,
                    ui::rect_w(&rc) as f32,
                    ui::rect_h(&rc) as f32,
                    r4,
                    fill,
                );
            }
            let icon = p.icon.as_deref();
            match &p.elem {
                Elem::Tile(_) | Elem::Sub(_) => {
                    let want = s(f.look.icon_size as i32);
                    let size = want.min(ui::rect_w(&rc) - s(16));
                    let x = rc.left + (ui::rect_w(&rc) - size) / 2;
                    let y = rc.top + s(4);
                    let is_sub = matches!(p.elem, Elem::Sub(_));
                    match icon.and_then(|k| icon_for(f, k, want)) {
                        Some(img) => cv.image(&img, x, y, size, 1.0),
                        None if is_sub => cv.folder(x as f32, y as f32, size as f32, c.accent),
                        None => {}
                    }
                    if is_sub {
                        // ▾, as on the strip's category buttons.
                        let cs = s(7) as f32;
                        cv.chevron(x as f32 + size as f32 + s(2) as f32, y as f32 + size as f32 - cs / 2.0, cs, c.text);
                    }
                    // Two lines reserved for the name, as in the original.
                    let trc =
                        RECT { left: rc.left + s(4), top: y + size + s(4), right: rc.right - s(4), bottom: rc.bottom };
                    cv.text(&p.text, trc, f.small, c.text, DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS);
                }
                Elem::Row(_) => {
                    let size = s(20);
                    let x = rc.left + s(8);
                    let y = rc.top + (ui::rect_h(&rc) - size) / 2;
                    if let Some(img) = icon.and_then(|k| icon_for(f, k, size)) {
                        cv.image(&img, x, y, size, 1.0);
                    }
                    let trc = RECT { left: x + size + s(8), right: rc.right - s(4), ..rc };
                    cv.text(&p.text, trc, f.font, c.text, DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                }
                Elem::Label => {
                    let trc = RECT { left: rc.left + s(4), ..rc };
                    cv.text(&p.text, trc, f.font, c.subtle, DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                }
            }
        }
        // A thin scroll indicator for a long All list.
        let (total, visible) = l.rows;
        if let View::All { first_row } = l.view
            && total > visible
        {
            let track_top = s(28) as f32;
            let track_h = h as f32 - track_top - s(6) as f32;
            let thumb_h = (track_h * visible as f32 / total as f32).max(s(16) as f32);
            let y = track_top + (track_h - thumb_h) * first_row as f32 / (total - visible) as f32;
            cv.fill_round_rect(w as f32 - s(6) as f32, y, s(3) as f32, thumb_h, s(2) as f32, c.subtle);
        }
        cv.present(l.hwnd, l.win.left, l.win.top);
    });
}

// ---------------------------------------------------------------- input

fn elem_at(idx: usize, x: i32, y: i32) -> Option<usize> {
    with(|f| {
        f.levels.get(idx)?.elems.iter().position(|p| {
            p.elem != Elem::Label && x >= p.rect.left && x < p.rect.right && y >= p.rect.top && y < p.rect.bottom
        })
    })
    .flatten()
}

fn elem(idx: usize, i: usize) -> Option<Elem> {
    with(|f| f.levels.get(idx)?.elems.get(i).map(|p| p.elem.clone())).flatten()
}

/// Makes the levels above `idx` match what the pointer rests on in it: the
/// flyout of a hovered subcategory opens above; anything else closes what was
/// open above.
fn follow_hover(idx: usize) {
    let Some((hover, child)) =
        with(|f| f.levels.get(idx).map(|l| (l.hover, f.levels.get(idx + 1).and_then(|c| c.source)))).flatten()
    else {
        return;
    };
    match hover.and_then(|i| elem(idx, i).map(|e| (i, e))) {
        Some((i, Elem::Sub(id))) => open_sub(idx, i, id),
        // Pointer on something else here: close what's above it. Resting on
        // empty space or padding keeps it, so the pointer can travel to it.
        Some(_) if child.is_some() => {
            truncate(idx + 1);
            render(idx);
        }
        _ => {}
    }
}

/// Opens the flyout of subcategory `id` (tile `i` of level `idx`) above it.
fn open_sub(idx: usize, i: usize, id: u64) {
    let child = with(|f| f.levels.get(idx + 1).map(|c| c.source)).flatten();
    if child == Some(Some(i)) {
        return; // already open
    }
    truncate(idx + 1);
    if push_level(View::Category(id), Some(i)) {
        rebuild(idx + 1);
    }
    render(idx);
}

fn activate(idx: usize, i: usize) {
    match elem(idx, i) {
        Some(Elem::Sub(id)) => open_sub(idx, i, id),
        Some(Elem::Tile(id) | Elem::Row(id)) => {
            close();
            app::launch_app(&id);
        }
        _ => {}
    }
}

fn row_menu(app_id: &str) {
    const PIN: usize = 1;
    const UNPIN: usize = 2;
    let pinned = app::with(|s| s.cfg.pinned.iter().any(|p| p == app_id));
    let chosen = unsafe {
        let m = CreatePopupMenu().unwrap_or_default();
        let (id, text) = if pinned { (UNPIN, "Unpin from strip") } else { (PIN, "Pin to strip") };
        let t = wide(text);
        let _ = InsertMenuW(m, 0, MF_BYPOSITION | MF_STRING, id, PCWSTR(t.as_ptr()));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let owner = app::main_hwnd();
        let _ = SetForegroundWindow(owner);
        let r = TrackPopupMenuEx(m, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, pt.x, pt.y, owner, None).0 as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        r
    };
    match chosen {
        PIN => {
            app::with(|s| s.cfg.pin(app_id));
            app::save();
        }
        UNPIN => {
            app::with(|s| s.cfg.unpin(app_id));
            app::save();
        }
        _ => {}
    }
}

fn pointer_inside() -> bool {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let over_flyout = with(|f| {
        f.levels.iter().any(|l| pt.x >= l.win.left && pt.x < l.win.right && pt.y >= l.win.top && pt.y < l.win.bottom)
    })
    .unwrap_or(false);
    over_flyout || with(|f| f.anchor).is_some_and(strip::pointer_over)
}

fn mouse_xy(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32)
}

thread_local! {
    /// Which level the pending hover timer is for.
    static HOVER_LEVEL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let level = level_of(hwnd);
    match (msg, level) {
        (WM_MOUSEACTIVATE, _) => LRESULT(MA_NOACTIVATE as isize),
        (WM_MOUSEMOVE, Some(idx)) => {
            cancel_close();
            let (x, y) = mouse_xy(lparam);
            let under = elem_at(idx, x, y);
            let (changed, start_leave) = with(|f| {
                let l = &mut f.levels[idx];
                let changed = l.hover != under;
                l.hover = under;
                (changed, !std::mem::replace(&mut l.tracking_leave, true))
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
                render(idx);
                if let Some(b) = base() {
                    HOVER_LEVEL.with(|h| h.set(idx));
                    unsafe {
                        SetTimer(Some(b), TIMER_HOVER, HOVER_DELAY_MS, None);
                    }
                }
            }
            LRESULT(0)
        }
        (WM_MOUSELEAVE, Some(idx)) => {
            with(|f| {
                let l = &mut f.levels[idx];
                l.tracking_leave = false;
                l.hover = None;
            });
            render(idx);
            start_close_timer();
            LRESULT(0)
        }
        (WM_TIMER, Some(0)) => {
            unsafe {
                let _ = KillTimer(Some(hwnd), wparam.0);
            }
            match wparam.0 {
                TIMER_CLOSE if !pointer_inside() => close(),
                TIMER_HOVER => follow_hover(HOVER_LEVEL.with(|h| h.get())),
                _ => {}
            }
            LRESULT(0)
        }
        (WM_LBUTTONUP, Some(idx)) => {
            let (x, y) = mouse_xy(lparam);
            if let Some(i) = elem_at(idx, x, y) {
                activate(idx, i);
            }
            LRESULT(0)
        }
        (WM_RBUTTONUP, Some(idx)) => {
            let (x, y) = mouse_xy(lparam);
            if let Some(Elem::Tile(id) | Elem::Row(id)) = elem_at(idx, x, y).and_then(|i| elem(idx, i)) {
                row_menu(&id);
            }
            LRESULT(0)
        }
        (WM_MOUSEWHEEL, Some(idx)) => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let changed = with(|f| {
                let l = &mut f.levels[idx];
                match &mut l.view {
                    View::All { first_row } => {
                        let max = l.rows.0.saturating_sub(l.rows.1);
                        let new = if delta > 0 { first_row.saturating_sub(3) } else { (*first_row + 3).min(max) };
                        std::mem::replace(first_row, new) != new
                    }
                    _ => false,
                }
            })
            .unwrap_or(false);
            if changed {
                rebuild(idx);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
