#![cfg_attr(not(windows), allow(dead_code))]

//! Horizontal layout of the icon strip: the launcher button on the left, then
//! the root categories and pinned apps centered on the strip (as on the
//! Windows 11 taskbar), shifted right if centering would overlap the launcher.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub x: i32,
    pub w: i32,
}

impl Slot {
    pub fn contains(&self, x: i32) -> bool {
        x >= self.x && x < self.x + self.w
    }
}

pub struct Layout {
    pub launcher: Slot,
    /// One slot per item that fits; items beyond the strip's width are left out.
    pub items: Vec<Slot>,
}

pub fn layout(width: i32, pad: i32, item_w: i32, gap: i32, count: usize) -> Layout {
    let launcher = Slot { x: pad, w: item_w };
    let first_free = launcher.x + launcher.w + gap * 2;
    let room = (width - pad - first_free).max(0);
    let fits = if item_w <= 0 { 0 } else { ((room + gap) / (item_w + gap)).max(0) as usize };
    let n = count.min(fits);
    let total = n as i32 * item_w + (n as i32 - 1).max(0) * gap;
    let start = ((width - total) / 2).max(first_free);
    let items = (0..n).map(|i| Slot { x: start + i as i32 * (item_w + gap), w: item_w }).collect();
    Layout { launcher, items }
}

/// Which slot is under `x`: `Some(None)` for the launcher, `Some(Some(i))` for
/// item `i`, `None` for empty strip.
pub fn hit(layout: &Layout, x: i32) -> Option<Option<usize>> {
    if layout.launcher.contains(x) {
        return Some(None);
    }
    layout.items.iter().position(|s| s.contains(x)).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_centered() {
        let l = layout(1000, 8, 40, 4, 3);
        assert_eq!(l.launcher, Slot { x: 8, w: 40 });
        let total = 3 * 40 + 2 * 4;
        assert_eq!(l.items[0].x, (1000 - total) / 2);
        assert_eq!(l.items[2].x + l.items[2].w, (1000 - total) / 2 + total);
    }

    #[test]
    fn crowded_strip_starts_after_the_launcher_and_drops_overflow() {
        let l = layout(300, 8, 40, 4, 50);
        assert!(l.items[0].x >= 8 + 40);
        assert!(l.items.last().unwrap().x + 40 <= 300 - 8);
        assert!(l.items.len() < 50);
    }

    #[test]
    fn hit_testing() {
        let l = layout(1000, 8, 40, 4, 2);
        assert_eq!(hit(&l, 10), Some(None));
        assert_eq!(hit(&l, l.items[1].x + 5), Some(Some(1)));
        assert_eq!(hit(&l, l.items[0].x + 40 + 1), None); // the gap
        assert_eq!(hit(&l, 999), None);
    }

    #[test]
    fn empty_strip() {
        let l = layout(1000, 8, 40, 4, 0);
        assert!(l.items.is_empty());
        assert_eq!(hit(&l, 500), None);
    }
}
