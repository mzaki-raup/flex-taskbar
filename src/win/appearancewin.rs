//! The Appearance window: theme, colours, transparency, border, corners and
//! sizes of the icon strip and its flyouts. Every change shows on the strip
//! straight away and is saved shortly after.

use super::app;
use super::canvas;
use super::ui::{self, scale, wide};
use super::{searchwin, strip, theme};
use crate::appearance::{Appearance, DockWidth, IconAlign, Rgba, ThemeMode};
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, GetSysColorBrush, HBRUSH, HFONT};
use windows::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, IsIconic, KillTimer, RegisterClassW, SW_RESTORE,
    SW_SHOW, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_HSCROLL, WM_TIMER, WM_USER, WNDCLASSW,
    WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{PCWSTR, w};

// Combo boxes.
const THEME: u16 = 10;
const DOCK: u16 = 11;
const POSITION: u16 = 12;
const ALIGN: u16 = 13;
// Colour buttons, and their "Default" buttons (+100).
const ACCENT: u16 = 20;
const BACKGROUND: u16 = 21;
const BORDER: u16 = 22;
const FLYOUT_BORDER: u16 = 23;
const DEFAULT_OFFSET: u16 = 100;
// Sliders; each has a value label at +200.
const OPACITY: u16 = 30;
const BORDER_WIDTH: u16 = 31;
const RADIUS: u16 = 32;
const MARGIN: u16 = 33;
const ICON_SIZE: u16 = 34;
const BAR_HEIGHT: u16 = 35;
const COLUMNS: u16 = 36;
const FLYOUT_RADIUS: u16 = 37;
const FLYOUT_BORDER_WIDTH: u16 = 38;
const VALUE_OFFSET: u16 = 200;
const RESET: u16 = 50;
const CLOSE: u16 = 51;
const LABEL_BASE: u16 = 500;

const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const CBN_SELCHANGE: u16 = 1;
const CBS_DROPDOWNLIST: u32 = 0x0003;
const TBM_GETPOS: u32 = WM_USER;
const TBM_SETPOS: u32 = WM_USER + 5;
const TBM_SETRANGE: u32 = WM_USER + 6;
const BN_CLICKED: u16 = 0;
const TIMER_SAVE: usize = 1;

struct Win {
    hwnd: HWND,
    controls: HashMap<u16, HWND>,
    font: HFONT,
    heading: HFONT,
    /// Custom colours of the colour picker, kept while the window is open.
    custom: [COLORREF; 16],
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
}

fn ctl(id: u16) -> HWND {
    WIN.with(|w| w.borrow().as_ref().and_then(|w| w.controls.get(&id).copied()).unwrap_or_default())
}

fn hwnd() -> Option<HWND> {
    WIN.with(|w| w.borrow().as_ref().map(|w| w.hwnd))
}

/// (id, label, min, max) of every slider.
const SLIDERS: [(u16, &str, u32, u32); 9] = [
    (OPACITY, "Background opacity (%)", 10, 100),
    (BORDER_WIDTH, "Border width", 0, 6),
    (RADIUS, "Corner radius", 0, 24),
    (MARGIN, "Gap from screen edge", 0, 24),
    (ICON_SIZE, "Icon size", 16, 48),
    (BAR_HEIGHT, "Bar thickness", 32, 96),
    (COLUMNS, "App tiles per row", 1, 12),
    (FLYOUT_RADIUS, "Corner radius", 0, 24),
    (FLYOUT_BORDER_WIDTH, "Border width", 0, 6),
];

const COLOURS: [(u16, &str); 4] = [
    (ACCENT, "Accent colour"),
    (BACKGROUND, "Background colour"),
    (BORDER, "Border colour"),
    (FLYOUT_BORDER, "Border colour"),
];

/// The window's rows, top to bottom.
enum Row {
    /// Start the next column.
    Column,
    Heading(&'static str),
    Combo(u16, &'static str, &'static [&'static str]),
    Colour(u16),
    Slider(u16),
}

/// Two columns: general look and the flyouts on the left, the bar on the right.
const ROWS: [Row; 22] = [
    Row::Heading("General"),
    Row::Combo(THEME, "Theme", &["Windows default", "Dark", "Light"]),
    Row::Colour(ACCENT),
    Row::Colour(BACKGROUND),
    Row::Slider(OPACITY),
    Row::Slider(ICON_SIZE),
    Row::Heading("Category flyouts"),
    Row::Slider(COLUMNS),
    Row::Colour(FLYOUT_BORDER),
    Row::Slider(FLYOUT_BORDER_WIDTH),
    Row::Slider(FLYOUT_RADIUS),
    Row::Column,
    Row::Heading("Bar"),
    Row::Combo(
        POSITION,
        "Position (or drag the bar)",
        &["Next to the Windows taskbar", "Bottom", "Top", "Left", "Right"],
    ),
    Row::Combo(DOCK, "Bar width", &["Full screen width", "Fit to icons (floating dock)"]),
    Row::Combo(ALIGN, "Icons", &["Centred", "At the start, right after All"]),
    Row::Colour(BORDER),
    Row::Slider(BORDER_WIDTH),
    Row::Slider(RADIUS),
    Row::Slider(MARGIN),
    Row::Slider(BAR_HEIGHT),
    Row::Heading(""),
];

pub fn show() {
    if let Some(h) = hwnd() {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    create();
}

fn create() {
    let class = wide("FlexTaskbar.Appearance");
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: app::instance(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hbrBackground: HBRUSH(GetSysColorBrush(COLOR_WINDOW).0),
            hIcon: app::app_icon(32),
            hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                None,
                windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
            )
            .unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            w!("FlexTaskbar — Appearance"),
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(app::instance()),
            None,
        ) else {
            return;
        };
        let dpi = ui::dpi_of(hwnd);
        let s = |v: i32| scale(v, dpi);
        let mut controls = HashMap::new();
        let mut labels = 0u16;
        let mut bold: Vec<HWND> = Vec::new();
        let mut label = |controls: &mut HashMap<u16, HWND>, text: &str| {
            let id = LABEL_BASE + labels;
            labels += 1;
            let h = ui::child(hwnd, "STATIC", text, WS_CHILD | WS_VISIBLE | ui::SS_LABEL, WINDOW_EX_STYLE(0), id);
            controls.insert(id, h);
            h
        };
        let button = |text: &str, id: u16| {
            ui::child(
                hwnd,
                "BUTTON",
                text,
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                WINDOW_EX_STYLE(0),
                id,
            )
        };
        let combo = |id: u16, items: &[&str]| {
            let h = ui::child(
                hwnd,
                "COMBOBOX",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST),
                WINDOW_EX_STYLE(0),
                id,
            );
            for item in items {
                let t = wide(item);
                SendMessageW(h, CB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(t.as_ptr() as isize)));
            }
            h
        };

        // One row per setting: a label on the left, the control on the right.
        let (m, label_w, row_h, ctl_h) = (s(14), s(170), s(34), s(26));
        let ctl_x = m + label_w;
        let ctl_w = s(250);
        let mut y = m;
        let place = |h: HWND, x: i32, w: i32, height: i32, y: i32| {
            let _ = SetWindowPos(h, None, x, y + (row_h - height) / 2, w, height, SWP_NOZORDER);
        };

        let (mut x0, mut bottom) = (0, 0);
        for row in &ROWS {
            match *row {
                Row::Column => {
                    bottom = bottom.max(y);
                    x0 += ctl_x + ctl_w + m;
                    y = m;
                    continue;
                }
                Row::Heading(text) => {
                    if !text.is_empty() {
                        let h = label(&mut controls, text);
                        place(h, x0 + m, label_w + ctl_w, s(18), y + s(4));
                        bold.push(h);
                    }
                }
                Row::Combo(id, text, items) => {
                    place(label(&mut controls, text), x0 + m, label_w, s(18), y);
                    let h = combo(id, items);
                    // A combo box's height includes its drop-down list.
                    let _ = SetWindowPos(h, None, x0 + ctl_x, y + (row_h - s(24)) / 2, ctl_w, s(200), SWP_NOZORDER);
                    controls.insert(id, h);
                }
                Row::Colour(id) => {
                    let text = COLOURS.iter().find(|(c, _)| *c == id).map(|(_, t)| *t).unwrap_or_default();
                    place(label(&mut controls, text), x0 + m, label_w, s(18), y);
                    let h = button("", id);
                    place(h, x0 + ctl_x, s(160), ctl_h, y);
                    controls.insert(id, h);
                    let d = button("Default", id + DEFAULT_OFFSET);
                    place(d, (x0 + ctl_x) + s(166), ctl_w - s(166), ctl_h, y);
                    controls.insert(id + DEFAULT_OFFSET, d);
                }
                Row::Slider(id) => {
                    let Some(&(_, text, min, max)) = SLIDERS.iter().find(|(s, ..)| *s == id) else { continue };
                    place(label(&mut controls, text), x0 + m, label_w, s(18), y);
                    let h = ui::child(
                        hwnd,
                        "msctls_trackbar32",
                        "",
                        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                        WINDOW_EX_STYLE(0),
                        id,
                    );
                    SendMessageW(h, TBM_SETRANGE, Some(WPARAM(1)), Some(LPARAM(((max << 16) | min) as isize)));
                    place(h, x0 + ctl_x, ctl_w - s(44), ctl_h, y);
                    controls.insert(id, h);
                    let v = ui::child(
                        hwnd,
                        "STATIC",
                        "",
                        WS_CHILD | WS_VISIBLE | ui::SS_LABEL,
                        WINDOW_EX_STYLE(0),
                        id + VALUE_OFFSET,
                    );
                    place(v, (x0 + ctl_x) + ctl_w - s(40), s(40), s(18), y);
                    controls.insert(id + VALUE_OFFSET, v);
                }
            }
            y += if matches!(row, Row::Heading(_)) { row_h * 3 / 4 } else { row_h };
        }
        y = y.max(bottom);
        let right = x0 + ctl_x + ctl_w + m;
        y += s(8);
        let r = button("Reset to defaults", RESET);
        place(r, m, s(150), ctl_h + s(2), y);
        controls.insert(RESET, r);
        let c = button("Close", CLOSE);
        place(c, right - m - s(100), s(100), ctl_h + s(2), y);
        controls.insert(CLOSE, c);
        y += row_h + m;

        let font = ui::message_font(dpi, 1.0);
        let heading = canvas::font(scale(14, dpi), true);
        for h in controls.values() {
            ui::set_font(*h, if bold.contains(h) { heading } else { font });
        }
        WIN.with(|w| *w.borrow_mut() = Some(Win { hwnd, controls, font, heading, custom: [COLORREF(0xFFFFFF); 16] }));
        app::register_dialog(hwnd, true);
        theme::style_window(hwnd, false, false);
        load();

        // Size the window around its client area and centre it.
        let client = windows::Win32::Foundation::RECT { left: 0, top: 0, right, bottom: y };
        let mut outer = client;
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(
            &mut outer,
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            false,
            WINDOW_EX_STYLE(0),
            dpi,
        );
        let (w, h) = (ui::rect_w(&outer), ui::rect_h(&outer));
        let wa = ui::work_area_at_cursor();
        let x = wa.left + (ui::rect_w(&wa) - w).max(0) / 2;
        let top = wa.top + (ui::rect_h(&wa) - h).max(0) / 2;
        let _ = SetWindowPos(hwnd, None, x, top, w, h, SWP_NOZORDER);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Puts the current settings into the controls.
/// Settings changed elsewhere (the bar was dragged to another edge).
pub fn settings_changed() {
    if hwnd().is_some() {
        load();
    }
}

fn load() {
    let (a, height) = app::with(|s| (s.cfg.settings.appearance.clamped(), s.cfg.settings.strip_height.clamp(32, 96)));
    let sel = |id: u16, i: usize| unsafe {
        SendMessageW(ctl(id), CB_SETCURSEL, Some(WPARAM(i)), Some(LPARAM(0)));
    };
    sel(THEME, a.theme as usize);
    sel(DOCK, a.dock_width as usize);
    sel(ALIGN, if a.icon_align == IconAlign::Centre { 0 } else { 1 });
    sel(POSITION, app::with(|s| s.cfg.settings.strip_edge) as usize);
    for (id, value) in [
        (OPACITY, a.opacity as u32),
        (BORDER_WIDTH, a.border_width),
        (RADIUS, a.corner_radius),
        (MARGIN, a.margin),
        (ICON_SIZE, a.icon_size),
        (BAR_HEIGHT, height),
        (COLUMNS, a.flyout_columns),
        (FLYOUT_RADIUS, a.flyout_corner_radius),
        (FLYOUT_BORDER_WIDTH, a.flyout_border_width),
    ] {
        unsafe {
            SendMessageW(ctl(id), TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(value as isize)));
        }
        ui::set_text(ctl(id + VALUE_OFFSET), &value.to_string());
    }
    for (id, value) in
        [(ACCENT, a.accent), (BACKGROUND, a.background), (BORDER, a.border), (FLYOUT_BORDER, a.flyout_border)]
    {
        let text = match value {
            Some(c) => c.with_alpha(255).to_hex(),
            None if id == FLYOUT_BORDER => "Same as the bar".to_string(),
            None => "Theme default".to_string(),
        };
        ui::set_text(ctl(id), &text);
    }
}

fn slider(id: u16) -> u32 {
    unsafe { SendMessageW(ctl(id), TBM_GETPOS, Some(WPARAM(0)), Some(LPARAM(0))).0 as u32 }
}

fn combo(id: u16) -> usize {
    unsafe { SendMessageW(ctl(id), CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0.max(0) as usize }
}

/// Applies a change: shows it on the strip now, saves it shortly.
fn change(f: impl FnOnce(&mut Appearance, &mut u32)) {
    change_settings(|st| f(&mut st.appearance, &mut st.strip_height));
}

fn change_settings(f: impl FnOnce(&mut crate::config::Settings)) {
    let theme_before = app::with(|s| s.cfg.settings.appearance.theme);
    let theme_now = app::with(|s| {
        let st = &mut s.cfg.settings;
        f(st);
        st.appearance.theme
    });
    if theme_now != theme_before {
        theme::set_mode(theme_now);
        searchwin::theme_changed();
    }
    strip::apply_appearance();
    if let Some(h) = hwnd() {
        unsafe {
            SetTimer(Some(h), TIMER_SAVE, 400, None);
        }
    }
}

fn pick_colour(owner: HWND, id: u16) {
    let current = app::with(|s| {
        let a = &s.cfg.settings.appearance;
        let colors = a.colors(theme::is_dark_cached());
        match id {
            ACCENT => colors.accent,
            BACKGROUND => colors.background,
            FLYOUT_BORDER => colors.flyout_border,
            _ => colors.border,
        }
    });
    let mut custom = WIN.with(|w| w.borrow().as_ref().map(|w| w.custom)).unwrap_or([COLORREF(0xFFFFFF); 16]);
    let mut cc = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner,
        rgbResult: theme::rgb(current.r, current.g, current.b),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_RGBINIT | CC_FULLOPEN,
        ..Default::default()
    };
    let ok = unsafe { ChooseColorW(&mut cc).as_bool() };
    WIN.with(|w| {
        if let Some(w) = w.borrow_mut().as_mut() {
            w.custom = custom;
        }
    });
    if !ok {
        return;
    }
    let v = cc.rgbResult.0;
    let picked = Rgba::rgb((v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8);
    change(|a, _| match id {
        ACCENT => a.accent = Some(picked),
        BACKGROUND => a.background = Some(picked),
        FLYOUT_BORDER => a.flyout_border = Some(picked),
        _ => a.border = Some(picked),
    });
    load();
}

fn save_now() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = KillTimer(Some(h), TIMER_SAVE);
        }
    }
    app::save();
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = ui::loword(wparam.0);
            let code = ui::hiword(wparam.0);
            match (id, code) {
                (POSITION, CBN_SELCHANGE) => {
                    use crate::config::StripEdge;
                    let i = combo(id);
                    let edge =
                        [StripEdge::Taskbar, StripEdge::Bottom, StripEdge::Top, StripEdge::Left, StripEdge::Right]
                            [i.min(4)];
                    change_settings(|st| st.strip_edge = edge);
                }
                (ALIGN, CBN_SELCHANGE) => {
                    let i = combo(id);
                    change(|a, _| a.icon_align = [IconAlign::Centre, IconAlign::Start][i.min(1)]);
                }
                (THEME | DOCK, CBN_SELCHANGE) => {
                    let i = combo(id);
                    change(|a, _| match id {
                        THEME => a.theme = [ThemeMode::System, ThemeMode::Dark, ThemeMode::Light][i.min(2)],
                        _ => a.dock_width = [DockWidth::Full, DockWidth::Fit][i.min(1)],
                    });
                }
                (ACCENT | BACKGROUND | BORDER | FLYOUT_BORDER, BN_CLICKED) => pick_colour(hwnd, id),
                (i, BN_CLICKED) if COLOURS.iter().any(|(c, _)| *c == i.wrapping_sub(DEFAULT_OFFSET)) => {
                    change(|a, _| match i - DEFAULT_OFFSET {
                        ACCENT => a.accent = None,
                        BACKGROUND => a.background = None,
                        FLYOUT_BORDER => a.flyout_border = None,
                        _ => a.border = None,
                    });
                    load();
                }
                (RESET, BN_CLICKED) => {
                    change(|a, h| {
                        *a = Appearance::default();
                        *h = crate::config::Settings::default().strip_height;
                    });
                    load();
                }
                (CLOSE, BN_CLICKED) => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            }
            LRESULT(0)
        }
        WM_HSCROLL => {
            let from = HWND(lparam.0 as *mut _);
            if let Some((id, ..)) = SLIDERS.iter().find(|(id, ..)| ctl(*id) == from) {
                let v = slider(*id);
                ui::set_text(ctl(id + VALUE_OFFSET), &v.to_string());
                let id = *id;
                change(|a, h| match id {
                    OPACITY => a.opacity = v as u8,
                    BORDER_WIDTH => a.border_width = v,
                    RADIUS => a.corner_radius = v,
                    MARGIN => a.margin = v,
                    ICON_SIZE => a.icon_size = v,
                    BAR_HEIGHT => *h = v,
                    FLYOUT_RADIUS => a.flyout_corner_radius = v,
                    FLYOUT_BORDER_WIDTH => a.flyout_border_width = v,
                    _ => a.flyout_columns = v,
                });
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_SAVE {
                save_now();
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => ui::static_colors(wparam),
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            save_now();
            app::register_dialog(hwnd, false);
            if let Some(w) = WIN.with(|w| w.borrow_mut().take()) {
                ui::delete_font(w.font);
                ui::delete_font(w.heading);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
