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
use crate::appearance::{Appearance, Colors, FlyoutAnim, FlyoutShadow};
use crate::shadow;
use crate::striplayout::{self, Edge, Hit};
use crate::{flyanim, flykeys};
use resvg::tiny_skia::{Pixmap, PixmapPaint, Transform};
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteObject, GetMonitorInfoW, HFONT, HGDIOBJ,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetCapture, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos, GetSystemMetrics,
    HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW, IDC_NO, InsertMenuW, KillTimer, LoadCursorW, MA_NOACTIVATE, MF_BYPOSITION,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, PostMessageW, RegisterClassW, SM_CXDRAG, SM_CYDRAG,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetCursor, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_ACTIVATE, WM_CAPTURECHANGED, WM_CHAR, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST, WM_NULL, WM_RBUTTONUP,
    WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const WM_MOUSELEAVE: u32 = 0x02A3;
/// Timers, all on the level-0 window.
const TIMER_CLOSE: usize = 1;
const TIMER_HOVER: usize = 2;
/// Frames of the opening animation.
const TIMER_ANIM: usize = 3;
const ANIM_FRAME_MS: u32 = 10;
/// The flyouts lost the activation while they had the keyboard.
const TIMER_DEACTIVATED: usize = 4;
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
    All {
        first_row: usize,
    },
    /// A folder pinned to the bar, or a folder inside one.
    Folder(std::path::PathBuf),
}

#[derive(Clone, PartialEq)]
enum Elem {
    Sub(u64),
    Tile(String),
    /// A folder inside a pinned folder: opens beyond, like a subcategory.
    Folder(std::path::PathBuf),
    /// A file in a pinned folder (or the folder itself, for *Open folder*).
    File(std::path::PathBuf),
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

/// An app tile or row held down with the left button, maybe being dragged
/// out to the bar or onto a subcategory.
struct Drag {
    level: usize,
    elem: usize,
    app_id: String,
    /// Where the button went down (screen).
    start: POINT,
    /// It has moved far enough to be a drag rather than a click.
    active: bool,
    /// The subcategory tile it would be filed in: (level, element).
    target: Option<(usize, usize)>,
    /// Esc was pressed: nothing happens until the button is let go.
    cancelled: bool,
}

thread_local! {
    static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
}

fn dragging() -> bool {
    DRAG.with(|d| d.borrow().as_ref().is_some_and(|d| d.active))
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
    /// Room around `win` for the shadow: (left, top, right, bottom). The
    /// window is that much larger than the flyout.
    pad: (i32, i32, i32, i32),
    /// The shadow, drawn once per size and look.
    shadow: Option<(ShadowKey, Pixmap)>,
    /// The element with the keyboard focus.
    focus: Option<usize>,
    /// Where the button it opened from lies across the window, for the
    /// closing animation.
    across: (f32, f32),
}

/// What a cached shadow was drawn for.
#[derive(Clone, Copy, PartialEq)]
struct ShadowKey {
    size: (i32, i32),
    style: FlyoutShadow,
    strength: u32,
    radius: i32,
    colour: crate::appearance::Rgba,
    dpi: u32,
    edge: Edge,
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
    /// The flyouts have the keyboard (opened by a click on *All*, the bar
    /// hotkey or Tab): arrows, Enter, Esc and typing work, and they close
    /// when another window is activated rather than when the pointer leaves.
    keyboard: bool,
    /// The level the keys act on.
    key_level: usize,
    typeahead: flykeys::TypeAhead,
    /// When the keyboard was taken: an activation lost right after (the
    /// click that opened them settling) takes it back instead of closing.
    took_keyboard: Option<std::time::Instant>,
    /// A key has been used (or the bar hotkey opened them): from then on the
    /// flyouts stay open while the pointer is elsewhere. Until then they close
    /// when the pointer leaves, as for a mouse user.
    keys_used: bool,
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

/// A folder pinned to the bar: its flyout lists what is in it.
pub fn open_folder(path: std::path::PathBuf, anchor: Hit) {
    let view = View::Folder(path);
    let already = with(|f| f.anchor == anchor && f.levels.first().is_some_and(|l| l.view == view));
    if already == Some(true) {
        cancel_close();
        return;
    }
    show(view, anchor);
}

pub fn toggle_all(anchor: Hit) {
    if with(|f| f.anchor == anchor) == Some(true) {
        close();
    } else {
        show(View::All { first_row: 0 }, anchor);
        // Clicked open: typing and the arrow keys go to the list.
        take_keyboard();
    }
}

/// The bar hotkey: opens the *All* list with the keyboard in it, or closes
/// the flyouts if they have it already.
pub fn keyboard_open() {
    if with(|f| f.keyboard) == Some(true) {
        close();
        return;
    }
    show(View::All { first_row: 0 }, strip::all_button());
    take_keyboard();
    with(|f| f.keys_used = true);
}

/// A category's hotkey: opens its flyout from its bar button with the
/// keyboard in it (or closes it if that is what is open).
pub fn keyboard_open_category(hit: Hit, id: u64) {
    let already = with(|f| f.keyboard && f.levels.first().is_some_and(|l| l.view == View::Category(id)));
    if already == Some(true) {
        close();
        return;
    }
    show(View::Category(id), hit);
    take_keyboard();
    with(|f| f.keys_used = true);
}

/// Gives the open flyouts the keyboard, focusing the first app or tile.
fn take_keyboard() {
    let Some(b) = base() else { return };
    with(|f| {
        f.keyboard = true;
        f.key_level = 0;
        f.took_keyboard = Some(std::time::Instant::now());
    });
    focus_first(0);
    unsafe {
        let _ = SetForegroundWindow(b);
        let _ = SetFocus(Some(b));
    }
    render(0);
}

fn show(view: View, anchor: Hit) {
    // An auto-hidden bar comes back with its flyout (the bar hotkey).
    strip::reveal();
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
                keyboard: false,
                key_level: 0,
                typeahead: flykeys::TypeAhead::default(),
                took_keyboard: None,
                keys_used: false,
            })
        });
    }
    truncate_now(0);
    // Opened by the pointer: keyboard callers take the keyboard afterwards.
    with(|f| {
        f.anchor = anchor;
        f.keyboard = false;
        f.keys_used = false;
        f.key_level = 0;
        f.typeahead.clear();
    });
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
            pad: (0, 0, 0, 0),
            focus: None,
            across: (0.0, 0.0),
            shadow: None,
        })
    });
    true
}

/// Closes every level from `keep` up, leaving `keep` levels open.
fn truncate(keep: usize) {
    close_levels(keep, true);
}

/// [`truncate`] without the closing animation (switching to another
/// category: the new flyout replaces the old one at once).
fn truncate_now(keep: usize) {
    close_levels(keep, false);
}

fn close_levels(keep: usize, animate: bool) {
    let (gone, style, ms, reverse) = with(|f| {
        let gone: Vec<Level> = f.levels.drain(keep.min(f.levels.len())..).collect();
        (gone, f.look.flyout_animation, f.look.flyout_animation_ms.clamp(60, 600), f.look.flyout_close_animation)
    })
    .unwrap_or((Vec::new(), FlyoutAnim::Off, 0, false));
    for l in gone {
        let play = animate && reverse && style != FlyoutAnim::Off && l.frame.is_some();
        match l.frame {
            Some(frame) if play => closing::start(closing::Closing {
                hwnd: l.hwnd,
                frame,
                at: (l.win.left - l.pad.0, l.win.top - l.pad.1),
                across: l.across,
                style,
                // A little quicker than opening, so it never lingers.
                ms: ms as f32 * 0.75,
                // Part-way through opening: close from where it got to.
                from: l.anim.map(|(t, _)| (t.elapsed().as_secs_f32() * 1000.0 / ms as f32).min(1.0)).unwrap_or(1.0),
                start: std::time::Instant::now(),
            }),
            _ => unsafe {
                let _ = DestroyWindow(l.hwnd);
            },
        }
    }
}

/// Flyouts playing their opening animation backwards before going away.
/// They are no longer part of the open flyouts (nothing they show can be
/// clicked: the pointer passes through them), so new ones open at once.
mod closing {
    use super::{FlyoutAnim, HWND, Pixmap, animation_frame};
    use std::cell::RefCell;
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, GWL_EXSTYLE, GetWindowLongW, KillTimer, SetTimer, SetWindowLongW, WS_EX_TRANSPARENT,
    };

    pub struct Closing {
        pub hwnd: HWND,
        pub frame: Pixmap,
        pub at: (i32, i32),
        pub across: (f32, f32),
        pub style: FlyoutAnim,
        pub ms: f32,
        /// How far open it was (1 = fully).
        pub from: f32,
        pub start: std::time::Instant,
    }

    thread_local! {
        static CLOSING: RefCell<Vec<Closing>> = const { RefCell::new(Vec::new()) };
        static TIMER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub fn start(c: Closing) {
        unsafe {
            // Clicks and the pointer go to whatever is under it now.
            let ex = GetWindowLongW(c.hwnd, GWL_EXSTYLE);
            SetWindowLongW(c.hwnd, GWL_EXSTYLE, ex | WS_EX_TRANSPARENT.0 as i32);
        }
        CLOSING.with(|v| v.borrow_mut().push(c));
        if TIMER.with(|t| t.get()) == 0 {
            let id = unsafe { SetTimer(None, 0, 10, Some(tick)) };
            TIMER.with(|t| t.set(id));
        }
    }

    unsafe extern "system" fn tick(_: HWND, _: u32, _: usize, _: u32) {
        let done: Vec<HWND> = CLOSING.with(|v| {
            let mut v = v.borrow_mut();
            let mut done = Vec::new();
            v.retain(|c| {
                let t = c.from - c.start.elapsed().as_secs_f32() * 1000.0 / c.ms;
                if t <= 0.0 {
                    done.push(c.hwnd);
                    return false;
                }
                animation_frame(&c.frame, c.style, t, c.across).present(c.hwnd, c.at.0, c.at.1);
                true
            });
            done
        });
        for h in done {
            unsafe {
                let _ = DestroyWindow(h);
            }
        }
        if CLOSING.with(|v| v.borrow().is_empty()) {
            let id = TIMER.with(|t| t.replace(0));
            unsafe {
                let _ = KillTimer(None, id);
            }
        }
    }
}

pub fn close() {
    end_drag();
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
                .filter(|p| match (&p.elem, &view) {
                    (Elem::Sub(a), View::Category(b)) => a == b,
                    (Elem::Folder(a), View::Folder(b)) => a == b,
                    _ => false,
                })
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
        /// A folder's tiles, ready made (subfolders, files, *Open folder*).
        folder: Vec<(Elem, String, String)>,
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
            folder: Vec::new(),
            empty: c.children.is_empty() && c.apps.is_empty(),
            rows: Vec::new(),
            counts: (0, 0),
            buttons: Default::default(),
        }),
        View::Folder(path) => {
            let folder = folder_tiles(path);
            Some(Content {
                subs: Vec::new(),
                tiles: Vec::new(),
                empty: folder.len() <= 1,
                folder,
                rows: Vec::new(),
                counts: (0, 0),
                buttons: Default::default(),
            })
        }
        View::All { .. } => {
            let (rows, counts) = all_lines(s);
            let v = &s.cfg.settings.all_apps;
            let buttons = (
                format!("Sort: {}", v.sort.short()),
                if v.filtering() { "Show: some".to_string() } else { "Show: all".to_string() },
            );
            Some(Content {
                subs: Vec::new(),
                tiles: Vec::new(),
                folder: Vec::new(),
                empty: false,
                rows,
                counts,
                buttons,
            })
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
        View::Category(_) | View::Folder(_) => {
            let empty_text =
                if matches!(view, View::Folder(_)) { "This folder is empty" } else { "No apps in this category" };
            let tile = (s(84), s(76));
            let tm = s(2);
            let cols = (look.flyout_columns as usize).max(1);
            // Subcategories sit nearest the flyouts they open: first (the top
            // row) above a bottom bar or beside a side bar; below a top bar the
            // apps fill the top rows first and the subcategories follow,
            // nearest the next level opening further down.
            let subs = content.subs.iter().map(|(id, name)| (Elem::Sub(*id), name.clone(), format!("cat:{id}")));
            let apps = content.tiles.iter().map(|(id, name)| (Elem::Tile(id.clone()), name.clone(), id.clone()));
            let items: Vec<(Elem, String, String)> = if matches!(view, View::Folder(_)) {
                content.folder.clone()
            } else if strip::edge() == Edge::Top {
                apps.chain(subs).collect()
            } else {
                subs.chain(apps).collect()
            };
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
            inner_w = if content.empty { tiles_w.max(canvas::measure(empty_text, font).0 + s(16)) } else { tiles_w };
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
                    text: empty_text.into(),
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
                Elem::Tile(_) | Elem::Sub(_) | Elem::Folder(_) | Elem::File(_) => s(look.icon_size as i32),
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

    let pad = shadow_pad(&look, dpi);
    let animated = look.flyout_animation != FlyoutAnim::Off;
    let starts = with(|f| {
        let l = &mut f.levels[idx];
        let first = !std::mem::replace(&mut l.shown, true);
        // Where the button lies across the window (along the bar).
        let across = if strip::edge().vertical() {
            ((anchor_rc.top - win.top + pad.1) as f32, (anchor_rc.bottom - win.top + pad.1) as f32)
        } else {
            ((anchor_rc.left - win.left + pad.0) as f32, (anchor_rc.right - win.left + pad.0) as f32)
        };
        l.across = across;
        if first && animated {
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
        l.pad = pad;
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
            let (ww, wh) = (w + pad.0 + pad.2, h + pad.1 + pad.3);
            let _ = SetWindowPos(hw, Some(HWND_TOPMOST), win.left - pad.0, win.top - pad.1, ww, wh, SWP_NOACTIVATE);
            let _ = ShowWindow(hw, SW_SHOWNOACTIVATE);
        }
    }
    render(idx);
}

// ---------------------------------------------------------------- drawing

fn render(idx: usize) {
    let started = std::time::Instant::now();
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
            } else if (l.hover == Some(i) || (f.keyboard && l.focus == Some(i))) && interactive(&p.elem) {
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
            let drop_here = DRAG.with(|d| d.borrow().as_ref().is_some_and(|d| d.target == Some((idx, i))));
            if drop_here || (f.keyboard && f.keys_used && f.key_level == idx && l.focus == Some(i)) {
                // The keyboard focus, or where a dragged app would be filed: a
                // ring in the accent colour.
                let (x, y, w, h) = (rc.left as f32, rc.top as f32, ui::rect_w(&rc) as f32, ui::rect_h(&rc) as f32);
                cv.stroke_round_rect(x, y, w, h, r4, s(2) as f32, c.accent);
            }
            let icon = p.icon.as_deref();
            match &p.elem {
                Elem::Tile(_) | Elem::Sub(_) | Elem::Folder(_) | Elem::File(_) => {
                    let want = s(f.look.icon_size as i32);
                    let size = want.min(ui::rect_w(&rc) - s(16));
                    let x = rc.left + (ui::rect_w(&rc) - size) / 2;
                    let y = rc.top + s(4);
                    let is_sub = matches!(p.elem, Elem::Sub(_) | Elem::Folder(_));
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
                    if let Elem::Tile(id) = &p.elem
                        && super::running::is_running(id)
                    {
                        // Just under the icon.
                        let under = RECT { left: x, top: y, right: x + size, bottom: y + size + s(5) };
                        super::indicator::running(&mut cv, &f.look, &c, Edge::Bottom, under, d);
                    }
                    // Two lines reserved for the name, as in the original.
                    let trc =
                        RECT { left: rc.left + s(4), top: y + size + s(4), right: rc.right - s(4), bottom: rc.bottom };
                    cv.text(&p.text, trc, f.small, c.text, DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS);
                }
                Elem::Row(id) => {
                    if super::running::is_running(id) {
                        // On the row's left edge.
                        super::indicator::running(&mut cv, &f.look, &c, Edge::Left, rc, d);
                    }
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
    if let Some(content) = drawn {
        with(|f| {
            let (look, colors, dpi) = (&f.look, f.colors, f.dpi);
            if let Some(l) = f.levels.get_mut(idx) {
                l.frame = Some(with_shadow(l, &content, look, &colors, dpi));
            }
        });
        present(idx);
        super::diagnostics::flyout_drawn(started);
    }
}

/// Room the flyouts' shadow needs around them, in pixels.
fn shadow_pad(look: &Appearance, dpi: u32) -> (i32, i32, i32, i32) {
    shadow::spec(look.flyout_shadow).map(|sp| shadow::margins(&sp, dpi as f32 / 96.0)).unwrap_or((0, 0, 0, 0))
}

/// The flyout's drawing on top of its shadow (made once per size and look,
/// then reused while the flyout is redrawn on hover).
fn with_shadow(l: &mut Level, content: &Pixmap, look: &Appearance, c: &Colors, dpi: u32) -> Pixmap {
    let Some(sp) = shadow::spec(look.flyout_shadow) else { return content.clone() };
    let (pl, pt, pr, pb) = l.pad;
    let (w, h) = (content.width() as i32, content.height() as i32);
    let key = ShadowKey {
        size: (w, h),
        style: look.flyout_shadow,
        strength: look.flyout_shadow_strength.clamp(10, 100),
        radius: scale(look.flyout_corner_radius as i32, dpi),
        colour: if sp.accent { c.accent } else { crate::appearance::Rgba::rgb(0, 0, 0) },
        dpi,
        edge: strip::edge(),
    };
    if l.shadow.as_ref().is_none_or(|(k, _)| *k != key) {
        l.shadow = shadow_layer(&sp, &key, l.pad).map(|p| (key, p));
    }
    let Some((_, layer)) = &l.shadow else { return content.clone() };
    let mut out = layer.clone();
    if out.width() as i32 != w + pl + pr || out.height() as i32 != h + pt + pb {
        return content.clone();
    }
    out.draw_pixmap(pl, pt, content.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    out
}

/// Draws the shadow of a `key.size` flyout with `pad` room around it: its
/// shape, offset and blurred, minus the flyout itself (so a translucent
/// flyout doesn't show its own shadow through it).
fn shadow_layer(sp: &shadow::Spec, key: &ShadowKey, pad: (i32, i32, i32, i32)) -> Option<Pixmap> {
    let (w, h) = key.size;
    let (ww, wh) = ((w + pad.0 + pad.2) as u32, (h + pad.1 + pad.3) as u32);
    let k = key.dpi as f32 / 96.0;
    let r = key.radius as f32;
    let shape = |dx: f32, dy: f32| -> Option<Vec<u8>> {
        let mut cv = Canvas::new(ww as i32, wh as i32)?;
        cv.fill_round_rect(
            pad.0 as f32 + dx,
            pad.1 as f32 + dy,
            w as f32,
            h as f32,
            r,
            crate::appearance::Rgba::rgb(0, 0, 0),
        );
        Some(cv.pix.data().as_chunks::<4>().0.iter().map(|p| p[3]).collect())
    };
    let mut mask = shape(sp.dx * k, sp.dy * k)?;
    shadow::blur(&mut mask, ww as usize, wh as usize, shadow::box_radius(sp, k));
    let body = shape(0.0, 0.0)?;
    let mut out = Pixmap::new(ww, wh)?;
    let strength = sp.alpha * key.strength as f32 / 100.0;
    let c = key.colour;
    // Nothing beyond the small gap on the bar's side: the shadow must not dim
    // the bar's icons (or the flyout below, for higher levels).
    let gap = scale(4, key.dpi);
    let ww_i = ww as i32;
    let keep = |x: i32, y: i32| match key.edge {
        Edge::Bottom => y < pad.1 + h + gap,
        Edge::Top => y >= pad.1 - gap,
        Edge::Left => x >= pad.0 - gap,
        Edge::Right => x < pad.0 + w + gap,
    };
    for (i, ((px, &m), &b)) in out.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(&mask).zip(&body).enumerate() {
        let (x, y) = (i as i32 % ww_i, i as i32 / ww_i);
        if !keep(x, y) {
            continue;
        }
        let a = (m as f32 * strength * (255 - b) as f32 / 255.0).round().clamp(0.0, 255.0) as u32;
        if a == 0 {
            continue;
        }
        // Premultiplied.
        px[0] = (c.r as u32 * a / 255) as u8;
        px[1] = (c.g as u32 * a / 255) as u8;
        px[2] = (c.b as u32 * a / 255) as u8;
        px[3] = a as u8;
    }
    Some(out)
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
            Some((_, across)) => {
                animation_frame(pix, style, t, across).present(l.hwnd, l.win.left - l.pad.0, l.win.top - l.pad.1)
            }
            None => canvas::present_pixmap(pix, l.hwnd, l.win.left - l.pad.0, l.win.top - l.pad.1),
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

/// The element at (`x`, `y`) in level `idx`'s window (which includes the
/// shadow's room).
fn elem_at(idx: usize, x: i32, y: i32) -> Option<usize> {
    with(|f| {
        let l = f.levels.get(idx)?;
        let (x, y) = (x - l.pad.0, y - l.pad.1);
        l.elems.iter().position(|p| {
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
        Some((i, Elem::Folder(path))) => open_subfolder(idx, i, path),
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
/// The tiles of a folder's flyout: its subfolders, its files and *Open
/// folder* last (see `folders::listing`).
fn folder_tiles(path: &std::path::Path) -> Vec<(Elem, String, String)> {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    let entries: Vec<(crate::folders::Entry, std::path::PathBuf)> = std::fs::read_dir(path)
        .map(|r| {
            r.flatten()
                // Never more than a few thousand looked at, however big.
                .take(5000)
                .filter_map(|e| {
                    let meta = e.metadata().ok()?;
                    let name = e.file_name().to_string_lossy().into_owned();
                    let hidden = meta.file_attributes() & (HIDDEN | SYSTEM) != 0;
                    Some((crate::folders::Entry { name, dir: meta.is_dir(), hidden }, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    let by_name: std::collections::HashMap<String, std::path::PathBuf> =
        entries.iter().map(|(e, p)| (e.name.clone(), p.clone())).collect();
    let (shown, more) = crate::folders::listing(entries.into_iter().map(|(e, _)| e).collect());
    let mut tiles: Vec<(Elem, String, String)> = shown
        .into_iter()
        .filter_map(|e| {
            let p = by_name.get(&e.name)?.clone();
            let icon = format!("path:{}", p.display());
            let text = crate::folders::display_name(&e.name);
            Some((if e.dir { Elem::Folder(p) } else { Elem::File(p) }, text, icon))
        })
        .collect();
    let open = if more > 0 { format!("Open folder ({more} more)") } else { "Open folder".to_string() };
    tiles.push((Elem::File(path.to_path_buf()), open, format!("path:{}", path.display())));
    tiles
}

/// Opens a file (or folder) from a pinned folder, as Explorer would.
fn open_path(path: &std::path::Path) {
    let target = super::launch::Target::Custom {
        target: path.display().to_string(),
        args: String::new(),
        dir: String::new(),
        admin: false,
    };
    super::launch::spawn(target, |msg| super::supervisor::log(&msg));
}

fn open_subfolder(idx: usize, i: usize, path: std::path::PathBuf) {
    let child = with(|f| f.levels.get(idx + 1).map(|c| c.source)).flatten();
    if child == Some(Some(i)) {
        return; // already open
    }
    truncate(idx + 1);
    // As deep as categories may go, so every level fits on screen.
    if idx + 1 < strip::max_levels() && push_level(View::Folder(path), Some(i)) {
        rebuild(idx + 1);
    }
    render(idx);
}

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
    if with(|f| f.keyboard) == Some(true)
        && let Some(b) = base()
    {
        unsafe {
            let _ = SetForegroundWindow(b);
        }
    }
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
        Some(Elem::Folder(path)) => open_subfolder(idx, i, path),
        Some(Elem::File(path)) => {
            close();
            open_path(&path);
        }
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

// ---------------------------------------------------------------- keyboard

/// Whether an element can take the keyboard focus.
fn focusable(e: &Elem) -> bool {
    interactive(e)
}

/// Focuses the first app or tile of level `idx` (or the first button).
fn focus_first(idx: usize) {
    with(|f| {
        let Some(l) = f.levels.get_mut(idx) else { return };
        let first = l
            .elems
            .iter()
            .position(|p| {
                matches!(p.elem, Elem::Row(_) | Elem::Tile(_) | Elem::Sub(_) | Elem::Folder(_) | Elem::File(_))
            })
            .or_else(|| l.elems.iter().position(|p| focusable(&p.elem)));
        l.focus = first;
        l.hover = first;
    });
}

/// Focuses the element of level `idx` that `pick` chooses from the
/// focusable ones (by element index).
fn focus_where(idx: usize, pick: impl Fn(&[(usize, &Placed)]) -> Option<usize>) {
    with(|f| {
        let Some(l) = f.levels.get_mut(idx) else { return };
        let cands: Vec<(usize, &Placed)> = l.elems.iter().enumerate().filter(|(_, p)| focusable(&p.elem)).collect();
        if let Some(i) = pick(&cands) {
            l.focus = Some(i);
            l.hover = Some(i);
        }
    });
}

/// A key arrived: the flyouts are being used from the keyboard.
fn mark_keys_used() -> bool {
    with(|f| f.keys_used = true);
    true
}

fn key_level() -> usize {
    with(|f| f.key_level.min(f.levels.len().saturating_sub(1))).unwrap_or(0)
}

/// The All list scrolled by `delta` rows (clamped). Returns whether it moved.
fn scroll_all(idx: usize, delta: isize) -> bool {
    let moved = with(|f| {
        let l = f.levels.get_mut(idx)?;
        let max = l.rows.0.saturating_sub(l.rows.1);
        match &mut l.view {
            View::All { first_row } => {
                let new = (*first_row as isize + delta).clamp(0, max as isize) as usize;
                Some(std::mem::replace(first_row, new) != new)
            }
            _ => None,
        }
    })
    .flatten()
    .unwrap_or(false);
    if moved {
        rebuild(idx);
    }
    moved
}

fn is_row(p: &Placed) -> bool {
    matches!(p.elem, Elem::Row(_))
}

fn on_key(vk: u16) -> bool {
    use flykeys::Dir;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_BACK, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT,
        VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
    };
    let kl = key_level();
    let dir = match vk {
        v if v == VK_LEFT.0 => Some(Dir::Left),
        v if v == VK_RIGHT.0 => Some(Dir::Right),
        v if v == VK_UP.0 => Some(Dir::Up),
        v if v == VK_DOWN.0 => Some(Dir::Down),
        _ => None,
    };
    if let Some(dir) = dir {
        move_focus(kl, dir);
        return true;
    }
    let page = with(|f| f.levels.get(kl).map(|l| l.rows.1.max(1) as isize)).flatten().unwrap_or(1);
    match vk {
        v if v == VK_RETURN.0 => enter(kl),
        // Space launches too, unless it's part of a name being typed.
        v if v == VK_SPACE.0 => return false,
        v if v == VK_ESCAPE.0 || v == VK_BACK.0 => back(kl),
        v if v == VK_TAB.0 => {
            let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
            next_button(if shift { -1 } else { 1 });
        }
        v if v == VK_PRIOR.0 || v == VK_NEXT.0 => {
            let up = v == VK_PRIOR.0;
            scroll_all(kl, if up { -page } else { page });
            focus_where(kl, |c| {
                if up { c.iter().find(|(_, p)| is_row(p)) } else { c.iter().rev().find(|(_, p)| is_row(p)) }
                    .map(|(i, _)| *i)
            });
            render(kl);
        }
        v if v == VK_HOME.0 || v == VK_END.0 => {
            let home = v == VK_HOME.0;
            scroll_all(kl, if home { isize::MIN / 2 } else { isize::MAX / 2 });
            focus_where(kl, |c| {
                if home { c.iter().find(|(_, p)| is_row(p)) } else { c.iter().rev().find(|(_, p)| is_row(p)) }
                    .map(|(i, _)| *i)
            });
            render(kl);
        }
        _ => return false,
    }
    true
}

/// An arrow key: the next element that way, scrolling the All list at its
/// ends.
fn move_focus(kl: usize, dir: flykeys::Dir) {
    let (rects, focus) = with(|f| {
        f.levels
            .get(kl)
            .map(|l| {
                let rects: Vec<flykeys::Rect> = l
                    .elems
                    .iter()
                    .map(|p| {
                        if focusable(&p.elem) {
                            (p.rect.left, p.rect.top, p.rect.right, p.rect.bottom)
                        } else {
                            // Not focusable: far away so it's never chosen.
                            (i32::MIN / 4, i32::MIN / 4, i32::MIN / 4, i32::MIN / 4)
                        }
                    })
                    .collect();
                (rects, l.focus)
            })
            .unwrap_or_default()
    })
    .unwrap_or_default();
    let Some(from) = focus else {
        focus_first(kl);
        render(kl);
        return;
    };
    match flykeys::neighbour(&rects, from, dir) {
        Some(i) if rects[i].0 > i32::MIN / 4 => {
            with(|f| {
                if let Some(l) = f.levels.get_mut(kl) {
                    l.focus = Some(i);
                    l.hover = Some(i);
                }
            });
        }
        // At the top or bottom of the All list: scroll a row.
        _ if matches!(dir, flykeys::Dir::Up | flykeys::Dir::Down) => {
            let up = dir == flykeys::Dir::Up;
            if scroll_all(kl, if up { -1 } else { 1 }) {
                focus_where(kl, |c| {
                    if up { c.iter().find(|(_, p)| is_row(p)) } else { c.iter().rev().find(|(_, p)| is_row(p)) }
                        .map(|(i, _)| *i)
                });
            }
        }
        _ => {}
    }
    render(kl);
}

/// Enter (or Space): open a subcategory and move into it, or do what a
/// click would.
fn enter(kl: usize) {
    let Some(i) = with(|f| f.levels.get(kl).and_then(|l| l.focus)).flatten() else { return };
    match elem(kl, i) {
        Some(e @ (Elem::Sub(_) | Elem::Folder(_))) => {
            match e {
                Elem::Sub(id) => open_sub(kl, i, id),
                Elem::Folder(path) => open_subfolder(kl, i, path),
                _ => {}
            }
            if with(|f| f.levels.len() > kl + 1) == Some(true) {
                with(|f| f.key_level = kl + 1);
                focus_first(kl + 1);
                render(kl);
                render(kl + 1);
            }
        }
        Some(_) => activate(kl, i),
        None => {}
    }
}

/// Esc or Backspace: back to the level below, or close.
fn back(kl: usize) {
    if kl == 0 {
        close();
        return;
    }
    truncate(kl);
    with(|f| f.key_level = kl - 1);
    render(kl - 1);
}

/// Tab / Shift+Tab: the next or previous button on the bar that has a
/// flyout (All, then the categories), keeping the keyboard.
fn next_button(step: isize) {
    let buttons = strip::flyout_buttons();
    if buttons.is_empty() {
        return;
    }
    let anchor = with(|f| f.anchor);
    let at = buttons.iter().position(|(h, _)| Some(*h) == anchor).unwrap_or(0) as isize;
    let n = buttons.len() as isize;
    let (hit, opens) = buttons[((at + step) % n + n) as usize % buttons.len()].clone();
    match opens {
        strip::Opens::Category(id) => show(View::Category(id), hit),
        strip::Opens::Folder(path) => show(View::Folder(path), hit),
        strip::Opens::All => show(View::All { first_row: 0 }, hit),
    }
    take_keyboard();
    with(|f| f.keys_used = true);
}

/// A typed character: jump to the first app or tile whose name starts with
/// what has been typed (or has a word that does).
fn on_char(ch: char) -> bool {
    if ch.is_control() {
        return false;
    }
    let kl = key_level();
    let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    let Some(typed) = with(|f| f.typeahead.push(ch, now).to_string()) else { return false };
    if typed.trim().is_empty() {
        // A lone space: launch what has the focus.
        with(|f| f.typeahead.clear());
        enter(kl);
        return true;
    }
    let all = with(|f| f.levels.get(kl).map(|l| matches!(l.view, View::All { .. }))).flatten().unwrap_or(false);
    if all {
        // Search the whole list, not just the rows on screen.
        let (lines, _) = app::with(|s| all_lines(s));
        let apps: Vec<(usize, &str, &str)> = lines
            .iter()
            .enumerate()
            .filter_map(|(row, l)| match l {
                AllLine::App { id, name, .. } => Some((row, id.as_str(), name.as_str())),
                AllLine::Heading(_) => None,
            })
            .collect();
        let names: Vec<&str> = apps.iter().map(|a| a.2).collect();
        let focused_id = with(|f| {
            let l = f.levels.get(kl)?;
            match &l.elems.get(l.focus?)?.elem {
                Elem::Row(id) => Some(id.clone()),
                _ => None,
            }
        })
        .flatten();
        let current = focused_id.and_then(|id| apps.iter().position(|a| a.1 == id));
        let Some(j) = flykeys::find(&names, &typed, current) else { return true };
        let (row, id) = (apps[j].0, apps[j].1.to_string());
        let first = with(|f| {
            f.levels.get(kl).map(|l| match l.view {
                View::All { first_row } => (first_row, l.rows.1),
                _ => (0, 0),
            })
        })
        .flatten()
        .unwrap_or((0, 0));
        // Scroll only if the match is off screen.
        if row < first.0 || row >= first.0 + first.1 {
            with(|f| {
                if let Some(View::All { first_row }) = f.levels.get_mut(kl).map(|l| &mut l.view) {
                    *first_row = row.saturating_sub(1);
                }
            });
            rebuild(kl);
        }
        focus_where(kl, |c| c.iter().find(|(_, p)| p.elem == Elem::Row(id.clone())).map(|(i, _)| *i));
    } else {
        let (cands, current) = with(|f| {
            f.levels
                .get(kl)
                .map(|l| {
                    let c: Vec<(usize, String)> = l
                        .elems
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| {
                            matches!(
                                p.elem,
                                Elem::Tile(_) | Elem::Sub(_) | Elem::Row(_) | Elem::Folder(_) | Elem::File(_)
                            )
                        })
                        .map(|(i, p)| (i, p.text.clone()))
                        .collect();
                    let cur = l.focus.and_then(|fi| c.iter().position(|(i, _)| *i == fi));
                    (c, cur)
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
        let names: Vec<&str> = cands.iter().map(|c| c.1.as_str()).collect();
        if let Some(j) = flykeys::find(&names, &typed, current) {
            let target = cands[j].0;
            focus_where(kl, |_| Some(target));
        }
    }
    render(kl);
    true
}

// ---------------------------------------------------------------- dragging out

/// The subcategory tile under the screen point `pt`, if any: (level, element, id).
fn sub_at(pt: POINT) -> Option<(usize, usize, u64)> {
    let hit = with(|f| {
        f.levels.iter().enumerate().find_map(|(idx, l)| {
            let inside = pt.x >= l.win.left && pt.x < l.win.right && pt.y >= l.win.top && pt.y < l.win.bottom;
            inside.then(|| (idx, pt.x - l.win.left + l.pad.0, pt.y - l.win.top + l.pad.1))
        })
    })
    .flatten()?;
    let (idx, x, y) = hit;
    let i = elem_at(idx, x, y)?;
    match elem(idx, i)? {
        Elem::Sub(id) => Some((idx, i, id)),
        _ => None,
    }
}

/// The pointer moved with an app held: start dragging once it has gone far
/// enough, then show where it would land. Returns whether a drag is under way.
fn drag_moved() -> bool {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let Some((start, active, level, i, cancelled)) =
        DRAG.with(|d| d.borrow().as_ref().map(|d| (d.start, d.active, d.level, d.elem, d.cancelled)))
    else {
        return false;
    };
    if cancelled {
        return true;
    }
    // Hovered flyouts don't have the keyboard, so Esc is checked here too.
    if active && unsafe { GetAsyncKeyState(0x1B) } < 0 {
        cancel_drag();
        return true;
    }
    if !active {
        let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
        if (pt.x - start.x).abs() <= cx && (pt.y - start.y).abs() <= cy {
            return false;
        }
        DRAG.with(|d| d.borrow_mut().as_mut().map(|d| d.active = true));
        // The picture under the pointer: the tile's icon, see-through.
        let shown = with(|f| {
            let p = f.levels.get(level)?.elems.get(i)?;
            let size = scale(f.look.icon_size as i32, f.dpi);
            // A row's icon is loaded small: use the largest one there is.
            let icon = p.icon.as_deref().and_then(|k| {
                icon_for(f, k, size).or_else(|| {
                    let mut sizes: Vec<_> =
                        f.icons.iter().filter(|((key, _), pix)| key == k && pix.is_some()).collect();
                    sizes.sort_by_key(|((_, sz), _)| *sz);
                    sizes.last().and_then(|(_, pix)| (*pix).clone())
                })
            });
            let letter = super::dragimage::Letter {
                text: p.text.chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default(),
                font: f.font,
                tile: f.colors.pressed,
                colour: f.colors.text,
            };
            Some((icon, letter, size))
        })
        .flatten();
        if let Some((icon, letter, size)) = shown {
            super::dragimage::show(icon.as_ref(), letter, size, pt);
        }
        // Nothing stays highlighted for the pointer meanwhile.
        cancel_close();
    }
    super::dragimage::move_to(pt);
    let on_bar = strip::app_drag_over(Some(pt));
    let target = if on_bar { None } else { sub_at(pt).map(|(l, i, _)| (l, i)) };
    let before = DRAG.with(|d| d.borrow_mut().as_mut().map(|d| std::mem::replace(&mut d.target, target))).flatten();
    if before != target {
        for (l, _) in before.into_iter().chain(target) {
            render(l);
        }
    }
    let cursor = if on_bar || target.is_some() { IDC_ARROW } else { IDC_NO };
    unsafe {
        let _ = SetCursor(LoadCursorW(None, cursor).ok());
    }
    true
}

/// The button was let go with an app held. A click (it hardly moved) does
/// what a click does; a drag drops the app where the pointer is: pinned to
/// the bar, filed in the category under it, or nowhere.
fn drag_released(idx: usize, x: i32, y: i32) {
    let Some(d) = DRAG.with(|d| d.borrow_mut().take()) else { return };
    unsafe {
        let _ = ReleaseCapture();
    }
    if d.cancelled {
        return;
    }
    if !d.active {
        if elem_at(idx, x, y) == Some(d.elem) && idx == d.level {
            activate(idx, d.elem);
        }
        return;
    }
    super::dragimage::hide();
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let sub = sub_at(pt).map(|(_, _, id)| id);
    close();
    if strip::app_drop(pt, &d.app_id) {
        return;
    }
    if let Some(id) = sub {
        app::with(|s| crate::tree::add_app(&mut s.cfg.categories, id, &d.app_id));
        app::save();
    }
}

/// Esc during a drag: put everything back; letting go then does nothing.
fn cancel_drag() -> bool {
    let target = DRAG.with(|d| {
        let mut d = d.borrow_mut();
        let d = d.as_mut().filter(|d| d.active && !d.cancelled)?;
        d.cancelled = true;
        Some(d.target.take())
    });
    let Some(target) = target else { return false };
    super::dragimage::hide();
    strip::app_drag_over(None);
    if let Some((l, _)) = target {
        render(l);
    }
    true
}

/// Stops a drag without dropping (the flyouts closed, or another window
/// took the mouse).
fn end_drag() {
    let Some(d) = DRAG.with(|d| d.borrow_mut().take()) else { return };
    if d.active {
        super::dragimage::hide();
        strip::app_drag_over(None);
        if let Some((l, _)) = d.target {
            render(l);
        }
    }
    unsafe {
        if GetCapture() == base_or(d.level) {
            let _ = ReleaseCapture();
        }
    }
}

fn base_or(level: usize) -> HWND {
    with(|f| f.levels.get(level).map(|l| l.hwnd)).flatten().unwrap_or_default()
}

fn pointer_inside() -> bool {
    if dragging() || MENU_UP.with(|m| m.get()) || with(|f| f.keyboard && f.keys_used) == Some(true) {
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

/// Whether the screen point in `lparam` (as `WM_NCHITTEST` gives it) is on
/// level `idx`'s flyout itself rather than its shadow.
fn over_flyout(idx: usize, lparam: LPARAM) -> bool {
    let (x, y) = mouse_xy(lparam);
    with(|f| {
        f.levels.get(idx).is_some_and(|l| x >= l.win.left && x < l.win.right && y >= l.win.top && y < l.win.bottom)
    })
    .unwrap_or(true)
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
        // The shadow lets the pointer through to what's under it (the bar,
        // or the flyout below), so it never gets in the way.
        (WM_NCHITTEST, Some(idx)) if !over_flyout(idx, lparam) => LRESULT(HTTRANSPARENT as isize),
        (WM_MOUSEMOVE, Some(_)) if drag_moved() => LRESULT(0),
        (WM_MOUSEMOVE, Some(idx)) => {
            // Over the shadow (nothing of ours underneath): like being outside.
            let (cx, cy) = mouse_xy(lparam);
            let on_body = with(|f| {
                f.levels.get(idx).is_some_and(|l| {
                    let (x, y) = (cx - l.pad.0, cy - l.pad.1);
                    x >= 0 && y >= 0 && x < ui::rect_w(&l.win) && y < ui::rect_h(&l.win)
                })
            })
            .unwrap_or(true);
            if !on_body {
                start_close_timer();
                return LRESULT(0);
            }
            cancel_close();
            let (x, y) = mouse_xy(lparam);
            let under = elem_at(idx, x, y);
            let (changed, start_leave) = with(|f| {
                let l = &mut f.levels[idx];
                let changed = l.hover != under;
                l.hover = under;
                if f.keyboard && under.is_some() {
                    l.focus = under;
                    f.key_level = idx;
                }
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
        (WM_MOUSELEAVE, Some(_)) if dragging() => LRESULT(0),
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
                TIMER_DEACTIVATED => {
                    let active = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
                    let ours = with(|f| f.levels.iter().any(|l| l.hwnd == active)).unwrap_or(false);
                    if !ours && !MENU_UP.with(|m| m.get()) && with(|f| f.keyboard) == Some(true) {
                        let just_taken =
                            with(|f| f.took_keyboard.is_some_and(|t| t.elapsed().as_millis() < 500)).unwrap_or(false);
                        if just_taken {
                            unsafe {
                                let _ = SetForegroundWindow(hwnd);
                                let _ = SetFocus(Some(hwnd));
                            }
                        } else {
                            close();
                        }
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        (WM_LBUTTONDOWN, Some(idx)) => {
            // An app might be dragged out to the bar or a subcategory.
            let (x, y) = mouse_xy(lparam);
            if let Some(i) = elem_at(idx, x, y)
                && let Some(Elem::Tile(id) | Elem::Row(id)) = elem(idx, i)
            {
                let mut start = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut start);
                }
                end_drag();
                DRAG.with(|d| {
                    *d.borrow_mut() = Some(Drag {
                        level: idx,
                        elem: i,
                        app_id: id,
                        start,
                        active: false,
                        target: None,
                        cancelled: false,
                    })
                });
                unsafe {
                    SetCapture(hwnd);
                }
            }
            LRESULT(0)
        }
        (WM_LBUTTONUP, Some(idx)) if DRAG.with(|d| d.borrow().is_some()) => {
            let (x, y) = mouse_xy(lparam);
            drag_released(idx, x, y);
            LRESULT(0)
        }
        (WM_CAPTURECHANGED, Some(_)) => {
            end_drag();
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
        (WM_KEYDOWN, Some(_)) if wparam.0 == 0x1B && cancel_drag() => LRESULT(0),
        (WM_KEYDOWN, Some(_)) if mark_keys_used() && on_key(wparam.0 as u16) => LRESULT(0),
        (WM_CHAR, Some(_)) if mark_keys_used() && on_char(char::from_u32(wparam.0 as u32).unwrap_or('\0')) => {
            LRESULT(0)
        }
        // Another window was activated while the flyouts had the keyboard:
        // close them, after the activation has settled (a menu of ours
        // doesn't count).
        (WM_ACTIVATE, Some(0)) => {
            if ui::loword(wparam.0) == 0 && with(|f| f.keyboard) == Some(true) && !MENU_UP.with(|m| m.get()) {
                unsafe {
                    SetTimer(Some(hwnd), TIMER_DEACTIVATED, 1, None);
                }
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
