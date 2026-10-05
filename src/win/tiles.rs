//! Tile rendering for category popups from the icon strip: each app or
//! subcategory is an owner-drawn menu item showing a large icon with its name
//! underneath, laid out in a horizontal row (wrapping into a grid when long).
//!
//! They stay real menu items, so subcategory tiles still cascade on hover and
//! keyboard navigation (arrow keys, Enter, Esc) keeps working.

use super::app;
use super::icons::{self, Source as IconSource};
use super::theme;
use super::ui::{self, scale, wide};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BLENDFUNCTION, COLOR_MENU, COLOR_MENUTEXT, CreateCompatibleDC,
    CreateSolidBrush, DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteDC,
    DeleteObject, DrawTextW, FillRect, GetStockObject, GetSysColor, HBITMAP, HFONT, HGDIOBJ, NULL_PEN, RoundRect,
    SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_DISABLED, ODS_GRAYED, ODS_SELECTED, ODT_MENU,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;

/// One tile; menu items carry its index in `dwItemData`.
pub struct Tile {
    pub text: String,
    /// Icon cache key (app id, `cat:<id>`); `None` for text-only tiles.
    pub key: Option<String>,
    /// A compact row (small icon, name beside it) instead of a tile; used for
    /// subcategories.
    pub row: bool,
}

thread_local! {
    static TILES: RefCell<Vec<Tile>> = const { RefCell::new(Vec::new()) };
    /// Tile-sized icons, keyed like the app's icon cache.
    static ICONS: RefCell<HashMap<String, Option<HBITMAP>>> = RefCell::new(HashMap::new());
    static FONT: Cell<Option<HFONT>> = const { Cell::new(None) };
}

fn dpi() -> u32 {
    unsafe { GetDpiForSystem() }.max(96)
}

fn icon_size() -> i32 {
    scale(32, dpi())
}

/// Starts a new menu: forgets the previous menu's tiles.
pub fn reset() {
    TILES.with(|t| t.borrow_mut().clear());
}

/// Registers a tile and returns its item data. Must not be called while the
/// app state is borrowed (it may load an icon).
pub fn add(tile: Tile) -> usize {
    TILES.with(|t| {
        let mut t = t.borrow_mut();
        t.push(tile);
        t.len() - 1
    })
}

/// Loads (once) the large icon for every tile registered so far. Call after
/// building the menu, outside any app-state borrow.
pub fn load_icons() {
    let keys: Vec<String> = TILES.with(|t| t.borrow().iter().filter_map(|t| t.key.clone()).collect());
    let missing: Vec<String> = ICONS.with(|i| keys.into_iter().filter(|k| !i.borrow().contains_key(k)).collect());
    if missing.is_empty() {
        return;
    }
    let sources: Vec<(String, Option<IconSource>)> =
        app::with(|s| missing.into_iter().map(|k| (k.clone(), app::icon_source_for(s, &k))).collect());
    let size = icon_size();
    for (key, src) in sources {
        let bmp = src.and_then(|src| icons::load(&src, size));
        ICONS.with(|i| i.borrow_mut().insert(key, bmp));
    }
}

/// An icon was replaced; drop the cached tile-sized copy.
pub fn icon_changed(key: &str) {
    if let Some(Some(bmp)) = ICONS.with(|i| i.borrow_mut().remove(key)) {
        icons::free(bmp);
    }
}

fn font() -> HFONT {
    FONT.with(|f| {
        if let Some(font) = f.get() {
            return font;
        }
        let font = ui::message_font(dpi(), 1.0);
        f.set(Some(font));
        font
    })
}

fn tile_size() -> (i32, i32) {
    let d = dpi();
    (scale(88, d), scale(80, d))
}

/// WM_MEASUREITEM for owner-drawn menu items. Returns false if it isn't ours.
pub fn measure(mis: &mut MEASUREITEMSTRUCT) -> bool {
    if mis.CtlType != ODT_MENU {
        return false;
    }
    let ours = TILES.with(|t| t.borrow().len() > mis.itemData);
    if !ours {
        return false;
    }
    let row = TILES.with(|t| t.borrow()[mis.itemData].row);
    let (w, h) = if row { (scale(170, dpi()), scale(36, dpi())) } else { tile_size() };
    mis.itemWidth = w as u32;
    mis.itemHeight = h as u32;
    true
}

/// WM_DRAWITEM for owner-drawn menu items. Returns false if it isn't ours.
pub fn draw(dis: &DRAWITEMSTRUCT) -> bool {
    if dis.CtlType != ODT_MENU {
        return false;
    }
    let tile = TILES.with(|t| t.borrow().get(dis.itemData).map(|t| (t.text.clone(), t.key.clone(), t.row)));
    let Some((text, key, row)) = tile else { return false };
    let icon = key.and_then(|k| ICONS.with(|i| i.borrow().get(&k).copied().flatten()));

    let dark = theme::is_dark_cached();
    let selected = dis.itemState.0 & ODS_SELECTED.0 != 0;
    let disabled = dis.itemState.0 & (ODS_DISABLED.0 | ODS_GRAYED.0) != 0;
    let (bg, hl, fg, dim) = if dark {
        (theme::rgb(43, 43, 43), theme::rgb(66, 66, 66), theme::rgb(240, 240, 240), theme::rgb(130, 130, 130))
    } else {
        unsafe {
            (
                windows::Win32::Foundation::COLORREF(GetSysColor(COLOR_MENU)),
                theme::rgb(222, 229, 240),
                windows::Win32::Foundation::COLORREF(GetSysColor(COLOR_MENUTEXT)),
                theme::rgb(140, 140, 140),
            )
        }
    };
    let hdc = dis.hDC;
    let rc = dis.rcItem;
    let d = dpi();
    unsafe {
        let brush = CreateSolidBrush(bg);
        FillRect(hdc, &rc, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        if selected && !disabled {
            let hb = CreateSolidBrush(hl);
            let old_b = SelectObject(hdc, HGDIOBJ(hb.0));
            let old_p = SelectObject(hdc, GetStockObject(NULL_PEN));
            let m = scale(2, d);
            let r = scale(8, d);
            let _ = RoundRect(hdc, rc.left + m, rc.top + m, rc.right - m, rc.bottom - m, r, r);
            SelectObject(hdc, old_p);
            SelectObject(hdc, old_b);
            let _ = DeleteObject(HGDIOBJ(hb.0));
        }

        let src_size = icon_size();
        let blit = |bmp: HBITMAP, x: i32, y: i32, size: i32| {
            let src = CreateCompatibleDC(Some(hdc));
            let prev = SelectObject(src, HGDIOBJ(bmp.0));
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: if disabled { 110 } else { 255 },
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = AlphaBlend(hdc, x, y, size, size, src, 0, 0, src_size, src_size, blend);
            SelectObject(src, prev);
            let _ = DeleteDC(src);
        };

        let old_font = SelectObject(hdc, HGDIOBJ(font().0));
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, if disabled { dim } else { fg });
        let mut w = wide(&text);
        let len = w.len() - 1;
        let pad = scale(4, d);
        if row {
            // Small icon on the left, name beside it (the system draws the ▸).
            let size = scale(20, d);
            let x = rc.left + scale(10, d);
            if let Some(bmp) = icon {
                blit(bmp, x, rc.top + (ui::rect_h(&rc) - size) / 2, size);
            }
            let mut text_rc =
                RECT { left: x + size + scale(8, d), top: rc.top, right: rc.right - scale(18, d), bottom: rc.bottom };
            DrawTextW(hdc, &mut w[..len], &mut text_rc, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
        } else {
            let size = src_size;
            let top = rc.top + scale(8, d);
            let text_top = if let Some(bmp) = icon {
                blit(bmp, rc.left + (ui::rect_w(&rc) - size) / 2, top, size);
                top + size + scale(4, d)
            } else {
                top + size / 3
            };
            let mut text_rc =
                RECT { left: rc.left + pad, top: text_top, right: rc.right - pad, bottom: rc.bottom - pad };
            DrawTextW(hdc, &mut w[..len], &mut text_rc, DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX);
        }
        SelectObject(hdc, old_font);
    }
    true
}
