#![cfg_attr(not(windows), allow(dead_code))]

//! Which program opens a folder. Kept free of Win32 so it can be unit-tested;
//! the looking-on-disk part is in `win::app`.
//!
//! Windows Explorer opens every folder in a window of its own. Eden Explorer —
//! the sibling project — opens them as **tabs**: it keeps one instance, and a
//! second launch hands its paths to the window already up, which opens each as
//! a new tab. So pointing *Open data folder* at it needs no special flag, only
//! its path and the folder as an argument.
//!
//! It is used when it is there and ignored when it isn't: a setting that
//! names a program which has since been uninstalled must not leave the button
//! doing nothing.

use std::path::{Path, PathBuf};

/// The program the settings name, if any, and where else to look for Eden
/// Explorer. `exists` answers whether a path is a file that can be run, which
/// is what makes this testable without touching the disk.
///
/// `configured` is `Settings::file_manager`: empty means "find one".
pub fn choose(configured: &str, candidates: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let named = PathBuf::from(configured);
        if exists(&named) {
            return Some(named);
        }
        // Named but missing: fall through and look, rather than do nothing.
        // Saying "it's gone" every time the button is pressed would help
        // nobody; Explorer opening the folder is still the right outcome.
    }
    candidates.iter().find(|p| exists(p)).cloned()
}

/// The file name to look for on the PATH and in the usual places.
pub const EDEN_EXE: &str = "EdenExplorer.exe";

/// Where Eden Explorer tends to be, given the folders Windows hands us.
/// `beside` is FlexTaskbar's own folder: the two are often installed as
/// siblings, which is the case this is really for.
pub fn usual_places(beside: &Path, program_files: &[PathBuf], local_programs: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // A sibling of FlexTaskbar: `C:\Apps\flex-taskbar` next to
    // `C:\Apps\eden-explorer`, and the plain case of both in one folder.
    if let Some(parent) = beside.parent() {
        for name in ["eden-explorer", "EdenExplorer", "Eden-Explorer"] {
            out.push(parent.join(name).join(EDEN_EXE));
        }
    }
    out.push(beside.join(EDEN_EXE));
    for dir in program_files {
        out.push(dir.join("EdenExplorer").join(EDEN_EXE));
        out.push(dir.join("Eden Explorer").join(EDEN_EXE));
    }
    if let Some(dir) = local_programs {
        out.push(dir.join("EdenExplorer").join(EDEN_EXE));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn on_disk(paths: &[&str]) -> impl Fn(&Path) -> bool + use<> {
        let set: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p: &Path| set.contains(p)
    }

    fn candidates() -> Vec<PathBuf> {
        ["C:/Apps/eden-explorer/EdenExplorer.exe", "C:/Program Files/EdenExplorer/EdenExplorer.exe"]
            .iter()
            .map(PathBuf::from)
            .collect()
    }

    #[test]
    fn the_program_the_settings_name_is_used() {
        let chosen = choose("C:/Tools/MyExplorer.exe", &candidates(), on_disk(&["C:/Tools/MyExplorer.exe"]));
        assert_eq!(chosen, Some(PathBuf::from("C:/Tools/MyExplorer.exe")));
    }

    #[test]
    fn a_named_program_wins_over_one_that_was_found() {
        let there = on_disk(&["C:/Tools/MyExplorer.exe", "C:/Apps/eden-explorer/EdenExplorer.exe"]);
        assert_eq!(
            choose("C:/Tools/MyExplorer.exe", &candidates(), there),
            Some(PathBuf::from("C:/Tools/MyExplorer.exe"))
        );
    }

    #[test]
    fn a_named_program_that_is_gone_falls_back_to_looking() {
        // Uninstalled since it was set: the button must still open the folder.
        let chosen =
            choose("C:/Tools/Removed.exe", &candidates(), on_disk(&["C:/Apps/eden-explorer/EdenExplorer.exe"]));
        assert_eq!(chosen, Some(PathBuf::from("C:/Apps/eden-explorer/EdenExplorer.exe")));
    }

    #[test]
    fn nothing_named_means_the_first_one_found() {
        let there = on_disk(&["C:/Program Files/EdenExplorer/EdenExplorer.exe"]);
        assert_eq!(
            choose("", &candidates(), there),
            Some(PathBuf::from("C:/Program Files/EdenExplorer/EdenExplorer.exe"))
        );
    }

    #[test]
    fn the_order_of_the_places_is_kept() {
        let both =
            on_disk(&["C:/Apps/eden-explorer/EdenExplorer.exe", "C:/Program Files/EdenExplorer/EdenExplorer.exe"]);
        assert_eq!(choose("", &candidates(), both), Some(PathBuf::from("C:/Apps/eden-explorer/EdenExplorer.exe")));
    }

    #[test]
    fn none_of_it_there_means_windows_explorer() {
        assert_eq!(choose("", &candidates(), on_disk(&[])), None, "the caller falls back to the shell");
        assert_eq!(choose("   ", &[], on_disk(&[])), None, "blank is the same as not set");
    }

    #[test]
    fn a_sibling_folder_is_looked_in_first() {
        let beside = PathBuf::from("C:/Apps/flex-taskbar");
        let places = usual_places(&beside, &[PathBuf::from("C:/Program Files")], None);
        assert_eq!(places.first(), Some(&PathBuf::from("C:/Apps/eden-explorer/EdenExplorer.exe")));
        assert!(places.contains(&PathBuf::from("C:/Apps/flex-taskbar/EdenExplorer.exe")), "and beside the exe itself");
        assert!(places.contains(&PathBuf::from("C:/Program Files/EdenExplorer/EdenExplorer.exe")));
    }

    #[test]
    fn looking_in_the_usual_places_works_without_a_parent_folder() {
        let places = usual_places(Path::new(""), &[], None);
        assert_eq!(places, [PathBuf::from(EDEN_EXE)], "no parent, so only beside the exe");
    }
}
