//! Light/dark theme support.
//!
//! Popup menus follow the system app theme through uxtheme's
//! `SetPreferredAppMode` (ordinal 135). It is undocumented but present on every
//! Windows 10 1903+ and Windows 11 build, and is what Explorer and Notepad use;
//! if the lookup fails the menus simply stay light.

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::{PCSTR, w};

pub fn is_dark() -> bool {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    ok.is_ok() && value == 0
}

thread_local! {
    static DARK: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// [`is_dark`], read once and then cached until [`forget_cached`].
pub fn is_dark_cached() -> bool {
    DARK.with(|d| {
        if let Some(v) = d.get() {
            return v;
        }
        let v = is_dark();
        d.set(Some(v));
        v
    })
}

/// The system theme changed.
pub fn forget_cached() {
    DARK.with(|d| d.set(None));
}

/// Makes popup menus follow the system theme. Call once at startup and again
/// when the theme changes.
pub fn allow_dark_menus() {
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        // SetPreferredAppMode(1 = AllowDark), then FlushMenuThemes.
        if let Some(f) = GetProcAddress(lib, PCSTR(135 as *const u8)) {
            let set_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_mode(1);
        }
        if let Some(f) = GetProcAddress(lib, PCSTR(136 as *const u8)) {
            let flush: extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

/// Lets menus owned by `hwnd` use the dark theme (uxtheme ordinal 133,
/// AllowDarkModeForWindow). Harmless if unavailable.
pub fn allow_dark_for_window(hwnd: HWND) {
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(f) = GetProcAddress(lib, PCSTR(133 as *const u8)) {
            let allow: extern "system" fn(HWND, i32) -> i32 = std::mem::transmute(f);
            allow(hwnd, 1);
        }
    }
}

pub fn style_window(hwnd: HWND, dark: bool, rounded: bool) {
    unsafe {
        let v: i32 = dark as i32;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const _ as *const _, 4);
        if rounded {
            let pref = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as *const _, 4);
        }
    }
}

pub struct Palette {
    pub window: COLORREF,
    pub field: COLORREF,
    pub text: COLORREF,
    pub subtle: COLORREF,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            window: rgb(32, 32, 32),
            field: rgb(45, 45, 45),
            text: rgb(240, 240, 240),
            subtle: rgb(160, 160, 160),
        }
    } else {
        Palette {
            window: rgb(249, 249, 249),
            field: rgb(255, 255, 255),
            text: rgb(26, 26, 26),
            subtle: rgb(96, 96, 96),
        }
    }
}

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}
