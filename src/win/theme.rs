//! Light/dark theme support.
//!
//! Popup menus follow the system app theme through uxtheme's
//! `SetPreferredAppMode` (ordinal 135). It is undocumented but present on every
//! Windows 10 1903+ and Windows 11 build, and is what Explorer and Notepad use;
//! if the lookup fails the menus simply stay light.
//!
//! The Appearance setting can force dark or light instead of following
//! Windows; [`set_mode`] applies it to menus and [`app_dark`] tells the
//! app's own windows which palette to use.

use crate::appearance::ThemeMode;

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
    static MODE: std::cell::Cell<ThemeMode> = const { std::cell::Cell::new(ThemeMode::Dark) };
}

/// Sets the theme setting and re-themes popup menus to match.
pub fn set_mode(mode: ThemeMode) {
    MODE.with(|m| m.set(mode));
    allow_dark_menus();
}

/// Whether the app's windows and menus are dark under the current setting.
pub fn app_dark() -> bool {
    match MODE.with(|m| m.get()) {
        ThemeMode::System => is_dark_cached(),
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
    }
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

/// The system theme, contrast or animation setting changed.
pub fn forget_cached() {
    DARK.with(|d| d.set(None));
    HIGH_CONTRAST.with(|c| c.set(None));
    ANIMATIONS.with(|c| c.set(None));
}

/// Makes popup menus follow the theme setting. Call once at startup and again
/// when the system theme changes.
pub fn allow_dark_menus() {
    // SetPreferredAppMode: 1 = follow Windows, 2 = force dark, 3 = force light.
    let mode = match MODE.with(|m| m.get()) {
        ThemeMode::System => 1,
        ThemeMode::Dark => 2,
        ThemeMode::Light => 3,
    };
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        // SetPreferredAppMode, then FlushMenuThemes.
        if let Some(f) = GetProcAddress(lib, PCSTR(135 as *const u8)) {
            let set_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_mode(mode);
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

thread_local! {
    /// [`high_contrast`] and [`animations_on`], read once until [`forget_cached`].
    static HIGH_CONTRAST: std::cell::Cell<Option<Option<crate::appearance::SystemColors>>> =
        const { std::cell::Cell::new(None) };
    static ANIMATIONS: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Windows' high-contrast colours while a high-contrast theme is on.
pub fn high_contrast() -> Option<crate::appearance::SystemColors> {
    HIGH_CONTRAST.with(|c| {
        if let Some(v) = c.get() {
            return v;
        }
        let v = read_high_contrast();
        c.set(Some(v));
        v
    })
}

fn read_high_contrast() -> Option<crate::appearance::SystemColors> {
    use windows::Win32::Graphics::Gdi::{
        COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
    };
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut hc = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some(&mut hc as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_err() || hc.dwFlags & HCF_HIGHCONTRASTON != HCF_HIGHCONTRASTON {
        return None;
    }
    let sys = |i| {
        let c = unsafe { GetSysColor(i) };
        crate::appearance::Rgba::rgb((c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8)
    };
    Some(crate::appearance::SystemColors {
        window: sys(COLOR_WINDOW),
        text: sys(COLOR_WINDOWTEXT),
        highlight: sys(COLOR_HIGHLIGHT),
        highlight_text: sys(COLOR_HIGHLIGHTTEXT),
        gray_text: sys(COLOR_GRAYTEXT),
    })
}

/// Windows' *Animation effects* setting (Accessibility › Visual effects).
pub fn animations_on() -> bool {
    ANIMATIONS.with(|c| {
        if let Some(v) = c.get() {
            return v;
        }
        let v = read_animations();
        c.set(Some(v));
        v
    })
}

fn read_animations() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut on = windows::core::BOOL(1);
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    // Unknown (an older Windows, or Wine): animate, as before.
    ok.is_err() || on.as_bool()
}

pub struct Palette {
    pub window: COLORREF,
    pub field: COLORREF,
    pub text: COLORREF,
    pub subtle: COLORREF,
}

/// Whether FlexTaskbar's own windows (search, settings) are dark: the
/// theme setting, except under a high-contrast theme, whose colours win.
pub fn windows_dark() -> bool {
    high_contrast().is_none() && app_dark()
}

/// The search window's colours: light, dark, or the high-contrast theme's.
pub fn palette(dark: bool) -> Palette {
    if let Some(sc) = high_contrast() {
        let c = |c: crate::appearance::Rgba| rgb(c.r, c.g, c.b);
        return Palette { window: c(sc.window), field: c(sc.window), text: c(sc.text), subtle: c(sc.gray_text) };
    }
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
