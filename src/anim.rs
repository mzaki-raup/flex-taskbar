#![cfg_attr(not(windows), allow(dead_code))]

//! Hover animations for the strip's icons, in the spirit of the macOS Dock.
//! Pure maths, so it can be unit-tested off Windows: the strip feeds in the
//! pointer and the time, and draws each icon at the scale and offset
//! returned here.

use crate::appearance::HoverAnim;

/// How an icon is drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effect {
    /// Size multiplier (1 = normal).
    pub scale: f32,
    /// Shift towards the screen (away from the bar's edge), as a fraction of
    /// the room there is for it (0..=1).
    pub lift: f32,
}

pub const REST: Effect = Effect { scale: 1.0, lift: 0.0 };

/// Bounce: two hops, the second lower, over this long.
pub const BOUNCE_MS: f32 = 700.0;
/// Pulse: a quick grow-and-settle.
pub const PULSE_MS: f32 = 320.0;

/// How strongly each icon is magnified (0..=1) with the pointer at `pointer`
/// along the bar (`None`: off the bar). Icons within `reach` of the pointer
/// grow, the nearest most, falling off smoothly like the Dock's.
pub fn magnify_targets(centres: &[f32], pointer: Option<f32>, reach: f32) -> Vec<f32> {
    centres
        .iter()
        .map(|&c| match pointer {
            Some(p) if reach > 0.0 => {
                let d = ((c - p).abs() / reach).min(1.0);
                // Cosine falloff: 1 under the pointer, 0 at `reach`.
                (0.5 + 0.5 * (d * std::f32::consts::PI).cos()).max(0.0)
            }
            _ => 0.0,
        })
        .collect()
}

/// Moves `current` towards `target`, smoothly, over `dt_ms`: about 90% of
/// the way in 150 ms, independent of the frame rate.
pub fn approach(current: f32, target: f32, dt_ms: f32) -> f32 {
    let k = 1.0 - (-dt_ms / 65.0).exp();
    let next = current + (target - current) * k;
    if (next - target).abs() < 0.002 { target } else { next }
}

/// Height of the bounce (0..=1) `ms` after the pointer arrived; `None` once
/// it has finished.
pub fn bounce(ms: f32) -> Option<f32> {
    if !(0.0..BOUNCE_MS).contains(&ms) {
        return None;
    }
    // Two arcs: the first full height, the second 40%.
    let half = BOUNCE_MS * 0.6;
    Some(if ms < half {
        (ms / half * std::f32::consts::PI).sin()
    } else {
        0.4 * ((ms - half) / (BOUNCE_MS - half) * std::f32::consts::PI).sin()
    })
}

/// Extra size of the pulse (0..=1) `ms` after the pointer arrived; `None` once
/// it has finished.
pub fn pulse(ms: f32) -> Option<f32> {
    (0.0..PULSE_MS).contains(&ms).then(|| (ms / PULSE_MS * std::f32::consts::PI).sin())
}

/// The effect for one icon.
///
/// - `level`: its smoothed magnify/lift amount (0..=1);
/// - `since_enter`: milliseconds since the pointer arrived on it, for the
///   one-shot animations (bounce, pulse), if it is the hovered icon;
/// - `max_scale`: the largest size that still fits in the bar.
pub fn effect(kind: HoverAnim, level: f32, since_enter: Option<f32>, max_scale: f32) -> Effect {
    let max_scale = max_scale.max(1.0);
    match kind {
        HoverAnim::Off => REST,
        HoverAnim::Magnify => Effect { scale: 1.0 + (max_scale - 1.0) * level, lift: 0.0 },
        HoverAnim::Lift => Effect { scale: 1.0 + (max_scale - 1.0).min(0.1) * level, lift: level },
        HoverAnim::Bounce => Effect { scale: 1.0, lift: since_enter.and_then(bounce).unwrap_or(0.0) },
        HoverAnim::Pulse => {
            let p = since_enter.and_then(pulse).unwrap_or(0.0);
            Effect { scale: 1.0 + (max_scale - 1.0).min(0.2) * p, lift: 0.0 }
        }
    }
}

/// Whether anything is still moving: a level not yet at its target, or a
/// one-shot animation still running.
pub fn busy(kind: HoverAnim, levels: &[f32], targets: &[f32], since_enter: Option<f32>) -> bool {
    match kind {
        HoverAnim::Off => false,
        HoverAnim::Magnify | HoverAnim::Lift => levels.iter().zip(targets).any(|(l, t)| l != t),
        HoverAnim::Bounce => since_enter.and_then(bounce).is_some(),
        HoverAnim::Pulse => since_enter.and_then(pulse).is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnify_falls_off_with_distance() {
        let t = magnify_targets(&[0.0, 50.0, 100.0, 300.0], Some(0.0), 120.0);
        assert!((t[0] - 1.0).abs() < 1e-6);
        assert!(t[1] < t[0] && t[1] > t[2] && t[2] > 0.0);
        assert_eq!(t[3], 0.0);
        assert_eq!(magnify_targets(&[0.0, 50.0], None, 120.0), vec![0.0, 0.0]);
    }

    #[test]
    fn approach_settles_regardless_of_frame_rate() {
        let (mut a, mut b) = (0.0, 0.0);
        for _ in 0..10 {
            a = approach(a, 1.0, 16.0);
        }
        for _ in 0..5 {
            b = approach(b, 1.0, 32.0);
        }
        assert!((a - b).abs() < 0.01); // same time elapsed, same place
        assert!(a > 0.85 && a < 1.0);
        let mut c: f32 = 0.0;
        for _ in 0..100 {
            c = approach(c, 1.0, 16.0);
        }
        assert_eq!(c, 1.0); // snaps when close
    }

    #[test]
    fn one_shots_end() {
        assert_eq!(bounce(0.0), Some(0.0));
        assert!(bounce(BOUNCE_MS * 0.3).unwrap() > 0.99);
        assert!(bounce(BOUNCE_MS * 0.8).unwrap() < 0.45);
        assert_eq!(bounce(BOUNCE_MS), None);
        assert!(pulse(PULSE_MS / 2.0).unwrap() > 0.99);
        assert_eq!(pulse(PULSE_MS + 1.0), None);
    }

    #[test]
    fn effects() {
        assert_eq!(effect(HoverAnim::Off, 1.0, Some(100.0), 1.4), REST);
        assert_eq!(effect(HoverAnim::Magnify, 1.0, None, 1.4).scale, 1.4);
        assert_eq!(effect(HoverAnim::Magnify, 0.0, None, 1.4), REST);
        let lift = effect(HoverAnim::Lift, 1.0, None, 1.4);
        assert_eq!(lift.lift, 1.0);
        assert!(lift.scale > 1.0 && lift.scale <= 1.1);
        assert!(effect(HoverAnim::Bounce, 0.0, Some(BOUNCE_MS * 0.3), 1.4).lift > 0.99);
        assert_eq!(effect(HoverAnim::Bounce, 0.0, None, 1.4), REST);
        // No room to grow: nothing grows.
        assert_eq!(effect(HoverAnim::Magnify, 1.0, None, 0.8).scale, 1.0);
        assert!(busy(HoverAnim::Magnify, &[0.5], &[1.0], None));
        assert!(!busy(HoverAnim::Magnify, &[1.0], &[1.0], None));
        assert!(!busy(HoverAnim::Bounce, &[], &[], Some(BOUNCE_MS + 5.0)));
    }
}
