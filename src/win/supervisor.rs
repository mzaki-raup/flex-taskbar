//! Crash and hang recovery. The supervisor is a tiny process with no UI: it runs
//! the app as a child (`--worker`), and if the child dies with a non-zero exit
//! code or stops responding, it logs the event and starts it again.
//!
//! A crash loop (too many crashes in a short window) stops the restarts so a
//! broken install can't spin forever.

use super::{Args, Command, MAIN_CLASS, paths, ui};
use std::collections::VecDeque;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::process::Child;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HANDLE, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetSystemMetrics, MB_ICONERROR, MB_OK, MessageBoxW, SM_SHUTTINGDOWN, SMTO_ABORTIFHUNG,
    SendMessageTimeoutW, WM_NULL,
};
use windows::core::PCWSTR;

const CRASH_WINDOW: Duration = Duration::from_secs(120);
const MAX_CRASHES_IN_WINDOW: usize = 5;
const POLL: Duration = Duration::from_secs(5);
/// Consecutive unresponsive polls before the worker is considered hung (~40 s).
const HUNG_POLLS: u32 = 8;

pub fn run(args: &Args) -> i32 {
    let mut crashes: VecDeque<Instant> = VecDeque::new();
    let mut first = true;

    loop {
        let mut cmd = std::process::Command::new(&paths::get().exe);
        cmd.arg("--worker");
        if first {
            if args.reset {
                cmd.arg("--reset");
            }
            match args.command {
                Some(Command::Search) => {
                    cmd.arg("--search");
                }
                Some(Command::Menu) => {
                    cmd.arg("--menu");
                }
                Some(Command::Manage) => {
                    cmd.arg("--manage");
                }
                Some(Command::Bar) => {
                    cmd.arg("--bar");
                }
                _ => {}
            }
        } else {
            cmd.arg("--restarted");
        }
        first = false;

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                log(&format!("could not start worker: {e}"));
                return 1;
            }
        };

        let reason = watch(&mut child);
        let code = match reason {
            Exit::Code(code) => code,
            Exit::Hung => {
                let _ = child.kill();
                let _ = child.wait();
                log("worker stopped responding and was restarted");
                0xDEAD
            }
        };

        if code == super::EXIT_CLEAN as u32 {
            return 0; // user chose Exit
        }
        if unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) } != 0 {
            return 0; // logoff/shutdown killed it; don't fight the OS
        }
        if !matches!(reason, Exit::Hung) {
            log(&format!("worker exited with code {code:#X}; restarting"));
        }

        let now = Instant::now();
        crashes.push_back(now);
        while crashes.front().is_some_and(|t| now.duration_since(*t) > CRASH_WINDOW) {
            crashes.pop_front();
        }
        if crashes.len() >= MAX_CRASHES_IN_WINDOW {
            log("crash loop detected; giving up");
            let text = ui::wide(&format!(
                "FlexTaskbar crashed {MAX_CRASHES_IN_WINDOW} times within two minutes, so it was stopped.\n\n\
                 Details are in:\n{}\n\nYou can start it again normally, or with --reset to start from a clean configuration \
                 (the current one is kept as a backup).",
                paths::get().crash_log().display()
            ));
            let title = ui::wide("FlexTaskbar");
            unsafe {
                MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR);
            }
            return 1;
        }

        // Back off a little on repeated crashes: 0.5 s, 1 s, 2 s, 4 s.
        let delay = Duration::from_millis(500 << (crashes.len() - 1).min(3));
        std::thread::sleep(delay);
    }
}

#[derive(Clone, Copy)]
enum Exit {
    Code(u32),
    Hung,
}

fn watch(child: &mut Child) -> Exit {
    let handle = HANDLE(child.as_raw_handle());
    let class = ui::wide(MAIN_CLASS);
    let mut unresponsive = 0;
    loop {
        let waited = unsafe { WaitForSingleObject(handle, POLL.as_millis() as u32) };
        if waited == WAIT_OBJECT_0 {
            let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1) as u32;
            return Exit::Code(code);
        }

        // Still running: make sure its UI thread is still pumping messages.
        let responsive = unsafe {
            match FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
                Ok(hwnd) if !hwnd.is_invalid() => {
                    SendMessageTimeoutW(hwnd, WM_NULL, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 5000, None).0 != 0
                }
                _ => true, // window not created yet (startup) — not evidence of a hang
            }
        };
        unresponsive = if responsive { 0 } else { unresponsive + 1 };
        if unresponsive >= HUNG_POLLS {
            return Exit::Hung;
        }
    }
}

pub fn log(message: &str) {
    let path = paths::get().crash_log();
    // Keep the log from growing without bound.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {message}", timestamp());
    }
}

fn timestamp() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}
