//! The mark on category and subcategory icons that says "this opens a
//! flyout": a badge with an arrow, a plain arrow, a dot, a folded corner, an
//! underline, the user's own picture, or nothing (`Appearance::indicator`).
//! Shared by the strip and its flyouts.

use super::canvas::{self, Canvas};
use super::icons::Source as IconSource;
use super::paths;
use crate::appearance::{Appearance, Colors, Indicator, RunningMark};
use crate::striplayout::Edge;
use resvg::tiny_skia::Pixmap;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// The user's pictures (the indicator's, the *All* button's), by file
    /// name and pixel size.
    static IMAGES: RefCell<HashMap<String, HashMap<i32, Option<Pixmap>>>> = RefCell::new(HashMap::new());
}

/// Calls `f` with a picture from the data folder's `icons` at `size` pixels,
/// loaded once. Drawn every frame of the bar, so it neither allocates nor
/// copies once loaded. `None` when the picture can't be read.
pub fn with_image<R>(file: &str, size: i32, f: impl FnOnce(&Pixmap) -> R) -> Option<R> {
    IMAGES.with(|m| {
        let mut m = m.borrow_mut();
        if !m.get(file).is_some_and(|sizes| sizes.contains_key(&size)) {
            let pix = paths::get().icon_file(file).and_then(|path| canvas::icon_pixmap(&IconSource::Image(path), size));
            m.entry(file.to_string()).or_default().insert(size, pix);
        }
        m.get(file)?.get(&size)?.as_ref().map(f)
    })
}

/// Forget loaded pictures (a new one was chosen).
pub fn forget_images() {
    IMAGES.with(|m| m.borrow_mut().clear());
}

/// Draws the mark on an icon at (x, y), `size` pixels square, of a strip on
/// `edge` (arrows point where the flyout opens).
#[allow(clippy::too_many_arguments)]
pub fn draw(cv: &mut Canvas, look: &Appearance, c: &Colors, edge: Edge, x: i32, y: i32, size: i32) {
    let (x, y, s) = (x as f32, y as f32, size as f32);
    let k = look.indicator_size.clamp(25, 80) as f32 / 100.0;
    let (dx, dy) = edge.opening();
    let ring = c.background.with_alpha(255);
    // Corner marks sit on the icon's bottom-right corner, a little outside it.
    let r = s * k / 2.0;
    let (cx, cy) = (x + s - r * 0.35, y + s - r * 0.35);
    match look.indicator {
        Indicator::None => {}
        Indicator::Badge => {
            cv.circle(cx, cy, r + (r * 0.22).max(1.0), ring);
            cv.circle(cx, cy, r, c.accent);
            cv.arrow(cx, cy, r, dx, dy, c.on_accent);
        }
        Indicator::Arrow => {
            // The background behind it keeps it readable on any icon.
            cv.arrow(cx, cy, r * 1.35, dx, dy, ring);
            cv.arrow(cx, cy, r, dx, dy, c.accent);
        }
        Indicator::Dot => {
            let r = r * 0.55;
            cv.circle(cx, cy, r + (r * 0.35).max(1.0), ring);
            cv.circle(cx, cy, r, c.accent);
        }
        Indicator::Corner => {
            let leg = s * k * 0.8;
            let (rx, by) = (x + s, y + s);
            cv.polygon(&[(rx + 1.5, by - leg - 2.0), (rx + 1.5, by + 1.5), (rx - leg - 2.0, by + 1.5)], ring);
            cv.polygon(&[(rx, by - leg), (rx, by), (rx - leg, by)], c.accent);
        }
        Indicator::Underline => {
            let (w, h) = (s * (0.35 + k * 0.6), (s * 0.09).max(2.0));
            let (lx, ly) = (x + (s - w) / 2.0, y + s - h / 2.0);
            cv.fill_round_rect(lx - 1.0, ly - 1.0, w + 2.0, h + 2.0, h / 2.0 + 1.0, ring);
            cv.fill_round_rect(lx, ly, w, h, h / 2.0, c.accent);
        }
        Indicator::Image => {
            let px = (s * k).round() as i32;
            if let Some(file) = &look.indicator_image {
                let (ix, iy) = ((cx - px as f32 / 2.0) as i32, (cy - px as f32 / 2.0) as i32);
                with_image(file, px, |img| cv.image(img, ix, iy, px, 1.0));
            }
        }
    }
}

/// The mark under an app that has a window open, drawn inside `cell` on the
/// side of `edge` (the screen edge for the bar; pass `Edge::Bottom` for a
/// tile, whose mark sits under it).
pub fn running(
    cv: &mut Canvas,
    look: &Appearance,
    c: &Colors,
    edge: Edge,
    cell: windows::Win32::Foundation::RECT,
    dpi: u32,
) {
    let s = |v: f32| v * dpi as f32 / 96.0;
    let (l, t, r, b) = (cell.left as f32, cell.top as f32, cell.right as f32, cell.bottom as f32);
    let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
    let inset = s(3.0);
    // Where the mark sits, and whether it runs along x.
    let (mx, my, along_x) = match edge {
        Edge::Bottom => (cx, b - inset, true),
        Edge::Top => (cx, t + inset, true),
        Edge::Left => (l + inset, cy, false),
        Edge::Right => (r - inset, cy, false),
    };
    match look.running_mark {
        RunningMark::Off => {}
        RunningMark::Dot => cv.circle(mx, my, s(2.2), c.subtle),
        RunningMark::Line => {
            let (long, thick) = (s(10.0), s(3.0));
            let (w, h) = if along_x { (long, thick) } else { (thick, long) };
            cv.fill_round_rect(mx - w / 2.0, my - h / 2.0, w, h, thick / 2.0, c.subtle);
        }
    }
}
