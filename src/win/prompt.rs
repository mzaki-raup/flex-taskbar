//! A small modal window asking for one line of text (a saved look's name),
//! in the settings windows' style.

use super::ui::{self, scale, wide};
use super::{app, panel};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, GetSysColorBrush, HBRUSH};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL,
    GetMessageW, GetWindowRect, IDCANCEL, IDOK, IsDialogMessageW, IsWindowVisible, MSG, PostQuitMessage,
    RegisterClassW, SW_SHOW, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
    WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_ERASEBKGND, WM_PAINT, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_DLGMODALFRAME, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::PCWSTR;

const EDIT: u16 = 10;
const EM_SETSEL: u32 = 0x00B1;

thread_local! {
    /// The box, and the answer once OK or Cancel is pressed.
    static STATE: std::cell::RefCell<Option<(HWND, Option<Option<String>>)>> = const { std::cell::RefCell::new(None) };
}

/// Asks for a line of text. `None` when cancelled.
pub fn ask(owner: HWND, title: &str, label: &str, initial: &str) -> Option<String> {
    if STATE.with(|s| s.borrow().is_some()) {
        return None;
    }
    let dpi = ui::dpi_of(owner);
    let s = |v: i32| scale(v, dpi);
    let pm = panel::metrics(dpi);
    let font = ui::message_font(dpi, 1.0);
    let class = wide("FlexTaskbar.Prompt");
    let title_w = wide(title);
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
    let hwnd = unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: app::instance(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hbrBackground: HBRUSH(GetSysColorBrush(COLOR_WINDOW).0),
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            PCWSTR(class.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            style,
            0,
            0,
            100,
            100,
            Some(owner),
            None,
            Some(app::instance()),
            None,
        )
        .ok()?
    };
    let card_w = s(380);
    let card_top = pm.margin;
    let y = pm.content_top(card_top, false);
    let x = pm.margin + pm.pad;
    let w = card_w - 2 * pm.pad;
    let add = |id: u16, class: &str, text: &str, st: WINDOW_STYLE, ex: WINDOW_EX_STYLE, r: (i32, i32, i32, i32)| {
        let c = ui::child(hwnd, class, text, WS_CHILD | WS_VISIBLE | st, ex, id);
        unsafe {
            let _ = SetWindowPos(c, None, r.0, r.1, r.2, r.3, SWP_NOZORDER);
        }
        ui::set_font(c, font);
        c
    };
    let edit = add(
        EDIT,
        "EDIT",
        initial,
        WS_TABSTOP | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        WINDOW_EX_STYLE(0),
        (x, y, w, s(24)),
    );
    let card_bottom = y + s(24) + pm.pad;
    let footer = card_bottom + pm.margin;
    let right = pm.margin + card_w + pm.margin;
    let by = footer + (pm.footer - s(26)) / 2;
    add(
        IDOK.0 as u16,
        "BUTTON",
        "OK",
        WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE(0),
        (right - pm.margin - s(2 * 96 + 6), by, s(96), s(26)),
    );
    add(
        IDCANCEL.0 as u16,
        "BUTTON",
        "Cancel",
        WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        WINDOW_EX_STYLE(0),
        (right - pm.margin - s(96), by, s(96), s(26)),
    );
    panel::set(
        hwnd,
        panel::Page {
            title: String::new(),
            subtitle: String::new(),
            cards: vec![panel::Card {
                rect: RECT { left: pm.margin, top: card_top, right: pm.margin + card_w, bottom: card_bottom },
                title: label.to_string(),
                subtitle: String::new(),
            }],
            footer: Some(footer),
        },
    );
    panel::apply_theme(hwnd);
    unsafe {
        let mut outer = RECT { left: 0, top: 0, right, bottom: footer + pm.footer };
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut outer, style, false, WS_EX_DLGMODALFRAME, dpi);
        let (ww, wh) = (ui::rect_w(&outer), ui::rect_h(&outer));
        // Over its owner, or (for the hidden main window: the tray menu) the
        // screen with the pointer on it.
        let mut orc = RECT::default();
        if IsWindowVisible(owner).as_bool() {
            let _ = GetWindowRect(owner, &mut orc);
        } else {
            orc = ui::work_area_at_cursor();
        }
        let left = orc.left + (ui::rect_w(&orc) - ww) / 2;
        let top = orc.top + (ui::rect_h(&orc) - wh) / 3;
        let _ = SetWindowPos(hwnd, None, left, top, ww, wh, SWP_NOZORDER);
        STATE.with(|st| *st.borrow_mut() = Some((edit, None)));
        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(edit));
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
    }
    let mut msg = MSG::default();
    loop {
        if STATE.with(|st| st.borrow().as_ref().is_none_or(|(_, a)| a.is_some())) {
            break;
        }
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 <= 0 {
            unsafe { PostQuitMessage(msg.wParam.0 as i32) };
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
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(hwnd);
        let _ = SetForegroundWindow(owner);
    }
    panel::forget(hwnd);
    ui::delete_font(font);
    STATE.with(|st| st.borrow_mut().take()).and_then(|(_, a)| a).flatten()
}

fn finish(ok: bool) {
    STATE.with(|st| {
        if let Some((edit, answer)) = st.borrow_mut().as_mut() {
            *answer = Some(ok.then(|| ui::get_text(*edit)));
        }
    });
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match ui::loword(wparam.0) as i32 {
                x if x == IDOK.0 => finish(true),
                x if x == IDCANCEL.0 => finish(false),
                _ => {}
            }
            LRESULT(0)
        }
        WM_PAINT => panel::paint(hwnd),
        WM_ERASEBKGND => LRESULT(1),
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => panel::color(hwnd, wparam, lparam),
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => panel::field_color(wparam),
        WM_CLOSE => {
            finish(false);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
