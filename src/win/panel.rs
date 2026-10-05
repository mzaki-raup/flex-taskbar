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
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

const PAGE: Rgba = Rgba::rgb(0xF3, 0xF3, 0xF3);
const CARD: Rgba = Rgba::rgb(0xFF, 0xFF, 0xFF);
const CARD_BORDER: Rgba = Rgba::rgb(0xE3, 0xE3, 0xE3);
const FOOTER: Rgba = Rgba::rgb(0xEA, 0xEA, 0xEA);
const FOOTER_LINE: Rgba = Rgba::rgb(0xDC, 0xDC, 0xDC);
const TEXT: Rgba = Rgba::rgb(0x1A, 0x1A, 0x1A);
const SUBTLE: Rgba = Rgba::rgb(0x5F, 0x5F, 0x5F);

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
    static BRUSHES: [HBRUSH; 3] = unsafe {
        [CreateSolidBrush(colorref(PAGE)), CreateSolidBrush(colorref(CARD)), CreateSolidBrush(colorref(FOOTER))]
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
    cv.fill_round_rect(0.0, 0.0, w as f32, h as f32, 0.0, PAGE);
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
                line(cv, &page.title, m.margin, s(10), w - m.margin, s(30), f.title, TEXT);
                if !page.subtitle.is_empty() {
                    line(cv, &page.subtitle, m.margin, s(38), w - m.margin, s(18), f.body, SUBTLE);
                }
            }
            let r = s(8) as f32;
            for c in &page.cards {
                let (x, y) = (c.rect.left as f32, c.rect.top as f32);
                let (cw, ch) = ((c.rect.right - c.rect.left) as f32, (c.rect.bottom - c.rect.top) as f32);
                cv.fill_round_rect(x, y, cw, ch, r, CARD);
                cv.stroke_round_rect(x, y, cw, ch, r, 1.0, CARD_BORDER);
                let right = c.rect.right - m.pad;
                line(cv, &c.title, c.rect.left + m.pad, c.rect.top + s(12), right, s(20), f.card, TEXT);
                if !c.subtitle.is_empty() {
                    line(cv, &c.subtitle, c.rect.left + m.pad, c.rect.top + s(33), right, s(16), f.body, SUBTLE);
                }
            }
            if let Some(top) = page.footer {
                cv.fill_round_rect(0.0, top as f32, w as f32, (h - top) as f32, 0.0, FOOTER);
                cv.fill_round_rect(0.0, top as f32, w as f32, 1.0, 0.0, FOOTER_LINE);
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
    let colour = [PAGE, CARD, FOOTER][bg];
    unsafe {
        let hdc = HDC(wparam.0 as *mut _);
        SetBkColor(hdc, colorref(colour));
        SetTextColor(hdc, colorref(if grey { SUBTLE } else { TEXT }));
        LRESULT(BRUSHES.with(|b| b[bg]).0 as isize)
    }
}
