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

/// At most this many entries of a folder are read, however big it is.
pub const MAX_READ: usize = 5000;

/// What *Open folder* says: how many more there are, or "more" when the
/// folder was bigger than [`MAX_READ`] and not counted to the end.
pub fn open_label(more: usize, read_all: bool) -> String {
    match (more, read_all) {
        (0, true) => "Open folder".into(),
        (n, true) => format!("Open folder ({n} more)"),
        (_, false) => "Open folder (and more)".into(),
    }
}

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
/// extension, as Explorer shows them; anything else in full. Characters
/// that reorder or hide text are left out (see [`plain_text`]), so a name
/// can't pass off `photo<right-to-left override>gpj.exe` as `photoexe.jpg`.
pub fn display_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let shown = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() && (lower.ends_with(".lnk") || lower.ends_with(".url")) => stem,
        _ => name,
    };
    plain_text(shown)
}

/// `text` without the characters that change how text around them is
/// shown: bidirectional overrides, embeddings, isolates and marks, and
/// control characters (which become spaces).
pub fn plain_text(text: &str) -> String {
    let hidden =
        |c: char| matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    text.chars().filter(|c| !hidden(*c)).map(|c| if c.is_control() { ' ' } else { c }).collect()
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
    fn open_folder_label() {
        assert_eq!(open_label(0, true), "Open folder");
        assert_eq!(open_label(12, true), "Open folder (12 more)");
        assert_eq!(open_label(4952, false), "Open folder (and more)");
    }

    #[test]
    fn names() {
        assert_eq!(display_name("Notepad.lnk"), "Notepad");
        assert_eq!(display_name("Site.URL"), "Site");
        assert_eq!(display_name("report.pdf"), "report.pdf");
        assert_eq!(display_name(".lnk"), ".lnk");
        assert_eq!(display_name("photos"), "photos");
    }

    #[test]
    fn names_cant_hide_what_they_are() {
        // "invoice<RLO>fdp.exe" would read as "invoiceexe.pdf".
        assert_eq!(display_name("invoice\u{202E}fdp.exe"), "invoicefdp.exe");
        assert_eq!(display_name("a\u{2066}b\u{2069}\u{200F}c.txt"), "abc.txt");
        assert_eq!(display_name("tab\there\n.txt"), "tab here .txt");
        assert_eq!(display_name("Ünïcödé 日本.txt"), "Ünïcödé 日本.txt");
    }
}
