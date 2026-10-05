//! Flyouts from the icon strip, in the original FlexTaskbar style:
//!
//! - a **category** flyout: its subcategories and apps as tiles (large icon,
//!   name underneath, `flyout_columns` per row; subcategories first, marked
//!   with an arrow badge). Resting the pointer on a subcategory opens *its* flyout beyond
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
use crate::appearance::{Appearance, Colors, FlyoutAnim};
use crate::flyanim;
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
    InsertMenuW, KillTimer, MA_NOACTIVATE, MF_BYPOSITION, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
    PostMessageW, RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetForegroundWindow, SetTimer, SetWindowPos,
    ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const WM_MOUSELEAVE: u32 = 0x02A3;
/// Timers, all on the level-0 window.
const TIMER_CLOSE: usize = 1;
const TIMER_HOVER: usize = 2;
/// Frames of the opening animation.
const TIMER_ANIM: usize = 3;
const ANIM_FRAME_MS: u32 = 10;
const CLOSE_DELAY_MS: u32 = 300;
/// After a menu from a flyout closes, the pointer is often outside the
/// flyout (where the menu was): give it this long to come back.
const MENU_GRACE_MS: u64 = 2000;
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
    /// A section heading in the All list (when grouped).
    Heading,
    /// The All list's Sort and Show buttons.
    SortButton,
    ShowButton,
}

/// One line of the All list.
enum AllLine {
    Heading(String),
    App { id: String, name: String, note: &'static str },
}

thread_local! {
    /// A menu from a flyout is open: don't close the flyout under it.
    static MENU_UP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// When the last such menu closed.
    static MENU_CLOSED: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// Still within `MENU_GRACE_MS` of a menu closing.
fn menu_grace() -> bool {
    MENU_CLOSED.with(|m| m.get()).is_some_and(|t| t.elapsed().as_millis() < MENU_GRACE_MS as u128)
}

struct Placed {
    rect: RECT,
    elem: Elem,
    text: String,
    icon: Option<String>,
    /// A small right-aligned note (the app's kind in the All list).
    note: &'static str,
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
    /// The finished drawing, which the opening animation is made from.
    frame: Option<Pixmap>,
    /// Whether the window has been placed on screen yet.
    shown: bool,
    /// The opening animation under way: when it began, and where the
    /// button it opens from lies across the flyout (window pixels).
    anim: Option<(std::time::Instant, (f32, f32))>,
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
            frame: None,
            shown: false,
            anim: None,
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
            let _ = KillTimer(Some(base), TIMER_ANIM);
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
        rows: Vec<AllLine>,
        /// Apps shown / in total, and the Sort and Show buttons' texts.
        counts: (usize, usize),
        buttons: (String, String),
    }
    let content = app::with(|s| match &view {
        View::Category(id) => crate::tree::find(&s.cfg.categories, *id).map(|c| Content {
            subs: c.children.iter().map(|ch| (ch.id, ch.name.clone())).collect(),
            tiles: c.apps.iter().filter_map(|a| s.catalog.get(a).map(|e| (a.clone(), e.name.clone()))).collect(),
            empty: c.children.is_empty() && c.apps.is_empty(),
            rows: Vec::new(),
            counts: (0, 0),
            buttons: Default::default(),
        }),
        View::All { .. } => {
            let (rows, counts) = all_lines(s);
            let v = &s.cfg.settings.all_apps;
            let buttons = (
                format!("Sort: {}", v.sort.short()),
                if v.filtering() { "Show: some".to_string() } else { "Show: all".to_string() },
            );
            Some(Content { subs: Vec::new(), tiles: Vec::new(), empty: false, rows, counts, buttons })
        }
    });
    let mon = unsafe {
        let m = MonitorFromPoint(POINT { x: anchor_rc.left, y: anchor_rc.top }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(m, &mut mi);
        mi.rcMonitor
    };
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
            // Subcategories sit nearest the flyouts they open: first (the top
            // row) above a bottom bar or beside a side bar; below a top bar the
            // apps fill the top rows first and the subcategories follow,
            // nearest the next level opening further down.
            let subs = content.subs.iter().map(|(id, name)| (Elem::Sub(*id), name.clone(), format!("cat:{id}")));
            let apps = content.tiles.iter().map(|(id, name)| (Elem::Tile(id.clone()), name.clone(), id.clone()));
            let items: Vec<(Elem, String, String)> =
                if strip::edge() == Edge::Top { apps.chain(subs).collect() } else { subs.chain(apps).collect() };
            // A horizontal strip for a top or bottom bar, a vertical one for
            // a side bar (see `flyout_cells`).
            let n = items.len();
            let edge = strip::edge();
            // A side bar's flyout is a single column, wrapping only once it
            // would be taller than the screen.
            let per_line = if edge.vertical() {
                (((mon.bottom - mon.top) - 2 * pad) / (tile.1 + 2 * tm)).max(1) as usize
            } else {
                cols
            };
            let cells = striplayout::flyout_cells(n, per_line, edge);
            let used_cols = cells.iter().map(|c| c.0 + 1).max().unwrap_or(1);
            let used_rows = cells.iter().map(|c| c.1 + 1).max().unwrap_or(0);
            let tiles_w = used_cols as i32 * (tile.0 + 2 * tm);
            inner_w = if content.empty { canvas::measure("No apps in this category", font).0 + s(16) } else { tiles_w };
            for ((col, row), (elem, text, icon)) in cells.into_iter().zip(items) {
                let left = pad + col as i32 * (tile.0 + 2 * tm) + tm;
                let top = y + row as i32 * (tile.1 + 2 * tm) + tm;
                elems.push(Placed {
                    rect: RECT { left, top, right: left + tile.0, bottom: top + tile.1 },
                    elem,
                    text,
                    icon: Some(icon),
                    note: "",
                });
            }
            y += used_rows as i32 * (tile.1 + 2 * tm);
            if content.empty {
                elems.push(Placed {
                    rect: RECT { left: pad + s(4), top: y + s(4), right: pad + inner_w, bottom: y + s(24) },
                    elem: Elem::Label,
                    text: "No apps in this category".into(),
                    icon: None,
                    note: "",
                });
                y += s(28);
            }
        }
        View::All { first_row } => {
            inner_w = s(360) - 2 * pad;
            // Title, then the Sort and Show buttons on the right.
            let (shown, total) = content.counts;
            let title = if total == 0 {
                "No apps found".to_string()
            } else if shown < total {
                format!("{shown} of {total} apps")
            } else {
                format!("All apps ({total})")
            };
            let bh = s(24);
            let (sort_text, show_text) = &content.buttons;
            let bw = |t: &str| canvas::measure(t, font).0 + s(20);
            let show_left = pad + inner_w - bw(show_text);
            let sort_left = show_left - s(4) - bw(sort_text);
            elems.push(Placed {
                rect: RECT { left: pad + s(4), top: y, right: sort_left - s(4), bottom: y + bh },
                elem: Elem::Label,
                text: title,
                icon: None,
                note: "",
            });
            for (left, w, elem, text) in [
                (sort_left, bw(sort_text), Elem::SortButton, sort_text.clone()),
                (show_left, bw(show_text), Elem::ShowButton, show_text.clone()),
            ] {
                elems.push(Placed {
                    rect: RECT { left, top: y, right: left + w, bottom: y + bh },
                    elem,
                    text,
                    icon: None,
                    note: "",
                });
            }
            y += bh + s(4);
            let row_h = s(28);
            let visible = ((s(480) - y - pad) / row_h).max(1) as usize;
            let first = (*first_row).min(content.rows.len().saturating_sub(visible));
            all_rows = Some((content.rows.len(), visible, first));
            for line in content.rows.iter().skip(first).take(visible) {
                let rect = RECT { left: pad, top: y, right: pad + inner_w, bottom: y + row_h };
                elems.push(match line {
                    AllLine::Heading(h) => Placed { rect, elem: Elem::Heading, text: h.clone(), icon: None, note: "" },
                    AllLine::App { id, name, note } => {
                        Placed { rect, elem: Elem::Row(id.clone()), text: name.clone(), icon: Some(id.clone()), note }
                    }
                });
                y += row_h;
            }
        }
    }
    let w = inner_w + 2 * pad;
    let h = y + pad;

    // Away from the strip's edge (above a bottom strip, right of a left one…),
    // kept on the monitor.
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

    let animated = look.flyout_animation != FlyoutAnim::Off;
    let starts = with(|f| {
        let l = &mut f.levels[idx];
        let first = !std::mem::replace(&mut l.shown, true);
        if first && animated {
            // Where the button lies across the flyout (along the bar).
            let across = if strip::edge().vertical() {
                ((anchor_rc.top - win.top) as f32, (anchor_rc.bottom - win.top) as f32)
            } else {
                ((anchor_rc.left - win.left) as f32, (anchor_rc.right - win.left) as f32)
            };
            l.anim = Some((std::time::Instant::now(), across));
        }
        first && animated
    })
    .unwrap_or(false);
    if starts && let Some(b) = base() {
        unsafe {
            SetTimer(Some(b), TIMER_ANIM, ANIM_FRAME_MS, None);
        }
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
    let drawn = FLYOUT.with(|cell| {
        let b = cell.borrow();
        let f = b.as_ref()?;
        let l = f.levels.get(idx)?;
        // The tile whose flyout is open above this one stays highlighted.
        let open_child = f.levels.get(idx + 1).and_then(|c| c.source);
        let (w, h) = (ui::rect_w(&l.win), ui::rect_h(&l.win));
        let mut cv = Canvas::new(w, h)?;
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
            } else if l.hover == Some(i) && interactive(&p.elem) {
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
                        // The same badge as the strip's category buttons, pointing
                        // where this subcategory's flyout opens.
                        super::indicator::draw(&mut cv, &f.look, &c, strip::edge(), x, y, size);
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
                    match icon.and_then(|k| icon_for(f, k, size)) {
                        Some(img) => cv.image(&img, x, y, size, 1.0),
                        None => {
                            // No icon: the name's first letter on a tile.
                            cv.fill_round_rect(x as f32, y as f32, size as f32, size as f32, s(4) as f32, c.pressed);
                            let letter: String =
                                p.text.chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
                            let lrc = RECT { left: x, top: y, right: x + size, bottom: y + size };
                            cv.text(&letter, lrc, f.small, c.text, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                        }
                    }
                    // The kind (Store app, Chrome web app…) on the right, subtle.
                    let note_w = if p.note.is_empty() { 0 } else { canvas::measure(p.note, f.small).0 + s(12) };
                    if note_w > 0 {
                        // Clear of the scroll indicator at the right edge.
                        // Placed by its measure but drawn left-aligned with room to
                        // spare: a final "t" can run a pixel past the measure.
                        let text_w = note_w - s(12);
                        let nrc = RECT { left: rc.right - s(14) - text_w, right: rc.right - s(8), ..rc };
                        cv.text(p.note, nrc, f.small, c.subtle, DT_VCENTER | DT_SINGLELINE);
                    }
                    let trc = RECT { left: x + size + s(8), right: rc.right - s(4) - note_w - s(6), ..rc };
                    cv.text(&p.text, trc, f.font, c.text, DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                }
                Elem::Label => {
                    let trc = RECT { left: rc.left + s(4), ..rc };
                    cv.text(&p.text, trc, f.font, c.subtle, DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                }
                Elem::Heading => {
                    // A section title in the accent colour, with a rule after it.
                    let trc = RECT { left: rc.left + s(8), top: rc.top + s(6), ..rc };
                    cv.text(&p.text, trc, f.small, c.accent, DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                    let tw = canvas::measure(&p.text, f.small).0.min(ui::rect_w(&rc) - s(24));
                    let ly = (rc.top + rc.bottom) as f32 / 2.0 + s(3) as f32;
                    let lx = (rc.left + s(16) + tw) as f32;
                    cv.fill_round_rect(lx, ly, rc.right as f32 - s(8) as f32 - lx, 1.0, 0.0, c.border);
                }
                Elem::SortButton | Elem::ShowButton => {
                    if l.hover != Some(i) {
                        cv.fill_round_rect(
                            rc.left as f32,
                            rc.top as f32,
                            ui::rect_w(&rc) as f32,
                            ui::rect_h(&rc) as f32,
                            r4,
                            c.hover,
                        );
                    }
                    cv.text(&p.text, rc, f.font, c.text, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
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
        Some(cv.pix)
    });
    if let Some(pix) = drawn {
        with(|f| f.levels.get_mut(idx).map(|l| l.frame = Some(pix)));
        present(idx);
    }
}

/// Puts level `idx` on screen: its finished drawing, or the opening
/// animation's current frame made from it. Returns whether it is still
/// animating.
fn present(idx: usize) -> bool {
    FLYOUT.with(|cell| {
        let mut b = cell.borrow_mut();
        let Some(f) = b.as_mut() else { return false };
        let (style, ms) = (f.look.flyout_animation, f.look.flyout_animation_ms.clamp(60, 600));
        let Some(l) = f.levels.get_mut(idx) else { return false };
        let Some(pix) = &l.frame else { return false };
        let t = l.anim.map(|(start, _)| start.elapsed().as_secs_f32() * 1000.0 / ms as f32).unwrap_or(1.0);
        let anim = l.anim.filter(|_| t < 1.0);
        l.anim = anim;
        match anim {
            Some((_, across)) => animation_frame(pix, style, t, across).present(l.hwnd, l.win.left, l.win.top),
            None => canvas::present_pixmap(pix, l.hwnd, l.win.left, l.win.top),
        }
        anim.is_some()
    })
}

/// One frame of the opening animation, drawn from the finished flyout.
fn animation_frame(pix: &Pixmap, style: FlyoutAnim, t: f32, across: (f32, f32)) -> Canvas {
    use resvg::tiny_skia::{FilterQuality, Paint, Pattern, Rect, SpreadMode, Transform};
    let (w, h) = (pix.width() as f32, pix.height() as f32);
    let (dx, dy) = strip::edge().opening();
    let vertical = dy != 0.0; // opens up or down: "along" is y
    let (len, width) = if vertical { (h, w) } else { (w, h) };
    let frame = flyanim::frame(style, t, len, width, across);
    let mut out = Canvas { pix: Pixmap::new(pix.width(), pix.height()).unwrap_or_else(|| pix.clone()) };
    // "Along" counts from the bar's side: flip it when opening up or left.
    let flip = if vertical { dy < 0.0 } else { dx < 0.0 };
    let span = |(a, b): (f32, f32)| if flip { (len - b, len - a) } else { (a, b) };
    for band in &frame.bands {
        let (s0, s1) = span(band.src);
        let (d0, d1) = span(band.along);
        let (c0, c1) = band.across;
        if s1 - s0 < 0.01 || d1 - d0 < 0.01 || c1 - c0 < 0.01 {
            continue;
        }
        // Source slice (window pixels) → destination rectangle.
        let ka = (d1 - d0) / (s1 - s0);
        let kc = (c1 - c0) / width;
        let (rect, t) = if vertical {
            (Rect::from_ltrb(c0, d0, c1, d1), Transform::from_row(kc, 0.0, 0.0, ka, c0, d0 - s0 * ka))
        } else {
            (Rect::from_ltrb(d0, c0, d1, c1), Transform::from_row(ka, 0.0, 0.0, kc, d0 - s0 * ka, c0))
        };
        let Some(rect) = rect else { continue };
        let paint = Paint {
            shader: Pattern::new(pix.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, frame.opacity, t),
            anti_alias: false,
            ..Default::default()
        };
        out.pix.fill_rect(rect, &paint, Transform::identity(), None);
    }
    out
}

/// Advances every opening animation by a frame.
fn animate() {
    let n = with(|f| f.levels.len()).unwrap_or(0);
    let mut running = false;
    for i in 0..n {
        if with(|f| f.levels.get(i).is_some_and(|l| l.anim.is_some())) == Some(true) {
            running |= present(i);
        }
    }
    if !running && let Some(b) = base() {
        unsafe {
            let _ = KillTimer(Some(b), TIMER_ANIM);
        }
    }
}

// ---------------------------------------------------------------- input

fn elem_at(idx: usize, x: i32, y: i32) -> Option<usize> {
    with(|f| {
        f.levels.get(idx)?.elems.iter().position(|p| {
            interactive(&p.elem) && x >= p.rect.left && x < p.rect.right && y >= p.rect.top && y < p.rect.bottom
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

/// Whether the pointer can hover or click an element.
fn interactive(e: &Elem) -> bool {
    !matches!(e, Elem::Label | Elem::Heading)
}

/// The All list's lines with the current sort and filter, and how many apps
/// are shown out of all of them.
fn all_lines(s: &app::State) -> (Vec<AllLine>, (usize, usize)) {
    use crate::allview::{Entry, KindGroup, Line, lines};
    let roots: Vec<(u64, String)> = s.cfg.categories.iter().map(|c| (c.id, c.name.clone())).collect();
    let in_root: Vec<(u64, std::collections::HashSet<String>)> =
        s.cfg.categories.iter().map(|c| (c.id, crate::tree::subtree_apps(c).into_iter().collect())).collect();
    let apps = &s.catalog.apps;
    let entries: Vec<Entry> = apps
        .iter()
        .map(|a| Entry {
            name: &a.name,
            group: KindGroup::of(a.kind),
            roots: in_root.iter().filter(|(_, set)| set.contains(&a.id)).map(|(id, _)| *id).collect(),
            recent: s.cfg.recents.iter().position(|r| *r == a.id),
        })
        .collect();
    let ls = lines(&entries, &s.cfg.settings.all_apps, &roots);
    let mut shown = std::collections::HashSet::new();
    let out = ls
        .into_iter()
        .map(|l| match l {
            Line::Header(h) => AllLine::Heading(h),
            Line::App(i) => {
                shown.insert(i);
                let a = &apps[i];
                AllLine::App { id: a.id.clone(), name: a.name.clone(), note: a.kind.label() }
            }
        })
        .collect();
    (out, (shown.len(), apps.len()))
}

/// Shows a popup menu at the pointer, keeping the flyouts open meanwhile.
/// `items`: (id, text, checked); id 0 is a separator. Returns the chosen id.
fn popup(items: &[(usize, String, bool)]) -> usize {
    MENU_UP.with(|m| m.set(true));
    let chosen = unsafe {
        let m = CreatePopupMenu().unwrap_or_default();
        for (pos, (id, text, checked)) in items.iter().enumerate() {
            if *id == 0 {
                let _ = InsertMenuW(m, pos as u32, MF_BYPOSITION | MF_SEPARATOR, 0, PCWSTR::null());
                continue;
            }
            let t = wide(text);
            let flags = MF_BYPOSITION | MF_STRING | if *checked { MF_CHECKED } else { MF_UNCHECKED };
            let _ = InsertMenuW(m, pos as u32, flags, *id, PCWSTR(t.as_ptr()));
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let owner = app::main_hwnd();
        let _ = SetForegroundWindow(owner);
        let r = TrackPopupMenuEx(m, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, pt.x, pt.y, owner, None).0 as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        r
    };
    MENU_UP.with(|m| m.set(false));
    MENU_CLOSED.with(|m| m.set(Some(std::time::Instant::now())));
    start_close_timer();
    chosen
}

/// The All list's settings changed: save, and show it from the top.
fn all_view_changed(f: impl FnOnce(&mut crate::allview::AllAppsView)) {
    app::with(|s| f(&mut s.cfg.settings.all_apps));
    with(|fl| {
        for l in &mut fl.levels {
            if let View::All { first_row } = &mut l.view {
                *first_row = 0;
            }
        }
    });
    app::save(); // refreshes the open flyout
}

fn sort_menu() {
    use crate::allview::AllSort;
    let current = app::with(|s| s.cfg.settings.all_apps.sort);
    let items: Vec<(usize, String, bool)> = AllSort::ALL
        .iter()
        .enumerate()
        .map(|(i, k)| (i + 1, format!("Sort by {}", k.title()), *k == current))
        .collect();
    let chosen = popup(&items);
    if let Some(&k) = chosen.checked_sub(1).and_then(|i| AllSort::ALL.get(i)) {
        all_view_changed(|v| v.sort = k);
    }
}

fn show_menu() {
    use crate::allview::KindGroup;
    const KIND: usize = 100;
    const CATEGORY: usize = 1000;
    const UNCATEGORIZED: usize = 2;
    const EVERYTHING: usize = 3;
    let (v, roots) = app::with(|s| {
        (s.cfg.settings.all_apps.clone(), s.cfg.categories.iter().map(|c| (c.id, c.name.clone())).collect::<Vec<_>>())
    });
    let mut items: Vec<(usize, String, bool)> = KindGroup::ALL
        .iter()
        .enumerate()
        .map(|(i, k)| (KIND + i, k.title().to_string(), !v.hidden_kinds.contains(k)))
        .collect();
    items.push((0, String::new(), false));
    for (i, (id, name)) in roots.iter().enumerate() {
        items.push((CATEGORY + i, format!("In “{name}”"), !v.hidden_categories.contains(id)));
    }
    items.push((UNCATEGORIZED, "Not in a category".into(), !v.hide_uncategorized));
    items.push((0, String::new(), false));
    items.push((EVERYTHING, "Show everything".into(), false));
    let chosen = popup(&items);
    match chosen {
        EVERYTHING => all_view_changed(|v| v.show_all()),
        UNCATEGORIZED => all_view_changed(|v| v.hide_uncategorized = !v.hide_uncategorized),
        c if (KIND..KIND + KindGroup::ALL.len()).contains(&c) => {
            all_view_changed(|v| v.toggle_kind(KindGroup::ALL[c - KIND]))
        }
        c if (CATEGORY..CATEGORY + roots.len()).contains(&c) => {
            let id = roots[c - CATEGORY].0;
            all_view_changed(|v| v.toggle_category(id))
        }
        _ => {}
    }
}

fn activate(idx: usize, i: usize) {
    match elem(idx, i) {
        Some(Elem::SortButton) => sort_menu(),
        Some(Elem::ShowButton) => show_menu(),
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
    let (id, text) = if pinned { (UNPIN, "Unpin from strip") } else { (PIN, "Pin to strip") };
    let chosen = popup(&[(id, text.to_string(), false)]);
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
    if MENU_UP.with(|m| m.get()) {
        return true;
    }
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
        (WM_TIMER, Some(0)) if wparam.0 == TIMER_ANIM => {
            animate();
            LRESULT(0)
        }
        (WM_TIMER, Some(0)) => {
            unsafe {
                let _ = KillTimer(Some(hwnd), wparam.0);
            }
            match wparam.0 {
                TIMER_CLOSE if !pointer_inside() => {
                    if menu_grace() {
                        start_close_timer();
                    } else {
                        close();
                    }
                }
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
