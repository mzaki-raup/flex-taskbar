//! Launching. Everything goes through ShellExecuteEx with explicit file /
//! parameters / directory fields. The one use of cmd.exe is to keep a package
//! manager's command-line tool open in a console (`cmd /s /k ""<path>""`, with
//! a path found on disk, never user-typed text, kept quoted; see
//! `pkgsources::console_args`).
//!
//! Launches run on a short-lived background thread so a slow shell handler can
//! never freeze the launcher's UI.

use super::ui::wide;
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoTaskMemFree};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::UI::Shell::{
    IShellItem, SEE_MASK_FLAG_NO_UI, SEE_MASK_IDLIST, SEE_MASK_NOASYNC, SHCreateItemFromParsingName, SHELLEXECUTEINFOW,
    SHGetIDListFromObject, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

#[derive(Clone, Debug)]
pub enum Target {
    /// Parsing name inside shell:AppsFolder.
    Shell(String),
    Custom {
        target: String,
        args: String,
        dir: String,
        admin: bool,
    },
}

/// Starts the launch in the background; `on_error` gets a readable message if it
/// fails (but not when the user just cancels a UAC prompt).
pub fn spawn(target: Target, on_error: impl FnOnce(String) + Send + 'static) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        }
        if let Err(msg) = launch(&target) {
            on_error(msg);
        }
    });
}

fn launch(target: &Target) -> Result<(), String> {
    match target {
        Target::Shell(parsing_name) => launch_shell(parsing_name),
        Target::Custom { target, args, dir, admin } => launch_custom(target, args, dir, *admin),
    }
}

fn launch_shell(parsing_name: &str) -> Result<(), String> {
    let path = format!("shell:AppsFolder\\{parsing_name}");
    let path_w = wide(&path);
    unsafe {
        // Invoking the item's own ID list runs it exactly like Start does.
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(path_w.as_ptr()), None)
            && let Ok(pidl) = SHGetIDListFromObject(&item)
        {
            let mut sei = base_info();
            sei.fMask |= SEE_MASK_IDLIST;
            sei.lpIDList = pidl as *mut _;
            let ok = ShellExecuteExW(&mut sei);
            CoTaskMemFree(Some(pidl as *const _));
            if ok.is_ok() {
                return Ok(());
            }
        }
        // Fallback: let the shell parse the path itself.
        let mut sei = base_info();
        sei.lpFile = PCWSTR(path_w.as_ptr());
        execute(&mut sei, &path)
    }
}

fn launch_custom(target: &str, args: &str, dir: &str, admin: bool) -> Result<(), String> {
    let target = expand(target.trim());
    if target.is_empty() {
        return Err("This app has no target to launch.".into());
    }
    let mut dir = expand(dir.trim());
    if dir.is_empty() {
        // Like a shortcut's "Start in": the target's own folder, when it's a file.
        let p = std::path::Path::new(&target);
        if p.is_file() {
            dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
        }
    }
    let file_w = wide(&target);
    let args_w = wide(args.trim());
    let dir_w = wide(&dir);
    let verb_w = wide("runas");
    let mut sei = base_info();
    sei.lpFile = PCWSTR(file_w.as_ptr());
    if !args.trim().is_empty() {
        sei.lpParameters = PCWSTR(args_w.as_ptr());
    }
    if !dir.is_empty() {
        sei.lpDirectory = PCWSTR(dir_w.as_ptr());
    }
    if admin {
        sei.lpVerb = PCWSTR(verb_w.as_ptr());
    }
    unsafe { execute(&mut sei, &target) }
}

fn base_info() -> SHELLEXECUTEINFOW {
    SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        hwnd: HWND::default(),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    }
}

unsafe fn execute(sei: &mut SHELLEXECUTEINFOW, what: &str) -> Result<(), String> {
    match unsafe { ShellExecuteExW(sei) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => Ok(()),
        Err(e) => Err(format!("Couldn't start {what}\n\n{}", e.message())),
    }
}

fn expand(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let src = wide(s);
    unsafe {
        let needed = ExpandEnvironmentStringsW(PCWSTR(src.as_ptr()), None);
        if needed == 0 {
            return s.to_string();
        }
        let mut buf = vec![0u16; needed as usize];
        let n = ExpandEnvironmentStringsW(PCWSTR(src.as_ptr()), Some(&mut buf));
        if n == 0 { s.to_string() } else { super::ui::from_wide(&buf) }
    }
}
