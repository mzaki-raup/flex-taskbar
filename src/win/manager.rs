//! The "Manage categories" window: the category tree (any depth), the apps in
//! the selected category, all apps, and the few settings the app has.
//!
//! It is destroyed when closed, so it costs nothing while you're not using it.

use super::app::{self, FOLDER_ICON};
use super::appdialog;
use super::icons::{self, ICON_EXTENSIONS};
use super::paths;
use super::ui::{self, scale, wide};
use super::{autostart, panel};
use crate::config::{Category, CustomApp, Hotkey, MOD_ALT, MOD_CONTROL, MOD_SHIFT};
use crate::{migrate, tree};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, GetSysColorBrush, HBRUSH, HFONT, InvalidateRect};
use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW};
use windows::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, HIMAGELIST, HKM_GETHOTKEY, HKM_SETHOTKEY, HTREEITEM, ImageList_Add, ImageList_Destroy,
    ImageList_Replace, LIST_VIEW_ITEM_STATE_FLAGS, LVCF_WIDTH, LVCOLUMNW, LVIF_IMAGE, LVIF_PARAM, LVIF_TEXT,
    LVIS_FOCUSED, LVIS_SELECTED, LVITEMW, LVM_DELETEALLITEMS, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW,
    LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETIMAGELIST, LVM_SETITEMCOUNT, LVM_SETITEMSTATE,
    LVN_GETDISPINFOW, LVNI_SELECTED, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER, LVS_OWNERDATA,
    LVS_REPORT, LVS_SHAREIMAGELISTS, LVS_SHOWSELALWAYS, LVSIL_SMALL, NMHDR, NMLVDISPINFOW, NMTREEVIEWW, NMTVDISPINFOW,
    SetWindowTheme, TVE_COLLAPSE, TVE_EXPAND, TVGN_CARET, TVI_LAST, TVI_ROOT, TVIF_IMAGE, TVIF_PARAM,
    TVIF_SELECTEDIMAGE, TVIF_TEXT, TVINSERTSTRUCTW, TVINSERTSTRUCTW_0, TVITEMEXW, TVITEMW, TVM_DELETEITEM,
    TVM_EDITLABELW, TVM_EXPAND, TVM_GETITEMW, TVM_GETNEXTITEM, TVM_INSERTITEMW, TVM_SELECTITEM, TVM_SETIMAGELIST,
    TVN_ENDLABELEDITW, TVN_ITEMEXPANDEDW, TVN_SELCHANGEDW, TVS_EDITLABELS, TVS_HASBUTTONS, TVS_HASLINES,
    TVS_LINESATROOT, TVS_SHOWSELALWAYS, TVSIL_NORMAL,
};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyMenu, DestroyWindow, ES_AUTOHSCROLL, GetClientRect, GetWindowRect, HMENU, InsertMenuW,
    IsIconic, MF_BYPOSITION, MF_STRING, MINMAXINFO, RegisterClassW, SW_RESTORE, SW_SHOW, SWP_NOZORDER, SendMessageW,
    SetForegroundWindow, SetWindowPos, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC,
    WM_DESTROY, WM_DPICHANGED, WM_DROPFILES, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_NOTIFY, WM_PAINT, WM_SIZE, WNDCLASSW,
    WS_BORDER, WS_CHILD, WS_CLIPCHILDREN, WS_EX_ACCEPTFILES, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
    WS_VISIBLE,
};
use windows::core::{PCWSTR, PWSTR, w};

// Control ids.
const TREE: u16 = 10;
const CAT_NEW: u16 = 11;
const CAT_SUB: u16 = 12;
const CAT_RENAME: u16 = 13;
const CAT_DELETE: u16 = 14;
const CAT_UP: u16 = 15;
const CAT_DOWN: u16 = 16;
const CAT_OUT: u16 = 17;
const CAT_IN: u16 = 18;
const CAT_ICON: u16 = 19;
const APPS: u16 = 30;
const APP_REMOVE: u16 = 31;
const APP_UP: u16 = 32;
const APP_DOWN: u16 = 33;
const APP_ICON: u16 = 34;
const ALL_FILTER: u16 = 40;
const ALL: u16 = 41;
const ALL_ADD: u16 = 42;
const ALL_ICON: u16 = 43;
const CUSTOM_NEW: u16 = 44;
const CUSTOM_EDIT: u16 = 45;
const CUSTOM_DELETE: u16 = 46;
const AUTOSTART: u16 = 50;
const HK_SEARCH: u16 = 51;
const HK_MENU: u16 = 52;
const HK_APPLY: u16 = 53;
const RESCAN: u16 = 54;
const IMPORT: u16 = 55;
const OPEN_DATA: u16 = 56;
const STATUS: u16 = 57;
const CLOSE: u16 = 58;
const LBL_CATS: u16 = 60;
const LBL_APPS: u16 = 61;
const LBL_ALL: u16 = 62;
const LBL_HK_SEARCH: u16 = 63;
const LBL_HK_MENU: u16 = 64;
const LBL_HINT: u16 = 65;
const STRIP_SHOW: u16 = 66;
const STRIP_RESERVE: u16 = 67;
const ALL_PIN: u16 = 68;
const APPEARANCE: u16 = 69;
const ARRANGE: u16 = 70;
const AUTO_RESCAN: u16 = 71;
const PACKAGE_APPS: u16 = 72;
const HK_BAR: u16 = 73;
const LBL_HK_BAR: u16 = 74;
const SWITCH_RUNNING: u16 = 75;

const EN_CHANGE: u16 = 0x0300;
/// Posted to ourselves after a rename so the label updates once the edit commits.
const REFRESH_LABEL: u16 = 0xFFFF;
const BN_CLICKED: u16 = 0;
const HOTKEYF_SHIFT: u32 = 0x1;
const HOTKEYF_CONTROL: u32 = 0x2;
const HOTKEYF_ALT: u32 = 0x4;

struct Manager {
    hwnd: HWND,
    controls: HashMap<u16, HWND>,
    font: HFONT,
    dpi: u32,
    tree_images: HIMAGELIST,
    tree_index: HashMap<String, i32>,
    tree_items: HashMap<u64, HTREEITEM>,
    collapsed: HashSet<u64>,
    app_images: HIMAGELIST,
    app_index: HashMap<String, i32>,
    /// App ids shown in the category list, by row.
    cat_rows: Vec<String>,
    /// App ids shown in the all-apps list, by row.
    all_rows: Vec<String>,
}

thread_local! {
    static MANAGER: RefCell<Option<Manager>> = const { RefCell::new(None) };
    /// Set while the tree is being rebuilt, so its notifications are ignored.
    static SUPPRESS: Cell<bool> = const { Cell::new(false) };
}

fn ctl(id: u16) -> HWND {
    MANAGER.with(|m| m.borrow().as_ref().and_then(|m| m.controls.get(&id).copied()).unwrap_or_default())
}

fn hwnd() -> Option<HWND> {
    MANAGER.with(|m| m.borrow().as_ref().map(|m| m.hwnd))
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

/// Opens the window with a category selected.
pub fn show_category(id: u64) {
    show();
    if hwnd().is_some() {
        rebuild_tree(Some(id));
    }
}

pub fn destroy() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

fn create() {
    let class = wide("FlexTaskbar.Manager");
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
            WS_EX_ACCEPTFILES,
            PCWSTR(class.as_ptr()),
            w!("FlexTaskbar — Manage categories"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
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
        let size = app::with(|s| s.icon_size);

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
        let label = |text: &str, id: u16| {
            ui::child(hwnd, "STATIC", text, WS_CHILD | WS_VISIBLE | ui::SS_LABEL, WINDOW_EX_STYLE(0), id)
        };

        controls.insert(LBL_CATS, label("", LBL_CATS));
        controls.insert(LBL_APPS, label("Apps in this category", LBL_APPS));
        controls.insert(LBL_ALL, label("Select apps, then add or pin them", LBL_ALL));

        let tree_style = WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WINDOW_STYLE(TVS_HASLINES | TVS_LINESATROOT | TVS_HASBUTTONS | TVS_SHOWSELALWAYS | TVS_EDITLABELS);
        let tree_hwnd = ui::child(hwnd, "SysTreeView32", "", tree_style, WS_EX_CLIENTEDGE, TREE);
        let _ = SetWindowTheme(tree_hwnd, w!("Explorer"), PCWSTR::null());
        controls.insert(TREE, tree_hwnd);
        for (text, id) in [
            ("New", CAT_NEW),
            ("New sub", CAT_SUB),
            ("Rename", CAT_RENAME),
            ("Delete", CAT_DELETE),
            ("▲", CAT_UP),
            ("▼", CAT_DOWN),
            ("← Out", CAT_OUT),
            ("In →", CAT_IN),
            ("Icon…", CAT_ICON),
        ] {
            controls.insert(id, button(text, id));
        }

        let list_style = |extra: u32| {
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WINDOW_STYLE(LVS_REPORT | LVS_NOCOLUMNHEADER | LVS_SHOWSELALWAYS | LVS_SHAREIMAGELISTS | extra)
        };
        let apps_hwnd = ui::child(hwnd, "SysListView32", "", list_style(0), WS_EX_CLIENTEDGE, APPS);
        let all_hwnd = ui::child(hwnd, "SysListView32", "", list_style(LVS_OWNERDATA), WS_EX_CLIENTEDGE, ALL);
        let app_images = ui::image_list(size, 64);
        for lv in [apps_hwnd, all_hwnd] {
            let _ = SetWindowTheme(lv, w!("Explorer"), PCWSTR::null());
            SendMessageW(
                lv,
                LVM_SETEXTENDEDLISTVIEWSTYLE,
                Some(WPARAM(0)),
                Some(LPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize)),
            );
            let col = LVCOLUMNW { mask: LVCF_WIDTH, cx: 200, ..Default::default() };
            SendMessageW(lv, LVM_INSERTCOLUMNW, Some(WPARAM(0)), Some(LPARAM(&col as *const _ as isize)));
            SendMessageW(lv, LVM_SETIMAGELIST, Some(WPARAM(LVSIL_SMALL as usize)), Some(LPARAM(app_images.0)));
        }
        controls.insert(APPS, apps_hwnd);
        controls.insert(ALL, all_hwnd);
        for (text, id) in [("Remove", APP_REMOVE), ("▲", APP_UP), ("▼", APP_DOWN), ("Icon…", APP_ICON)] {
            controls.insert(id, button(text, id));
        }

        let filter = ui::child(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            WINDOW_EX_STYLE(0),
            ALL_FILTER,
        );
        let cue = wide("Filter");
        SendMessageW(
            filter,
            windows::Win32::UI::Controls::EM_SETCUEBANNER,
            Some(WPARAM(1)),
            Some(LPARAM(cue.as_ptr() as isize)),
        );
        controls.insert(ALL_FILTER, filter);
        for (text, id) in [
            ("← Add to category", ALL_ADD),
            ("Pin to strip", ALL_PIN),
            ("Icon…", ALL_ICON),
            ("New custom app…", CUSTOM_NEW),
            ("Edit…", CUSTOM_EDIT),
            ("Delete", CUSTOM_DELETE),
        ] {
            controls.insert(id, button(text, id));
        }

        controls.insert(
            AUTOSTART,
            ui::child(
                hwnd,
                "BUTTON",
                "Start with Windows",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                WINDOW_EX_STYLE(0),
                AUTOSTART,
            ),
        );
        for (text, id) in [
            ("Show icon strip", STRIP_SHOW),
            ("Reserve its space (maximized windows stop at it)", STRIP_RESERVE),
            ("Rescan when apps are installed or removed", AUTO_RESCAN),
            ("Include package managers' apps (winget, npm…)", PACKAGE_APPS),
            ("Clicking a running app switches to it", SWITCH_RUNNING),
        ] {
            controls.insert(
                id,
                ui::child(
                    hwnd,
                    "BUTTON",
                    text,
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    WINDOW_EX_STYLE(0),
                    id,
                ),
            );
        }
        controls.insert(LBL_HK_SEARCH, label("Search", LBL_HK_SEARCH));
        controls.insert(LBL_HK_MENU, label("Menu", LBL_HK_MENU));
        controls.insert(LBL_HK_BAR, label("Bar", LBL_HK_BAR));
        controls.insert(
            HK_BAR,
            ui::child(hwnd, "msctls_hotkey32", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP, WINDOW_EX_STYLE(0), HK_BAR),
        );
        controls.insert(
            HK_SEARCH,
            ui::child(hwnd, "msctls_hotkey32", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP, WINDOW_EX_STYLE(0), HK_SEARCH),
        );
        controls.insert(
            HK_MENU,
            ui::child(hwnd, "msctls_hotkey32", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP, WINDOW_EX_STYLE(0), HK_MENU),
        );
        controls.insert(HK_APPLY, button("Apply hotkeys", HK_APPLY));
        controls.insert(LBL_HINT, label("Backspace in a box turns it off.", LBL_HINT));
        controls.insert(STATUS, label("", STATUS));
        for (text, id) in [
            ("Rescan apps", RESCAN),
            ("Import old settings…", IMPORT),
            ("Open data folder", OPEN_DATA),
            ("Arrange the bar…", ARRANGE),
            ("Appearance…", APPEARANCE),
            ("Close", CLOSE),
        ] {
            controls.insert(id, button(text, id));
        }

        let tree_images = ui::image_list(size, 16);
        SendMessageW(tree_hwnd, TVM_SETIMAGELIST, Some(WPARAM(TVSIL_NORMAL as usize)), Some(LPARAM(tree_images.0)));

        let font = ui::message_font(dpi, 1.0);
        for h in controls.values() {
            ui::set_font(*h, font);
        }
        for id in [LBL_CATS, LBL_APPS, LBL_ALL, LBL_HINT, STATUS] {
            panel::subtle(hwnd, controls[&id]);
        }

        MANAGER.with(|m| {
            *m.borrow_mut() = Some(Manager {
                hwnd,
                controls,
                font,
                dpi,
                tree_images,
                tree_index: HashMap::new(),
                tree_items: HashMap::new(),
                collapsed: HashSet::new(),
                app_images,
                app_index: HashMap::new(),
                cat_rows: Vec::new(),
                all_rows: Vec::new(),
            })
        });
        app::register_dialog(hwnd, true);
        DragAcceptFiles(hwnd, true);
        panel::apply_theme(hwnd);

        load_settings();
        rebuild_tree(None);
        refresh_all();
        update_status();

        let (w, h) = (scale(1100, dpi), scale(720, dpi));
        let wa = ui::work_area_at_cursor();
        let x = wa.left + (ui::rect_w(&wa) - w).max(0) / 2;
        let y = wa.top + (ui::rect_h(&wa) - h).max(0) / 2;
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

// ---------------------------------------------------------------- layout

fn layout() {
    let Some(h) = hwnd() else { return };
    let dpi = MANAGER.with(|m| m.borrow().as_ref().map(|m| m.dpi).unwrap_or(96));
    let s = |v: i32| scale(v, dpi);
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(h, &mut rc);
    }
    let pm = panel::metrics(dpi);
    let (gap, bh, lh, pitch) = (s(6), s(28), s(20), s(32));
    let col_w = (rc.right - 2 * pm.margin - 2 * pm.gap) / 3;
    let cols = [0, 1, 2].map(|i| pm.margin + i * (col_w + pm.gap));
    // Inside a card.
    let (inner_w, ix) = (col_w - 2 * pm.pad, cols.map(|x| x + pm.pad));
    let footer = rc.bottom - pm.footer;
    let options_h = pm.card_title + 4 * pitch + s(6);
    let options_top = footer - pm.margin - options_h;
    let top = pm.header;
    let pane_bottom = options_top - pm.gap;

    let place = |id: u16, x: i32, y: i32, w: i32, hh: i32| unsafe {
        let _ = SetWindowPos(ctl(id), None, x, y, w.max(0), hh.max(0), SWP_NOZORDER);
    };
    let row = |ids: &[u16], x: i32, y: i32, w: i32| {
        let n = ids.len() as i32;
        let bw = (w - gap * (n - 1)) / n;
        for (i, id) in ids.iter().enumerate() {
            place(*id, x + i as i32 * (bw + gap), y, bw, bh);
        }
    };
    let card = |i: usize, t: i32, b: i32, title: &str| panel::Card {
        rect: RECT { left: cols[i], top: t, right: cols[i] + col_w, bottom: b },
        title: title.into(),
        subtitle: String::new(),
    };
    // The panes' grey subtitles are labels (they change with the selection).
    let sub_y = top + s(33);
    let content = pm.content_top(top, true);
    let last_row = pane_bottom - pm.pad - bh;

    // Categories.
    place(LBL_CATS, ix[0], sub_y, inner_w, s(16));
    let tree_bottom = last_row - bh - 2 * gap;
    place(TREE, ix[0], content, inner_w, tree_bottom - content);
    row(&[CAT_NEW, CAT_SUB, CAT_RENAME, CAT_DELETE, CAT_ICON], ix[0], tree_bottom + gap, inner_w);
    row(&[CAT_UP, CAT_DOWN, CAT_OUT, CAT_IN], ix[0], last_row, inner_w);

    // Apps in the category.
    place(LBL_APPS, ix[1], sub_y, inner_w, s(16));
    let apps_bottom = last_row - gap;
    place(APPS, ix[1], content, inner_w, apps_bottom - content);
    row(&[APP_REMOVE, APP_UP, APP_DOWN, APP_ICON], ix[1], last_row, inner_w);

    // All apps.
    place(LBL_ALL, ix[2], sub_y, inner_w, s(16));
    place(ALL_FILTER, ix[2], content, inner_w, s(24));
    let all_top = content + s(24) + gap;
    let all_bottom = last_row - bh - 2 * gap;
    place(ALL, ix[2], all_top, inner_w, all_bottom - all_top);
    row(&[ALL_ADD, ALL_PIN, ALL_ICON], ix[2], all_bottom + gap, inner_w);
    row(&[CUSTOM_NEW, CUSTOM_EDIT, CUSTOM_DELETE], ix[2], last_row, inner_w);

    // Options: one card per subject under the panes.
    let oy = pm.content_top(options_top, false);
    let line = |n: i32| oy + n * pitch;
    place(AUTOSTART, ix[0], line(0), inner_w, bh);
    place(STRIP_SHOW, ix[0], line(1), inner_w, bh);
    place(STRIP_RESERVE, ix[0], line(2), inner_w, bh);
    place(SWITCH_RUNNING, ix[0], line(3), inner_w, bh);
    place(AUTO_RESCAN, ix[1], line(0), inner_w, bh);
    place(PACKAGE_APPS, ix[1], line(1), inner_w, bh);
    row(&[RESCAN, IMPORT], ix[1], line(2), inner_w);
    let lw = s(70);
    place(LBL_HK_SEARCH, ix[2], line(0) + s(5), lw, lh);
    place(HK_SEARCH, ix[2] + lw, line(0) + s(2), inner_w - lw, s(24));
    place(LBL_HK_MENU, ix[2], line(1) + s(5), lw, lh);
    place(HK_MENU, ix[2] + lw, line(1) + s(2), inner_w - lw, s(24));
    place(LBL_HK_BAR, ix[2], line(2) + s(5), lw, lh);
    place(HK_BAR, ix[2] + lw, line(2) + s(2), inner_w - lw, s(24));
    place(HK_APPLY, ix[2], line(3), s(120), bh);
    place(LBL_HINT, ix[2] + s(128), line(3) + s(5), inner_w - s(128), lh);

    // Command bar: where the data lives, and the other windows.
    let by = footer + (pm.footer - bh) / 2;
    let actions_w = 4 * s(130) + 3 * gap;
    place(STATUS, pm.margin, by + s(5), rc.right - 2 * pm.margin - actions_w - pm.gap, lh);
    row(&[OPEN_DATA, ARRANGE, APPEARANCE, CLOSE], rc.right - pm.margin - actions_w, by, actions_w);

    panel::set(
        h,
        panel::Page {
            title: "Manage categories".into(),
            subtitle: "Changes are saved straight away. Drop files or shortcuts on this window to add them as apps."
                .into(),
            cards: vec![
                card(0, top, pane_bottom, "Categories"),
                card(1, top, pane_bottom, "In this category"),
                card(2, top, pane_bottom, "All apps"),
                card(0, options_top, footer - pm.margin, "Startup and bar"),
                card(1, options_top, footer - pm.margin, "App list"),
                card(2, options_top, footer - pm.margin, "Hotkeys"),
            ],
            footer: Some(footer),
        },
    );

    for lv in [APPS, ALL] {
        let mut lrc = RECT::default();
        unsafe {
            let _ = GetClientRect(ctl(lv), &mut lrc);
            SendMessageW(ctl(lv), LVM_SETCOLUMNWIDTH, Some(WPARAM(0)), Some(LPARAM((lrc.right - s(4)) as isize)));
        }
    }
}

// ---------------------------------------------------------------- category tree

fn selected_category() -> Option<u64> {
    let tree_hwnd = ctl(TREE);
    unsafe {
        let item = SendMessageW(tree_hwnd, TVM_GETNEXTITEM, Some(WPARAM(TVGN_CARET as usize)), Some(LPARAM(0)));
        if item.0 == 0 {
            return None;
        }
        let mut tv = TVITEMW { mask: TVIF_PARAM, hItem: HTREEITEM(item.0), ..Default::default() };
        SendMessageW(tree_hwnd, TVM_GETITEMW, Some(WPARAM(0)), Some(LPARAM(&mut tv as *mut _ as isize)));
        Some(tv.lParam.0 as u64)
    }
}

fn tree_image(key: &str) -> i32 {
    if let Some(i) = MANAGER.with(|m| m.borrow().as_ref().and_then(|m| m.tree_index.get(key).copied())) {
        return i;
    }
    let Some(bmp) = app::icon(key) else { return ui::NO_IMAGE };
    MANAGER.with(|m| {
        let mut b = m.borrow_mut();
        let Some(m) = b.as_mut() else { return ui::NO_IMAGE };
        let i = unsafe { ImageList_Add(m.tree_images, bmp, None) };
        if i < 0 {
            return ui::NO_IMAGE;
        }
        m.tree_index.insert(key.to_string(), i);
        i
    })
}

/// Rebuilds the tree from the configuration and selects `select` (or keeps the
/// current selection).
fn rebuild_tree(select: Option<u64>) {
    ui::set_text(
        ctl(LBL_CATS),
        &format!("Up to {} levels deep · drop files here to add apps", super::strip::max_levels()),
    );
    let select = select.or_else(selected_category);
    let tree_hwnd = ctl(TREE);
    let cats = app::with(|s| s.cfg.categories.clone());
    let collapsed = MANAGER.with(|m| m.borrow().as_ref().map(|m| m.collapsed.clone()).unwrap_or_default());
    let folder = tree_image(FOLDER_ICON);

    SUPPRESS.with(|s| s.set(true));
    let mut items: HashMap<u64, HTREEITEM> = HashMap::new();
    unsafe {
        SendMessageW(tree_hwnd, TVM_DELETEITEM, Some(WPARAM(0)), Some(LPARAM(TVI_ROOT.0)));
    }
    fn insert(tree_hwnd: HWND, parent: HTREEITEM, cats: &[Category], folder: i32, items: &mut HashMap<u64, HTREEITEM>) {
        for c in cats {
            let mut text = wide(&c.name);
            let image = if c.icon.is_some() { tree_image(&format!("cat:{}", c.id)) } else { ui::NO_IMAGE };
            let image = if image < 0 { folder } else { image };
            let mut ins = TVINSERTSTRUCTW {
                hParent: parent,
                hInsertAfter: TVI_LAST,
                Anonymous: TVINSERTSTRUCTW_0 {
                    itemex: TVITEMEXW {
                        mask: TVIF_TEXT | TVIF_PARAM | TVIF_IMAGE | TVIF_SELECTEDIMAGE,
                        pszText: PWSTR(text.as_mut_ptr()),
                        lParam: LPARAM(c.id as isize),
                        iImage: image,
                        iSelectedImage: image,
                        ..Default::default()
                    },
                },
            };
            let h = unsafe {
                SendMessageW(tree_hwnd, TVM_INSERTITEMW, Some(WPARAM(0)), Some(LPARAM(&mut ins as *mut _ as isize)))
            };
            let h = HTREEITEM(h.0);
            items.insert(c.id, h);
            insert(tree_hwnd, h, &c.children, folder, items);
        }
    }
    insert(tree_hwnd, HTREEITEM(0), &cats, folder, &mut items);
    tree::walk(&cats, &mut |c, _| {
        if !c.children.is_empty()
            && let Some(h) = items.get(&c.id)
        {
            let action = if collapsed.contains(&c.id) { TVE_COLLAPSE } else { TVE_EXPAND };
            unsafe {
                SendMessageW(tree_hwnd, TVM_EXPAND, Some(WPARAM(action.0 as usize)), Some(LPARAM(h.0)));
            }
        }
    });
    let target =
        select.and_then(|id| items.get(&id).copied()).or_else(|| cats.first().and_then(|c| items.get(&c.id).copied()));
    if let Some(h) = target {
        unsafe {
            SendMessageW(tree_hwnd, TVM_SELECTITEM, Some(WPARAM(TVGN_CARET as usize)), Some(LPARAM(h.0)));
        }
    }
    MANAGER.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.tree_items = items;
        }
    });
    SUPPRESS.with(|s| s.set(false));
    refresh_category_apps();
}

/// Subcategories can nest only as deep as the screen can show their flyouts.
fn too_deep(owner: HWND) {
    let limit = super::strip::max_levels();
    ui::info(
        Some(owner),
        &format!(
            "Categories can be nested {limit} levels deep on this screen, so that every level of flyouts fits.\n\n\
             Move this category up a level first, or put it somewhere less deep."
        ),
    );
}

fn new_category(parent: Option<u64>) {
    if let Some(p) = parent {
        let limit = super::strip::max_levels();
        if !app::with(|s| tree::sub_fits(&s.cfg.categories, p, limit)) {
            if let Some(h) = hwnd() {
                too_deep(h);
            }
            return;
        }
    }
    let id = app::with(|s| {
        let id = s.cfg.alloc_id();
        let cat = Category { id, name: "New category".into(), ..Default::default() };
        if tree::add(&mut s.cfg.categories, parent, cat) { Some(id) } else { None }
    });
    let Some(id) = id else { return };
    app::save();
    if let Some(p) = parent {
        MANAGER.with(|m| {
            if let Some(m) = m.borrow_mut().as_mut() {
                m.collapsed.remove(&p);
            }
        });
    }
    rebuild_tree(Some(id));
    edit_label(id);
}

fn edit_label(id: u64) {
    let item = MANAGER.with(|m| m.borrow().as_ref().and_then(|m| m.tree_items.get(&id).copied()));
    if let Some(h) = item {
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(ctl(TREE)));
            SendMessageW(ctl(TREE), TVM_EDITLABELW, Some(WPARAM(0)), Some(LPARAM(h.0)));
        }
    }
}

fn delete_category(owner: HWND) {
    let Some(id) = selected_category() else { return };
    let info = app::with(|s| {
        tree::find(&s.cfg.categories, id).map(|c| {
            let mut subs = 0;
            tree::walk(&c.children, &mut |_, _| subs += 1);
            (c.name.clone(), subs, c.apps.len())
        })
    });
    let Some((name, subs, apps)) = info else { return };
    if subs + apps > 0 {
        let what = match (subs, apps) {
            (0, a) => format!("{a} app shortcut(s)"),
            (s, 0) => format!("{s} subcategory(ies)"),
            (s, a) => format!("{s} subcategory(ies) and {a} app shortcut(s)"),
        };
        if !ui::confirm(Some(owner), &format!("Delete “{name}” with its {what}?\n\nNo apps are uninstalled.")) {
            return;
        }
    }
    let parent = app::with(|s| {
        let parent = tree::parent_of(&s.cfg.categories, id);
        if let Some(removed) = tree::remove(&mut s.cfg.categories, id) {
            let mut files = Vec::new();
            tree::walk(std::slice::from_ref(&removed), &mut |c, _| files.extend(c.icon.clone()));
            for f in files {
                paths::remove_icon(&f);
            }
        }
        parent
    });
    app::save();
    rebuild_tree(parent);
}

fn tree_op(op: fn(&mut Vec<Category>, u64) -> bool) {
    let Some(id) = selected_category() else { return };
    if app::with(|s| op(&mut s.cfg.categories, id)) {
        app::save();
        rebuild_tree(Some(id));
    }
}

// ---------------------------------------------------------------- app lists

fn app_image(id: &str) -> i32 {
    if let Some(i) = MANAGER.with(|m| m.borrow().as_ref().and_then(|m| m.app_index.get(id).copied())) {
        return i;
    }
    let Some(bmp) = app::icon(id) else { return ui::NO_IMAGE };
    MANAGER.with(|m| {
        let mut b = m.borrow_mut();
        let Some(m) = b.as_mut() else { return ui::NO_IMAGE };
        let i = unsafe { ImageList_Add(m.app_images, bmp, None) };
        if i < 0 {
            return ui::NO_IMAGE;
        }
        m.app_index.insert(id.to_string(), i);
        i
    })
}

fn refresh_category_apps() {
    let cat = selected_category();
    let rows: Vec<(String, String)> = app::with(|s| {
        let Some(c) = cat.and_then(|id| tree::find(&s.cfg.categories, id)) else { return Vec::new() };
        c.apps
            .iter()
            .map(|id| {
                let name = s.catalog.get(id).map(|a| a.name.clone()).unwrap_or_else(|| format!("{id} (not found)"));
                (id.clone(), name)
            })
            .collect()
    });
    let list = ctl(APPS);
    unsafe {
        SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    for (i, (id, name)) in rows.iter().enumerate() {
        let mut text = wide(name);
        let item = LVITEMW {
            mask: LVIF_TEXT | LVIF_IMAGE | LVIF_PARAM,
            iItem: i as i32,
            pszText: PWSTR(text.as_mut_ptr()),
            iImage: app_image(id),
            lParam: LPARAM(i as isize),
            ..Default::default()
        };
        unsafe {
            SendMessageW(list, LVM_INSERTITEMW, Some(WPARAM(0)), Some(LPARAM(&item as *const _ as isize)));
        }
    }
    MANAGER.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.cat_rows = rows.into_iter().map(|(id, _)| id).collect();
        }
    });
    let label =
        app::with(|s| cat.and_then(|id| tree::find(&s.cfg.categories, id)).map(|c| format!("Apps in “{}”", c.name)));
    ui::set_text(ctl(LBL_APPS), &label.unwrap_or_else(|| "Select a category".into()));
}

fn refresh_all() {
    let filter = ui::get_text(ctl(ALL_FILTER));
    let rows: Vec<String> = app::with(|s| {
        let apps = &s.catalog.apps;
        if filter.trim().is_empty() {
            return apps.iter().map(|a| a.id.clone()).collect();
        }
        let items: Vec<crate::search::Item> =
            apps.iter().map(|a| crate::search::Item::new(&a.name, &a.file_hint, "", None)).collect();
        crate::search::rank(&items, &filter).into_iter().map(|i| apps[i].id.clone()).collect()
    });
    let count = rows.len();
    MANAGER.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut() {
            m.all_rows = rows;
        }
    });
    unsafe {
        let list = ctl(ALL);
        SendMessageW(list, LVM_SETITEMCOUNT, Some(WPARAM(count)), Some(LPARAM(0)));
        let _ = InvalidateRect(Some(list), None, true);
    }
}

fn selected_rows(list_id: u16) -> Vec<String> {
    let list = ctl(list_id);
    let mut idx = Vec::new();
    let mut cur: isize = -1;
    loop {
        let next = unsafe {
            SendMessageW(list, LVM_GETNEXTITEM, Some(WPARAM(cur as usize)), Some(LPARAM(LVNI_SELECTED as isize))).0
        };
        if next < 0 {
            break;
        }
        idx.push(next as usize);
        cur = next;
    }
    MANAGER.with(|m| {
        let b = m.borrow();
        let Some(m) = b.as_ref() else { return Vec::new() };
        let rows = if list_id == APPS { &m.cat_rows } else { &m.all_rows };
        idx.into_iter().filter_map(|i| rows.get(i).cloned()).collect()
    })
}

fn select_row(list_id: u16, index: usize) {
    let state = LVITEMW {
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        ..Default::default()
    };
    unsafe {
        SendMessageW(ctl(list_id), LVM_SETITEMSTATE, Some(WPARAM(index)), Some(LPARAM(&state as *const _ as isize)));
    }
}

fn add_selected_to_category(owner: HWND) {
    let Some(cat) = selected_category() else {
        ui::info(Some(owner), "Select (or create) a category on the left first.");
        return;
    };
    let ids = selected_rows(ALL);
    if ids.is_empty() {
        ui::info(Some(owner), "Select one or more apps in the All apps list first (Ctrl+click selects several).");
        return;
    }
    let changed = app::with(|s| ids.iter().fold(false, |acc, id| tree::add_app(&mut s.cfg.categories, cat, id) | acc));
    if changed {
        app::save();
        refresh_category_apps();
    }
}

fn remove_selected_from_category() {
    let Some(cat) = selected_category() else { return };
    let ids = selected_rows(APPS);
    if ids.is_empty() {
        return;
    }
    app::with(|s| {
        for id in &ids {
            tree::remove_app(&mut s.cfg.categories, cat, id);
        }
    });
    app::save();
    refresh_category_apps();
}

fn move_selected_app(delta: isize) {
    let Some(cat) = selected_category() else { return };
    let ids = selected_rows(APPS);
    let [id] = ids.as_slice() else { return };
    let moved = app::with(|s| tree::move_app(&mut s.cfg.categories, cat, id, delta));
    if moved {
        app::save();
        refresh_category_apps();
        let pos = MANAGER.with(|m| m.borrow().as_ref().and_then(|m| m.cat_rows.iter().position(|r| r == id)));
        if let Some(p) = pos {
            select_row(APPS, p);
        }
    }
}

// ---------------------------------------------------------------- custom apps

fn new_custom_app(owner: HWND, initial: CustomApp) {
    let Some(fields) = appdialog::edit(owner, &initial, "New custom app") else { return };
    let cat = selected_category();
    let id = app::with(|s| {
        let id = format!("custom:{}", s.cfg.alloc_id());
        s.cfg.custom_apps.push(CustomApp { id: id.clone(), ..fields });
        if let Some(c) = cat {
            tree::add_app(&mut s.cfg.categories, c, &id);
        }
        id
    });
    after_custom_change(&id);
}

fn after_custom_change(id: &str) {
    app::save();
    app::rebuild_catalog();
    app::reload_icon(id);
    refresh_all();
    refresh_category_apps();
    super::searchwin::catalog_changed();
}

fn edit_custom_app(owner: HWND) {
    let ids = selected_rows(ALL);
    let [id] = ids.as_slice() else {
        ui::info(Some(owner), "Select one custom app in the All apps list.");
        return;
    };
    let Some(current) = app::with(|s| s.cfg.custom_app(id).cloned()) else {
        ui::info(
            Some(owner),
            "Only custom apps can be edited. The others come from Windows' own app list; you can still change their icon.",
        );
        return;
    };
    let Some(fields) = appdialog::edit(owner, &current, "Edit custom app") else { return };
    app::with(|s| {
        if let Some(c) = s.cfg.custom_apps.iter_mut().find(|c| c.id == *id) {
            *c = CustomApp { id: id.clone(), ..fields };
        }
    });
    after_custom_change(id);
}

fn delete_custom_app(owner: HWND) {
    let ids = selected_rows(ALL);
    let custom: Vec<(String, String)> = app::with(|s| {
        ids.iter().filter_map(|id| s.cfg.custom_app(id).map(|c| (c.id.clone(), c.name.clone()))).collect()
    });
    if custom.is_empty() {
        ui::info(
            Some(owner),
            "Select one or more custom apps in the All apps list. Apps from Windows' own list can't be deleted here.",
        );
        return;
    }
    let names: Vec<&str> = custom.iter().map(|(_, n)| n.as_str()).collect();
    if !ui::confirm(Some(owner), &format!("Delete {}?\n\nIt is removed from every category.", names.join(", "))) {
        return;
    }
    app::with(|s| {
        for (id, _) in &custom {
            s.cfg.custom_apps.retain(|c| c.id != *id);
            tree::remove_app_everywhere(&mut s.cfg.categories, id);
            s.cfg.recents.retain(|r| r != id);
            s.cfg.unpin(id);
            if let Some(f) = s.cfg.app_icons.remove(id) {
                paths::remove_icon(&f);
            }
        }
    });
    app::save();
    app::rebuild_catalog();
    refresh_all();
    refresh_category_apps();
    super::searchwin::catalog_changed();
}

fn on_drop(owner: HWND, drop: HDROP) {
    let mut files = Vec::new();
    unsafe {
        let count = DragQueryFileW(drop, u32::MAX, None);
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(drop, i, Some(&mut buf));
            files.push(PathBuf::from(ui::from_wide(&buf)));
        }
        DragFinish(drop);
    }
    if files.is_empty() {
        return;
    }
    let cat = selected_category();
    app::with(|s| {
        for f in &files {
            let id = format!("custom:{}", s.cfg.alloc_id());
            let name =
                f.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.display().to_string());
            s.cfg.custom_apps.push(CustomApp {
                id: id.clone(),
                name,
                target: f.display().to_string(),
                ..Default::default()
            });
            if let Some(c) = cat {
                tree::add_app(&mut s.cfg.categories, c, &id);
            }
        }
    });
    app::save();
    app::rebuild_catalog();
    app::request_all_icons();
    refresh_all();
    refresh_category_apps();
    super::searchwin::catalog_changed();
    if cat.is_none() {
        ui::info(Some(owner), "Added to All apps. Select a category first to file dropped apps straight into it.");
    }
}

// ---------------------------------------------------------------- icons

pub(super) fn pick_image(owner: HWND) -> Option<PathBuf> {
    let filter: String = format!(
        "Images ({})\0{}\0All files\0*.*\0\0",
        ICON_EXTENSIONS.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(", "),
        ICON_EXTENSIONS.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";")
    );
    let filter_w: Vec<u16> = filter.encode_utf16().collect();
    let mut file = vec![0u16; 1024];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter_w.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() { Some(PathBuf::from(ui::from_wide(&file))) } else { None }
}

/// Small "Choose image… / Reset" menu under an Icon… button. Returns 1 or 2.
fn icon_menu(owner: HWND, button: u16, reset_label: &str) -> u32 {
    unsafe {
        let menu: HMENU = CreatePopupMenu().unwrap_or_default();
        let a = wide("Choose image…");
        let b = wide(reset_label);
        let _ = InsertMenuW(menu, 0, MF_BYPOSITION | MF_STRING, 1, PCWSTR(a.as_ptr()));
        let _ = InsertMenuW(menu, 1, MF_BYPOSITION | MF_STRING, 2, PCWSTR(b.as_ptr()));
        let mut rc = RECT::default();
        let _ = GetWindowRect(ctl(button), &mut rc);
        let id = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, rc.left, rc.bottom, owner, None).0 as u32;
        let _ = DestroyMenu(menu);
        id
    }
}

fn category_icon(owner: HWND) {
    let Some(id) = selected_category() else { return };
    let choice = icon_menu(owner, CAT_ICON, "Use folder icon");
    let picked = choice == 1;
    let new_file = match choice {
        1 => {
            let Some(path) = pick_image(owner) else { return };
            match icons::import_file(&path, &paths::get().icons) {
                Ok(name) => Some(name),
                Err(e) => {
                    ui::error(Some(owner), &e);
                    return;
                }
            }
        }
        2 => None,
        _ => return,
    };
    let old =
        app::with(|s| tree::find_mut(&mut s.cfg.categories, id).and_then(|c| std::mem::replace(&mut c.icon, new_file)));
    if let Some(old) = old {
        paths::remove_icon(&old);
    }
    app::save();
    let key = format!("cat:{id}");
    app::reload_icon(&key);
    if picked && app::icon(&key).is_none() {
        ui::warn(Some(owner), "That image couldn't be read, so the folder icon is used instead.");
    }
    rebuild_tree(Some(id));
}

fn app_icon(owner: HWND, list_id: u16, button: u16) {
    let ids = selected_rows(list_id);
    let [id] = ids.as_slice() else {
        ui::info(Some(owner), "Select one app first.");
        return;
    };
    let choice = icon_menu(owner, button, "Reset to default icon");
    let picked = choice == 1;
    let new_file = match choice {
        1 => {
            let Some(path) = pick_image(owner) else { return };
            match icons::import_file(&path, &paths::get().icons) {
                Ok(name) => Some(name),
                Err(e) => {
                    ui::error(Some(owner), &e);
                    return;
                }
            }
        }
        2 => None,
        _ => return,
    };
    let old = app::with(|s| match new_file {
        Some(f) => s.cfg.app_icons.insert(id.clone(), f),
        None => s.cfg.app_icons.remove(id),
    });
    if let Some(old) = old {
        paths::remove_icon(&old);
    }
    app::save();
    app::reload_icon(id);
    if picked && app::icon(id).is_none() {
        ui::warn(Some(owner), "That image couldn't be read, so the app has no icon.");
    }
}

/// Called after the app's icon cache gained new icons.
pub fn icons_arrived(keys: &[String]) {
    if hwnd().is_none() || keys.is_empty() {
        return;
    }
    let missing_in_cat = MANAGER.with(|m| {
        m.borrow()
            .as_ref()
            .is_some_and(|m| m.cat_rows.iter().any(|id| keys.contains(id) && !m.app_index.contains_key(id)))
    });
    if missing_in_cat {
        refresh_category_apps();
    }
    if keys.iter().any(|k| k.starts_with("cat:")) {
        rebuild_tree(None);
    }
    unsafe {
        let _ = InvalidateRect(Some(ctl(ALL)), None, false);
    }
}

/// Called after icons were replaced (custom icon picked or reset).
pub fn icons_changed(keys: &[String]) {
    if hwnd().is_none() {
        return;
    }
    for key in keys {
        let (images, index, is_cat) = MANAGER.with(|m| {
            let b = m.borrow();
            let m = b.as_ref().unwrap();
            if key.starts_with("cat:") {
                (m.tree_images, m.tree_index.get(key).copied(), true)
            } else {
                (m.app_images, m.app_index.get(key).copied(), false)
            }
        });
        match (index, app::icon(key)) {
            (Some(i), Some(bmp)) => unsafe {
                let _ = ImageList_Replace(images, i, bmp, None);
            },
            (Some(_), None) => MANAGER.with(|m| {
                if let Some(m) = m.borrow_mut().as_mut() {
                    if is_cat {
                        m.tree_index.remove(key)
                    } else {
                        m.app_index.remove(key)
                    };
                }
            }),
            _ => {}
        }
    }
    refresh_category_apps();
    unsafe {
        let _ = InvalidateRect(Some(ctl(ALL)), None, false);
        let _ = InvalidateRect(Some(ctl(TREE)), None, false);
    }
}

pub fn catalog_changed() {
    if hwnd().is_some() {
        refresh_all();
        refresh_category_apps();
        update_status();
    }
}

pub fn scan_state_changed() {
    if hwnd().is_some() {
        update_status();
    }
}

// ---------------------------------------------------------------- settings

fn hotkey_to_control(hk: Option<Hotkey>) -> usize {
    let Some(hk) = hk else { return 0 };
    let mut flags = 0;
    if hk.modifiers & MOD_SHIFT != 0 {
        flags |= HOTKEYF_SHIFT;
    }
    if hk.modifiers & MOD_CONTROL != 0 {
        flags |= HOTKEYF_CONTROL;
    }
    if hk.modifiers & MOD_ALT != 0 {
        flags |= HOTKEYF_ALT;
    }
    ((flags << 8) | (hk.key & 0xFF)) as usize
}

fn hotkey_from_control(id: u16) -> Option<Hotkey> {
    let v = unsafe { SendMessageW(ctl(id), HKM_GETHOTKEY, Some(WPARAM(0)), Some(LPARAM(0))).0 as u32 };
    let key = v & 0xFF;
    if key == 0 {
        return None;
    }
    let flags = (v >> 8) & 0xFF;
    let mut modifiers = 0;
    if flags & HOTKEYF_SHIFT != 0 {
        modifiers |= MOD_SHIFT;
    }
    if flags & HOTKEYF_CONTROL != 0 {
        modifiers |= MOD_CONTROL;
    }
    if flags & HOTKEYF_ALT != 0 {
        modifiers |= MOD_ALT;
    }
    Some(Hotkey { modifiers, key })
}

fn load_settings() {
    let (search, menu, bar) =
        app::with(|s| (s.cfg.settings.search_hotkey, s.cfg.settings.menu_hotkey, s.cfg.settings.bar_hotkey));
    unsafe {
        SendMessageW(ctl(HK_SEARCH), HKM_SETHOTKEY, Some(WPARAM(hotkey_to_control(search))), Some(LPARAM(0)));
        SendMessageW(ctl(HK_MENU), HKM_SETHOTKEY, Some(WPARAM(hotkey_to_control(menu))), Some(LPARAM(0)));
        SendMessageW(ctl(HK_BAR), HKM_SETHOTKEY, Some(WPARAM(hotkey_to_control(bar))), Some(LPARAM(0)));
        let check = if autostart::is_enabled() { BST_CHECKED } else { BST_UNCHECKED };
        SendMessageW(ctl(AUTOSTART), BM_SETCHECK, Some(WPARAM(check.0 as usize)), Some(LPARAM(0)));
        let (show, reserve, auto, packages, switch) = app::with(|s| {
            let st = &s.cfg.settings;
            (st.show_strip, st.reserve_space, st.auto_rescan, st.package_apps, st.switch_to_running)
        });
        for (id, on) in [
            (STRIP_SHOW, show),
            (STRIP_RESERVE, reserve),
            (AUTO_RESCAN, auto),
            (PACKAGE_APPS, packages),
            (SWITCH_RUNNING, switch),
        ] {
            let check = if on { BST_CHECKED } else { BST_UNCHECKED };
            SendMessageW(ctl(id), BM_SETCHECK, Some(WPARAM(check.0 as usize)), Some(LPARAM(0)));
        }
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(ctl(STRIP_RESERVE), show);
    }
}

/// The strip was shown/hidden from elsewhere (tray menu, strip context menu).
pub fn settings_changed() {
    if hwnd().is_some() {
        load_settings();
    }
}

fn is_checked(id: u16) -> bool {
    unsafe { SendMessageW(ctl(id), BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))).0 == BST_CHECKED.0 as isize }
}

fn pin_selected(owner: HWND) {
    let ids = selected_rows(ALL);
    if ids.is_empty() {
        ui::info(Some(owner), "Select one or more apps in the All apps list first.");
        return;
    }
    let changed = app::with(|s| ids.iter().fold(false, |acc, id| s.cfg.pin(id) | acc));
    if changed {
        app::save();
        ui::set_text(ctl(STATUS), "Pinned to the strip. Right-click an icon on the strip to move or unpin it.");
    }
}

fn apply_hotkeys(owner: HWND) {
    let search = hotkey_from_control(HK_SEARCH);
    let menu = hotkey_from_control(HK_MENU);
    let bar = hotkey_from_control(HK_BAR);
    for hk in [search, menu, bar].into_iter().flatten() {
        let is_f_key = (0x70..=0x87).contains(&hk.key);
        if hk.modifiers == 0 && !is_f_key {
            ui::warn(
                Some(owner),
                &format!(
                    "{} has no Ctrl/Alt/Shift, so it would steal that key from every program. Add a modifier.",
                    hk.describe()
                ),
            );
            return;
        }
    }
    let set: Vec<_> = [search, menu, bar].into_iter().flatten().collect();
    if (1..set.len()).any(|i| set[..i].contains(&set[i])) {
        ui::warn(Some(owner), "The search, menu and bar hotkeys must all be different.");
        return;
    }
    app::with(|s| {
        s.cfg.settings.search_hotkey = search;
        s.cfg.settings.menu_hotkey = menu;
        s.cfg.settings.bar_hotkey = bar;
    });
    app::save();
    let problems = app::register_hotkeys();
    if problems.is_empty() {
        ui::set_text(ctl(STATUS), "Hotkeys applied.");
    } else {
        ui::warn(Some(owner), &problems.join("\n"));
    }
}

fn update_status() {
    let p = paths::get();
    let (count, scanning) = app::with(|s| (s.catalog.apps.len(), s.scanning));
    let place = if p.portable {
        format!("Data: {} (portable)", p.data.display())
    } else {
        format!("Data: {} — the exe's folder isn't writable, so settings are not portable", p.data.display())
    };
    let apps = if scanning { "scanning apps…".to_string() } else { format!("{count} apps") };
    ui::set_text(ctl(STATUS), &format!("{place} · {apps}"));
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(ctl(RESCAN), !scanning);
    }
}

fn import_old(owner: HWND) {
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else { return };
    let dir = appdata.join("FlexTaskbar");
    let cats_path = dir.join("categories.json");
    let Ok(cats_json) = std::fs::read_to_string(&cats_path) else {
        ui::info(
            Some(owner),
            &format!("No settings from the previous FlexTaskbar were found at\n{}", cats_path.display()),
        );
        return;
    };
    let apps_json = std::fs::read_to_string(dir.join("applications.json")).unwrap_or_default();
    if !ui::confirm(
        Some(owner),
        "Import the categories from the previous (.NET) FlexTaskbar?\n\nThey are added next to your current categories; nothing is overwritten. Each imported app keeps its exact launch command.",
    ) {
        return;
    }
    let result = app::with(|s| migrate::import(&cats_json, &apps_json, &mut s.cfg));
    let summary = match result {
        Ok(sm) => sm,
        Err(e) => {
            ui::error(Some(owner), &format!("Couldn't read the old settings: {e}"));
            return;
        }
    };
    let icons_dir = paths::get().icons.clone();
    let mut icons_failed = 0;
    for copy in &summary.icons {
        let (source, apply): (&String, Box<dyn FnOnce(String)>) = match copy {
            migrate::IconCopy::Category { id, source } => {
                let id = *id;
                (
                    source,
                    Box::new(move |name| {
                        app::with(|s| {
                            if let Some(c) = tree::find_mut(&mut s.cfg.categories, id) {
                                c.icon = Some(name);
                            }
                        })
                    }),
                )
            }
            migrate::IconCopy::App { id, source } => {
                let id = id.clone();
                (
                    source,
                    Box::new(move |name| {
                        app::with(|s| {
                            s.cfg.app_icons.insert(id, name);
                        })
                    }),
                )
            }
        };
        match icons::import_file(std::path::Path::new(source), &icons_dir) {
            Ok(name) => apply(name),
            Err(_) => icons_failed += 1,
        }
    }
    app::save();
    app::rebuild_catalog();
    app::request_all_icons();
    rebuild_tree(None);
    refresh_all();
    super::searchwin::catalog_changed();
    let mut text = format!("Imported {} categories and {} apps.", summary.categories, summary.apps);
    if icons_failed > 0 {
        text.push_str(&format!("\n{icons_failed} custom icon(s) couldn't be copied and use the default icon."));
    }
    ui::info(Some(owner), &text);
}

// ---------------------------------------------------------------- window proc

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            layout();
            LRESULT(0)
        }
        WM_PAINT => panel::paint(hwnd),
        WM_ERASEBKGND => LRESULT(1),
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => panel::color(hwnd, wparam, lparam),
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => panel::field_color(wparam),
        WM_GETMINMAXINFO => {
            let mmi = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            let dpi = ui::dpi_of(hwnd);
            mmi.ptMinTrackSize = POINT { x: scale(1000, dpi), y: scale(620, dpi) };
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = ui::loword(wparam.0);
            let code = ui::hiword(wparam.0);
            if id == ALL_FILTER && code == EN_CHANGE {
                refresh_all();
                return LRESULT(0);
            }
            if code != BN_CLICKED {
                return LRESULT(0);
            }
            match id {
                CAT_NEW => {
                    new_category(selected_category().and_then(|c| app::with(|s| tree::parent_of(&s.cfg.categories, c))))
                }
                CAT_SUB => match selected_category() {
                    Some(c) => new_category(Some(c)),
                    None => new_category(None),
                },
                CAT_RENAME => {
                    if let Some(c) = selected_category() {
                        edit_label(c);
                    }
                }
                CAT_DELETE => delete_category(hwnd),
                CAT_UP => tree_op(|t, id| tree::move_sibling(t, id, -1)),
                CAT_DOWN => tree_op(|t, id| tree::move_sibling(t, id, 1)),
                CAT_OUT => tree_op(tree::outdent),
                CAT_IN => {
                    let limit = super::strip::max_levels();
                    let fits = selected_category()
                        .is_some_and(|id| app::with(|s| tree::indent_fits(&s.cfg.categories, id, limit)));
                    if fits {
                        tree_op(tree::indent)
                    } else if selected_category().is_some() {
                        too_deep(hwnd);
                    }
                }
                CAT_ICON => category_icon(hwnd),
                APP_REMOVE => remove_selected_from_category(),
                APP_UP => move_selected_app(-1),
                APP_DOWN => move_selected_app(1),
                APP_ICON => app_icon(hwnd, APPS, APP_ICON),
                ALL_ADD => add_selected_to_category(hwnd),
                ALL_ICON => app_icon(hwnd, ALL, ALL_ICON),
                ALL_PIN => pin_selected(hwnd),
                STRIP_SHOW => app::set_strip(Some(is_checked(STRIP_SHOW))),
                SWITCH_RUNNING => {
                    let on = is_checked(SWITCH_RUNNING);
                    app::with(|s| s.cfg.settings.switch_to_running = on);
                    app::save();
                }
                AUTO_RESCAN => {
                    let on = is_checked(AUTO_RESCAN);
                    app::with(|s| s.cfg.settings.auto_rescan = on);
                    app::save();
                }
                PACKAGE_APPS => {
                    let on = is_checked(PACKAGE_APPS);
                    app::with(|s| s.cfg.settings.package_apps = on);
                    app::save();
                    app::request_rescan();
                }
                STRIP_RESERVE => {
                    let reserve = is_checked(STRIP_RESERVE);
                    app::with(|s| s.cfg.settings.reserve_space = reserve);
                    app::save();
                    super::strip::apply_settings();
                }
                CUSTOM_NEW => new_custom_app(hwnd, CustomApp::default()),
                CUSTOM_EDIT => edit_custom_app(hwnd),
                CUSTOM_DELETE => delete_custom_app(hwnd),
                AUTOSTART => {
                    let want = unsafe { SendMessageW(ctl(AUTOSTART), BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))).0 }
                        == BST_CHECKED.0 as isize;
                    if want != autostart::is_enabled() {
                        app::toggle_autostart(Some(hwnd));
                    }
                    load_settings();
                }
                HK_APPLY => apply_hotkeys(hwnd),
                RESCAN => app::start_scan(),
                IMPORT => import_old(hwnd),
                OPEN_DATA => app::open_data_folder(),
                APPEARANCE => super::appearancewin::show(),
                ARRANGE => super::arrangewin::show(),
                REFRESH_LABEL => refresh_category_apps(),
                CLOSE => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            }
            LRESULT(0)
        }
        WM_NOTIFY => unsafe { on_notify(hwnd, msg, wparam, lparam) },
        WM_DROPFILES => {
            on_drop(hwnd, HDROP(wparam.0 as *mut _));
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = ui::hiword(wparam.0) as u32;
            let (font, controls) = MANAGER.with(|m| {
                let mut b = m.borrow_mut();
                let m = b.as_mut().unwrap();
                ui::delete_font(m.font);
                m.font = ui::message_font(dpi, 1.0);
                m.dpi = dpi;
                (m.font, m.controls.values().copied().collect::<Vec<_>>())
            });
            for c in controls {
                ui::set_font(c, font);
            }
            let rc = unsafe { &*(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(hwnd, None, rc.left, rc.top, ui::rect_w(rc), ui::rect_h(rc), SWP_NOZORDER);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            app::register_dialog(hwnd, false);
            panel::forget(hwnd);
            let taken = MANAGER.with(|m| m.borrow_mut().take());
            if let Some(m) = taken {
                ui::delete_font(m.font);
                unsafe {
                    let _ = ImageList_Destroy(Some(m.tree_images));
                    let _ = ImageList_Destroy(Some(m.app_images));
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn on_notify(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hdr = unsafe { &*(lparam.0 as *const NMHDR) };
    let id = hdr.idFrom as u16;
    match (id, hdr.code) {
        (TREE, TVN_SELCHANGEDW) => {
            if !SUPPRESS.with(|s| s.get()) {
                refresh_category_apps();
            }
            LRESULT(0)
        }
        (TREE, TVN_ITEMEXPANDEDW) => {
            let nm = unsafe { &*(lparam.0 as *const NMTREEVIEWW) };
            if !SUPPRESS.with(|s| s.get()) {
                let cat = nm.itemNew.lParam.0 as u64;
                let expanded = nm.action.0 == TVE_EXPAND.0;
                MANAGER.with(|m| {
                    if let Some(m) = m.borrow_mut().as_mut() {
                        if expanded {
                            m.collapsed.remove(&cat);
                        } else {
                            m.collapsed.insert(cat);
                        }
                    }
                });
            }
            LRESULT(0)
        }
        (TREE, TVN_ENDLABELEDITW) => {
            let di = unsafe { &*(lparam.0 as *const NMTVDISPINFOW) };
            if di.item.pszText.is_null() {
                return LRESULT(0); // cancelled
            }
            let name = unsafe { di.item.pszText.to_string().unwrap_or_default() }.trim().to_string();
            if name.is_empty() {
                return LRESULT(0);
            }
            let cat = di.item.lParam.0 as u64;
            app::with(|s| {
                if let Some(c) = tree::find_mut(&mut s.cfg.categories, cat) {
                    c.name = name;
                }
            });
            app::save();
            // Refresh the "Apps in …" label after the edit is committed.
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_COMMAND,
                    WPARAM(REFRESH_LABEL as usize),
                    LPARAM(0),
                );
            }
            LRESULT(1)
        }
        (ALL, LVN_GETDISPINFOW) | (APPS, LVN_GETDISPINFOW) => {
            let di = unsafe { &mut *(lparam.0 as *mut NMLVDISPINFOW) };
            let row = di.item.iItem as usize;
            let app_id = MANAGER.with(|m| {
                m.borrow()
                    .as_ref()
                    .and_then(|m| if id == ALL { m.all_rows.get(row).cloned() } else { m.cat_rows.get(row).cloned() })
            });
            let Some(app_id) = app_id else { return LRESULT(0) };
            if id == ALL
                && di.item.mask & LVIF_TEXT == LVIF_TEXT
                && !di.item.pszText.is_null()
                && di.item.cchTextMax > 0
            {
                let (name, kind) =
                    app::with(|s| s.catalog.get(&app_id).map(|a| (a.name.clone(), a.kind.label()))).unwrap_or_default();
                let text = wide(&if kind.is_empty() { name } else { format!("{name}  ({})", kind.to_lowercase()) });
                let n = text.len().min(di.item.cchTextMax as usize);
                unsafe {
                    std::ptr::copy_nonoverlapping(text.as_ptr(), di.item.pszText.0, n);
                    *di.item.pszText.0.add(n - 1) = 0;
                }
            }
            if di.item.mask & LVIF_IMAGE == LVIF_IMAGE {
                di.item.iImage = app_image(&app_id);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
