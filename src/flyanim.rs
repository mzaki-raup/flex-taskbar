#![cfg_attr(not(windows), allow(dead_code))]

//! How a flyout appears (`Appearance::flyout_animation`): fading in, sliding,
//! scaling out of its button, pulled out of the bar like a drawer, or the
//! macOS "genie" effect. Pure, so it can be unit-tested off Windows.
//!
//! A frame is described in the flyout's own terms: *along* runs from the side
//! nearest the bar (0) in the direction the flyout opens; *across* is the
//! other axis (along the bar). A frame is a list of bands, each a slice of
//! the finished flyout drawn stretched into a rectangle.

use crate::appearance::FlyoutAnim;

/// A slice of the finished flyout (`src`, along it; the slice spans the
/// whole width across) drawn into `along` × `across` of the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub src: (f32, f32),
    pub along: (f32, f32),
    pub across: (f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub opacity: f32,
    pub bands: Vec<Band>,
}

/// The finished flyout, as one band.
fn whole(len: f32, width: f32) -> Band {
    Band { src: (0.0, len), along: (0.0, len), across: (0.0, width) }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The frame at `t` (0 = start, 1 = done) for a flyout `len` long and `width`
/// wide, opening from a button spanning `anchor` across it.
pub fn frame(style: FlyoutAnim, t: f32, len: f32, width: f32, anchor: (f32, f32)) -> Frame {
    let t = t.clamp(0.0, 1.0);
    let e = ease_out(t);
    let (a0, a1) = (anchor.0.clamp(0.0, width), anchor.1.clamp(0.0, width));
    let (ac, aw) = ((a0 + a1) / 2.0, (a1 - a0).max(1.0));
    let one = |opacity: f32, b: Band| Frame { opacity: opacity.clamp(0.0, 1.0), bands: vec![b] };
    if t >= 1.0 {
        return one(1.0, whole(len, width));
    }
    match style {
        FlyoutAnim::Off => one(1.0, whole(len, width)),
        FlyoutAnim::Fade => one(e, whole(len, width)),
        FlyoutAnim::Slide => {
            // In from a little nearer the bar, fading in.
            let off = (1.0 - e) * len * 0.15;
            one(e, Band { src: (0.0, len), along: (-off, len - off), across: (0.0, width) })
        }
        FlyoutAnim::Scale => {
            // Grows out of the button.
            let k = lerp(0.4, 1.0, e);
            one(
                (e * 1.5).min(1.0),
                Band { src: (0.0, len), along: (0.0, len * k), across: (ac - ac * k, ac + (width - ac) * k) },
            )
        }
        FlyoutAnim::Drawer => {
            // Pulled out of the bar: the far end comes first.
            let shown = len * e;
            one(1.0, Band { src: (len - shown, len), along: (0.0, shown), across: (0.0, width) })
        }
        FlyoutAnim::Genie => {
            // Stretched out of the button: the far end widens first, the end
            // at the bar stays as narrow as the button until last.
            let reach = smoothstep(t * 1.25);
            let n = (len / 3.0).clamp(8.0, 64.0) as usize;
            let step = len / n as f32;
            let bands = (0..n)
                .map(|i| {
                    let (s0, s1) = (i as f32 * step, (i + 1) as f32 * step);
                    let u = (s0 + s1) / 2.0 / len;
                    let widen = smoothstep(t * 1.6 - (1.0 - u) * 0.6);
                    let (c, w) = (lerp(ac, width / 2.0, widen), lerp(aw, width, widen));
                    Band { src: (s0, s1), along: (s0 * reach, s1 * reach), across: (c - w / 2.0, c + w / 2.0) }
                })
                .collect();
            Frame { opacity: (t * 4.0).min(1.0), bands }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: [FlyoutAnim; 6] = [
        FlyoutAnim::Off,
        FlyoutAnim::Fade,
        FlyoutAnim::Slide,
        FlyoutAnim::Scale,
        FlyoutAnim::Drawer,
        FlyoutAnim::Genie,
    ];

    #[test]
    fn every_style_ends_on_the_finished_flyout() {
        for s in STYLES {
            let f = frame(s, 1.0, 300.0, 200.0, (20.0, 60.0));
            assert_eq!(f.opacity, 1.0);
            assert_eq!(f.bands, vec![whole(300.0, 200.0)], "{s:?}");
        }
        // Off shows it finished straight away.
        assert_eq!(frame(FlyoutAnim::Off, 0.0, 300.0, 200.0, (0.0, 10.0)).bands, vec![whole(300.0, 200.0)]);
    }

    #[test]
    fn scale_grows_out_of_the_button() {
        let f = frame(FlyoutAnim::Scale, 0.0, 300.0, 200.0, (20.0, 60.0));
        let b = f.bands[0];
        assert_eq!(b.along.0, 0.0);
        assert!(b.along.1 < 300.0 * 0.5);
        // The button's centre (40) stays put.
        let k = (b.across.1 - b.across.0) / 200.0;
        assert!((b.across.0 + 40.0 * k - 40.0).abs() < 1e-3);
    }

    #[test]
    fn drawer_shows_the_far_end_first() {
        let f = frame(FlyoutAnim::Drawer, 0.3, 300.0, 200.0, (0.0, 10.0));
        let b = f.bands[0];
        assert_eq!(b.src.1, 300.0);
        assert!(b.src.0 > 0.0);
        assert!((b.along.1 - (b.src.1 - b.src.0)).abs() < 1e-3); // not stretched
        assert_eq!(f.opacity, 1.0);
    }

    #[test]
    fn genie_narrows_towards_the_button() {
        let f = frame(FlyoutAnim::Genie, 0.4, 300.0, 200.0, (20.0, 60.0));
        assert!(f.bands.len() >= 8);
        let w = |b: &Band| b.across.1 - b.across.0;
        let (near, far) = (f.bands.first().unwrap(), f.bands.last().unwrap());
        assert!(w(near) < w(far));
        // The end at the bar is still about the button's width, around it.
        assert!(w(near) < 60.0);
        assert!(near.across.0 >= 0.0 && near.across.1 <= 100.0);
        // Bands follow each other without gaps.
        for p in f.bands.windows(2) {
            assert!((p[0].along.1 - p[1].along.0).abs() < 1e-3);
            assert!((p[0].src.1 - p[1].src.0).abs() < 1e-3);
        }
        // At the start it is all squeezed into the button.
        let f = frame(FlyoutAnim::Genie, 0.0, 300.0, 200.0, (20.0, 60.0));
        assert!(f.bands.iter().all(|b| b.along.1 == 0.0 && (w(b) - 40.0).abs() < 1e-3));
    }

    #[test]
    fn fade_and_slide_fade_in() {
        let a = frame(FlyoutAnim::Fade, 0.2, 100.0, 100.0, (0.0, 10.0)).opacity;
        let b = frame(FlyoutAnim::Fade, 0.6, 100.0, 100.0, (0.0, 10.0)).opacity;
        assert!(0.0 < a && a < b && b < 1.0);
        let s = frame(FlyoutAnim::Slide, 0.2, 100.0, 100.0, (0.0, 10.0)).bands[0];
        assert!(s.along.0 < 0.0 && (s.along.1 - s.along.0 - 100.0).abs() < 1e-3);
    }
}
