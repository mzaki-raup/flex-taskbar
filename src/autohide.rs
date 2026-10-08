#![cfg_attr(not(windows), allow(dead_code))]

//! Auto-hiding the bar (`Appearance::auto_hide`): where the bar is and how
//! opaque, part-way between shown and hidden, and how that changes over
//! time. Pure, so it can be unit-tested off Windows.

use crate::appearance::AutoHide;
use crate::striplayout::Edge;

/// How long sliding or fading takes, in milliseconds.
pub const DURATION_MS: f32 = 180.0;

/// How much of a hidden bar stays on screen, in pixels at 96 DPI: enough to
/// catch the pointer at the screen edge.
pub const REVEAL_DIP: i32 = 2;

/// Moves `amount` (0 = shown, 1 = hidden) towards `target` by `dt_ms` of
/// animation.
pub fn step(amount: f32, target: f32, dt_ms: f32) -> f32 {
    let delta = dt_ms / DURATION_MS;
    if target > amount { (amount + delta).min(target) } else { (amount - delta).max(target) }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where the bar's window goes (offset from where it docks, towards the
/// screen edge) and its opacity (0–255), `amount` of the way to hidden.
/// `thickness`: the window's size across the bar; `reveal`: what stays on
/// screen when hidden.
pub fn placement(style: AutoHide, amount: f32, edge: Edge, thickness: i32, reveal: i32) -> ((i32, i32), u8) {
    let full = (thickness - reveal).max(0);
    let (along_out, alpha) = match style {
        AutoHide::Off => (0, 255),
        AutoHide::Slide => ((full as f32 * ease(amount)).round() as i32, 255),
        // Fades out in place, then waits off screen (a thin line showing).
        AutoHide::Fade if amount >= 1.0 => (full, 255),
        AutoHide::Fade => (0, ((1.0 - ease(amount)) * 255.0).round() as u8),
    };
    // Out means towards the screen edge: away from where flyouts open.
    let (ox, oy) = edge.opening();
    (((-ox * along_out as f32) as i32, (-oy * along_out as f32) as i32), alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping() {
        assert_eq!(step(0.0, 1.0, DURATION_MS / 2.0), 0.5);
        assert_eq!(step(0.9, 1.0, DURATION_MS), 1.0); // never past the target
        assert_eq!(step(0.3, 0.0, DURATION_MS), 0.0);
        assert_eq!(step(0.5, 0.5, 10.0), 0.5);
    }

    #[test]
    fn sliding_towards_the_edge() {
        // Shown: where it docks.
        assert_eq!(placement(AutoHide::Slide, 0.0, Edge::Bottom, 48, 2), ((0, 0), 255));
        // Hidden: all but the reveal line past the screen edge.
        assert_eq!(placement(AutoHide::Slide, 1.0, Edge::Bottom, 48, 2), ((0, 46), 255));
        assert_eq!(placement(AutoHide::Slide, 1.0, Edge::Top, 48, 2), ((0, -46), 255));
        assert_eq!(placement(AutoHide::Slide, 1.0, Edge::Left, 40, 2), ((-38, 0), 255));
        assert_eq!(placement(AutoHide::Slide, 1.0, Edge::Right, 40, 2), ((38, 0), 255));
        // Part-way, eased.
        let ((_, y), _) = placement(AutoHide::Slide, 0.5, Edge::Bottom, 48, 2);
        assert_eq!(y, 23);
    }

    #[test]
    fn fading_then_waiting_off_screen() {
        let ((_, y), a) = placement(AutoHide::Fade, 0.5, Edge::Bottom, 48, 2);
        assert_eq!((y, a), (0, 128));
        assert_eq!(placement(AutoHide::Fade, 0.0, Edge::Bottom, 48, 2), ((0, 0), 255));
        // Fully hidden: off screen with the line showing, at full opacity so
        // the line can catch the pointer.
        assert_eq!(placement(AutoHide::Fade, 1.0, Edge::Bottom, 48, 2), ((0, 46), 255));
        // Off: never moves.
        assert_eq!(placement(AutoHide::Off, 1.0, Edge::Bottom, 48, 2), ((0, 0), 255));
    }
}
