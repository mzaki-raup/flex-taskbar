//! The search window: a type-to-filter list of every app, opened by hotkey.
//! The list is virtual (`LVS_OWNERDATA`), so it never holds more rows than are
//! on screen, however many apps are installed.

use super::app;
use super::theme::{self, Palette};
use super::ui::{self, scale, wide};
use crate::search;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, GetMonitorInfoW, HBRUSH, HDC, HFONT, HGDIOBJ, InvalidateRect,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, SetBkColor, SetTextColor,
};
use windows::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDRF_DODEFAULT, CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYSUBITEMDRAW,
    EM_SETCUEBANNER, HIMAGELIST, ImageList_Add, ImageList_Destroy, ImageList_Replace, LIST_VIEW_ITEM_STATE_FLAGS,
    LVCF_WIDTH, LVCOLUMNW, LVIF_IMAGE, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_ENSUREVISIBLE,
    LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_SETBKCOLOR, LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETIMAGELIST, LVM_SETITEMCOUNT, LVM_SETITEMSTATE, LVM_SETTEXTBKCOLOR, LVM_SETTEXTCOLOR, LVN_GETDISPINFOW,
    LVNI_SELECTED, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER, LVS_OWNERDATA, LVS_REPORT,
    LVS_SHOWSELALWAYS, LVS_SINGLESEL, LVSIL_SMALL, NM_CLICK, NM_CUSTOMDRAW, NM_RETURN, NMHDR, NMLVCUSTOMDRAW,
    NMLVDISPINFOW, SetWindowTheme,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_DOWN, VK_ESCAPE, VK_NEXT, VK_PRIOR, VK_RETURN, VK_UP};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyWindow, ES_AUTOHSCROLL, GetClientRect, GetCursorPos,
    GetForegroundWindow, IsWindowVisible, RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW,
    SetForegroundWindow, SetWindowPos, ShowWindow, WA_INACTIVE, WINDOW_EX_STYLE, WM_ACTIVATE, WM_CHAR, WM_COMMAND,
    WM_CTLCOLOREDIT, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_NOTIFY, WM_SIZE, WNDCLASSW, WS_CHILD,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

const ID_EDIT: u16 = 1;
const ID_LIST: u16 = 2;
const EN_CHANGE: u16 = 0x0300;

struct Row {
    id: String,
    name: Vec<u16>,
    detail: Vec<u16>,
}

struct Search {
    hwnd: HWND,
    edit: HWND,
    list: HWND,
    font_edit: HFONT,
    font_list: HFONT,
    images: HIMAGELIST,
    image_index: HashMap<String, i32>,
    rows: Vec<Row>,
    palette: Palette,
    dark: bool,
    brush_window: HBRUSH,
    brush_field: HBRUSH,
    dpi: u32,
}

thread_local! {
    static SEARCH: RefCell<Option<Search>> = const { RefCell::new(None) };
}

fn handles() -> Option<(HWND, HWND, HWND)> {
    SEARCH.with(|s| s.borrow().as_ref().map(|s| (s.hwnd, s.edit, s.list)))
}

pub fn show() {
    if handles().is_none() && !create() {
        return;
    }
    let Some((hwnd, edit, _)) = handles() else { return };
    position(hwnd);
    ui::set_text(edit, "");
    refilter();
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(edit));
    }
}

pub fn toggle() {
    if let Some((hwnd, _, _)) = handles() {
        unsafe {
            if IsWindowVisible(hwnd).as_bool() && GetForegroundWindow() == hwnd {
                hide();
                return;
            }
        }
    }
    show();
}

fn hide() {
    if let Some((hwnd, _, _)) = handles() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

pub fn destroy() {
    let taken = SEARCH.with(|s| s.borrow_mut().take());
    if let Some(s) = taken {
        unsafe {
            let _ = DestroyWindow(s.hwnd);
            let _ = ImageList_Destroy(Some(s.images));
            let _ = DeleteObject(HGDIOBJ(s.brush_window.0));
            let _ = DeleteObject(HGDIOBJ(s.brush_field.0));
        }
        ui::delete_font(s.font_edit);
        ui::delete_font(s.font_list);
    }
}

fn create() -> bool {
    let class = wide("FlexTaskbar.Search");
    unsafe {
        let wc = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(proc_),
            hInstance: app::instance(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            PCWSTR(class.as_ptr()),
            w!("FlexTaskbar Search"),
            WS_POPUP,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(app::instance()),
            None,
        ) else {
            return false;
        };

        let edit = ui::child(
            hwnd,
            "EDIT",
            "",
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            WINDOW_EX_STYLE(0),
            ID_EDIT,
        );
        let cue = wide("Search apps and categories");
        SendMessageW(edit, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cue.as_ptr() as isize)));
        let _ = SetWindowSubclass(edit, Some(edit_proc), 1, 0);

        let list_style = WS_CHILD
            | WS_VISIBLE
            | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                LVS_REPORT | LVS_OWNERDATA | LVS_NOCOLUMNHEADER | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            );
        let list = ui::child(hwnd, "SysListView32", "", list_style, WINDOW_EX_STYLE(0), ID_LIST);
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(0)),
            Some(LPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize)),
        );
        for _ in 0..2 {
            let col = LVCOLUMNW { mask: LVCF_WIDTH, cx: 100, ..Default::default() };
            SendMessageW(list, LVM_INSERTCOLUMNW, Some(WPARAM(9)), Some(LPARAM(&col as *const _ as isize)));
        }

        let size = app::with(|s| s.icon_size);
        let images = ui::image_list(size, 64);
        SendMessageW(list, LVM_SETIMAGELIST, Some(WPARAM(LVSIL_SMALL as usize)), Some(LPARAM(images.0)));

        let dpi = ui::dpi_of(hwnd);
        let dark = theme::windows_dark();
        let palette = theme::palette(dark);
        let s = Search {
            hwnd,
            edit,
            list,
            font_edit: ui::message_font(dpi, 1.5),
            font_list: ui::message_font(dpi, 1.1),
            images,
            image_index: HashMap::new(),
            rows: Vec::new(),
            brush_window: CreateSolidBrush(palette.window),
            brush_field: CreateSolidBrush(palette.field),
            palette,
            dark,
            dpi,
        };
        ui::set_font(edit, s.font_edit);
        ui::set_font(list, s.font_list);
        SEARCH.with(|cell| *cell.borrow_mut() = Some(s));
        apply_theme();
        size_window(hwnd, dpi);
    }
    true
}

fn size_window(hwnd: HWND, dpi: u32) {
    unsafe {
        let _ = SetWindowPos(hwnd, None, 0, 0, scale(620, dpi), scale(470, dpi), SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Centered horizontally on the monitor under the mouse, a fifth of the way down.
fn position(hwnd: HWND) {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return;
        }
        let dpi = ui::dpi_of(hwnd);
        let (w, h) = (scale(620, dpi), scale(470, dpi));
        let wa = mi.rcWork;
        let x = wa.left + (ui::rect_w(&wa) - w) / 2;
        let y = wa.top + ui::rect_h(&wa) / 5;
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn layout(hwnd: HWND) {
    let Some((_, edit, list)) = handles() else { return };
    let dpi = SEARCH.with(|s| s.borrow().as_ref().map(|s| s.dpi).unwrap_or(96));
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let pad = scale(16, dpi);
        let edit_h = scale(30, dpi);
        let field_pad = scale(10, dpi);
        let _ = SetWindowPos(
            edit,
            None,
            pad + field_pad,
            pad + field_pad,
            rc.right - 2 * (pad + field_pad),
            edit_h,
            SWP_NOZORDER,
        );
        let list_top = pad + edit_h + 2 * field_pad + scale(10, dpi);
        let list_w = rc.right - 2 * pad;
        let _ = SetWindowPos(list, None, pad, list_top, list_w, rc.bottom - list_top - pad, SWP_NOZORDER);
        let scroll = scale(20, dpi);
        let name_w = (list_w - scroll) * 62 / 100;
        SendMessageW(list, LVM_SETCOLUMNWIDTH, Some(WPARAM(0)), Some(LPARAM(name_w as isize)));
        SendMessageW(list, LVM_SETCOLUMNWIDTH, Some(WPARAM(1)), Some(LPARAM((list_w - scroll - name_w) as isize)));
    }
}

fn field_rect(hwnd: HWND, dpi: u32) -> RECT {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let pad = scale(16, dpi);
    RECT { left: pad, top: pad, right: rc.right - pad, bottom: pad + scale(30, dpi) + 2 * scale(10, dpi) }
}

pub fn theme_changed() {
    let exists = SEARCH.with(|s| {
        let mut b = s.borrow_mut();
        let Some(s) = b.as_mut() else { return false };
        s.dark = theme::windows_dark();
        s.palette = theme::palette(s.dark);
        unsafe {
            let _ = DeleteObject(HGDIOBJ(s.brush_window.0));
            let _ = DeleteObject(HGDIOBJ(s.brush_field.0));
            s.brush_window = CreateSolidBrush(s.palette.window);
            s.brush_field = CreateSolidBrush(s.palette.field);
        }
        true
    });
    if exists {
        apply_theme();
    }
}

fn apply_theme() {
    let Some((hwnd, _, list)) = handles() else { return };
    let (dark, window, text) = SEARCH.with(|s| {
        let b = s.borrow();
        let s = b.as_ref().unwrap();
        (s.dark, s.palette.window, s.palette.text)
    });
    theme::style_window(hwnd, dark, true);
    unsafe {
        let _ = SetWindowTheme(list, if dark { w!("DarkMode_Explorer") } else { w!("Explorer") }, PCWSTR::null());
        SendMessageW(list, LVM_SETBKCOLOR, Some(WPARAM(0)), Some(LPARAM(window.0 as isize)));
        SendMessageW(list, LVM_SETTEXTBKCOLOR, Some(WPARAM(0)), Some(LPARAM(window.0 as isize)));
        SendMessageW(list, LVM_SETTEXTCOLOR, Some(WPARAM(0)), Some(LPARAM(text.0 as isize)));
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

/// Recomputes the rows from the current query.
fn refilter() {
    let Some((_, edit, list)) = handles() else { return };
    let query = ui::get_text(edit);
    let rows = app::with(|s| {
        let mut paths: HashMap<&str, Vec<String>> = HashMap::new();
        let all_paths = crate::tree::app_paths(&s.cfg.categories);
        for (id, path) in &all_paths {
            paths.entry(id.as_str()).or_default().push(path.clone());
        }
        // Category paths, then the kind ("Chrome web app", "Store app"…), so
        // typing "web app" or "store" finds those too.
        let detail = |id: &str| {
            let mut d = paths.get(id).map(|p| p.join(", ")).unwrap_or_default();
            let kind = s.catalog.get(id).map(|a| a.kind.label()).unwrap_or_default();
            if !kind.is_empty() {
                if !d.is_empty() {
                    d.push_str("  ·  ");
                }
                d.push_str(kind);
            }
            d
        };
        let make = |id: &str, name: &str| Row { id: id.to_string(), name: wide(name), detail: wide(&detail(id)) };

        let apps = &s.catalog.apps;
        if query.trim().is_empty() {
            let mut rows: Vec<Row> = Vec::with_capacity(apps.len());
            let mut seen = std::collections::HashSet::new();
            for id in &s.cfg.recents {
                if let Some(a) = s.catalog.get(id)
                    && seen.insert(a.id.clone())
                {
                    rows.push(make(&a.id, &a.name));
                }
            }
            for a in apps {
                if !seen.contains(&a.id) {
                    rows.push(make(&a.id, &a.name));
                }
            }
            return rows;
        }
        let items: Vec<search::Item> = apps
            .iter()
            .map(|a| {
                let recent = s.cfg.recents.iter().position(|r| r == &a.id);
                search::Item::new(&a.name, &a.file_hint, &detail(&a.id), recent)
            })
            .collect();
        search::rank(&items, &query).into_iter().map(|i| make(&apps[i].id, &apps[i].name)).collect()
    });
    let count = rows.len();
    SEARCH.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            s.rows = rows;
        }
    });
    unsafe {
        SendMessageW(list, LVM_SETITEMCOUNT, Some(WPARAM(count)), Some(LPARAM(0)));
        let _ = InvalidateRect(Some(list), None, true);
    }
    select(if count > 0 { 0 } else { -1 });
}

fn selected() -> i32 {
    let Some((_, _, list)) = handles() else { return -1 };
    unsafe {
        SendMessageW(list, LVM_GETNEXTITEM, Some(WPARAM(usize::MAX)), Some(LPARAM(LVNI_SELECTED as isize))).0 as i32
    }
}

fn select(index: i32) {
    let Some((_, _, list)) = handles() else { return };
    if index < 0 {
        return;
    }
    let state = LVITEMW {
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        ..Default::default()
    };
    unsafe {
        SendMessageW(list, LVM_SETITEMSTATE, Some(WPARAM(index as usize)), Some(LPARAM(&state as *const _ as isize)));
        SendMessageW(list, LVM_ENSUREVISIBLE, Some(WPARAM(index as usize)), Some(LPARAM(0)));
    }
}

fn move_selection(delta: i32) {
    let count = SEARCH.with(|s| s.borrow().as_ref().map(|s| s.rows.len()).unwrap_or(0)) as i32;
    if count == 0 {
        return;
    }
    let cur = selected().max(0);
    select((cur + delta).clamp(0, count - 1));
}

fn launch_index(index: i32) {
    let id = SEARCH.with(|s| s.borrow().as_ref().and_then(|s| s.rows.get(index.max(0) as usize).map(|r| r.id.clone())));
    if let Some(id) = id {
        hide();
        app::launch_app(&id);
    }
}

pub fn catalog_changed() {
    if let Some((hwnd, _, _)) = handles()
        && unsafe { IsWindowVisible(hwnd) }.as_bool()
    {
        refilter();
    }
}

/// New icons arrived: rows pick them up on the next paint.
pub fn icons_arrived(_keys: &[String]) {
    if let Some((_, _, list)) = handles() {
        unsafe {
            let _ = InvalidateRect(Some(list), None, false);
        }
    }
}

/// Icons were replaced: update the copies already in the image list.
pub fn icons_changed(keys: &[String]) {
    let Some((_, _, list)) = handles() else { return };
    for key in keys {
        let idx = SEARCH.with(|s| s.borrow().as_ref().map(|s| (s.images, s.image_index.get(key).copied())));
        if let Some((images, Some(i))) = idx {
            match app::icon(key) {
                Some(bmp) => unsafe {
                    let _ = ImageList_Replace(images, i, bmp, None);
                },
                None => SEARCH.with(|s| {
                    if let Some(s) = s.borrow_mut().as_mut() {
                        s.image_index.remove(key);
                    }
                }),
            }
        }
    }
    unsafe {
        let _ = InvalidateRect(Some(list), None, false);
    }
}

fn image_for(id: &str) -> i32 {
    if let Some(i) = SEARCH.with(|s| s.borrow().as_ref().and_then(|s| s.image_index.get(id).copied())) {
        return i;
    }
    let Some(bmp) = app::icon(id) else { return ui::NO_IMAGE };
    SEARCH.with(|s| {
        let mut b = s.borrow_mut();
        let Some(s) = b.as_mut() else { return ui::NO_IMAGE };
        let i = unsafe { ImageList_Add(s.images, bmp, None) };
        if i < 0 {
            return ui::NO_IMAGE;
        }
        s.image_index.insert(id.to_string(), i);
        i
    })
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ACTIVATE => {
            if ui::loword(wparam.0) as u32 == WA_INACTIVE {
                hide();
            }
            LRESULT(0)
        }
        WM_SIZE => {
            layout(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            let hdc = HDC(wparam.0 as *mut _);
            let info = SEARCH.with(|s| s.borrow().as_ref().map(|s| (s.brush_window, s.brush_field, s.dpi)));
            if let Some((bw, bf, dpi)) = info {
                unsafe {
                    let mut rc = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rc);
                    FillRect(hdc, &rc, bw);
                    FillRect(hdc, &field_rect(hwnd, dpi), bf);
                }
            }
            LRESULT(1)
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut _);
            let info = SEARCH.with(|s| s.borrow().as_ref().map(|s| (s.palette.text, s.palette.field, s.brush_field)));
            match info {
                Some((text, field, brush)) => unsafe {
                    SetTextColor(hdc, text);
                    SetBkColor(hdc, field);
                    LRESULT(brush.0 as isize)
                },
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_COMMAND => {
            if ui::loword(wparam.0) == ID_EDIT && ui::hiword(wparam.0) == EN_CHANGE {
                refilter();
            }
            LRESULT(0)
        }
        WM_NOTIFY => unsafe { on_notify(hwnd, msg, wparam, lparam) },
        WM_DPICHANGED => {
            let dpi = ui::hiword(wparam.0) as u32;
            let fonts = SEARCH.with(|s| {
                let mut b = s.borrow_mut();
                let s = b.as_mut()?;
                ui::delete_font(s.font_edit);
                ui::delete_font(s.font_list);
                s.dpi = dpi;
                s.font_edit = ui::message_font(dpi, 1.5);
                s.font_list = ui::message_font(dpi, 1.1);
                Some((s.edit, s.list, s.font_edit, s.font_list))
            });
            if let Some((edit, list, fe, fl)) = fonts {
                ui::set_font(edit, fe);
                ui::set_font(list, fl);
            }
            let rc = unsafe { &*(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rc.left,
                    rc.top,
                    ui::rect_w(rc),
                    ui::rect_h(rc),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn on_notify(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hdr = unsafe { &*(lparam.0 as *const NMHDR) };
    if hdr.idFrom != ID_LIST as usize {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    match hdr.code {
        LVN_GETDISPINFOW => {
            let di = unsafe { &mut *(lparam.0 as *mut NMLVDISPINFOW) };
            let index = di.item.iItem as usize;
            let row = SEARCH.with(|s| {
                s.borrow().as_ref().and_then(|s| {
                    s.rows
                        .get(index)
                        .map(|r| (r.id.clone(), if di.item.iSubItem == 0 { r.name.clone() } else { r.detail.clone() }))
                })
            });
            let Some((id, text)) = row else { return LRESULT(0) };
            if di.item.mask & LVIF_TEXT == LVIF_TEXT && !di.item.pszText.is_null() && di.item.cchTextMax > 0 {
                let n = text.len().min(di.item.cchTextMax as usize);
                unsafe {
                    std::ptr::copy_nonoverlapping(text.as_ptr(), di.item.pszText.0, n);
                    *di.item.pszText.0.add(n - 1) = 0;
                }
            }
            if di.item.mask & LVIF_IMAGE == LVIF_IMAGE && di.item.iSubItem == 0 {
                di.item.iImage = image_for(&id);
            }
            LRESULT(0)
        }
        NM_CUSTOMDRAW => {
            let cd = unsafe { &mut *(lparam.0 as *mut NMLVCUSTOMDRAW) };
            let stage = cd.nmcd.dwDrawStage;
            if stage == CDDS_PREPAINT {
                return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
            }
            if stage == CDDS_ITEMPREPAINT {
                return LRESULT(CDRF_NOTIFYSUBITEMDRAW as isize);
            }
            if stage.0 == (CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0) {
                let p = SEARCH.with(|s| s.borrow().as_ref().map(|s| (s.palette.text, s.palette.subtle)));
                if let Some((text, subtle)) = p {
                    cd.clrText = if cd.iSubItem == 1 { subtle } else { text };
                }
            }
            LRESULT(CDRF_DODEFAULT as isize)
        }
        NM_CLICK => {
            let item = unsafe { &*(lparam.0 as *const windows::Win32::UI::Controls::NMITEMACTIVATE) };
            if item.iItem >= 0 {
                launch_index(item.iItem);
            }
            LRESULT(0)
        }
        NM_RETURN => {
            launch_index(selected());
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Arrow keys / Enter / Escape in the search box drive the list.
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN {
        let page = 8;
        match wparam.0 as u16 {
            k if k == VK_DOWN.0 => {
                move_selection(1);
                return LRESULT(0);
            }
            k if k == VK_UP.0 => {
                move_selection(-1);
                return LRESULT(0);
            }
            k if k == VK_NEXT.0 => {
                move_selection(page);
                return LRESULT(0);
            }
            k if k == VK_PRIOR.0 => {
                move_selection(-page);
                return LRESULT(0);
            }
            k if k == VK_RETURN.0 => {
                launch_index(selected());
                return LRESULT(0);
            }
            k if k == VK_ESCAPE.0 => {
                hide();
                return LRESULT(0);
            }
            _ => {}
        }
    }
    if msg == WM_CHAR && (wparam.0 == 0x0D || wparam.0 == 0x1B) {
        return LRESULT(0); // swallow the beep
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
