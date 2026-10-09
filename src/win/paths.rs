//! Portable data location: a `FlexTaskbarData\` folder next to the executable.
//! Only if that folder can't be written (e.g. the exe sits in Program Files)
//! does the app fall back to `%LOCALAPPDATA%\FlexTaskbar`.
//!
//! Versions up to 2.2.0 called it `data\`, which says nothing about whose it is
//! when the exe shares a folder with other things. A folder of the old name is
//! moved across once, the first time this version runs (see `adopt_old_name`),
//! so settings, icons and the app cache carry over by themselves.

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

/// The folder beside the exe that everything is kept in.
const DATA_DIR: &str = "FlexTaskbarData";
/// What it was called up to 2.2.0.
const OLD_DATA_DIR: &str = "data";

fn resolve() -> Paths {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("FlexTaskbar.exe"));
    let exe_dir = exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));

    let portable_dir = adopt_old_name(&exe_dir);
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

/// Whether a folder is one of ours rather than something else that happens to
/// be called `data`. The exe can sit in a folder it doesn't own, and taking
/// over a stranger's folder — then *renaming* it — would be rude at best.
fn looks_like_ours(dir: &Path) -> bool {
    dir.join("config.json").is_file() || dir.join("icons").is_dir()
}

/// The folder to keep things in, moving one left by an older version if that
/// is what is there. Done once: afterwards only the new name exists.
fn adopt_old_name(exe_dir: &Path) -> PathBuf {
    let new = exe_dir.join(DATA_DIR);
    if new.exists() {
        return new;
    }
    let old = exe_dir.join(OLD_DATA_DIR);
    if !old.is_dir() || !looks_like_ours(&old) {
        return new;
    }
    match std::fs::rename(&old, &new) {
        Ok(()) => new,
        // The supervisor and the worker start together, so the other process
        // may have just moved it.
        Err(_) if new.is_dir() => new,
        // It wouldn't move — a file held open, or a folder that can't be
        // written. Go on using the old one: starting again with empty
        // settings would be far worse than keeping the old name.
        Err(_) => old,
    }
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

    /// A picture in `icons` by the file name stored in the settings. Only a
    /// bare file name is accepted, so a hand-edited or shared config can't
    /// make the app read, or delete, a file outside that folder.
    pub fn icon_file(&self, name: &str) -> Option<PathBuf> {
        crate::config::is_plain_file_name(name).then(|| self.icons.join(name))
    }
}

/// Deletes a picture the app copied into `icons` (see [`Paths::icon_file`]).
pub fn remove_icon(name: &str) {
    if let Some(p) = get().icon_file(name) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own for each test, cleaned up at the end.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("flextaskbar-paths-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("making the test folder");
            Temp(dir)
        }

        /// A data folder with something of ours in it.
        fn ours(&self, name: &str) -> PathBuf {
            let dir = self.0.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.json"), b"{}").unwrap();
            dir
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_fresh_install_uses_the_new_name() {
        let t = Temp::new("fresh");
        assert_eq!(adopt_old_name(&t.0), t.0.join(DATA_DIR));
    }

    #[test]
    fn a_folder_from_an_older_version_is_moved_across() {
        let t = Temp::new("move");
        let old = t.ours(OLD_DATA_DIR);
        let got = adopt_old_name(&t.0);
        assert_eq!(got, t.0.join(DATA_DIR));
        assert!(got.join("config.json").is_file(), "the settings came with it");
        assert!(!old.exists(), "and the old folder is gone, so it only happens once");
    }

    #[test]
    fn the_new_folder_wins_and_an_old_one_is_left_alone() {
        let t = Temp::new("both");
        let old = t.ours(OLD_DATA_DIR);
        let new = t.ours(DATA_DIR);
        assert_eq!(adopt_old_name(&t.0), new);
        assert!(old.join("config.json").is_file(), "nothing of the old one is touched");
    }

    #[test]
    fn a_data_folder_that_isnt_ours_is_not_taken_over() {
        let t = Temp::new("stranger");
        let other = t.0.join(OLD_DATA_DIR);
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("notes.txt"), b"someone else's").unwrap();
        assert_eq!(adopt_old_name(&t.0), t.0.join(DATA_DIR), "a fresh folder of our own");
        assert!(other.join("notes.txt").is_file(), "and theirs is untouched");
        assert!(!other.join("config.json").exists());
    }

    #[test]
    fn an_old_folder_with_only_icons_still_counts_as_ours() {
        let t = Temp::new("icons");
        let old = t.0.join(OLD_DATA_DIR);
        std::fs::create_dir_all(old.join("icons")).unwrap();
        assert_eq!(adopt_old_name(&t.0), t.0.join(DATA_DIR));
        assert!(t.0.join(DATA_DIR).join("icons").is_dir());
    }
}
