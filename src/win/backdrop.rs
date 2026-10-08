//! Windows 11's blurred backdrop (Mica and Acrylic) behind a window.
//!
//! A backdrop is drawn by DWM *behind* the window, and the window is painted
//! over it. That only works for an ordinary composited window: a per-pixel
//! alpha layered window — which is what the bar is by default, and what gives
//! it its plain translucency — has no DWM surface to put a backdrop behind,
//! and asking for one there replaces what is behind the window instead of
//! blurring it. So a bar with a backdrop is built as a plain window and
//! painted in `WM_PAINT` (see `strip`), and one without stays layered.
//!
//! Three attributes make it work, all documented:
//!
//! - `DWMWA_SYSTEMBACKDROP_TYPE` picks Mica or Acrylic. It arrived in Windows
//!   11 22H2; earlier Windows returns an error, which is how [`available`]
//!   tells whether to offer it at all.
//! - `DwmExtendFrameIntoClientArea` with margins of −1 makes the whole client
//!   area glass, so the backdrop reaches every pixel rather than just a frame.
//! - `DWMWA_WINDOW_CORNER_PREFERENCE` rounds the window itself. With a
//!   backdrop the corners have to be the window's, not drawn into the picture:
//!   anything painted outside them would be clipped away.
//!
//! The backdrop keeps being drawn while the window is inactive, which matters
//! because the bar never takes the focus.

use crate::appearance::Backdrop;
use std::sync::OnceLock;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DWMSBT_NONE, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DEFAULT, DWMWCP_ROUND, DWMWCP_ROUNDSMALL, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::UI::Controls::MARGINS;

/// Whether this Windows can draw a backdrop at all. Asked of the system once,
/// using the app's own main window: `DWMWA_SYSTEMBACKDROP_TYPE` is unknown
/// before Windows 11 22H2 and returns an error there.
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let hwnd = super::app::main_hwnd();
        if hwnd.is_invalid() {
            return false;
        }
        // Setting it to "none" changes nothing but says whether it is known.
        let value = DWMSBT_NONE.0 as u32;
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &raw const value as *const _,
                std::mem::size_of::<u32>() as u32,
            )
        }
        .is_ok()
    })
}

/// Puts `backdrop` behind `hwnd`, rounding its corners by `radius` DIPs (0
/// leaves them square). Returns whether it took: a caller that gets `false`
/// should go on drawing its own background.
///
/// `dark` picks which way DWM tints the backdrop; it follows the bar's theme
/// rather than Windows', so a light bar on a dark system still reads.
pub fn apply(hwnd: HWND, backdrop: Backdrop, dark: bool, radius: u32) -> bool {
    if hwnd.is_invalid() || !backdrop.on() || !available() {
        return false;
    }
    unsafe {
        let dark_value = windows::core::BOOL::from(dark);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &raw const dark_value as *const _,
            std::mem::size_of::<windows::core::BOOL>() as u32,
        );

        let kind = backdrop.dwm_value();
        if DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &raw const kind as *const _,
            std::mem::size_of::<u32>() as u32,
        )
        .is_err()
        {
            return false;
        }

        // Every pixel of the client area is glass, so the backdrop fills it.
        let all = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
        if DwmExtendFrameIntoClientArea(hwnd, &all).is_err() {
            return false;
        }

        let corner = match radius {
            0 => DWMWCP_DEFAULT,
            1..=6 => DWMWCP_ROUNDSMALL,
            _ => DWMWCP_ROUND,
        }
        .0 as u32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &raw const corner as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
    true
}
