#![cfg_attr(not(windows), allow(dead_code))]

//! Which monitors carry a bar, kept free of Win32 so it can be unit-tested.
//!
//! Windows names each attached monitor (`\\.\DISPLAY1`), and that name is what
//! the settings store when one screen is chosen. Names are reused as monitors
//! come and go, so a stored name can point at nothing: choosing falls back to
//! the main screen rather than leaving the user without a bar.

use serde::{Deserialize, Serialize};

/// Which screens the settings ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Monitors {
    /// Only the main screen — what versions before 2.1 did.
    #[default]
    Primary,
    /// A bar on every screen, each docked and reserving space on its own.
    All,
    /// Only the screen with this device name, or the main one if it has been
    /// detached since.
    Chosen(String),
}

/// One attached monitor, as much of it as choosing needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// Windows' device name, e.g. `\\.\DISPLAY1`.
    pub device: String,
    /// Monitor bounds in virtual-screen pixels: (left, top, right, bottom).
    pub bounds: (i32, i32, i32, i32),
    /// The part left over by the taskbar and other app bars.
    pub work: (i32, i32, i32, i32),
    pub primary: bool,
}

impl Screen {
    pub fn width(&self) -> i32 {
        self.bounds.2 - self.bounds.0
    }

    pub fn height(&self) -> i32 {
        self.bounds.3 - self.bounds.1
    }
}

/// The main screen, or the first one if Windows flagged none (it always does,
/// but an empty flag shouldn't cost the user their bar).
fn main_screen(screens: &[Screen]) -> Option<&Screen> {
    screens.iter().find(|s| s.primary).or_else(|| screens.first())
}

/// The screens that should carry a bar, in the order given.
pub fn chosen<'a>(want: &Monitors, screens: &'a [Screen]) -> Vec<&'a Screen> {
    match want {
        Monitors::All => screens.iter().collect(),
        Monitors::Primary => main_screen(screens).into_iter().collect(),
        Monitors::Chosen(device) => {
            screens.iter().find(|s| s.device == *device).or_else(|| main_screen(screens)).into_iter().collect()
        }
    }
}

/// How a screen reads in the settings: `Screen 2 — 2560 × 1440 (main)`.
/// `index` is zero-based; people count screens from one, as Windows does.
pub fn label(index: usize, screen: &Screen) -> String {
    let mut s = format!("Screen {} — {} × {}", index + 1, screen.width(), screen.height());
    if screen.primary {
        s.push_str(" (main)");
    }
    s
}

/// The choices the settings offer, in order: the main screen, every screen,
/// then each attached screen on its own.
pub fn options(screens: &[Screen]) -> Vec<String> {
    let mut v = vec!["Main screen only".to_string(), "Every screen".to_string()];
    v.extend(screens.iter().enumerate().map(|(i, s)| label(i, s)));
    v
}

/// Which choice a setting is now. A screen that has been detached has no row
/// of its own any more, so it reads as the main screen — which is where its
/// bar went (see `chosen`).
pub fn option_of(want: &Monitors, screens: &[Screen]) -> usize {
    match want {
        Monitors::Primary => 0,
        Monitors::All => 1,
        Monitors::Chosen(device) => screens.iter().position(|s| s.device == *device).map_or(0, |i| i + 2),
    }
}

/// What choosing row `index` means. Out of range (a screen detached while the
/// list was open) means the main screen.
pub fn from_option(index: usize, screens: &[Screen]) -> Monitors {
    match index {
        0 => Monitors::Primary,
        1 => Monitors::All,
        i => screens.get(i - 2).map_or(Monitors::Primary, |s| Monitors::Chosen(s.device.clone())),
    }
}

/// What has to change to go from the bars there are now to the ones wanted:
/// the devices whose bar goes, and the devices that gain one. A screen in both
/// lists keeps the bar it has — its icons, DPI and hover state stay put, and
/// it doesn't flicker when another screen is attached or detached.
pub fn plan(have: &[String], want: &[Screen]) -> (Vec<String>, Vec<String>) {
    let drop = have.iter().filter(|d| !want.iter().any(|s| s.device == **d)).cloned().collect();
    let add = want.iter().filter(|s| !have.contains(&s.device)).map(|s| s.device.clone()).collect();
    (drop, add)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(device: &str, w: i32, h: i32, primary: bool) -> Screen {
        Screen { device: device.into(), bounds: (0, 0, w, h), work: (0, 0, w, h), primary }
    }

    fn two() -> Vec<Screen> {
        vec![screen(r"\\.\DISPLAY2", 1920, 1080, false), screen(r"\\.\DISPLAY1", 2560, 1440, true)]
    }

    #[test]
    fn no_screens_means_no_bars() {
        for want in [Monitors::Primary, Monitors::All, Monitors::Chosen(r"\\.\DISPLAY1".into())] {
            assert!(chosen(&want, &[]).is_empty(), "{want:?} on no screens");
        }
    }

    #[test]
    fn all_takes_every_screen_in_order() {
        let screens = two();
        let got = chosen(&Monitors::All, &screens);
        assert_eq!(got.iter().map(|s| s.device.as_str()).collect::<Vec<_>>(), [r"\\.\DISPLAY2", r"\\.\DISPLAY1"]);
    }

    #[test]
    fn primary_takes_the_main_screen_wherever_it_is_listed() {
        let screens = two();
        let got = chosen(&Monitors::Primary, &screens);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].device, r"\\.\DISPLAY1");
    }

    #[test]
    fn primary_falls_back_to_the_first_screen_when_none_is_flagged() {
        let screens = vec![screen(r"\\.\DISPLAY2", 1920, 1080, false)];
        assert_eq!(chosen(&Monitors::Primary, &screens)[0].device, r"\\.\DISPLAY2");
    }

    #[test]
    fn a_chosen_screen_is_the_only_one_with_a_bar() {
        let screens = two();
        let got = chosen(&Monitors::Chosen(r"\\.\DISPLAY2".into()), &screens);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].device, r"\\.\DISPLAY2");
    }

    #[test]
    fn a_chosen_screen_that_was_detached_falls_back_to_the_main_one() {
        let screens = two();
        let got = chosen(&Monitors::Chosen(r"\\.\DISPLAY9".into()), &screens);
        assert_eq!(got.len(), 1, "one bar, not none");
        assert_eq!(got[0].device, r"\\.\DISPLAY1", "the main screen");
    }

    #[test]
    fn the_options_are_the_two_modes_then_one_row_per_screen() {
        let screens = two();
        assert_eq!(
            options(&screens),
            ["Main screen only", "Every screen", "Screen 1 — 1920 × 1080", "Screen 2 — 2560 × 1440 (main)"]
        );
    }

    #[test]
    fn every_option_round_trips_through_the_setting_it_means() {
        let screens = two();
        for i in 0..options(&screens).len() {
            let want = from_option(i, &screens);
            assert_eq!(option_of(&want, &screens), i, "option {i} came back as {want:?}");
        }
    }

    #[test]
    fn a_chosen_screen_no_longer_attached_reads_as_the_main_one() {
        let screens = two();
        let gone = Monitors::Chosen(r"\\.\DISPLAY9".into());
        assert_eq!(option_of(&gone, &screens), 0, "no row of its own, so the main screen");
    }

    #[test]
    fn an_option_past_the_end_means_the_main_screen() {
        assert_eq!(from_option(99, &two()), Monitors::Primary);
    }

    #[test]
    fn the_first_bars_are_all_new_and_none_go() {
        let screens = two();
        let (drop, add) = plan(&[], &screens);
        assert!(drop.is_empty());
        assert_eq!(add, [r"\\.\DISPLAY2", r"\\.\DISPLAY1"]);
    }

    #[test]
    fn a_screen_that_keeps_its_bar_is_left_alone() {
        let screens = two();
        let have = vec![r"\\.\DISPLAY2".to_string()];
        let (drop, add) = plan(&have, &screens);
        assert!(drop.is_empty(), "the bar it already has stays");
        assert_eq!(add, [r"\\.\DISPLAY1"], "only the screen without one gains a bar");
    }

    #[test]
    fn a_detached_screens_bar_goes_and_the_other_stays() {
        let left = vec![screen(r"\\.\DISPLAY1", 2560, 1440, true)];
        let have = vec![r"\\.\DISPLAY2".to_string(), r"\\.\DISPLAY1".to_string()];
        let (drop, add) = plan(&have, &left);
        assert_eq!(drop, [r"\\.\DISPLAY2"]);
        assert!(add.is_empty());
    }

    #[test]
    fn switching_to_one_screen_drops_only_the_others() {
        let screens = two();
        let have: Vec<String> = screens.iter().map(|s| s.device.clone()).collect();
        let only = chosen(&Monitors::Chosen(r"\\.\DISPLAY1".into()), &screens);
        let want: Vec<Screen> = only.into_iter().cloned().collect();
        let (drop, add) = plan(&have, &want);
        assert_eq!(drop, [r"\\.\DISPLAY2"]);
        assert!(add.is_empty(), "DISPLAY1 keeps the bar it has");
    }

    #[test]
    fn nothing_changes_when_the_screens_and_the_setting_stay_the_same() {
        let screens = two();
        let have: Vec<String> = screens.iter().map(|s| s.device.clone()).collect();
        let (drop, add) = plan(&have, &screens);
        assert!(drop.is_empty() && add.is_empty());
    }

    #[test]
    fn labels_count_from_one_and_mark_the_main_screen() {
        let screens = two();
        assert_eq!(label(0, &screens[0]), "Screen 1 — 1920 × 1080");
        assert_eq!(label(1, &screens[1]), "Screen 2 — 2560 × 1440 (main)");
    }
}
