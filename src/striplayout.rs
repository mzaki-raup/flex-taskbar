#![cfg_attr(not(windows), allow(dead_code))]

//! Geometry for the icon strip and its flyouts, kept free of Win32 so it can
//! be unit-tested.
//!
//! The bar has three zones, as in the original FlexTaskbar: buttons on the
//! left ("All"), the root categories and pinned apps centred, and buttons on
//! the right ("Link", settings). In `fit` mode the bar shrinks to its content
//! and is centred on the screen like a floating dock.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub x: i32,
    pub w: i32,
}

impl Slot {
    pub fn contains(&self, x: i32) -> bool {
        x >= self.x && x < self.x + self.w
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }
}

pub struct Metrics {
    /// Padding inside the bar's left and right ends.
    pub pad: i32,
    /// Width of one category / pinned-app button.
    pub item_w: i32,
    /// Space between buttons.
    pub gap: i32,
    /// Widths of the left-hand buttons, in order.
    pub left: Vec<i32>,
    /// Widths of the right-hand buttons, in order.
    pub right: Vec<i32>,
}

pub struct BarLayout {
    /// The bar's visible extent within the window.
    pub bar: Slot,
    pub left: Vec<Slot>,
    /// One slot per item that fits; items that don't fit are left out.
    pub items: Vec<Slot>,
    pub right: Vec<Slot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Left(usize),
    Item(usize),
    Right(usize),
}

fn run(start: i32, widths: &[i32], gap: i32) -> Vec<Slot> {
    let mut x = start;
    widths
        .iter()
        .map(|&w| {
            let s = Slot { x, w };
            x += w + gap;
            s
        })
        .collect()
}

fn run_width(widths: &[i32], gap: i32) -> i32 {
    widths.iter().sum::<i32>() + gap * (widths.len() as i32 - 1).max(0)
}

pub fn bar_layout(width: i32, m: &Metrics, count: usize, fit: bool) -> BarLayout {
    let zone_gap = m.gap * 4;
    let left_w = run_width(&m.left, m.gap);
    let right_w = run_width(&m.right, m.gap);
    let item_w = |n: usize| n as i32 * m.item_w + (n as i32 - 1).max(0) * m.gap;

    if fit {
        // Shrink to content, centred; drop items only if even that won't fit.
        let fixed = m.pad * 2 + left_w + right_w + zone_gap * 2;
        let room = (width - fixed).max(0);
        let n = count.min(((room + m.gap) / (m.item_w + m.gap)).max(0) as usize);
        let bar_w = (fixed + item_w(n)).min(width);
        let bar = Slot { x: (width - bar_w) / 2, w: bar_w };
        let left = run(bar.x + m.pad, &m.left, m.gap);
        let items_x = bar.x + m.pad + left_w + zone_gap;
        let items = (0..n).map(|i| Slot { x: items_x + i as i32 * (m.item_w + m.gap), w: m.item_w }).collect();
        let right = run(bar.right() - m.pad - right_w, &m.right, m.gap);
        return BarLayout { bar, left, items, right };
    }

    let bar = Slot { x: 0, w: width };
    let left = run(m.pad, &m.left, m.gap);
    let right = run(width - m.pad - right_w, &m.right, m.gap);
    let lo = m.pad + left_w + zone_gap;
    let hi = width - m.pad - right_w - zone_gap;
    let room = (hi - lo).max(0);
    let n = count.min(((room + m.gap) / (m.item_w + m.gap)).max(0) as usize);
    let total = item_w(n);
    // Centred on the whole bar, nudged inward if that would overlap a side zone.
    let start = ((width - total) / 2).max(lo).min(hi - total);
    let items = (0..n).map(|i| Slot { x: start + i as i32 * (m.item_w + m.gap), w: m.item_w }).collect();
    BarLayout { bar, left, items, right }
}

pub fn hit(layout: &BarLayout, x: i32) -> Option<Hit> {
    if let Some(i) = layout.left.iter().position(|s| s.contains(x)) {
        return Some(Hit::Left(i));
    }
    if let Some(i) = layout.right.iter().position(|s| s.contains(x)) {
        return Some(Hit::Right(i));
    }
    layout.items.iter().position(|s| s.contains(x)).map(Hit::Item)
}

/// Where an item dragged to bar position `x` lands: the slot nearest the
/// pointer (by centre). The other items fill the remaining slots in order.
pub fn drop_index(items: &[Slot], x: i32) -> usize {
    let centre = |s: &Slot| s.x + s.w / 2;
    items.windows(2).filter(|w| x >= (centre(&w[0]) + centre(&w[1])) / 2).count()
}

/// Where each of `n` tiles goes in a category flyout of a bar on `edge`, as
/// (column, row). The flyout runs the same way as its bar: tiles left to
/// right for a top or bottom bar, top to bottom for a side bar, wrapping
/// after `per_line`. The first tiles (the subcategories) end up on the side
/// nearest where their own flyouts open: the top row above a bottom bar, the
/// bottom row below a top bar, the right column beside a left bar, the left
/// column beside a right bar.
pub fn flyout_cells(n: usize, per_line: usize, edge: Edge) -> Vec<(usize, usize)> {
    let per = per_line.max(1);
    let lines = n.div_ceil(per);
    (0..n)
        .map(|i| {
            let (line, pos) = (i / per, i % per);
            match edge {
                Edge::Bottom => (pos, line),
                Edge::Top => (pos, lines - 1 - line),
                Edge::Left => (lines - 1 - line, pos),
                Edge::Right => (line, pos),
            }
        })
        .collect()
}

/// A screen edge the strip can dock against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

impl Edge {
    /// Left and right bars stand upright: their buttons run top to bottom.
    pub fn vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

/// The edge a bar being dragged to `(x, y)` should dock against: the one the
/// point is closest to, relative to the screen's size (as the Windows taskbar
/// does). `screen` is (left, top, right, bottom).
pub fn edge_for_point(screen: (i32, i32, i32, i32), x: i32, y: i32) -> Edge {
    let (l, t, r, b) = screen;
    let w = (r - l).max(1) as f32;
    let h = (b - t).max(1) as f32;
    let candidates = [
        (Edge::Left, (x - l) as f32 / w),
        (Edge::Right, (r - x) as f32 / w),
        (Edge::Top, (y - t) as f32 / h),
        (Edge::Bottom, (b - y) as f32 / h),
    ];
    candidates.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|c| c.0).unwrap_or(Edge::Bottom)
}

/// Top-left corner for a flyout of `size` (w, h) hanging off `anchor` (a
/// button, or a tile of the flyout below; left, top, right, bottom) of a bar
/// on `edge`: it opens away from that edge, towards the middle of the
/// screen, `gap` pixels clear, and is kept on `screen`.
pub fn flyout_origin(
    edge: Edge,
    anchor: (i32, i32, i32, i32),
    size: (i32, i32),
    screen: (i32, i32, i32, i32),
    gap: i32,
) -> (i32, i32) {
    let (al, at, ar, ab) = anchor;
    let (w, h) = size;
    let (sl, st, sr, sb) = screen;
    let along_x = al.min(sr - w).max(sl);
    let along_y = at.min(sb - h).max(st);
    match edge {
        Edge::Bottom => (along_x, (at - gap - h).max(st)),
        Edge::Top => (along_x, (ab + gap).min(sb - h)),
        Edge::Left => ((ar + gap).min(sr - w), along_y),
        Edge::Right => ((al - gap - w).max(sl), along_y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics { pad: 8, item_w: 44, gap: 4, left: vec![40], right: vec![60, 30] }
    }

    #[test]
    fn full_width_has_three_zones() {
        let l = bar_layout(1000, &metrics(), 3, false);
        assert_eq!(l.bar, Slot { x: 0, w: 1000 });
        assert_eq!(l.left, vec![Slot { x: 8, w: 40 }]);
        assert_eq!(l.right.last().unwrap().right(), 1000 - 8);
        let total = 3 * 44 + 2 * 4;
        assert_eq!(l.items[0].x, (1000 - total) / 2);
    }

    #[test]
    fn crowded_bar_keeps_clear_of_the_side_buttons() {
        let l = bar_layout(400, &metrics(), 50, false);
        assert!(!l.items.is_empty() && l.items.len() < 50);
        assert!(l.items[0].x >= l.left[0].right());
        assert!(l.items.last().unwrap().right() <= l.right[0].x);
    }

    #[test]
    fn fit_mode_shrinks_and_centres() {
        let l = bar_layout(1000, &metrics(), 3, true);
        assert!(l.bar.w < 1000);
        assert_eq!(l.bar.x, (1000 - l.bar.w) / 2);
        assert_eq!(l.left[0].x, l.bar.x + 8);
        assert_eq!(l.right.last().unwrap().right(), l.bar.right() - 8);
        assert!(l.items[0].x > l.left[0].right());
        assert!(l.items[2].right() < l.right[0].x);
    }

    #[test]
    fn hit_testing() {
        let l = bar_layout(1000, &metrics(), 2, false);
        assert_eq!(hit(&l, 10), Some(Hit::Left(0)));
        assert_eq!(hit(&l, l.items[1].x + 1), Some(Hit::Item(1)));
        assert_eq!(hit(&l, l.right[1].x + 1), Some(Hit::Right(1)));
        assert_eq!(hit(&l, 300), None);
    }

    #[test]
    fn empty_bar() {
        let l = bar_layout(1000, &metrics(), 0, true);
        assert!(l.items.is_empty());
        assert_eq!(hit(&l, 100), None); // outside the shrunken bar
    }

    #[test]
    fn flyouts_run_the_way_their_bar_does() {
        // Bottom bar: a horizontal strip, wrapping upwards (first row on top).
        assert_eq!(flyout_cells(5, 4, Edge::Bottom), vec![(0, 0), (1, 0), (2, 0), (3, 0), (0, 1)]);
        // Top bar: the first row at the bottom, nearest the next level.
        assert_eq!(flyout_cells(5, 4, Edge::Top), vec![(0, 1), (1, 1), (2, 1), (3, 1), (0, 0)]);
        // Side bars: a vertical strip, wrapping into further columns.
        assert_eq!(flyout_cells(3, 4, Edge::Right), vec![(0, 0), (0, 1), (0, 2)]);
        assert_eq!(flyout_cells(5, 4, Edge::Right), vec![(0, 0), (0, 1), (0, 2), (0, 3), (1, 0)]);
        assert_eq!(flyout_cells(5, 4, Edge::Left), vec![(1, 0), (1, 1), (1, 2), (1, 3), (0, 0)]);
        assert!(flyout_cells(0, 4, Edge::Left).is_empty());
    }

    #[test]
    fn edges() {
        let screen = (0, 0, 1600, 900);
        assert_eq!(edge_for_point(screen, 800, 880), Edge::Bottom);
        assert_eq!(edge_for_point(screen, 800, 20), Edge::Top);
        assert_eq!(edge_for_point(screen, 30, 450), Edge::Left);
        assert_eq!(edge_for_point(screen, 1580, 450), Edge::Right);
        // Relative to the screen's size: 200 px from the left of a wide
        // screen is nearer (proportionally) than 150 px from the bottom.
        assert_eq!(edge_for_point(screen, 200, 750), Edge::Left);
        assert!(Edge::Left.vertical() && Edge::Right.vertical());
        assert!(!Edge::Top.vertical() && !Edge::Bottom.vertical());
    }

    #[test]
    fn flyouts_open_away_from_the_edge() {
        let screen = (0, 0, 1600, 900);
        let size = (300, 200);
        // Bottom bar: above the button, left-aligned with it.
        assert_eq!(flyout_origin(Edge::Bottom, (500, 852, 548, 900), size, screen, 4), (500, 648));
        // Top bar: below it.
        assert_eq!(flyout_origin(Edge::Top, (500, 0, 548, 48), size, screen, 4), (500, 52));
        // Left bar: to its right, top-aligned with it.
        assert_eq!(flyout_origin(Edge::Left, (0, 300, 48, 348), size, screen, 4), (52, 300));
        // Right bar: to its left.
        assert_eq!(flyout_origin(Edge::Right, (1552, 300, 1600, 348), size, screen, 4), (1248, 300));
        // Kept on screen: a button near the far end shifts it back.
        assert_eq!(flyout_origin(Edge::Bottom, (1500, 852, 1548, 900), size, screen, 4), (1300, 648));
        assert_eq!(flyout_origin(Edge::Left, (0, 800, 48, 848), size, screen, 4), (52, 700));
    }

    #[test]
    fn dragging() {
        let l = bar_layout(1000, &metrics(), 4, false);
        let items = &l.items;
        assert_eq!(drop_index(items, 0), 0);
        assert_eq!(drop_index(items, items[0].x + 5), 0);
        assert_eq!(drop_index(items, items[2].x + items[2].w / 2), 2);
        assert_eq!(drop_index(items, items[3].right() + 300), 3);
        assert_eq!(drop_index(&[], 50), 0);
    }
}
