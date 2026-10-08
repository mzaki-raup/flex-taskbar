#![cfg_attr(not(windows), allow(dead_code))]

//! Saved looks (`settings.looks`): named copies of the appearance settings
//! to switch between, and the rules for naming them and for the pictures
//! they refer to. Pure, so it can be unit-tested off Windows.

use crate::appearance::Appearance;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Look {
    pub name: String,
    pub appearance: Appearance,
}

/// `wanted`, or `wanted (2)`, `wanted (3)`… if a look already has that
/// name (compared without case). Blank names become "My look".
pub fn unique_name(existing: &[&str], wanted: &str) -> String {
    let tidy = tidy_name(wanted);
    let base = tidy.as_str();
    let base = if base.is_empty() { "My look" } else { base };
    let taken = |n: &str| existing.iter().any(|e| e.trim().eq_ignore_ascii_case(n));
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} ({i})")).find(|n| !taken(n)).unwrap_or_default()
}

/// The longest name a look (or a bar) may have, in characters.
pub const MAX_NAME: usize = 60;

/// A name as typed, made fit for a menu: on one line, without characters
/// that reorder text, trimmed and at most [`MAX_NAME`] characters.
pub fn tidy_name(wanted: &str) -> String {
    let plain = crate::folders::plain_text(wanted);
    plain.trim().chars().take(MAX_NAME).collect::<String>().trim_end().to_string()
}

/// Saves `appearance` as `name`, replacing a look of the same name.
pub fn save(looks: &mut Vec<Look>, name: &str, appearance: &Appearance) {
    let name = tidy_name(name);
    match looks.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&name)) {
        Some(l) => l.appearance = appearance.clone(),
        None => looks.push(Look { name, appearance: appearance.clone() }),
    }
}

/// The pictures (file names in `icons\`) a look shows.
pub fn pictures(a: &Appearance) -> Vec<String> {
    a.indicator_image.iter().chain(a.all_icon.iter()).cloned().collect()
}

/// Points a look at a picture's new file name.
pub fn rename_picture(a: &mut Appearance, from: &str, to: &str) {
    for p in [&mut a.indicator_image, &mut a.all_icon].into_iter().flatten() {
        if p == from {
            *p = to.to_string();
        }
    }
}

/// Whether any saved look (or `current`) still shows the picture `file`
/// (so removing it from the current look mustn't delete it).
pub fn picture_in_use(looks: &[Look], current: &Appearance, file: &str) -> bool {
    let shows = |a: &Appearance| pictures(a).iter().any(|p| p == file);
    shows(current) || looks.iter().any(|l| shows(&l.appearance))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(unique_name(&[], "Night"), "Night");
        assert_eq!(unique_name(&["Night"], "night"), "night (2)");
        assert_eq!(unique_name(&["Night", "Night (2)"], "Night"), "Night (3)");
        assert_eq!(unique_name(&[], "   "), "My look");
        assert_eq!(unique_name(&["My look"], ""), "My look (2)");
        // On one line, nothing reordering it, and not endless.
        assert_eq!(unique_name(&[], "Night\nshift\u{202E}"), "Night shift");
        assert_eq!(unique_name(&[], &"x".repeat(500)).chars().count(), MAX_NAME);
        assert_eq!(tidy_name("  \u{200F}  "), "");
    }

    #[test]
    fn saving_replaces_by_name() {
        let mut looks = Vec::new();
        let a = Appearance { opacity: 50, ..Default::default() };
        save(&mut looks, " Glass ", &a);
        save(&mut looks, "glass", &Appearance { opacity: 70, ..Default::default() });
        save(&mut looks, "Solid", &Appearance::default());
        assert_eq!(looks.len(), 2);
        assert_eq!((looks[0].name.as_str(), looks[0].appearance.opacity), ("Glass", 70));
    }

    #[test]
    fn pictures_of_a_look() {
        let mut a =
            Appearance { indicator_image: Some("i.png".into()), all_icon: Some("a.png".into()), ..Default::default() };
        assert_eq!(pictures(&a), ["i.png", "a.png"]);
        rename_picture(&mut a, "a.png", "b.png");
        assert_eq!(a.all_icon.as_deref(), Some("b.png"));
        assert_eq!(pictures(&Appearance::default()), Vec::<String>::new());
        let looks = vec![Look { name: "x".into(), appearance: a.clone() }];
        let current = Appearance::default();
        assert!(picture_in_use(&looks, &current, "b.png"));
        assert!(!picture_in_use(&looks, &current, "a.png"));
        assert!(picture_in_use(&[], &a, "i.png"));
    }
}
