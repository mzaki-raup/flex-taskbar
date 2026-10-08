#![cfg_attr(not(windows), allow(dead_code))]

//! Apps installed by package managers that don't (always) add a Start Menu
//! entry: winget's portable packages, Scoop, Chocolatey, npm, pip, pipx,
//! Cargo, .NET tools and Go. Each keeps its programs (or launchers for them)
//! in a known folder; this module knows those folders, which files in them
//! are worth listing, and which manager a path belongs to. Pure, so it can be
//! unit-tested off Windows.
//!
//! Apps these managers install with a normal installer (most winget,
//! Chocolatey and UniGetUI packages) add a Start Menu shortcut, so they are
//! already in the Apps folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Manager {
    Winget,
    Scoop,
    Chocolatey,
    Npm,
    Pip,
    Pipx,
    Cargo,
    Dotnet,
    Go,
}

impl Manager {
    pub fn label(self) -> &'static str {
        match self {
            Manager::Winget => "winget",
            Manager::Scoop => "Scoop",
            Manager::Chocolatey => "Chocolatey",
            Manager::Npm => "npm",
            Manager::Pip => "pip",
            Manager::Pipx => "pipx",
            Manager::Cargo => "Cargo",
            Manager::Dotnet => ".NET tool",
            Manager::Go => "Go",
        }
    }
}

/// The folders to look in, from environment variables (`env("APPDATA")`…)
/// and, for Python, the version folders that exist under its roots
/// (`subdirs(root)` lists a folder's subfolders).
pub fn tool_dirs(
    env: impl Fn(&str) -> Option<String>,
    subdirs: impl Fn(&Path) -> Vec<PathBuf>,
) -> Vec<(Manager, PathBuf)> {
    // Windows paths, joined with '\\' whatever the host (the tests run on Linux too).
    let p = |v: &str| env(v).filter(|s| !s.is_empty()).map(PathBuf::from);
    let j = |base: &Path, sub: &str| PathBuf::from(format!("{}\\{sub}", base.to_string_lossy().trim_end_matches('\\')));
    let profile = p("USERPROFILE");
    let mut out = Vec::new();
    let mut add = |m: Manager, dir: Option<PathBuf>| {
        if let Some(d) = dir
            && !out.iter().any(|(_, e)| *e == d)
        {
            out.push((m, d));
        }
    };
    // winget portable packages: per user and machine-wide.
    add(Manager::Winget, p("LOCALAPPDATA").map(|d| j(&d, "Microsoft\\WinGet\\Links")));
    add(Manager::Winget, p("ProgramFiles").map(|d| j(&d, "WinGet\\Links")));
    // Scoop: its shims (one per app command), per user and global.
    add(Manager::Scoop, p("SCOOP").or(profile.as_ref().map(|d| j(d, "scoop"))).map(|d| j(&d, "shims")));
    add(Manager::Scoop, p("SCOOP_GLOBAL").or(p("ProgramData").map(|d| j(&d, "scoop"))).map(|d| j(&d, "shims")));
    // Chocolatey: its shims for portable packages.
    add(
        Manager::Chocolatey,
        p("ChocolateyInstall").or(p("ProgramData").map(|d| j(&d, "chocolatey"))).map(|d| j(&d, "bin")),
    );
    // npm global packages.
    add(Manager::Npm, p("APPDATA").map(|d| j(&d, "npm")));
    // pip --user and per-user Python installs: every PythonXY\Scripts.
    for root in [p("APPDATA").map(|d| j(&d, "Python")), p("LOCALAPPDATA").map(|d| j(&d, "Programs\\Python"))]
        .into_iter()
        .flatten()
    {
        for version in subdirs(&root) {
            let name = version.to_string_lossy().rsplit(['\\', '/']).next().unwrap_or_default().to_ascii_lowercase();
            if name.starts_with("python") {
                add(Manager::Pip, Some(j(&version, "Scripts")));
            }
        }
    }
    add(Manager::Pipx, p("PIPX_BIN_DIR").or(profile.as_ref().map(|d| j(d, ".local\\bin"))));
    add(Manager::Cargo, p("CARGO_HOME").or(profile.as_ref().map(|d| j(d, ".cargo"))).map(|d| j(&d, "bin")));
    add(Manager::Dotnet, profile.as_ref().map(|d| j(d, ".dotnet\\tools")));
    add(Manager::Go, p("GOBIN").or(p("GOPATH").or(profile.as_ref().map(|d| j(d, "go"))).map(|d| j(&d, "bin"))));
    out
}

/// Whether a file in a manager's folder is an app worth listing: programs
/// (`.exe`; npm's launchers are `.cmd`), not the package managers' own
/// plumbing.
pub fn wanted(manager: Manager, file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let Some((stem, ext)) = lower.rsplit_once('.') else { return false };
    let ok_ext = match manager {
        Manager::Npm => ext == "cmd",
        _ => ext == "exe",
    };
    if !ok_ext || stem.is_empty() {
        return false;
    }
    let plumbing: &[&str] = match manager {
        Manager::Winget => &[],
        Manager::Scoop => &["scoop"],
        Manager::Chocolatey => {
            &["choco", "chocolatey", "cinst", "clist", "cpush", "cuninst", "cup", "refreshenv", "shimgen"]
        }
        Manager::Npm => &["npm", "npx", "corepack", "pnpm", "pnpx", "yarn", "yarnpkg"],
        Manager::Pip => &["pip", "pip3", "wheel", "easy_install", "python", "pythonw"],
        Manager::Pipx => &["pipx"],
        Manager::Cargo => &[
            "cargo",
            "rustc",
            "rustup",
            "rustdoc",
            "rust-gdb",
            "rust-lldb",
            "rustfmt",
            "cargo-fmt",
            "cargo-clippy",
            "clippy-driver",
            "rust-analyzer",
        ],
        Manager::Dotnet => &[],
        Manager::Go => &[],
    };
    // pip3.12, easy_install-3.12…
    let base = stem.split(['-', '.']).next().unwrap_or(stem);
    !plumbing.contains(&stem) && !plumbing.contains(&base) && !stem.starts_with("uninstall")
}

/// Which manager's folder a path is in, if any.
pub fn manager_of(path: &str) -> Option<Manager> {
    let p = path.to_ascii_lowercase().replace('/', "\\");
    let has = |s: &str| p.contains(s);
    if has("\\winget\\links\\") {
        Some(Manager::Winget)
    } else if has("\\scoop\\shims\\") || has("\\scoop\\apps\\") {
        Some(Manager::Scoop)
    } else if has("\\chocolatey\\bin\\") {
        Some(Manager::Chocolatey)
    } else if has("\\appdata\\roaming\\npm\\") {
        Some(Manager::Npm)
    } else if has("\\python") && has("\\scripts\\") {
        Some(Manager::Pip)
    } else if has("\\.local\\bin\\") {
        Some(Manager::Pipx)
    } else if has("\\.cargo\\bin\\") {
        Some(Manager::Cargo)
    } else if has("\\.dotnet\\tools\\") {
        Some(Manager::Dotnet)
    } else if has("\\go\\bin\\") {
        Some(Manager::Go)
    } else {
        None
    }
}

/// The name to show for a program: its file name without the extension.
pub fn display_name(file_name: &str) -> String {
    file_name.rsplit_once('.').map(|(s, _)| s).unwrap_or(file_name).to_string()
}

/// Whether a Windows executable's header says it is a console program
/// (`Some(true)`), a GUI one (`Some(false)`), or isn't readable (`None`).
/// Needs the first ~0x200 bytes of the file.
pub fn pe_is_console(head: &[u8]) -> Option<bool> {
    let u16_at = |o: usize| head.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| head.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if head.get(0..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(0x3C)? as usize;
    if head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    // COFF header (20 bytes), then the optional header: Subsystem at +68.
    let subsystem = u16_at(pe + 4 + 20 + 68)?;
    match subsystem {
        3 => Some(true),  // IMAGE_SUBSYSTEM_WINDOWS_CUI
        2 => Some(false), // IMAGE_SUBSYSTEM_WINDOWS_GUI
        _ => None,
    }
}

/// How long to wait after the last change before rescanning: installers
/// write many files, and a rescan should see the finished result.
pub const SETTLE_MS: u32 = 4000;

/// The `cmd.exe` arguments that run the program at `path` in a console
/// that stays open afterwards, or `None` when `path` can't be handed to cmd
/// safely.
///
/// `/s` makes cmd strip only the outer pair of quotes, so the path stays
/// quoted and characters such as `&`, `|` or `^` (allowed in file names) are
/// taken literally instead of starting another command. `%` and `!` would
/// still be expanded inside quotes, so such paths are refused (the caller
/// then starts the program directly).
pub fn console_args(path: &str) -> Option<String> {
    if path.is_empty() || path.contains(['%', '!', '"']) || path.chars().any(char::is_control) {
        return None;
    }
    Some(format!("/v:off /s /k \"\"{path}\"\""))
}

#[cfg(test)]
mod tests {
    #[test]
    fn console_arguments_keep_the_path_quoted() {
        assert_eq!(
            console_args(r"C:\Users\me\AppData\Roaming\npm\tsc.cmd").as_deref(),
            Some(r#"/v:off /s /k ""C:\Users\me\AppData\Roaming\npm\tsc.cmd"""#)
        );
        // An & in a file name stays inside the quotes.
        assert!(console_args(r"C:\tools\a&calc.exe").unwrap().ends_with(r#"""C:\tools\a&calc.exe"""#));
        // cmd would expand these even in quotes.
        for bad in [r"C:\t\%PATH%.exe", r"C:\t\a!b!.exe", "", "C:\\t\\a\nb.exe"] {
            assert_eq!(console_args(bad), None, "{bad:?}");
        }
    }

    use super::*;
    use std::collections::HashMap;

    fn env(map: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = map.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn folders_follow_the_environment() {
        let e = env(&[
            ("USERPROFILE", "C:\\Users\\me"),
            ("LOCALAPPDATA", "C:\\Users\\me\\AppData\\Local"),
            ("APPDATA", "C:\\Users\\me\\AppData\\Roaming"),
            ("ProgramData", "C:\\ProgramData"),
            ("ProgramFiles", "C:\\Program Files"),
        ]);
        let py = |root: &Path| {
            let r = root.to_string_lossy();
            if r == "C:\\Users\\me\\AppData\\Roaming\\Python" {
                vec![PathBuf::from(format!("{r}\\Python312")), PathBuf::from(format!("{r}\\Cache"))]
            } else {
                vec![]
            }
        };
        let dirs = tool_dirs(e, py);
        let has = |m: Manager, d: &str| dirs.iter().any(|(mm, p)| *mm == m && p == Path::new(d));
        assert!(has(Manager::Winget, "C:\\Users\\me\\AppData\\Local\\Microsoft\\WinGet\\Links"));
        assert!(has(Manager::Winget, "C:\\Program Files\\WinGet\\Links"));
        assert!(has(Manager::Scoop, "C:\\Users\\me\\scoop\\shims"));
        assert!(has(Manager::Scoop, "C:\\ProgramData\\scoop\\shims"));
        assert!(has(Manager::Chocolatey, "C:\\ProgramData\\chocolatey\\bin"));
        assert!(has(Manager::Npm, "C:\\Users\\me\\AppData\\Roaming\\npm"));
        assert!(has(Manager::Pip, "C:\\Users\\me\\AppData\\Roaming\\Python\\Python312\\Scripts"));
        assert!(!dirs.iter().any(|(_, p)| p.to_string_lossy().contains("Cache")));
        assert!(has(Manager::Pipx, "C:\\Users\\me\\.local\\bin"));
        assert!(has(Manager::Cargo, "C:\\Users\\me\\.cargo\\bin"));
        assert!(has(Manager::Dotnet, "C:\\Users\\me\\.dotnet\\tools"));
        assert!(has(Manager::Go, "C:\\Users\\me\\go\\bin"));
        // Custom locations win.
        let dirs = tool_dirs(env(&[("ChocolateyInstall", "D:\\choco"), ("SCOOP", "D:\\scoop")]), |_| vec![]);
        assert!(dirs.contains(&(Manager::Chocolatey, PathBuf::from("D:\\choco\\bin"))));
        assert!(dirs.contains(&(Manager::Scoop, PathBuf::from("D:\\scoop\\shims"))));
    }

    #[test]
    fn which_files_are_apps() {
        assert!(wanted(Manager::Winget, "yt-dlp.exe"));
        assert!(wanted(Manager::Scoop, "firefox.exe"));
        assert!(!wanted(Manager::Scoop, "scoop.cmd"));
        assert!(!wanted(Manager::Scoop, "firefox.shim"));
        assert!(wanted(Manager::Chocolatey, "7z.exe"));
        assert!(!wanted(Manager::Chocolatey, "choco.exe"));
        assert!(!wanted(Manager::Chocolatey, "RefreshEnv.cmd"));
        assert!(wanted(Manager::Npm, "tsc.cmd"));
        assert!(!wanted(Manager::Npm, "tsc"));
        assert!(!wanted(Manager::Npm, "tsc.ps1"));
        assert!(!wanted(Manager::Npm, "npm.cmd"));
        assert!(wanted(Manager::Pip, "black.exe"));
        assert!(!wanted(Manager::Pip, "pip3.12.exe"));
        assert!(!wanted(Manager::Pip, "easy_install-3.12.exe"));
        assert!(!wanted(Manager::Cargo, "cargo.exe"));
        assert!(wanted(Manager::Cargo, "ripgrep.exe"));
        assert!(!wanted(Manager::Winget, "uninstall.exe"));
        assert_eq!(display_name("yt-dlp.exe"), "yt-dlp");
    }

    #[test]
    fn paths_to_managers() {
        assert_eq!(
            manager_of("C:\\Users\\me\\AppData\\Local\\Microsoft\\WinGet\\Links\\yt-dlp.exe"),
            Some(Manager::Winget)
        );
        assert_eq!(manager_of("C:\\Users\\me\\scoop\\shims\\firefox.exe"), Some(Manager::Scoop));
        assert_eq!(manager_of("C:\\ProgramData\\chocolatey\\bin\\7z.exe"), Some(Manager::Chocolatey));
        assert_eq!(manager_of("C:\\Users\\me\\AppData\\Roaming\\npm\\tsc.cmd"), Some(Manager::Npm));
        assert_eq!(
            manager_of("C:\\Users\\me\\AppData\\Roaming\\Python\\Python312\\Scripts\\black.exe"),
            Some(Manager::Pip)
        );
        assert_eq!(manager_of("C:\\Users\\me\\.cargo\\bin\\rg.exe"), Some(Manager::Cargo));
        assert_eq!(manager_of("C:\\Program Files\\VideoLAN\\VLC\\vlc.exe"), None);
        assert_eq!(Manager::Dotnet.label(), ".NET tool");
    }

    fn pe(subsystem: u16) -> Vec<u8> {
        let mut b = vec![0u8; 0x200];
        b[0] = b'M';
        b[1] = b'Z';
        b[0x3C] = 0x80;
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let o = 0x80 + 4 + 20 + 68;
        b[o..o + 2].copy_from_slice(&subsystem.to_le_bytes());
        b
    }

    #[test]
    fn console_programs() {
        assert_eq!(pe_is_console(&pe(3)), Some(true));
        assert_eq!(pe_is_console(&pe(2)), Some(false));
        assert_eq!(pe_is_console(&pe(9)), None);
        assert_eq!(pe_is_console(b"not a program"), None);
        assert_eq!(pe_is_console(&pe(3)[..0x90]), None); // cut short
    }
}
