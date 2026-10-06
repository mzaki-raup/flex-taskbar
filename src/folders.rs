#![cfg_attr(not(windows), allow(dead_code))]

//! Folders pinned to the bar (like the Dock's stacks): what a folder's
//! flyout lists and in what order. Pure, so it can be unit-tested off
//! Windows; the flyout reads the folder and draws the tiles.

/// One thing in a folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// File or folder name, as on disk.
    pub name: String,
    pub dir: bool,
    /// Hidden or system (left out, like Explorer does by default).
    pub hidden: bool,
}

/// At most this many tiles; the rest are reached with *Open folder*.
pub const MAX_SHOWN: usize = 48;

/// What a folder's flyout shows: no hidden or system files, folders first,
/// then by name (ignoring case), at most [`MAX_SHOWN`]. Also returns how many
/// more there are.
pub fn listing(mut entries: Vec<Entry>) -> (Vec<Entry>, usize) {
    entries.retain(|e| !e.hidden);
    entries.sort_by_cached_key(|e| (!e.dir, e.name.to_lowercase()));
    let more = entries.len().saturating_sub(MAX_SHOWN);
    entries.truncate(MAX_SHOWN);
    (entries, more)
}

/// The name shown under a tile: shortcuts (`.lnk`, `.url`) without their
/// extension, as Explorer shows them; anything else in full.
pub fn display_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() && (lower.ends_with(".lnk") || lower.ends_with(".url")) => stem.to_string(),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, dir: bool, hidden: bool) -> Entry {
        Entry { name: name.into(), dir, hidden }
    }

    #[test]
    fn order_and_hidden() {
        let (shown, more) = listing(vec![
            e("zeta.txt", false, false),
            e("Alpha.docx", false, false),
            e("photos", true, false),
            e("desktop.ini", false, true),
            e("Archive", true, false),
        ]);
        let names: Vec<&str> = shown.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Archive", "photos", "Alpha.docx", "zeta.txt"]);
        assert_eq!(more, 0);
    }

    #[test]
    fn capped() {
        let many: Vec<Entry> = (0..60).map(|i| e(&format!("f{i:02}"), false, false)).collect();
        let (shown, more) = listing(many);
        assert_eq!((shown.len(), more), (MAX_SHOWN, 12));
        assert_eq!(listing(Vec::new()), (Vec::new(), 0));
    }

    #[test]
    fn names() {
        assert_eq!(display_name("Notepad.lnk"), "Notepad");
        assert_eq!(display_name("Site.URL"), "Site");
        assert_eq!(display_name("report.pdf"), "report.pdf");
        assert_eq!(display_name(".lnk"), ".lnk");
        assert_eq!(display_name("photos"), "photos");
    }
}
