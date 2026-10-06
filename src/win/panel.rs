//! The look of the settings windows (Manage, Appearance, Arrange the bar,
//! the custom-app dialog), in the style of Windows 11 Settings: a page title,
//! controls grouped on white cards with rounded corners and a title, on a
//! soft grey page, and a command bar along the bottom.
//!
//! A window describes its page with [`set`] (recomputed when it is laid
//! out), paints it from `WM_PAINT` with [`paint`] and colours its labels,
//! check boxes and sliders from `WM_CTLCOLORSTATIC`/`WM_CTLCOLORBTN` with
//! [`color`], which gives each control the background it sits on. The page is
//! drawn anti-aliased with tiny-skia, only when Windows asks for it.

use super::canvas::{self, Canvas};
use crate::appearance::Rgba;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, EndPaint, HBRUSH, HDC, HFONT,
    InvalidateRect, MapWindowPoints, PAINTSTRUCT, SetBkColor, SetTextColor,
};
use windows::Win32::Graphics::Gdi::{RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RedrawWindow};
use windows::Win32::UI::Controls::{
    LVM_SETBKCOLOR, LVM_SETTEXTBKCOLOR, LVM_SETTEXTCOLOR, SetWindowTheme, TVM_SETBKCOLOR, TVM_SETTEXTCOLOR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GWL_STYLE, GetClassNameW, GetClientRect, GetWindowLongW, GetWindowRect, SendMessageW,
};
use windows::core::{BOOL, PCWSTR, w};

/// The page's colours, light or dark.
#[derive(Clone, Copy)]
struct Colours {
    page: Rgba,
    card: Rgba,
    card_border: Rgba,
    footer: Rgba,
    footer_line: Rgba,
    /// Text boxes and lists.
    field: Rgba,
    text: Rgba,
    subtle: Rgba,
}

const LIGHT: Colours = Colours {
    page: Rgba::rgb(0xF3, 0xF3, 0xF3),
    card: Rgba::rgb(0xFF, 0xFF, 0xFF),
    card_border: Rgba::rgb(0xE3, 0xE3, 0xE3),
    footer: Rgba::rgb(0xEA, 0xEA, 0xEA),
    footer_line: Rgba::rgb(0xDC, 0xDC, 0xDC),
    field: Rgba::rgb(0xFF, 0xFF, 0xFF),
    text: Rgba::rgb(0x1A, 0x1A, 0x1A),
    subtle: Rgba::rgb(0x5F, 0x5F, 0x5F),
};

/// Like Windows 11 Settings in dark mode.
const DARK: Colours = Colours {
    page: Rgba::rgb(0x20, 0x20, 0x20),
    card: Rgba::rgb(0x2B, 0x2B, 0x2B),
    card_border: Rgba::rgb(0x3A, 0x3A, 0x3A),
    footer: Rgba::rgb(0x1C, 0x1C, 0x1C),
    footer_line: Rgba::rgb(0x33, 0x33, 0x33),
    field: Rgba::rgb(0x1F, 0x1F, 0x1F),
    text: Rgba::rgb(0xF0, 0xF0, 0xF0),
    subtle: Rgba::rgb(0xA8, 0xA8, 0xA8),
};

/// The colours for the current theme setting, or Windows' own while a
/// high-contrast theme is on.
fn colours() -> Colours {
    if let Some(sc) = super::theme::high_contrast() {
        return Colours {
            page: sc.window,
            card: sc.window,
            card_border: sc.text,
            footer: sc.window,
            footer_line: sc.text,
            field: sc.window,
            text: sc.text,
            subtle: sc.gray_text,
        };
    }
    if dark() { DARK } else { LIGHT }
}

/// Whether the settings windows are dark right now (never in high
/// contrast: Windows draws the controls in its contrast colours then).
pub fn dark() -> bool {
    super::theme::windows_dark()
}

/// The brush for background `i` (page, card, command bar, field).
fn brush(i: usize) -> HBRUSH {
    if super::theme::high_contrast().is_some() {
        // Every background is the window colour; Windows owns this brush.
        return unsafe {
            HBRUSH(windows::Win32::Graphics::Gdi::GetSysColorBrush(windows::Win32::Graphics::Gdi::COLOR_WINDOW).0)
        };
    }
    BRUSHES.with(|b| b[dark() as usize][i])
}

/// A group of controls on a card.
pub struct Card {
    pub rect: RECT,
    pub title: String,
    /// A line under the title in grey (may be empty).
    pub subtitle: String,
}

#[derive(Default)]
pub struct Page {
    /// The page title at the top (empty: none).
    pub title: String,
    pub subtitle: String,
    pub cards: Vec<Card>,
    /// Where the command bar along the bottom starts (client y).
    pub footer: Option<i32>,
}

/// Spacing shared by every page, in pixels at `dpi`.
#[derive(Clone, Copy)]
pub struct Metrics {
    /// Around the page.
    pub margin: i32,
    /// Between cards.
    pub gap: i32,
    /// Inside a card, around its controls.
    pub pad: i32,
    /// The page title's height (title and subtitle).
    pub header: i32,
    /// A card's title (and subtitle) above its controls.
    pub card_title: i32,
    pub card_title_sub: i32,
    /// The command bar's height.
    pub footer: i32,
}

pub fn metrics(dpi: u32) -> Metrics {
    let s = |v: i32| super::ui::scale(v, dpi);
    Metrics {
        margin: s(16),
        gap: s(12),
        pad: s(14),
        header: s(58),
        card_title: s(40),
        card_title_sub: s(58),
        footer: s(56),
    }
}

impl Metrics {
    /// Where a card's controls start, below its title.
    pub fn content_top(&self, card_top: i32, has_subtitle: bool) -> i32 {
        card_top + if has_subtitle { self.card_title_sub } else { self.card_title }
    }
}

struct Fonts {
    title: HFONT,
    card: HFONT,
    body: HFONT,
}

struct State {
    page: Page,
    /// Labels drawn in grey.
    subtle: HashSet<isize>,
}

thread_local! {
    static PAGES: RefCell<HashMap<isize, State>> = RefCell::new(HashMap::new());
    /// Fonts by DPI, made once (a handful at most).
    static FONTS: RefCell<HashMap<u32, Fonts>> = RefCell::new(HashMap::new());
    /// Page, card, command bar and field brushes, light then dark; made
    /// once.
    static BRUSHES: [[HBRUSH; 4]; 2] = unsafe {
        let make = |c: &Colours| {
            [c.page, c.card, c.footer, c.field].map(|x| CreateSolidBrush(colorref(x)))
        };
        [make(&LIGHT), make(&DARK)]
    };
}

const fn colorref(c: Rgba) -> COLORREF {
    super::theme::rgb(c.r, c.g, c.b)
}

fn with_fonts<R>(dpi: u32, f: impl FnOnce(&Fonts) -> R) -> R {
    FONTS.with(|m| {
        let mut m = m.borrow_mut();
        let fonts = m.entry(dpi).or_insert_with(|| Fonts {
            title: canvas::font(super::ui::scale(22, dpi), true),
            card: canvas::font(super::ui::scale(15, dpi), true),
            body: canvas::font(super::ui::scale(12, dpi), false),
        });
        f(fonts)
    })
}

/// Sets (or replaces) the page of `hwnd` and repaints it.
pub fn set(hwnd: HWND, page: Page) {
    PAGES.with(|p| {
        let mut p = p.borrow_mut();
        let st = p.entry(hwnd.0 as isize).or_insert_with(|| State { page: Page::default(), subtle: HashSet::new() });
        st.page = page;
    });
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// Draws `label` (a child of `hwnd`) in grey.
pub fn subtle(hwnd: HWND, label: HWND) {
    PAGES.with(|p| {
        p.borrow_mut()
            .entry(hwnd.0 as isize)
            .or_insert_with(|| State { page: Page::default(), subtle: HashSet::new() })
            .subtle
            .insert(label.0 as isize);
    });
}

/// The window is going away.
pub fn forget(hwnd: HWND) {
    PAGES.with(|p| p.borrow_mut().remove(&(hwnd.0 as isize)));
}

/// `WM_PAINT`: the page, its cards and the command bar.
pub fn paint(hwnd: HWND) -> LRESULT {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let dpi = super::ui::dpi_of(hwnd);
        if let Some(mut cv) = Canvas::new(rc.right, rc.bottom) {
            draw(hwnd, &mut cv, dpi);
            cv.blit(hdc);
        }
        let _ = EndPaint(hwnd, &ps);
    }
    LRESULT(0)
}

fn draw(hwnd: HWND, cv: &mut Canvas, dpi: u32) {
    let s = |v: i32| super::ui::scale(v, dpi);
    let m = metrics(dpi);
    let (w, h) = (cv.width(), cv.height());
    let k = colours();
    cv.fill_round_rect(0.0, 0.0, w as f32, h as f32, 0.0, k.page);
    PAGES.with(|p| {
        let p = p.borrow();
        let Some(st) = p.get(&(hwnd.0 as isize)) else { return };
        let page = &st.page;
        with_fonts(dpi, |f| {
            let line = |cv: &mut Canvas, text: &str, x: i32, y: i32, right: i32, hh: i32, font: HFONT, c: Rgba| {
                let r = RECT { left: x, top: y, right, bottom: y + hh };
                cv.text(text, r, font, c, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            };
            if !page.title.is_empty() {
                line(cv, &page.title, m.margin, s(10), w - m.margin, s(30), f.title, k.text);
                if !page.subtitle.is_empty() {
                    line(cv, &page.subtitle, m.margin, s(38), w - m.margin, s(18), f.body, k.subtle);
                }
            }
            let r = s(8) as f32;
            for c in &page.cards {
                let (x, y) = (c.rect.left as f32, c.rect.top as f32);
                let (cw, ch) = ((c.rect.right - c.rect.left) as f32, (c.rect.bottom - c.rect.top) as f32);
                cv.fill_round_rect(x, y, cw, ch, r, k.card);
                cv.stroke_round_rect(x, y, cw, ch, r, 1.0, k.card_border);
                let right = c.rect.right - m.pad;
                line(cv, &c.title, c.rect.left + m.pad, c.rect.top + s(12), right, s(20), f.card, k.text);
                if !c.subtitle.is_empty() {
                    line(cv, &c.subtitle, c.rect.left + m.pad, c.rect.top + s(33), right, s(16), f.body, k.subtle);
                }
            }
            if let Some(top) = page.footer {
                cv.fill_round_rect(0.0, top as f32, w as f32, (h - top) as f32, 0.0, k.footer);
                cv.fill_round_rect(0.0, top as f32, w as f32, 1.0, 0.0, k.footer_line);
            }
        });
    });
}

/// `WM_CTLCOLORSTATIC` / `WM_CTLCOLORBTN`: the background of whatever the
/// control sits on (a card, the command bar or the page), and grey text for
/// labels marked [`subtle`].
pub fn color(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let control = HWND(lparam.0 as *mut _);
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(control, &mut r);
        let mut pts = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
        MapWindowPoints(None, Some(hwnd), &mut pts);
        r = RECT { left: pts[0].x, top: pts[0].y, right: pts[1].x, bottom: pts[1].y };
    }
    let centre = POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 };
    let inside = |c: &RECT| centre.x >= c.left && centre.x < c.right && centre.y >= c.top && centre.y < c.bottom;
    let (bg, grey) = PAGES.with(|p| {
        let p = p.borrow();
        let Some(st) = p.get(&(hwnd.0 as isize)) else { return (0, false) };
        let grey = st.subtle.contains(&(control.0 as isize));
        if st.page.cards.iter().any(|c| inside(&c.rect)) {
            (1, grey)
        } else if st.page.footer.is_some_and(|f| centre.y >= f) {
            (2, grey)
        } else {
            (0, grey)
        }
    });
    let k = colours();
    let colour = [k.page, k.card, k.footer][bg];
    unsafe {
        let hdc = HDC(wparam.0 as *mut _);
        SetBkColor(hdc, colorref(colour));
        SetTextColor(hdc, colorref(if grey { k.subtle } else { k.text }));
        LRESULT(brush(bg).0 as isize)
    }
}

/// `WM_CTLCOLOREDIT` / `WM_CTLCOLORLISTBOX`: text boxes and the lists of
/// combo boxes, in the field colour.
pub fn field_color(wparam: WPARAM) -> LRESULT {
    let k = colours();
    unsafe {
        let hdc = HDC(wparam.0 as *mut _);
        SetBkColor(hdc, colorref(k.field));
        SetTextColor(hdc, colorref(k.text));
        LRESULT(brush(3).0 as isize)
    }
}

/// Gives `hwnd` and its controls the current theme: the title bar, buttons,
/// scroll bars, lists and trees. Call after the controls are made, and
/// again when the theme changes (see [`theme_changed`]).
pub fn apply_theme(hwnd: HWND) {
    let dark = dark();
    super::theme::style_window(hwnd, dark, false);
    if dark {
        super::theme::allow_dark_for_window(hwnd);
    }
    unsafe extern "system" fn each(child: HWND, lparam: LPARAM) -> BOOL {
        theme_control(child, lparam.0 != 0);
        BOOL(1)
    }
    unsafe {
        let _ = EnumChildWindows(Some(hwnd), Some(each), LPARAM(dark as isize));
        let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME);
    }
}

fn theme_control(h: HWND, dark: bool) {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
    let class = String::from_utf16_lossy(&buf[..n]).to_ascii_lowercase();
    let style = unsafe { GetWindowLongW(h, GWL_STYLE) } as u32 & 0xF;
    let k = colours();
    // The visual-styles class names Windows uses for its own dark windows
    // (Explorer, the file dialogs). Unknown names are harmless.
    let theme = |name: Option<&str>| {
        let w = name.map(super::ui::wide);
        unsafe {
            let _ = SetWindowTheme(h, w.as_ref().map(|w| PCWSTR(w.as_ptr())).unwrap_or(w!("")), PCWSTR::null());
        }
    };
    match class.as_str() {
        "button" => {
            const BS_CHECKBOX_KINDS: [u32; 4] = [2, 3, 5, 6]; // check boxes and 3-state boxes
            if BS_CHECKBOX_KINDS.contains(&style) {
                // A themed check box ignores the text colour, so in dark mode
                // it is drawn unthemed (its label then follows `color`).
                if dark { theme(Some("")) } else { theme(Some("Explorer")) }
            } else {
                theme(Some(if dark { "DarkMode_Explorer" } else { "Explorer" }));
            }
        }
        "combobox" | "edit" => theme(Some(if dark { "DarkMode_CFD" } else { "Explorer" })),
        "syslistview32" => {
            theme(Some(if dark { "DarkMode_Explorer" } else { "Explorer" }));
            let (bg, fg) = (colorref(k.field).0 as isize, colorref(k.text).0 as isize);
            unsafe {
                SendMessageW(h, LVM_SETBKCOLOR, Some(WPARAM(0)), Some(LPARAM(bg)));
                SendMessageW(h, LVM_SETTEXTBKCOLOR, Some(WPARAM(0)), Some(LPARAM(bg)));
                SendMessageW(h, LVM_SETTEXTCOLOR, Some(WPARAM(0)), Some(LPARAM(fg)));
            }
        }
        "systreeview32" => {
            theme(Some(if dark { "DarkMode_Explorer" } else { "Explorer" }));
            let (bg, fg) = (colorref(k.field).0 as isize, colorref(k.text).0 as isize);
            unsafe {
                SendMessageW(h, TVM_SETBKCOLOR, Some(WPARAM(0)), Some(LPARAM(bg)));
                SendMessageW(h, TVM_SETTEXTCOLOR, Some(WPARAM(0)), Some(LPARAM(fg)));
            }
        }
        _ => {}
    }
}

/// The theme setting changed: re-theme every open settings window.
pub fn theme_changed() {
    let open: Vec<isize> = PAGES.with(|p| p.borrow().keys().copied().collect());
    for h in open {
        apply_theme(HWND(h as *mut _));
    }
}
