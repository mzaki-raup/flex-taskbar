//! Portable data location: a `data\` folder next to the executable. Only if that
//! folder can't be written (e.g. the exe sits in Program Files) does the app fall
//! back to `%LOCALAPPDATA%\FlexTaskbar`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub struct Paths {
    pub exe: PathBuf,
    pub data: PathBuf,
    pub icons: PathBuf,
    pub portable: bool,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

pub fn get() -> &'static Paths {
    PATHS.get_or_init(resolve)
}

fn resolve() -> Paths {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("FlexTaskbar.exe"));
    let exe_dir = exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));

    let portable_dir = exe_dir.join("data");
    let (data, portable) = if writable(&portable_dir) {
        (portable_dir, true)
    } else {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join("FlexTaskbar");
        let _ = std::fs::create_dir_all(&dir);
        (dir, false)
    };
    let icons = data.join("icons");
    let _ = std::fs::create_dir_all(&icons);
    Paths { exe, data, icons, portable }
}

fn writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-test");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

impl Paths {
    pub fn crash_log(&self) -> PathBuf {
        self.data.join("crash.log")
    }

    pub fn apps_cache(&self) -> PathBuf {
        self.data.join("apps-cache.json")
    }
}
