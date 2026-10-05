//! Launcher search ranking. Matches the app name, its target file name, and the
//! names of every category (at any depth) the app is filed under — so searching
//! "dev" finds everything inside "Dev › Editors" too.

/// Pre-lowercased search fields for one app.
pub struct Item {
    pub name: String,
    pub file: String,
    pub categories: String,
    /// Position in the recents list (0 = most recent), if any.
    pub recent: Option<usize>,
}

impl Item {
    pub fn new(name: &str, file: &str, categories: &str, recent: Option<usize>) -> Item {
        Item { name: name.to_lowercase(), file: file.to_lowercase(), categories: categories.to_lowercase(), recent }
    }
}

/// Lower is better; `None` means no match.
fn score(item: &Item, query: &str, tokens: &[&str]) -> Option<u32> {
    let name = item.name.as_str();
    if name == query {
        return Some(0);
    }
    if name.starts_with(query) {
        return Some(1);
    }
    if name.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(query)) {
        return Some(2);
    }
    if name.contains(query) {
        return Some(3);
    }
    if item.file.contains(query) {
        return Some(4);
    }
    if item.categories.contains(query) {
        return Some(5);
    }
    if tokens.len() > 1
        && tokens.iter().all(|t| name.contains(t) || item.file.contains(t) || item.categories.contains(t))
    {
        return Some(6);
    }
    if is_initials(name, query) {
        return Some(7);
    }
    if is_subsequence(name, query) {
        return Some(8);
    }
    None
}

/// "vsc" matches "Visual Studio Code".
fn is_initials(name: &str, query: &str) -> bool {
    let initials: String = name.split(|c: char| !c.is_alphanumeric()).filter_map(|w| w.chars().next()).collect();
    query.chars().count() >= 2 && initials.starts_with(query)
}

fn is_subsequence(name: &str, query: &str) -> bool {
    if query.chars().count() < 3 {
        return false;
    }
    // Anchored on the first letter so short queries don't match half the list.
    if name.chars().next() != query.chars().next() {
        return false;
    }
    let mut chars = name.chars();
    query.chars().filter(|c| !c.is_whitespace()).all(|q| chars.any(|c| c == q))
}

/// Indexes into `items`, best match first. Ties go to recently used apps, then
/// alphabetical order.
pub fn rank(items: &[Item], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let tokens: Vec<&str> = query.split_whitespace().collect();
    let mut hits: Vec<(u32, usize, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| score(it, &query, &tokens).map(|s| (s, it.recent.unwrap_or(usize::MAX), i)))
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then_with(|| items[a.2].name.cmp(&items[b.2].name)));
    hits.into_iter().map(|h| h.2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<Item> {
        vec![
            Item::new("Visual Studio Code", "code.exe", "Dev › Editors", None),
            Item::new("Notepad", "notepad.exe", "", None),
            Item::new("Notepad++", "notepad++.exe", "Dev › Editors", Some(0)),
            Item::new("Steam", "steam.exe", "Games", None),
            Item::new("Windows Terminal", "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "Dev", None),
        ]
    }

    fn names(query: &str) -> Vec<String> {
        let it = items();
        rank(&it, query).into_iter().map(|i| it[i].name.clone()).collect()
    }

    #[test]
    fn empty_query_returns_nothing() {
        assert!(names("  ").is_empty());
    }

    #[test]
    fn exact_beats_prefix_and_recent_breaks_ties() {
        assert_eq!(names("notepad"), vec!["notepad", "notepad++"]);
        assert_eq!(names("note"), vec!["notepad++", "notepad"]);
    }

    #[test]
    fn word_start_and_category_matches() {
        assert_eq!(names("term"), vec!["windows terminal"]);
        assert_eq!(names("editors"), vec!["notepad++", "visual studio code"]);
        assert_eq!(names("dev")[..2], ["notepad++".to_string(), "visual studio code".to_string()]);
    }

    #[test]
    fn file_name_initials_and_subsequence() {
        assert_eq!(names("code.exe"), vec!["visual studio code"]);
        assert_eq!(names("vsc"), vec!["visual studio code"]);
        assert_eq!(names("stm"), vec!["steam"]);
    }

    #[test]
    fn multi_word_queries_need_every_word() {
        assert_eq!(names("games steam"), vec!["steam"]);
        assert!(names("games notepad").is_empty());
    }
}
