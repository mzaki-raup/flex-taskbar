//! Modal dialog for adding or editing a custom app.

use super::ui::{self, scale, wide};
use super::{app, panel};
use crate::config::CustomApp;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, GetSysColorBrush, HBRUSH, HFONT};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NODEREFERENCELINKS, OPENFILENAMEW,
};
use windows::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, GetMessageW, GetWindowRect, IDCANCEL, IDOK, IsDialogMessageW, MSG,
    PostQuitMessage, RegisterClassW, SW_SHOW, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos,
    ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
    WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_ERASEBKGND, WM_PAINT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_DLGMODALFRAME, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, PWSTR};

const NAME: u16 = 10;
const TARGET: u16 = 11;
const BROWSE: u16 = 12;
const ARGS: u16 = 13;
const DIR: u16 = 14;
const ADMIN: u16 = 15;

struct Dialog {
    controls: HashMap<u16, HWND>,
    result: Option<CustomApp>,
    done: bool,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

fn ctl(id: u16) -> HWND {
    DIALOG.with(|d| d.borrow().as_ref().and_then(|d| d.controls.get(&id).copied()).unwrap_or_default())
}

/// Shows the dialog and returns the edited fields (id left empty), or `None` if
/// cancelled.
pub fn edit(owner: HWND, initial: &CustomApp, title: &str) -> Option<CustomApp> {
    if DIALOG.with(|d| d.borrow().is_some()) {
        return None; // already open
    }
    let class = wide("FlexTaskbar.AppDialog");
    let title_w = wide(title);
    let dpi = ui::dpi_of(owner);
    let s = |v: i32| scale(v, dpi);
    let font: HFONT = ui::message_font(dpi, 1.0);

    let hwnd = unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: app::instance(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hbrBackground: HBRUSH(GetSysColorBrush(COLOR_WINDOW).0),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let mut orc = RECT::default();
        let _ = GetWindowRect(owner, &mut orc);
        let (w, h) = (s(560), s(330));
        let (x, y) =
            ui::keep_on_screen(orc.left + (ui::rect_w(&orc) - w) / 2, orc.top + (ui::rect_h(&orc) - h) / 3, w, h);
        match CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            PCWSTR(class.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
            x,
            y,
            w,
            h,
            Some(owner),
            None,
            Some(app::instance()),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return None,
        }
    };

    let mut controls = HashMap::new();
    let pm = panel::metrics(dpi);
    let card_w = s(560);
    let (lx, rh) = (pm.margin + pm.pad, s(26));
    let fx = lx + s(100);
    let fw = pm.margin + card_w - pm.pad - fx;
    let card_top = pm.header;
    let mut y = pm.content_top(card_top, false);
    let mut add = |id: u16, class: &str, text: &str, style: WINDOW_STYLE, x: i32, yy: i32, w: i32, h: i32| {
        let c = ui::child(hwnd, class, text, WS_CHILD | WS_VISIBLE | style, WINDOW_EX_STYLE(0), id);
        unsafe {
            let _ = SetWindowPos(c, None, x, yy, w, h, SWP_NOZORDER);
        }
        ui::set_font(c, font);
        if id != 0 {
            controls.insert(id, c);
        }
        c
    };
    let edit_style = WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
    let label_style = ui::SS_LABEL;
    for (label, id, value) in [
        ("Name", NAME, initial.name.as_str()),
        ("Target", TARGET, initial.target.as_str()),
        ("Arguments", ARGS, initial.args.as_str()),
        ("Start in", DIR, initial.working_dir.as_str()),
    ] {
        add(0, "STATIC", label, label_style, lx, y + s(4), fx - lx - s(6), s(20));
        let w = if id == TARGET { fw - s(90) } else { fw };
        add(id, "EDIT", value, edit_style, fx, y, w, s(24));
        if id == TARGET {
            add(
                BROWSE,
                "BUTTON",
                "Browse…",
                WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                fx + fw - s(84),
                y - s(1),
                s(84),
                rh,
            );
        }
        y += s(34);
    }
    add(ADMIN, "BUTTON", "Run as administrator", WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), fx, y, fw, s(22));
    y += s(30);
    let hint = add(
        0,
        "STATIC",
        "Target can be a program, a shortcut, any file, a URL (https://…), shell:AppsFolder\\… or a protocol such as ms-settings:. \
         For a browser web app use the browser's proxy exe with --app-id=… in Arguments.",
        WINDOW_STYLE(0x0080), // SS_NOPREFIX, wrapping
        fx,
        y,
        fw,
        s(50),
    );
    panel::subtle(hwnd, hint);
    y += s(50) + pm.pad;
    let footer = y + pm.margin;
    let right = pm.margin + card_w + pm.margin;
    let by = footer + (pm.footer - rh) / 2;
    add(
        IDOK.0 as u16,
        "BUTTON",
        "OK",
        WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        right - pm.margin - s(2 * 96 + 6),
        by,
        s(96),
        rh,
    );
    add(
        IDCANCEL.0 as u16,
        "BUTTON",
        "Cancel",
        WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        right - pm.margin - s(96),
        by,
        s(96),
        rh,
    );
    panel::set(
        hwnd,
        panel::Page {
            title: title.to_string(),
            subtitle: "A program, file, website or shell location to launch from the bar and the menus.".into(),
            cards: vec![panel::Card {
                rect: RECT { left: pm.margin, top: card_top, right: pm.margin + card_w, bottom: y },
                title: "App".into(),
                subtitle: String::new(),
            }],
            footer: Some(footer),
        },
    );
    // Size the window around the page, over its owner.
    unsafe {
        let mut outer = RECT { left: 0, top: 0, right, bottom: footer + pm.footer };
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(
            &mut outer,
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            false,
            WS_EX_DLGMODALFRAME,
            dpi,
        );
        let (w, h) = (ui::rect_w(&outer), ui::rect_h(&outer));
        let mut orc = RECT::default();
        let _ = GetWindowRect(owner, &mut orc);
        let (x, top) =
            ui::keep_on_screen(orc.left + (ui::rect_w(&orc) - w) / 2, orc.top + (ui::rect_h(&orc) - h) / 3, w, h);
        let _ = SetWindowPos(hwnd, None, x, top, w, h, SWP_NOZORDER);
    }

    panel::apply_theme(hwnd);
    DIALOG.with(|d| *d.borrow_mut() = Some(Dialog { controls, result: None, done: false }));
    let check = if initial.run_as_admin { BST_CHECKED } else { BST_UNCHECKED };
    unsafe {
        SendMessageW(ctl(ADMIN), BM_SETCHECK, Some(WPARAM(check.0 as usize)), Some(LPARAM(0)));
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(ctl(NAME)));
    }

    // Modal loop.
    let mut msg = MSG::default();
    loop {
        if DIALOG.with(|d| d.borrow().as_ref().is_none_or(|d| d.done)) {
            break;
        }
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 <= 0 {
            unsafe { PostQuitMessage(msg.wParam.0 as i32) }; // let the outer loop see WM_QUIT
            break;
        }
        unsafe {
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe {
        // Re-enable the owner before destroying, so activation returns to it.
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(hwnd);
        let _ = SetForegroundWindow(owner);
    }
    ui::delete_font(font);
    panel::forget(hwnd);
    DIALOG.with(|d| d.borrow_mut().take()).and_then(|d| d.result)
}

fn finish(ok: bool, hwnd: HWND) {
    if ok {
        let name = ui::get_text(ctl(NAME)).trim().to_string();
        let target = ui::get_text(ctl(TARGET)).trim().trim_matches('"').to_string();
        if name.is_empty() || target.is_empty() {
            ui::warn(Some(hwnd), "Name and Target are required.");
            return;
        }
        let admin = unsafe { SendMessageW(ctl(ADMIN), BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))).0 }
            == BST_CHECKED.0 as isize;
        let result = CustomApp {
            id: String::new(),
            name,
            target,
            args: ui::get_text(ctl(ARGS)).trim().to_string(),
            working_dir: ui::get_text(ctl(DIR)).trim().trim_matches('"').to_string(),
            run_as_admin: admin,
        };
        DIALOG.with(|d| {
            if let Some(d) = d.borrow_mut().as_mut() {
                d.result = Some(result);
            }
        });
    }
    DIALOG.with(|d| {
        if let Some(d) = d.borrow_mut().as_mut() {
            d.done = true;
        }
    });
}

fn browse(hwnd: HWND) {
    let filter: Vec<u16> =
        "Programs and shortcuts\0*.exe;*.lnk;*.bat;*.cmd;*.url;*.msc\0All files\0*.*\0\0".encode_utf16().collect();
    let mut file = vec![0u16; 1024];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        // Keep .lnk files as picked: launching the shortcut honours its own arguments.
        Flags: OFN_FILEMUSTEXIST | OFN_NODEREFERENCELINKS,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return;
    }
    let path = ui::from_wide(&file);
    ui::set_text(ctl(TARGET), &path);
    if ui::get_text(ctl(NAME)).trim().is_empty() {
        let stem =
            std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        ui::set_text(ctl(NAME), &stem);
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match ui::loword(wparam.0) as i32 {
                x if x == IDOK.0 => finish(true, hwnd),
                x if x == IDCANCEL.0 => finish(false, hwnd),
                x if x == BROWSE as i32 => browse(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_PAINT => panel::paint(hwnd),
        WM_ERASEBKGND => LRESULT(1),
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => panel::color(hwnd, wparam, lparam),
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => panel::field_color(wparam),
        WM_CLOSE => {
            finish(false, hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
