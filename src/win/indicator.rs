//! The mark on category and subcategory icons that says "this opens a
//! flyout": a badge with an arrow, a plain arrow, a dot, a folded corner, an
//! underline, the user's own picture, or nothing (`Appearance::indicator`).
//! Shared by the strip and its flyouts.

use super::canvas::{self, Canvas};
use super::icons::Source as IconSource;
use super::paths;
use crate::appearance::{Appearance, Colors, Indicator};
use crate::striplayout::Edge;
use resvg::tiny_skia::Pixmap;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// The user's indicator picture, by (file name, pixel size).
    static IMAGES: RefCell<HashMap<(String, i32), Option<Pixmap>>> = RefCell::new(HashMap::new());
}

/// A picture from the data folder's `icons` at `size` pixels, loaded once
/// (the indicator's picture, the *All* button's).
pub fn image(file: &str, size: i32) -> Option<Pixmap> {
    let key = (file.to_string(), size);
    if let Some(p) = IMAGES.with(|m| m.borrow().get(&key).cloned()) {
        return p;
    }
    let path = paths::get().icons.join(file);
    let pix = canvas::icon_pixmap(&IconSource::Image(path), size);
    IMAGES.with(|m| m.borrow_mut().insert(key, pix.clone()));
    pix
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
            if let Some(file) = &look.indicator_image
                && let Some(img) = image(file, px)
            {
                let (ix, iy) = ((cx - px as f32 / 2.0) as i32, (cy - px as f32 / 2.0) as i32);
                cv.image(&img, ix, iy, px, 1.0);
            }
        }
    }
}
