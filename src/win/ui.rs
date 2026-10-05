//! Small Win32 helpers shared by every window.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, GetMonitorInfoW, HFONT, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDYES, MB_ICONERROR, MB_ICONINFORMATION,
    MB_ICONWARNING, MB_OK, MB_YESNO, MessageBoxW, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW, SetWindowTextW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT,
};
use windows::core::PCWSTR;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Doubles `&` so menu/button text isn't turned into an accelerator.
pub fn escape_amp(s: &str) -> String {
    s.replace('&', "&&")
}

pub fn loword(v: usize) -> u16 {
    (v & 0xFFFF) as u16
}

pub fn hiword(v: usize) -> u16 {
    ((v >> 16) & 0xFFFF) as u16
}

pub fn dpi_of(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}

pub fn scale(v: i32, dpi: u32) -> i32 {
    (v as i64 * dpi as i64 / 96) as i32
}

/// The system message font at the given DPI, optionally enlarged.
pub fn message_font(dpi: u32, size_factor: f32) -> HFONT {
    unsafe {
        let mut ncm =
            NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let _ = SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0).0,
            dpi,
        );
        let mut lf = ncm.lfMessageFont;
        lf.lfHeight = (lf.lfHeight as f32 * size_factor) as i32;
        CreateFontIndirectW(&lf)
    }
}

pub fn delete_font(font: HFONT) {
    if !font.is_invalid() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}

pub fn set_font(hwnd: HWND, font: HFONT) {
    unsafe {
        SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    }
}

/// Creates a child control. `id` is what arrives in WM_COMMAND / WM_NOTIFY.
pub fn child(parent: HWND, class: &str, text: &str, style: WINDOW_STYLE, ex_style: WINDOW_EX_STYLE, id: u16) -> HWND {
    let class_w = wide(class);
    let text_w = wide(text);
    unsafe {
        CreateWindowExW(
            ex_style,
            PCWSTR(class_w.as_ptr()),
            PCWSTR(text_w.as_ptr()),
            style,
            0,
            0,
            0,
            0,
            Some(parent),
            Some(HMENU(id as usize as *mut _)),
            None,
            None,
        )
        .unwrap_or_default()
    }
}

pub fn get_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

pub fn set_text(hwnd: HWND, text: &str) {
    let w = wide(text);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(w.as_ptr()));
    }
}

fn message_box(
    owner: Option<HWND>,
    text: &str,
    flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE,
) -> i32 {
    let t = wide(text);
    let c = wide("FlexTaskbar");
    unsafe { MessageBoxW(owner, PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), flags).0 }
}

pub fn error(owner: Option<HWND>, text: &str) {
    message_box(owner, text, MB_OK | MB_ICONERROR);
}

pub fn warn(owner: Option<HWND>, text: &str) {
    message_box(owner, text, MB_OK | MB_ICONWARNING);
}

pub fn info(owner: Option<HWND>, text: &str) {
    message_box(owner, text, MB_OK | MB_ICONINFORMATION);
}

pub fn confirm(owner: Option<HWND>, text: &str) -> bool {
    message_box(owner, text, MB_YESNO | MB_ICONWARNING) == IDYES.0
}

pub fn rect_w(r: &RECT) -> i32 {
    r.right - r.left
}

pub fn rect_h(r: &RECT) -> i32 {
    r.bottom - r.top
}

/// Static-control style for labels: SS_NOPREFIX (show `&` literally) |
/// SS_ENDELLIPSIS (truncate with … instead of wrapping).
pub const SS_LABEL: WINDOW_STYLE = WINDOW_STYLE(0x0080 | 0x4000);

/// Work area of the monitor under the mouse cursor.
pub fn work_area_at_cursor() -> RECT {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        mi.rcWork
    }
}

/// "No image" for list and tree view items: index 0 of every image list made
/// by [`image_list`] is a transparent blank. (-1 would be I_IMAGECALLBACK, and
/// tree views don't honour I_IMAGENONE.)
pub const NO_IMAGE: i32 = 0;

/// A 32-bit image list whose index 0 is a transparent placeholder.
pub fn image_list(size: i32, grow: i32) -> windows::Win32::UI::Controls::HIMAGELIST {
    use windows::Win32::UI::Controls::{ILC_COLOR32, ImageList_Add, ImageList_Create};
    unsafe {
        let list = ImageList_Create(size, size, ILC_COLOR32, grow, grow);
        if let Some(blank) = super::icons::blank(size) {
            ImageList_Add(list, blank, None);
            super::icons::free(blank);
        }
        list
    }
}

/// Creates an owned top-level popup of a system class (e.g. a tooltip window).
pub fn child_popup(owner: HWND, class: &str, style: WINDOW_STYLE) -> HWND {
    use windows::Win32::UI::WindowsAndMessaging::{CW_USEDEFAULT, WS_EX_TOPMOST, WS_POPUP};
    let class_w = wide(class);
    unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST,
            PCWSTR(class_w.as_ptr()),
            PCWSTR::null(),
            WS_POPUP | style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            Some(owner),
            None,
            None,
            None,
        )
        .unwrap_or_default()
    }
}
