//! Everything Windows-specific: process model, UI, shell integration.

mod app;
mod appdialog;
mod appearancewin;
mod arrangewin;
mod autostart;
mod canvas;
mod catalog;
mod flyout;
mod icons;
mod indicator;
mod launch;
mod manager;
mod menu;
mod panel;
mod paths;
mod searchwin;
mod strip;
mod supervisor;
mod theme;
mod ui;
mod watch;

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, FindWindowW, PostMessageW};
use windows::core::PCWSTR;

pub const MAIN_CLASS: &str = "FlexTaskbar.Main";

/// Exit code of a deliberate, clean exit. Anything else — including 0, which a
/// killed process can also report — tells the supervisor to restart the app.
pub const EXIT_CLEAN: i32 = 0x464C_4558; // "FLEX"

/// What a second launch (or a CLI flag) asks the running instance to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Command {
    Manage = 0,
    Search = 1,
    Menu = 2,
    Exit = 3,
}

impl Command {
    pub fn from_usize(v: usize) -> Option<Command> {
        match v {
            0 => Some(Command::Manage),
            1 => Some(Command::Search),
            2 => Some(Command::Menu),
            3 => Some(Command::Exit),
            _ => None,
        }
    }
}

pub struct Args {
    pub worker: bool,
    pub no_supervisor: bool,
    pub restarted: bool,
    pub reset: bool,
    pub command: Option<Command>,
}

fn parse_args() -> Args {
    let mut a = Args { worker: false, no_supervisor: false, restarted: false, reset: false, command: None };
    for arg in std::env::args().skip(1) {
        match arg.to_ascii_lowercase().as_str() {
            "--worker" => a.worker = true,
            "--no-supervisor" => a.no_supervisor = true,
            "--restarted" => a.restarted = true,
            "--reset" => a.reset = true,
            "--manage" | "--settings" => a.command = Some(Command::Manage),
            "--search" => a.command = Some(Command::Search),
            "--menu" => a.command = Some(Command::Menu),
            "--exit" => a.command = Some(Command::Exit),
            _ => {}
        }
    }
    a
}

/// Process entry. Without `--worker` this process is the supervisor, which
/// starts the real app as a child process and restarts it if it crashes or hangs.
pub fn run() -> i32 {
    let args = parse_args();

    if args.worker || args.no_supervisor {
        // The worker owns its own mutex too, so `--no-supervisor` can't start a
        // second copy next to a supervised one.
        if !acquire_mutex("Local\\FlexTaskbar.Worker") {
            signal_running(args.command.unwrap_or(Command::Manage));
            return EXIT_CLEAN;
        }
        return app::run(&args);
    }

    // A running instance (supervised or not) takes the command.
    if find_main_window().is_some() {
        signal_running(args.command.unwrap_or(Command::Manage));
        return 0;
    }
    if !acquire_mutex("Local\\FlexTaskbar.Supervisor") {
        signal_running(args.command.unwrap_or(Command::Manage));
        return 0;
    }
    if args.command == Some(Command::Exit) {
        return 0; // nothing running, nothing to exit
    }
    supervisor::run(&args)
}

/// Creates a named mutex that lives as long as the process. Returns false if
/// another process already holds it.
fn acquire_mutex(name: &str) -> bool {
    let wide = ui::wide(name);
    match unsafe { CreateMutexW(None, false, PCWSTR(wide.as_ptr())) } {
        Ok(handle) => {
            let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            // Intentionally leaked: the OS releases it when the process exits.
            let _: HANDLE = handle;
            !existed
        }
        Err(_) => true, // can't tell; don't block startup
    }
}

fn find_main_window() -> Option<windows::Win32::Foundation::HWND> {
    let class = ui::wide(MAIN_CLASS);
    unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.ok().filter(|h| !h.is_invalid())
}

/// Forwards a command to the running instance's main window. Retries briefly in
/// case that instance is still starting up (or being restarted).
fn signal_running(cmd: Command) {
    for _ in 0..30 {
        if let Some(hwnd) = find_main_window() {
            unsafe {
                // Let the running instance bring its window to the foreground.
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let _ = PostMessageW(Some(hwnd), app::WM_APP_COMMAND, WPARAM(cmd as usize), LPARAM(0));
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
