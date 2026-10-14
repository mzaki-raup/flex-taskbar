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

/// Whether the window at `win` counts as running full screen on the monitor
/// at `mon`, so a bar there should get out of its way. Both rectangles are
/// `(left, top, right, bottom)` in virtual-screen pixels; `framed` says the
/// window still has a caption or a sizing border of its own.
///
/// Reaching every edge of the monitor is not enough on its own. Whenever
/// nothing reserves space on an edge — a bar of ours that auto-hides, or a
/// Windows taskbar set to hide itself — an ordinary maximized window covers
/// the monitor too. Windows 10's shell calls those full screen and tells
/// every app bar on every screen to disappear, which is why this is decided
/// here instead of taken on trust: a window really running full screen has
/// dropped its frame.
pub fn full_screen_on(win: (i32, i32, i32, i32), mon: (i32, i32, i32, i32), framed: bool) -> bool {
    !framed && win.0 <= mon.0 && win.1 <= mon.1 && win.2 >= mon.2 && win.3 >= mon.3
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

    #[test]
    fn full_screen_needs_the_whole_monitor_and_no_frame() {
        let mon = (0, 0, 1920, 1080);
        // Frameless and edge to edge: a game or a video player.
        assert!(full_screen_on((0, 0, 1920, 1080), mon, false));
        // Some report a rectangle a little larger than the monitor.
        assert!(full_screen_on((-1, -1, 1921, 1081), mon, false));
        // Maximized, but it kept its caption and sizing border: not full
        // screen, however much of the monitor it covers. This is what
        // Windows 10 calls full screen and what used to hide a bar set
        // never to hide.
        assert!(!full_screen_on((0, 0, 1920, 1080), mon, true));
        // Not quite covering it: a window above a bar that reserves space.
        assert!(!full_screen_on((0, 0, 1920, 1032), mon, false));
    }

    #[test]
    fn full_screen_is_decided_per_monitor() {
        let (left, right) = ((0, 0, 1920, 1080), (1920, 0, 3840, 1080));
        // A game filling the left screen leaves the right one alone, so the
        // bar there stays where it is.
        assert!(full_screen_on((0, 0, 1920, 1080), left, false));
        assert!(!full_screen_on((0, 0, 1920, 1080), right, false));
    }
}
