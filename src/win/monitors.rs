//! Asking Windows which monitors are attached, and where each one is.

use crate::monitors::{Monitors, Screen};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

fn rect(r: RECT) -> (i32, i32, i32, i32) {
    (r.left, r.top, r.right, r.bottom)
}

pub fn to_rect(r: (i32, i32, i32, i32)) -> RECT {
    RECT { left: r.0, top: r.1, right: r.2, bottom: r.3 }
}

/// Fills in one monitor from its handle. `MONITORINFOEXW` carries the device
/// name on top of what `MONITORINFO` reports.
fn describe(h: HMONITOR) -> Option<Screen> {
    let mut mi = MONITORINFOEXW {
        monitorInfo: MONITORINFO { cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32, ..Default::default() },
        ..Default::default()
    };
    // GetMonitorInfoW reads cbSize to tell the two structures apart.
    let ok = unsafe { GetMonitorInfoW(h, &mut mi.monitorInfo as *mut MONITORINFO) };
    if !ok.as_bool() {
        return None;
    }
    let len = mi.szDevice.iter().position(|&c| c == 0).unwrap_or(mi.szDevice.len());
    Some(Screen {
        device: String::from_utf16_lossy(&mi.szDevice[..len]),
        bounds: rect(mi.monitorInfo.rcMonitor),
        work: rect(mi.monitorInfo.rcWork),
        primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

unsafe extern "system" fn collect(h: HMONITOR, _hdc: HDC, _rc: *mut RECT, data: LPARAM) -> BOOL {
    let out = unsafe { &mut *(data.0 as *mut Vec<Screen>) };
    if let Some(screen) = describe(h) {
        out.push(screen);
    }
    BOOL(1)
}

/// Every attached monitor, left to right and then top to bottom, so screen
/// numbers in the settings follow how the desktop is laid out.
pub fn all() -> Vec<Screen> {
    let mut out: Vec<Screen> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut out as *mut _ as isize));
    }
    out.sort_by_key(|s| (s.bounds.0, s.bounds.1));
    out
}

/// The screens that should carry a bar right now.
pub fn chosen(want: &Monitors) -> Vec<Screen> {
    let screens = all();
    crate::monitors::chosen(want, &screens).into_iter().cloned().collect()
}
