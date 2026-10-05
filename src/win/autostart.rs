//! "Start with Windows" via the per-user Run key. This is the only thing the app
//! writes outside its own folder, and only when the user turns it on. Because the
//! app is portable, the entry counts as "on" only if it points at *this* exe — a
//! copy that was moved shows as off and re-enabling fixes the path.

use super::paths;
use super::ui::{from_wide, wide};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegDeleteValueW,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("FlexTaskbar");

fn command() -> String {
    format!("\"{}\"", paths::get().exe.display())
}

pub fn is_enabled() -> bool {
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    ok.is_ok() && from_wide(&buf).eq_ignore_ascii_case(&command())
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    unsafe {
        let mut key = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_READ | KEY_SET_VALUE, &mut key)
            .ok()
            .map_err(|e| e.message())?;
        let result = if enabled {
            let data = wide(&command());
            let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
            RegSetValueExW(key, VALUE, None, REG_SZ, Some(bytes)).ok()
        } else {
            let r = RegDeleteValueW(key, VALUE);
            if r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND { Ok(()) } else { r.ok() }
        };
        let _ = RegCloseKey(key);
        result.map_err(|e| e.message())
    }
}
