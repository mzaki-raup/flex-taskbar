#![cfg_attr(not(windows), allow(dead_code))]

//! What an app's right-click menu offers beyond pinning: the files it was
//! last used on (its Windows jump list) and the quick actions Windows knows
//! how to do for it. Kept free of Win32 so it can be unit-tested; the shell
//! calls live in `win::appmenu`.

/// A quick action Windows can carry out for an app. Which ones an app offers
/// is asked of the shell rather than guessed, so a Store app — which has no
/// file on disk — simply never offers *Open file location*, and something
/// that can't be removed (Settings) never offers *Uninstall*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    OpenFileLocation,
    /// Start another copy of an app that is already open. This one is ours,
    /// not Windows': there is no shell verb for "again", and a plain click
    /// switches to the window instead of starting anything.
    RunNew,
    RunAsAdmin,
    Uninstall,
}

/// Every action, in the order they appear in the menu. *Run* sits directly
/// above *Run as administrator*, so the two ways of starting it read together.
pub const ACTIONS: [Action; 4] = [Action::OpenFileLocation, Action::RunNew, Action::RunAsAdmin, Action::Uninstall];

impl Action {
    /// The canonical verb Windows knows the action by, for the ones that are
    /// Windows'. Canonical verbs are not translated, so matching on them works
    /// in any language. `None` is ours to carry out.
    pub fn verb(self) -> Option<&'static str> {
        match self {
            Action::OpenFileLocation => Some("openfilelocation"),
            Action::RunAsAdmin => Some("runas"),
            Action::Uninstall => Some("uninstall"),
            Action::RunNew => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::OpenFileLocation => "Open file location",
            Action::RunNew => "Run",
            Action::RunAsAdmin => "Run as administrator",
            Action::Uninstall => "Uninstall",
        }
    }

    /// The action a verb reported by the shell means, if it is one we offer.
    /// Case is ignored: the shell spells the same verb `runas` for a Store
    /// app and `RunAs` for a desktop program.
    pub fn from_verb(verb: &str) -> Option<Action> {
        let verb = verb.trim();
        if verb.is_empty() {
            return None;
        }
        ACTIONS.into_iter().find(|a| a.verb().is_some_and(|v| v.eq_ignore_ascii_case(verb)))
    }
}

/// One line of the part of the menu this module describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// The recent file at this index.
    Recent(usize),
    Separator,
    Action(Action),
}

/// The menu's lines, given how many recent files there are and which actions
/// Windows offers. A separator appears only where there is something on both
/// sides of it, so the menu never opens or ends with a line.
pub fn entries(recent: usize, actions: &[Action], max_recent: usize) -> Vec<Entry> {
    let shown = recent.min(max_recent);
    let mut v: Vec<Entry> = (0..shown).map(Entry::Recent).collect();
    if shown > 0 && !actions.is_empty() {
        v.push(Entry::Separator);
    }
    v.extend(actions.iter().copied().map(Entry::Action));
    v
}

/// A file name as it should read in a menu. Both ends of a name carry
/// meaning — what it is, and what kind of file — so a long one is shortened
/// in the middle rather than at the end. Never splits a character.
pub fn menu_name(name: &str, max_chars: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max_chars {
        return name.to_string();
    }
    // Too short to shorten and still say anything: just cut it.
    if max_chars <= 2 {
        return chars.into_iter().take(max_chars).collect();
    }
    let keep = max_chars - 1; // room for the ellipsis
    let tail = keep / 2;
    let head = keep - tail;
    let mut s: String = chars[..head].iter().collect();
    s.push('…');
    s.extend(&chars[chars.len() - tail..]);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_windows_knows_is_found_from_the_verb_it_reports() {
        for a in ACTIONS {
            match a.verb() {
                Some(v) => assert_eq!(Action::from_verb(v), Some(a)),
                // Ours, so the shell never reports it and never should.
                None => assert_eq!(a, Action::RunNew),
            }
        }
    }

    #[test]
    fn run_sits_directly_above_run_as_administrator() {
        let at = |a: Action| ACTIONS.iter().position(|x| *x == a).unwrap();
        assert_eq!(at(Action::RunNew) + 1, at(Action::RunAsAdmin));
    }

    #[test]
    fn our_own_action_is_never_matched_from_a_verb() {
        // A shell handler reporting nothing must not be read as "Run".
        for empty in ["", "   "] {
            assert_eq!(Action::from_verb(empty), None, "{empty:?}");
        }
        assert_eq!(Action::RunNew.verb(), None);
    }

    #[test]
    fn the_shells_spelling_of_a_verb_doesnt_matter() {
        // Windows 11 reports `runas` for Store apps and `RunAs` for desktop
        // programs, and `OpenFileLocation` with capitals.
        assert_eq!(Action::from_verb("RunAs"), Some(Action::RunAsAdmin));
        assert_eq!(Action::from_verb("runas"), Some(Action::RunAsAdmin));
        assert_eq!(Action::from_verb("OpenFileLocation"), Some(Action::OpenFileLocation));
        assert_eq!(Action::from_verb("Uninstall"), Some(Action::Uninstall));
        assert_eq!(Action::from_verb("  Uninstall  "), Some(Action::Uninstall), "trimmed");
    }

    #[test]
    fn verbs_we_dont_offer_are_ignored() {
        for other in ["open", "link", "pintohome", "PinToStartScreen", "OpenNewWindow", ""] {
            assert_eq!(Action::from_verb(other), None, "{other}");
        }
    }

    #[test]
    fn an_app_with_nothing_to_offer_adds_nothing_to_the_menu() {
        assert!(entries(0, &[], 10).is_empty());
    }

    #[test]
    fn recent_files_alone_end_without_a_separator() {
        let v = entries(2, &[], 10);
        assert_eq!(v, [Entry::Recent(0), Entry::Recent(1)]);
    }

    #[test]
    fn actions_alone_start_without_a_separator() {
        let v = entries(0, &[Action::Uninstall], 10);
        assert_eq!(v, [Entry::Action(Action::Uninstall)]);
    }

    #[test]
    fn one_separator_sits_between_the_two_groups() {
        let v = entries(2, &[Action::RunAsAdmin, Action::Uninstall], 10);
        assert_eq!(
            v,
            [
                Entry::Recent(0),
                Entry::Recent(1),
                Entry::Separator,
                Entry::Action(Action::RunAsAdmin),
                Entry::Action(Action::Uninstall),
            ]
        );
    }

    #[test]
    fn no_more_recent_files_are_shown_than_the_limit() {
        let v = entries(50, &[], 3);
        assert_eq!(v, [Entry::Recent(0), Entry::Recent(1), Entry::Recent(2)]);
    }

    #[test]
    fn a_name_that_fits_is_left_alone() {
        assert_eq!(menu_name("notes.txt", 40), "notes.txt");
        assert_eq!(menu_name("exactly-ten", 11), "exactly-ten", "the limit itself fits");
    }

    #[test]
    fn a_long_name_is_shortened_in_the_middle_keeping_its_extension() {
        let got = menu_name("a-very-long-report-about-everything.docx", 20);
        assert_eq!(got.chars().count(), 20);
        assert!(got.starts_with("a-very-lon"), "{got}");
        assert!(got.ends_with(".docx"), "the kind of file still shows: {got}");
        assert!(got.contains('…'));
    }

    #[test]
    fn shortening_never_splits_a_character() {
        let name = "ドキュメント-very-long-name-here.txt";
        let got = menu_name(name, 15);
        assert_eq!(got.chars().count(), 15);
        assert!(got.starts_with('ド'), "{got}");
    }

    #[test]
    fn a_silly_limit_still_gives_something_short() {
        for max in 0..=3 {
            let got = menu_name("some-long-file-name.txt", max);
            assert!(got.chars().count() <= max, "max {max} gave {got:?}");
        }
    }
}
