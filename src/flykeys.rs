#![cfg_attr(not(windows), allow(dead_code))]

//! Keyboard control of the flyouts: which tile or row an arrow key moves
//! to, and type-to-jump. Pure, so it can be unit-tested off Windows; the
//! flyout feeds it its elements' rectangles and names.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// (left, top, right, bottom).
pub type Rect = (i32, i32, i32, i32);

fn centre(r: &Rect) -> (i32, i32) {
    ((r.0 + r.2) / 2, (r.1 + r.3) / 2)
}

/// The element an arrow key moves to from `from`: the nearest one whose
/// centre lies that way, preferring ones in line (the same row for Left and
/// Right, the same column for Up and Down). `None` at the edge.
pub fn neighbour(rects: &[Rect], from: usize, dir: Dir) -> Option<usize> {
    let (fx, fy) = centre(rects.get(from)?);
    rects
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != from)
        .filter_map(|(i, r)| {
            let (x, y) = centre(r);
            let (along, across) = match dir {
                Dir::Left => (fx - x, (y - fy).abs()),
                Dir::Right => (x - fx, (y - fy).abs()),
                Dir::Up => (fy - y, (x - fx).abs()),
                Dir::Down => (y - fy, (x - fx).abs()),
            };
            // In line counts far more than distance, so Down in a grid goes
            // to the tile below rather than one diagonally nearer.
            (along > 0).then_some((i, along as i64 + across as i64 * 4))
        })
        .min_by_key(|&(i, score)| (score, i))
        .map(|(i, _)| i)
}

/// Typed letters, collected until the user pauses.
#[derive(Debug, Default, Clone)]
pub struct TypeAhead {
    text: String,
    last_ms: u64,
}

/// A pause longer than this starts a new search.
pub const PAUSE_MS: u64 = 1000;

impl TypeAhead {
    /// Adds `ch` typed at `now_ms` and returns the text to look for.
    pub fn push(&mut self, ch: char, now_ms: u64) -> &str {
        if now_ms.saturating_sub(self.last_ms) > PAUSE_MS {
            self.text.clear();
        }
        self.last_ms = now_ms;
        // Pressing the same letter again cycles through names starting with it.
        if self.text.chars().count() == 1 && self.text.starts_with(ch.to_ascii_lowercase()) {
            return &self.text;
        }
        self.text.extend(ch.to_lowercase());
        &self.text
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }
}

/// The first name matching `typed` after `current` (wrapping round): one
/// starting with it, or failing that one with a word starting with it.
/// Case-insensitive. Repeating a single letter moves to the next match.
pub fn find(names: &[&str], typed: &str, current: Option<usize>) -> Option<usize> {
    if typed.is_empty() || names.is_empty() {
        return None;
    }
    let n = names.len();
    // A single letter looks after the current name; a longer text may stay on it.
    let skip = if typed.chars().count() == 1 { 1 } else { 0 };
    let start = current.map(|c| c + skip).unwrap_or(0);
    let order = || (0..n).map(|k| (start + k) % n);
    let lower: Vec<String> = names.iter().map(|s| s.to_lowercase()).collect();
    let word_start = |s: &str| s.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(typed));
    order().find(|&i| lower[i].starts_with(typed)).or_else(|| order().find(|&i| word_start(&lower[i])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3×2 grid of 10×10 tiles, 2 apart.
    fn grid() -> Vec<Rect> {
        let mut v = Vec::new();
        for row in 0..2 {
            for col in 0..3 {
                let (x, y) = (col * 12, row * 12);
                v.push((x, y, x + 10, y + 10));
            }
        }
        v
    }

    #[test]
    fn arrows_move_through_a_grid() {
        let g = grid();
        assert_eq!(neighbour(&g, 0, Dir::Right), Some(1));
        assert_eq!(neighbour(&g, 1, Dir::Down), Some(4));
        assert_eq!(neighbour(&g, 4, Dir::Up), Some(1));
        assert_eq!(neighbour(&g, 5, Dir::Left), Some(4));
        // Edges.
        assert_eq!(neighbour(&g, 0, Dir::Left), None);
        assert_eq!(neighbour(&g, 2, Dir::Up), None);
        // A short last row: Down from the right end goes to the nearest below.
        let mut short = grid();
        short.truncate(4);
        assert_eq!(neighbour(&short, 2, Dir::Down), Some(3));
        assert_eq!(neighbour(&g, 9, Dir::Down), None);
    }

    #[test]
    fn arrows_in_a_list_and_buttons_above() {
        // Two buttons side by side, then three full-width rows.
        let v = vec![(200, 0, 260, 20), (270, 0, 330, 20), (0, 30, 340, 50), (0, 50, 340, 70), (0, 70, 340, 90)];
        assert_eq!(neighbour(&v, 2, Dir::Down), Some(3));
        assert_eq!(neighbour(&v, 4, Dir::Down), None);
        assert_eq!(neighbour(&v, 2, Dir::Up), Some(0)); // the nearer button
        assert_eq!(neighbour(&v, 0, Dir::Right), Some(1));
        assert_eq!(neighbour(&v, 1, Dir::Down), Some(2));
    }

    #[test]
    fn type_ahead() {
        let names = ["7z", "Command Prompt", "Control Panel", "curl", "Notepad", "Visual Studio Code"];
        assert_eq!(find(&names, "co", None), Some(1));
        assert_eq!(find(&names, "con", None), Some(2));
        assert_eq!(find(&names, "note", Some(0)), Some(4));
        // A word inside the name.
        assert_eq!(find(&names, "studio", None), Some(5));
        assert_eq!(find(&names, "panel", None), Some(2));
        // The same letter again moves on, wrapping round.
        assert_eq!(find(&names, "c", Some(1)), Some(2));
        assert_eq!(find(&names, "c", Some(3)), Some(1));
        assert_eq!(find(&names, "zz", None), None);
        assert_eq!(find(&[], "a", None), None);
    }

    #[test]
    fn typing_collects_until_a_pause() {
        let mut t = TypeAhead::default();
        assert_eq!(t.push('N', 10_000), "n");
        assert_eq!(t.push('o', 10_300), "no");
        assert_eq!(t.push('t', 10_600), "not");
        // A pause starts again.
        assert_eq!(t.push('C', 12_000), "c");
        // The same first letter again stays one letter (cycling).
        assert_eq!(t.push('c', 12_200), "c");
        t.clear();
        assert_eq!(t.push('x', 12_300), "x");
    }
}
