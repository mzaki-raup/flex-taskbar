//! The picture that follows the pointer while an app is dragged out of a
//! flyout: its icon, see-through, in a small layered window that lets the
//! pointer through. Drawn once when the drag starts and then only moved.

use super::app;
use super::canvas::{self, Canvas};
use super::ui::wide;
use crate::appearance::Rgba;
use resvg::tiny_skia::Pixmap;
use std::cell::Cell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{DT_CENTER, DT_SINGLELINE, DT_VCENTER, HFONT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOSIZE, SetWindowPos, ShowWindow, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

thread_local! {
    /// The window, and how far up and left of the pointer it sits.
    static IMAGE: Cell<Option<(HWND, i32)>> = const { Cell::new(None) };
}

/// What an app without an icon looks like: its name's first letter on a tile.
pub struct Letter {
    pub text: String,
    pub font: HFONT,
    pub tile: Rgba,
    pub colour: Rgba,
}

/// Starts showing `icon` (or `letter` when there is none) at `pt`.
pub fn show(icon: Option<&Pixmap>, letter: Letter, size: i32, pt: POINT) {
    hide();
    let Some(mut cv) = Canvas::new(size, size) else { return };
    match icon {
        Some(img) => cv.image(img, 0, 0, size, 0.7),
        None => {
            cv.fill_round_rect(0.0, 0.0, size as f32, size as f32, size as f32 / 6.0, letter.tile);
            let rc = RECT { left: 0, top: 0, right: size, bottom: size };
            cv.text(&letter.text, rc, letter.font, letter.colour, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
    }
    let class = wide("FlexTaskbar.DragImage");
    let hwnd = unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: app::instance(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            PCWSTR(class.as_ptr()),
            w!("FlexTaskbar drag"),
            WS_POPUP,
            0,
            0,
            size,
            size,
            None,
            None,
            Some(app::instance()),
            None,
        )
    };
    let Ok(hwnd) = hwnd else { return };
    // Above and left of the pointer, so what it points at stays in view.
    let offset = size * 7 / 8;
    canvas::present_pixmap(&cv.pix, hwnd, pt.x - offset, pt.y - offset);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    IMAGE.with(|i| i.set(Some((hwnd, offset))));
}

/// Moves it to follow the pointer.
pub fn move_to(pt: POINT) {
    if let Some((hwnd, offset)) = IMAGE.with(|i| i.get()) {
        unsafe {
            let _ =
                SetWindowPos(hwnd, Some(HWND_TOPMOST), pt.x - offset, pt.y - offset, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn hide() {
    if let Some((hwnd, _)) = IMAGE.with(|i| i.take()) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
