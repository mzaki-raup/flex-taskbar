#![cfg_attr(not(windows), allow(dead_code))]

//! Flyout shadows (`Appearance::flyout_shadow`): what each style looks like,
//! how much room it needs around the flyout, and the blur that makes it.
//! Pure, so it can be unit-tested off Windows; the flyout draws the result.

use crate::appearance::FlyoutShadow;

/// One style, in pixels at 96 DPI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    /// How soft the edge is (the blur's reach).
    pub blur: f32,
    pub dx: f32,
    pub dy: f32,
    /// Darkness at full strength, 0–1.
    pub alpha: f32,
    /// Drawn in the accent colour instead of black.
    pub accent: bool,
}

pub fn spec(style: FlyoutShadow) -> Option<Spec> {
    let s = |blur, dx, dy, alpha, accent| Some(Spec { blur, dx, dy, alpha, accent });
    match style {
        FlyoutShadow::Off => None,
        // Like Windows 11's menus and flyouts.
        FlyoutShadow::Soft => s(16.0, 0.0, 6.0, 0.50, false),
        // Lifted well above the desktop.
        FlyoutShadow::Floating => s(30.0, 0.0, 14.0, 0.55, false),
        // The same all round, no offset.
        FlyoutShadow::Even => s(12.0, 0.0, 0.0, 0.50, false),
        // A crisp offset shadow.
        FlyoutShadow::Sharp => s(2.0, 5.0, 5.0, 0.55, false),
        // A halo in the accent colour.
        FlyoutShadow::Glow => s(14.0, 0.0, 0.0, 0.90, true),
    }
}

/// The box blur's radius for `blur` pixels at `scale` (DPI / 96). Three box
/// passes of this radius approximate a Gaussian.
pub fn box_radius(spec: &Spec, scale: f32) -> usize {
    ((spec.blur * scale / 2.0).round() as usize).max(1)
}

/// Room the shadow needs outside the flyout: (left, top, right, bottom).
pub fn margins(spec: &Spec, scale: f32) -> (i32, i32, i32, i32) {
    let reach = (3 * box_radius(spec, scale) + 1) as f32;
    let (dx, dy) = (spec.dx * scale, spec.dy * scale);
    let side = |towards: f32| (reach + towards.max(0.0)).ceil() as i32;
    (side(-dx), side(-dy), side(dx), side(dy))
}

/// Blurs an alpha mask in place: three horizontal then three vertical box
/// passes of `radius`, each a running sum, so the cost doesn't grow with the
/// radius.
pub fn blur(mask: &mut [u8], w: usize, h: usize, radius: usize) {
    if radius == 0 || w == 0 || h == 0 || mask.len() < w * h {
        return;
    }
    let mut line = vec![0u8; w.max(h)];
    for _ in 0..3 {
        for y in 0..h {
            box_pass(mask, y * w, 1, w, radius, &mut line);
        }
    }
    for _ in 0..3 {
        for x in 0..w {
            box_pass(mask, x, w, h, radius, &mut line);
        }
    }
}

/// One box pass over `n` values starting at `start`, `step` apart; outside
/// the line counts as 0.
fn box_pass(mask: &mut [u8], start: usize, step: usize, n: usize, radius: usize, line: &mut [u8]) {
    for (i, v) in line.iter_mut().take(n).enumerate() {
        *v = mask[start + i * step];
    }
    let width = (2 * radius + 1) as u32;
    let at = |i: isize| if i < 0 || i as usize >= n { 0 } else { line[i as usize] as u32 };
    let mut sum: u32 = (0..=radius as isize).map(at).sum();
    for i in 0..n {
        mask[start + i * step] = ((sum + width / 2) / width) as u8;
        sum += at(i as isize + radius as isize + 1);
        sum -= at(i as isize - radius as isize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [FlyoutShadow; 6] = [
        FlyoutShadow::Off,
        FlyoutShadow::Soft,
        FlyoutShadow::Floating,
        FlyoutShadow::Even,
        FlyoutShadow::Sharp,
        FlyoutShadow::Glow,
    ];

    #[test]
    fn styles() {
        assert_eq!(spec(FlyoutShadow::Off), None);
        for s in ALL.iter().filter_map(|s| spec(*s)) {
            assert!(s.blur > 0.0 && s.alpha > 0.0 && s.alpha <= 1.0);
        }
        assert!(spec(FlyoutShadow::Glow).unwrap().accent);
        assert!(!spec(FlyoutShadow::Soft).unwrap().accent);
    }

    #[test]
    fn margins_cover_blur_and_offset() {
        let soft = spec(FlyoutShadow::Soft).unwrap();
        let (l, t, r, b) = margins(&soft, 1.0);
        let reach = 3 * box_radius(&soft, 1.0) as i32 + 1;
        assert_eq!((l, r, t), (reach, reach, reach));
        assert_eq!(b, reach + 6); // the shadow falls downwards
        // Twice the DPI, twice the room.
        let (l2, ..) = margins(&soft, 2.0);
        assert!((l2 - 2 * l).abs() <= 2);
        let sharp = spec(FlyoutShadow::Sharp).unwrap();
        let (l, t, r, b) = margins(&sharp, 1.0);
        assert!(r > l && b > t);
    }

    #[test]
    fn blur_spreads_and_keeps_the_amount() {
        let (w, h) = (40, 40);
        let mut m = vec![0u8; w * h];
        for y in 10..30 {
            for x in 10..30 {
                m[y * w + x] = 255;
            }
        }
        let before: u32 = m.iter().map(|&v| v as u32).sum();
        blur(&mut m, w, h, 2);
        let after: u32 = m.iter().map(|&v| v as u32).sum();
        // Nothing is lost while the shadow stays inside the mask.
        assert!((before as i64 - after as i64).abs() < before as i64 / 50);
        // It spreads past the edge, fading out, and stays symmetric.
        assert!(m[20 * w + 8] > 0 && m[20 * w + 8] < 255);
        assert_eq!(m[20 * w + 3], 0); // beyond the reach (3 passes × radius 2)
        assert_eq!(m[20 * w + 9], m[20 * w + 30]);
        // The middle stays solid.
        assert_eq!(m[20 * w + 20], 255);
    }

    #[test]
    fn blur_edge_cases() {
        let mut m = vec![7u8; 4];
        blur(&mut m, 2, 2, 0);
        assert_eq!(m, [7; 4]);
        blur(&mut m, 0, 0, 3); // empty: nothing happens
        let mut short = vec![1u8; 3];
        blur(&mut short, 2, 2, 1); // too short for 2×2: left alone
        assert_eq!(short, [1; 3]);
    }
}
