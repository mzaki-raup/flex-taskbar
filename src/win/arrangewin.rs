//! The "Arrange the bar" window: the strip's categories and pinned apps in
//! order, left to right. Drag a row to move it, or use the buttons. Every
//! change shows on the strip straight away.

use super::app::{self, FOLDER_ICON};
use super::panel;
use super::ui::{self, scale, wide};
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, GetSysColorBrush, HBRUSH, HFONT, MapWindowPoints};
use windows::Win32::UI::Controls::{
    HIMAGELIST, ImageList_Add, ImageList_Destroy, LIST_VIEW_ITEM_STATE_FLAGS, LVCF_WIDTH, LVCOLUMNW, LVHITTESTINFO,
    LVIF_IMAGE, LVIF_PARAM, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_DELETEALLITEMS, LVM_ENSUREVISIBLE,
    LVM_GETNEXTITEM, LVM_HITTEST, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETIMAGELIST,
    LVM_SETITEMSTATE, LVNI_SELECTED, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER, LVS_REPORT,
    LVS_SHOWSELALWAYS, LVS_SINGLESEL, LVSIL_SMALL, NMHDR, SetWindowTheme,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, IsIconic, RegisterClassW, SW_RESTORE, SW_SHOW,
    SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CAPTURECHANGED, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC,
    WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NOTIFY, WM_PAINT, WNDCLASSW, WS_CAPTION, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, PWSTR, w};

const LIST: u16 = 10;
const TO_START: u16 = 20;
const UP: u16 = 21;
const DOWN: u16 = 22;
const TO_END: u16 = 23;
const UNPIN: u16 = 24;
const CLOSE: u16 = 25;

const BN_CLICKED: u16 = 0;
const LVN_FIRST: i32 = -100;
const LVN_ITEMCHANGED: u32 = (LVN_FIRST - 1) as u32;
const LVN_BEGINDRAG: u32 = (LVN_FIRST - 9) as u32;

struct Win {
    hwnd: HWND,
    controls: HashMap<u16, HWND>,
    font: HFONT,
    images: HIMAGELIST,
    image_index: HashMap<String, i32>,
    /// Bar keys (see `Config::bar_order`), by row.
    rows: Vec<String>,
    /// The key being dragged.
    dragging: Option<String>,
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

/// The bar's contents or order changed (from anywhere): refresh the list.
pub fn bar_changed() {
    if hwnd().is_some() {
        fill();
    }
}

fn create() {
    let class = wide("FlexTaskbar.Arrange");
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
        let style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            w!("FlexTaskbar — Arrange the bar"),
            style,
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
        let place = |h: HWND, x: i32, y: i32, w: i32, height: i32| {
            let _ = SetWindowPos(h, None, x, y, w, height, SWP_NOZORDER);
        };

        let pm = panel::metrics(dpi);
        let (list_w, list_h, bw, bh) = (s(300), s(360), s(130), s(28));
        let card_top = pm.header;
        let (cx, top) = (pm.margin + pm.pad, pm.content_top(card_top, true));
        let m = pm.pad;

        let list = ui::child(
            hwnd,
            "SysListView32",
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WINDOW_STYLE(LVS_REPORT | LVS_NOCOLUMNHEADER | LVS_SHOWSELALWAYS | LVS_SINGLESEL),
            WS_EX_CLIENTEDGE,
            LIST,
        );
        let _ = SetWindowTheme(list, w!("Explorer"), PCWSTR::null());
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(0)),
            Some(LPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize)),
        );
        let col = LVCOLUMNW { mask: LVCF_WIDTH, cx: list_w - s(24), ..Default::default() };
        SendMessageW(list, LVM_INSERTCOLUMNW, Some(WPARAM(0)), Some(LPARAM(&col as *const _ as isize)));
        let images = ui::image_list(app::with(|s| s.icon_size), 16);
        SendMessageW(list, LVM_SETIMAGELIST, Some(WPARAM(LVSIL_SMALL as usize)), Some(LPARAM(images.0)));
        place(list, cx, top, list_w, list_h);
        controls.insert(LIST, list);

        let bx = cx + list_w + m;
        let mut y = top;
        for (text, id) in
            [("Move to start", TO_START), ("Move left  ▲", UP), ("Move right  ▼", DOWN), ("Move to end", TO_END)]
        {
            let h = button(text, id);
            place(h, bx, y, bw, bh);
            controls.insert(id, h);
            y += bh + s(6);
        }
        y += s(12);
        let h = button("Unpin", UNPIN);
        place(h, bx, y, bw, bh);
        controls.insert(UNPIN, h);
        let card_right = bx + bw + pm.pad;
        let card_bottom = top + list_h + pm.pad;
        let footer = card_bottom + pm.margin;
        let right = card_right + pm.margin;
        let h = button("Close", CLOSE);
        place(h, right - pm.margin - s(110), footer + (pm.footer - bh) / 2, s(110), bh);
        controls.insert(CLOSE, h);
        panel::set(
            hwnd,
            panel::Page {
                title: "Arrange the bar".into(),
                subtitle: "Changes show on the bar straight away.".into(),
                cards: vec![panel::Card {
                    rect: RECT { left: pm.margin, top: card_top, right: card_right, bottom: card_bottom },
                    title: "Buttons".into(),
                    subtitle: "The bar's buttons, left to right (top to bottom on a side bar). Drag a row to move it."
                        .into(),
                }],
                footer: Some(footer),
            },
        );

        let font = ui::message_font(dpi, 1.0);
        for h in controls.values() {
            ui::set_font(*h, font);
        }
        WIN.with(|w| {
            *w.borrow_mut() = Some(Win {
                hwnd,
                controls,
                font,
                images,
                image_index: HashMap::new(),
                rows: Vec::new(),
                dragging: None,
            })
        });
        app::register_dialog(hwnd, true);
        panel::apply_theme(hwnd);
        fill();

        let client = RECT { left: 0, top: 0, right, bottom: footer + pm.footer };
        let mut outer = client;
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut outer, style, false, WINDOW_EX_STYLE(0), dpi);
        let (w, h) = (ui::rect_w(&outer), ui::rect_h(&outer));
        let wa = ui::work_area_at_cursor();
        let x = wa.left + (ui::rect_w(&wa) - w).max(0) / 2;
        let y = wa.top + (ui::rect_h(&wa) - h).max(0) / 2;
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn image(key: &str, is_cat: bool) -> i32 {
    if let Some(i) = WIN.with(|w| w.borrow().as_ref().and_then(|w| w.image_index.get(key).copied())) {
        return i;
    }
    let bmp = app::with(|s| if is_cat { s.icon_of(key).or_else(|| s.icon_of(FOLDER_ICON)) } else { s.icon_of(key) });
    let Some(bmp) = bmp else { return ui::NO_IMAGE };
    WIN.with(|w| {
        let mut b = w.borrow_mut();
        let Some(w) = b.as_mut() else { return ui::NO_IMAGE };
        let i = unsafe { ImageList_Add(w.images, bmp, None) };
        if i < 0 {
            return ui::NO_IMAGE;
        }
        w.image_index.insert(key.to_string(), i);
        i
    })
}

/// Rebuilds the list from the configuration, keeping the selection.
fn fill() {
    let selected = selected();
    let rows: Vec<(String, String, bool)> = app::with(|s| {
        s.cfg
            .bar_keys()
            .into_iter()
            .map(|key| {
                if let Some(id) = key.strip_prefix("cat:").and_then(|id| id.parse::<u64>().ok()) {
                    let name = s.cfg.categories.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
                    (key, format!("{name}   (category)"), true)
                } else {
                    let name =
                        s.catalog.get(&key).map(|a| a.name.clone()).unwrap_or_else(|| format!("{key} (not found)"));
                    (key, name, false)
                }
            })
            .collect()
    });
    let list = ctl(LIST);
    unsafe {
        SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    for (i, (key, name, is_cat)) in rows.iter().enumerate() {
        let mut text = wide(name);
        let item = LVITEMW {
            mask: LVIF_TEXT | LVIF_IMAGE | LVIF_PARAM,
            iItem: i as i32,
            pszText: PWSTR(text.as_mut_ptr()),
            iImage: image(key, *is_cat),
            lParam: LPARAM(i as isize),
            ..Default::default()
        };
        unsafe {
            SendMessageW(list, LVM_INSERTITEMW, Some(WPARAM(0)), Some(LPARAM(&item as *const _ as isize)));
        }
    }
    let keys: Vec<String> = rows.into_iter().map(|(k, ..)| k).collect();
    let reselect = selected.and_then(|k| keys.iter().position(|r| *r == k));
    WIN.with(|w| {
        if let Some(w) = w.borrow_mut().as_mut() {
            w.rows = keys;
        }
    });
    if let Some(i) = reselect {
        select_row(i);
    }
    update_buttons();
}

fn selected() -> Option<String> {
    let i = unsafe {
        SendMessageW(ctl(LIST), LVM_GETNEXTITEM, Some(WPARAM(usize::MAX)), Some(LPARAM(LVNI_SELECTED as isize))).0
    };
    if i < 0 {
        return None;
    }
    WIN.with(|w| w.borrow().as_ref().and_then(|w| w.rows.get(i as usize).cloned()))
}

fn select_row(index: usize) {
    let state = LVITEMW {
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        ..Default::default()
    };
    unsafe {
        SendMessageW(ctl(LIST), LVM_SETITEMSTATE, Some(WPARAM(index)), Some(LPARAM(&state as *const _ as isize)));
        SendMessageW(ctl(LIST), LVM_ENSUREVISIBLE, Some(WPARAM(index)), Some(LPARAM(0)));
    }
}

fn update_buttons() {
    let key = selected();
    let (pos, len) = WIN
        .with(|w| {
            w.borrow().as_ref().map(|w| (key.as_ref().and_then(|k| w.rows.iter().position(|r| r == k)), w.rows.len()))
        })
        .unwrap_or((None, 0));
    let enable = |id: u16, on: bool| unsafe {
        let _ = EnableWindow(ctl(id), on);
    };
    enable(TO_START, pos.is_some_and(|p| p > 0));
    enable(UP, pos.is_some_and(|p| p > 0));
    enable(DOWN, pos.is_some_and(|p| p + 1 < len));
    enable(TO_END, pos.is_some_and(|p| p + 1 < len));
    enable(UNPIN, key.is_some_and(|k| !k.starts_with("cat:")));
}

/// Moves the selected button to `to(current index, count)`.
fn move_selected(to: impl FnOnce(usize, usize) -> usize) {
    let Some(key) = selected() else { return };
    let changed = app::with(|s| {
        let keys = s.cfg.bar_keys();
        let Some(pos) = keys.iter().position(|k| *k == key) else { return false };
        s.cfg.move_bar_item(&key, to(pos, keys.len()))
    });
    if changed {
        app::save(); // refreshes the strip and this list
    }
}

/// While dragging: move the dragged row to the row under the pointer.
fn drag_to(pt: POINT) {
    let Some(key) = WIN.with(|w| w.borrow().as_ref().and_then(|w| w.dragging.clone())) else { return };
    let (Some(main), list) = (hwnd(), ctl(LIST)) else { return };
    let mut p = [pt];
    unsafe {
        MapWindowPoints(Some(main), Some(list), &mut p);
    }
    let mut hit = LVHITTESTINFO { pt: p[0], ..Default::default() };
    let row = unsafe { SendMessageW(list, LVM_HITTEST, Some(WPARAM(0)), Some(LPARAM(&mut hit as *mut _ as isize))).0 };
    if row < 0 {
        return;
    }
    if app::with(|s| s.cfg.move_bar_item(&key, row as usize)) {
        app::save();
    }
}

fn mouse_xy(lparam: LPARAM) -> POINT {
    POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let (id, code) = (ui::loword(wparam.0), ui::hiword(wparam.0));
            if code == BN_CLICKED {
                match id {
                    TO_START => move_selected(|_, _| 0),
                    UP => move_selected(|p, _| p.saturating_sub(1)),
                    DOWN => move_selected(|p, n| (p + 1).min(n.saturating_sub(1))),
                    TO_END => move_selected(|_, n| n.saturating_sub(1)),
                    UNPIN => {
                        if let Some(key) = selected() {
                            app::with(|s| s.cfg.unpin(&key));
                            app::save();
                        }
                    }
                    CLOSE => unsafe {
                        let _ = DestroyWindow(hwnd);
                    },
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_NOTIFY => {
            let hdr = unsafe { &*(lparam.0 as *const NMHDR) };
            if hdr.idFrom == LIST as usize {
                match hdr.code {
                    LVN_ITEMCHANGED => update_buttons(),
                    LVN_BEGINDRAG => {
                        let key = selected();
                        if key.is_some() {
                            WIN.with(|w| {
                                if let Some(w) = w.borrow_mut().as_mut() {
                                    w.dragging = key;
                                }
                            });
                            unsafe {
                                SetCapture(hwnd);
                            }
                        }
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            drag_to(mouse_xy(lparam));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if WIN.with(|w| w.borrow_mut().as_mut().and_then(|w| w.dragging.take())).is_some() {
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            WIN.with(|w| {
                if let Some(w) = w.borrow_mut().as_mut() {
                    w.dragging = None;
                }
            });
            LRESULT(0)
        }
        WM_PAINT => panel::paint(hwnd),
        WM_ERASEBKGND => LRESULT(1),
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => panel::color(hwnd, wparam, lparam),
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => panel::field_color(wparam),
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            app::register_dialog(hwnd, false);
            panel::forget(hwnd);
            if let Some(w) = WIN.with(|w| w.borrow_mut().take()) {
                ui::delete_font(w.font);
                unsafe {
                    let _ = ImageList_Destroy(Some(w.images));
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
