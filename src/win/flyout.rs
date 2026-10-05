//! Flyouts from the icon strip, in the original FlexTaskbar style:
//!
//! - a **category** flyout: its subcategories as a row of buttons, its apps as
//!   tiles (large icon, name underneath, `flyout_columns` per row), and
//!   "Manage Category" at the bottom. Clicking (or, if set, resting on) a
//!   subcategory opens it in the same flyout, with a Back button;
//! - the **All** flyout: every app as a scrollable list.
//!
//! A flyout opens just above its button (below it for a top strip) and closes
//! shortly after the pointer has left both, as the original's did. It is a
//! per-pixel-alpha layered window drawn with `canvas`, using the strip's
//! appearance settings.

use super::app;
use super::canvas::{self, Canvas};
use super::strip;
use super::ui::{self, scale, wide};
use crate::appearance::{Appearance, Colors, SubcategoryOpen};
use crate::striplayout::{self, Hit};
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
const TIMER_CLOSE: usize = 1;
const TIMER_SUB: usize = 2;
const CLOSE_DELAY_MS: u32 = 300;
const SUB_HOVER_MS: u32 = 400;

#[derive(Clone, PartialEq)]
enum View {
    /// A category; `back` holds the categories drilled through to get here.
    Category {
        id: u64,
        back: Vec<u64>,
    },
    All {
        first_row: usize,
    },
}

#[derive(Clone, PartialEq)]
enum Elem {
    Back,
    Sub(u64),
    Tile(String),
    Manage(u64),
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

struct Flyout {
    hwnd: HWND,
    view: View,
    anchor: Hit,
    elems: Vec<Placed>,
    hover: Option<usize>,
    /// Window rectangle on screen.
    win: RECT,
    look: Appearance,
    colors: Colors,
    dpi: u32,
    font: HFONT,
    small: HFONT,
    /// Icons by (cache key, pixel size).
    icons: HashMap<(String, i32), Option<Pixmap>>,
    tracking_leave: bool,
    /// Total rows in the All list, and how many fit.
    rows: (usize, usize),
}

thread_local! {
    static FLYOUT: RefCell<Option<Flyout>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Flyout) -> R) -> Option<R> {
    FLYOUT.with(|s| s.borrow_mut().as_mut().map(f))
}

pub fn is_open() -> bool {
    FLYOUT.with(|f| f.borrow().is_some())
}

fn hwnd() -> Option<HWND> {
    FLYOUT.with(|f| f.borrow().as_ref().map(|f| f.hwnd))
}

// ---------------------------------------------------------------- opening

pub fn open_category(id: u64, anchor: Hit) {
    let already = FLYOUT
        .with(|f| f.borrow().as_ref().is_some_and(|f| f.anchor == anchor && matches!(&f.view, View::Category { .. })));
    if already {
        cancel_close();
        return;
    }
    show(View::Category { id, back: Vec::new() }, anchor);
}

pub fn toggle_all(anchor: Hit) {
    let open_here = FLYOUT.with(|f| f.borrow().as_ref().is_some_and(|f| f.anchor == anchor));
    if open_here {
        close();
    } else {
        show(View::All { first_row: 0 }, anchor);
    }
}

fn show(view: View, anchor: Hit) {
    if hwnd().is_none() && !create(anchor) {
        return;
    }
    with(|f| {
        f.view = view;
        f.anchor = anchor;
        f.hover = None;
    });
    strip::set_open(Some(anchor));
    cancel_close();
    rebuild();
}

fn create(anchor: Hit) -> bool {
    let class = wide("FlexTaskbar.Flyout");
    let (look, colors) = strip::current_look();
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
        match CreateWindowExW(
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
        ) {
            Ok(h) => h,
            Err(_) => return false,
        }
    };
    let dpi = ui::dpi_of(hwnd);
    FLYOUT.with(|f| {
        *f.borrow_mut() = Some(Flyout {
            hwnd,
            view: View::All { first_row: 0 },
            anchor,
            elems: Vec::new(),
            hover: None,
            win: RECT::default(),
            look,
            colors,
            dpi,
            font: canvas::font(scale(13, dpi), false),
            small: canvas::font(scale(11, dpi), false),
            icons: HashMap::new(),
            tracking_leave: false,
            rows: (0, 0),
        })
    });
    true
}

pub fn close() {
    let taken = FLYOUT.with(|f| f.borrow_mut().take());
    if let Some(f) = taken {
        unsafe {
            let _ = DestroyWindow(f.hwnd);
            let _ = DeleteObject(HGDIOBJ(f.font.0));
            let _ = DeleteObject(HGDIOBJ(f.small.0));
        }
        strip::set_open(None);
    }
}

/// The configuration changed: redraw the open flyout's content.
pub fn refresh() {
    if is_open() {
        rebuild();
    }
}

pub fn icon_changed(key: &str) {
    with(|f| f.icons.retain(|(k, _), _| k != key));
}

/// The pointer left the flyout's button on the strip: close soon, unless it
/// arrives in the flyout.
pub fn pointer_left_anchor() {
    if let Some(h) = hwnd() {
        unsafe {
            SetTimer(Some(h), TIMER_CLOSE, CLOSE_DELAY_MS, None);
        }
    }
}

fn cancel_close() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = KillTimer(Some(h), TIMER_CLOSE);
        }
    }
}

// ---------------------------------------------------------------- layout

fn icon_for(f: &Flyout, key: &str, size: i32) -> Option<Pixmap> {
    f.icons.get(&(key.to_string(), size)).cloned().flatten()
}

/// Lays out the current view, sizes and positions the window, and draws it.
fn rebuild() {
    let Some(view) = with(|f| f.view.clone()) else { return };
    let Some(anchor_rc) = with(|f| f.anchor).and_then(strip::button_rect) else {
        close();
        return;
    };
    let (look, colors) = strip::current_look();
    // Gather what to show (outside any borrow of the flyout).
    struct Content {
        /// The category's name, shown after Back once drilled in.
        back: Option<String>,
        subs: Vec<(u64, String)>,
        tiles: Vec<(String, String)>,
        manage: Option<u64>,
        empty: bool,
        rows: Vec<(String, String)>,
        missing: bool,
    }
    let content = app::with(|s| match &view {
        View::Category { id, back } => match crate::tree::find(&s.cfg.categories, *id) {
            Some(c) => Content {
                back: (!back.is_empty()).then(|| c.name.clone()),
                subs: c.children.iter().map(|ch| (ch.id, ch.name.clone())).collect(),
                tiles: c.apps.iter().filter_map(|a| s.catalog.get(a).map(|e| (a.clone(), e.name.clone()))).collect(),
                manage: Some(*id),
                empty: c.children.is_empty() && c.apps.is_empty(),
                rows: Vec::new(),
                missing: false,
            },
            None => Content {
                back: None,
                subs: Vec::new(),
                tiles: Vec::new(),
                manage: None,
                empty: false,
                rows: Vec::new(),
                missing: true,
            },
        },
        View::All { .. } => Content {
            back: None,
            subs: Vec::new(),
            tiles: Vec::new(),
            manage: None,
            empty: false,
            rows: s.catalog.apps.iter().map(|a| (a.id.clone(), a.name.clone())).collect(),
            missing: false,
        },
    });
    if content.missing {
        close();
        return;
    }

    let Some((dpi, font)) = with(|f| {
        f.look = look.clone();
        f.colors = colors;
        (f.dpi, f.font)
    }) else {
        return;
    };
    let s = |v: i32| scale(v, dpi);
    let border = if look.border_width > 0 { s(look.border_width as i32).max(1) } else { 0 };
    let pad = s(4) + border;
    let mut elems: Vec<Placed> = Vec::new();
    let mut y = pad;
    let inner_w;

    match &view {
        View::Category { .. } => {
            let tile = (s(84), s(76));
            let tm = s(2);
            let cols = (look.flyout_columns as usize).max(1);
            let tiles_w = cols.min(content.tiles.len().max(1)) as i32 * (tile.0 + 2 * tm);
            let max_w = (cols as i32 * (tile.0 + 2 * tm)).max(s(360));
            // Subcategory buttons (and Back), wrapped.
            let mut pills: Vec<(Elem, String, Option<String>)> = Vec::new();
            if let Some(name) = &content.back {
                pills.push((Elem::Back, "‹  Back".into(), None));
                pills.push((Elem::Label, name.clone(), None));
            }
            for (id, name) in &content.subs {
                pills.push((Elem::Sub(*id), name.clone(), Some(format!("cat:{id}"))));
            }
            let widths: Vec<i32> = pills
                .iter()
                .map(|(e, t, _)| {
                    let icon = if matches!(e, Elem::Sub(_)) { s(16) + s(6) } else { 0 };
                    s(10) + icon + canvas::measure(t, font).0 + s(10)
                })
                .collect();
            let pill_h = s(28);
            let wrapped = striplayout::wrap(&widths, s(4), max_w);
            let pills_w = wrapped.iter().zip(&widths).map(|((x, _), w)| x + w).max().unwrap_or(0);
            inner_w = tiles_w.max(pills_w).max(canvas::measure("Manage Category", font).0 + s(48)).max(s(180));
            let rows = wrapped.last().map(|(_, r)| r + 1).unwrap_or(0);
            for ((x, row), (elem, text, icon)) in wrapped.iter().zip(pills) {
                let top = y + *row as i32 * (pill_h + s(4));
                let w = widths[elems.len()];
                elems.push(Placed {
                    rect: RECT { left: pad + x, top, right: pad + x + w, bottom: top + pill_h },
                    elem,
                    text,
                    icon,
                });
            }
            if rows > 0 {
                y += rows as i32 * (pill_h + s(4));
            }
            // App tiles.
            for ((col, row), (id, name)) in striplayout::grid(content.tiles.len(), cols).into_iter().zip(&content.tiles)
            {
                let left = pad + col as i32 * (tile.0 + 2 * tm) + tm;
                let top = y + row as i32 * (tile.1 + 2 * tm) + tm;
                elems.push(Placed {
                    rect: RECT { left, top, right: left + tile.0, bottom: top + tile.1 },
                    elem: Elem::Tile(id.clone()),
                    text: name.clone(),
                    icon: Some(id.clone()),
                });
            }
            if !content.tiles.is_empty() {
                y += content.tiles.len().div_ceil(cols) as i32 * (tile.1 + 2 * tm);
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
            if let Some(id) = content.manage {
                y += s(4);
                elems.push(Placed {
                    rect: RECT { left: pad, top: y, right: pad + inner_w, bottom: y + pill_h },
                    elem: Elem::Manage(id),
                    text: "Manage Category".into(),
                    icon: None,
                });
                y += pill_h;
            }
        }
        View::All { first_row } => {
            inner_w = s(280) - 2 * pad;
            elems.push(Placed {
                rect: RECT { left: pad + s(8), top: y + s(4), right: pad + inner_w, bottom: y + s(22) },
                elem: Elem::Label,
                text: if content.rows.is_empty() { "No apps found".into() } else { "All apps".into() },
                icon: None,
            });
            y += s(24);
            let row_h = s(28);
            let visible = ((s(480) - y - pad) / row_h).max(1) as usize;
            let first = (*first_row).min(content.rows.len().saturating_sub(visible));
            with(|f| {
                f.rows = (content.rows.len(), visible);
                if let View::All { first_row } = &mut f.view {
                    *first_row = first;
                }
            });
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

    // Above the button (below it for a top strip), kept on the monitor.
    let top_strip = strip::edge_is_top();
    let mon = unsafe {
        let m = MonitorFromPoint(POINT { x: anchor_rc.left, y: anchor_rc.top }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(m, &mut mi);
        mi.rcMonitor
    };
    let gap = s(4);
    let x = anchor_rc.left.min(mon.right - w).max(mon.left);
    let y = if top_strip { anchor_rc.bottom + gap } else { anchor_rc.top - gap - h };
    let win = RECT { left: x, top: y, right: x + w, bottom: y + h };

    // Icons for everything on show, loaded once per size.
    let wanted: Vec<(String, i32)> = elems
        .iter()
        .filter_map(|p| {
            let size = match p.elem {
                Elem::Tile(_) => s(look.icon_size as i32),
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

    let h_win = with(|f| {
        f.elems = elems;
        f.win = win;
        f.hwnd
    });
    if let Some(hw) = h_win {
        unsafe {
            let _ = SetWindowPos(hw, Some(HWND_TOPMOST), win.left, win.top, w, h, SWP_NOACTIVATE);
            let _ = ShowWindow(hw, SW_SHOWNOACTIVATE);
        }
    }
    render();
}

// ---------------------------------------------------------------- drawing

fn render() {
    FLYOUT.with(|cell| {
        let mut b = cell.borrow_mut();
        let Some(f) = b.as_mut() else { return };
        let (w, h) = (ui::rect_w(&f.win), ui::rect_h(&f.win));
        let Some(mut cv) = Canvas::new(w, h) else { return };
        let d = f.dpi;
        let s = |v: i32| scale(v, d);
        let c = f.colors;
        let radius = s(6.max(f.look.corner_radius.min(12) as i32)) as f32;
        let border = if f.look.border_width > 0 { s(f.look.border_width as i32).max(1) as f32 } else { 0.0 };
        cv.fill_round_rect(0.0, 0.0, w as f32, h as f32, radius, c.background);
        cv.stroke_round_rect(0.0, 0.0, w as f32, h as f32, radius, border, c.border);

        let r4 = s(4) as f32;
        let elems: Vec<(RECT, Elem, String, Option<String>)> =
            f.elems.iter().map(|p| (p.rect, p.elem.clone(), p.text.clone(), p.icon.clone())).collect();
        for (i, (rc, elem, text, icon)) in elems.into_iter().enumerate() {
            let hovered = f.hover == Some(i) && elem != Elem::Label;
            // Subcategories and Back are buttons, as in the original: a faint
            // fill that strengthens on hover.
            let pill = matches!(elem, Elem::Sub(_) | Elem::Back);
            let fill = match (pill, hovered) {
                (true, true) => Some(c.pressed),
                (true, false) | (false, true) => Some(c.hover),
                (false, false) => None,
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
            match &elem {
                Elem::Back => {
                    cv.text(&text, rc, f.font, c.text, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                }
                Elem::Sub(_) => {
                    let size = s(16);
                    let x = rc.left + s(10);
                    let y = rc.top + (ui::rect_h(&rc) - size) / 2;
                    match icon.as_ref().and_then(|k| icon_for(f, k, size)) {
                        Some(img) => cv.image(&img, x, y, size, 1.0),
                        None => cv.folder(x as f32, y as f32, size as f32, c.accent),
                    }
                    let trc = RECT { left: x + size + s(6), ..rc };
                    cv.text(&text, trc, f.font, c.text, DT_VCENTER | DT_SINGLELINE);
                }
                Elem::Tile(_) => {
                    let size = s(f.look.icon_size as i32).min(ui::rect_w(&rc) - s(8));
                    let x = rc.left + (ui::rect_w(&rc) - size) / 2;
                    let y = rc.top + s(4);
                    if let Some(img) = icon.as_ref().and_then(|k| icon_for(f, k, s(f.look.icon_size as i32))) {
                        cv.image(&img, x, y, size, 1.0);
                    }
                    // Two lines reserved for the name, as in the original.
                    let trc =
                        RECT { left: rc.left + s(4), top: y + size + s(4), right: rc.right - s(4), bottom: rc.bottom };
                    cv.text(&text, trc, f.small, c.text, DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS);
                }
                Elem::Manage(_) => {
                    let gx = rc.left as f32 + s(18) as f32;
                    let gy = (rc.top + rc.bottom) as f32 / 2.0;
                    cv.gear(gx, gy, s(7) as f32, c.text);
                    let trc = RECT { left: rc.left + s(32), ..rc };
                    cv.text(&text, trc, f.font, c.text, DT_VCENTER | DT_SINGLELINE);
                }
                Elem::Row(_) => {
                    let size = s(20);
                    let x = rc.left + s(8);
                    let y = rc.top + (ui::rect_h(&rc) - size) / 2;
                    if let Some(img) = icon.as_ref().and_then(|k| icon_for(f, k, size)) {
                        cv.image(&img, x, y, size, 1.0);
                    }
                    let trc = RECT { left: x + size + s(8), right: rc.right - s(4), ..rc };
                    cv.text(&text, trc, f.font, c.text, DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                }
                Elem::Label => {
                    let trc = RECT { left: rc.left + s(4), ..rc };
                    cv.text(&text, trc, f.font, c.subtle, DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                }
            }
        }
        // A thin scroll indicator for a long All list.
        let (total, visible) = f.rows;
        if let View::All { first_row } = f.view
            && total > visible
        {
            let track_top = s(28) as f32;
            let track_h = h as f32 - track_top - s(6) as f32;
            let thumb_h = (track_h * visible as f32 / total as f32).max(s(16) as f32);
            let y = track_top + (track_h - thumb_h) * first_row as f32 / (total - visible) as f32;
            cv.fill_round_rect(w as f32 - s(6) as f32, y, s(3) as f32, thumb_h, s(2) as f32, c.subtle);
        }
        cv.present(f.hwnd, f.win.left, f.win.top);
    });
}

// ---------------------------------------------------------------- input

fn elem_at(x: i32, y: i32) -> Option<usize> {
    FLYOUT.with(|f| {
        let b = f.borrow();
        let f = b.as_ref()?;
        f.elems.iter().position(|p| {
            p.elem != Elem::Label && x >= p.rect.left && x < p.rect.right && y >= p.rect.top && y < p.rect.bottom
        })
    })
}

fn activate(elem: Elem) {
    match elem {
        Elem::Back => {
            with(|f| {
                if let View::Category { id, back } = &mut f.view
                    && let Some(prev) = back.pop()
                {
                    *id = prev;
                }
                f.hover = None;
            });
            rebuild();
        }
        Elem::Sub(sub) => {
            with(|f| {
                if let View::Category { id, back } = &mut f.view {
                    back.push(*id);
                    *id = sub;
                }
                f.hover = None;
            });
            rebuild();
        }
        Elem::Tile(id) | Elem::Row(id) => {
            close();
            app::launch_app(&id);
        }
        Elem::Manage(id) => {
            close();
            super::manager::show_category(id);
        }
        Elem::Label => {}
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
    let over_flyout = FLYOUT.with(|f| {
        f.borrow()
            .as_ref()
            .is_some_and(|f| pt.x >= f.win.left && pt.x < f.win.right && pt.y >= f.win.top && pt.y < f.win.bottom)
    });
    over_flyout || with(|f| f.anchor).is_some_and(strip::pointer_over)
}

fn mouse_xy(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            cancel_close();
            let (x, y) = mouse_xy(lparam);
            let under = elem_at(x, y);
            let (changed, start_leave, hover_sub) = with(|f| {
                let changed = f.hover != under;
                f.hover = under;
                let hover_sub = f.look.subcategory_open == SubcategoryOpen::Hover
                    && under.is_some_and(|i| matches!(f.elems[i].elem, Elem::Sub(_) | Elem::Back));
                (changed, !std::mem::replace(&mut f.tracking_leave, true), hover_sub)
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
                render();
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER_SUB);
                    if hover_sub {
                        SetTimer(Some(hwnd), TIMER_SUB, SUB_HOVER_MS, None);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|f| {
                f.tracking_leave = false;
                f.hover = None;
            });
            render();
            unsafe {
                SetTimer(Some(hwnd), TIMER_CLOSE, CLOSE_DELAY_MS, None);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), wparam.0);
            }
            match wparam.0 {
                TIMER_CLOSE if !pointer_inside() => close(),
                TIMER_SUB => {
                    let elem = with(|f| f.hover.map(|i| f.elems[i].elem.clone())).flatten();
                    if let Some(e @ (Elem::Sub(_) | Elem::Back)) = elem {
                        activate(e);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = mouse_xy(lparam);
            if let Some(elem) = elem_at(x, y).and_then(|i| with(|f| f.elems[i].elem.clone())) {
                activate(elem);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (x, y) = mouse_xy(lparam);
            if let Some(Elem::Tile(id) | Elem::Row(id)) = elem_at(x, y).and_then(|i| with(|f| f.elems[i].elem.clone()))
            {
                row_menu(&id);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let changed = with(|f| match &mut f.view {
                View::All { first_row } => {
                    let step = 3usize;
                    let max = f.rows.0.saturating_sub(f.rows.1);
                    let new = if delta > 0 { first_row.saturating_sub(step) } else { (*first_row + step).min(max) };
                    std::mem::replace(first_row, new) != new
                }
                _ => false,
            })
            .unwrap_or(false);
            if changed {
                rebuild();
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
