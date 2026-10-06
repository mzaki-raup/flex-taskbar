//! How the icon strip and its flyouts look: theme, colours, transparency,
//! border, corners, size. Pure data plus the colour resolution, so it can be
//! unit-tested off Windows.
//!
//! The defaults reproduce the original .NET FlexTaskbar bar.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Follow Windows' app theme (light or dark).
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockWidth {
    /// The whole width of the screen, like the Windows taskbar.
    Full,
    /// Just wide enough for its buttons, centred (a floating dock).
    Fit,
}

/// How the strip's icons react to the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HoverAnim {
    Off,
    /// The icon under the pointer grows, its neighbours a little (the Dock).
    Magnify,
    /// The icon rises towards the screen.
    Lift,
    /// The icon hops twice.
    Bounce,
    /// The icon briefly grows and settles.
    Pulse,
}

/// How a flyout appears when it opens (see `flyanim`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlyoutAnim {
    /// Shown at once.
    Off,
    Fade,
    /// Fades in, moving out a little from the bar.
    Slide,
    /// Grows out of its button.
    Scale,
    /// Pulled out of the bar, far end first.
    Drawer,
    /// Stretched out of its button, like macOS's genie effect.
    Genie,
}

/// Hiding the bar until the pointer reaches the screen edge (see
/// `autohide`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoHide {
    /// Always shown.
    Off,
    /// Slides out of view past the screen edge.
    Slide,
    /// Fades out.
    Fade,
}

/// The mark under apps that have a window open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunningMark {
    Off,
    /// A small dot, like the macOS Dock.
    Dot,
    /// A short line, like the Windows 11 taskbar.
    Line,
}

/// The shadow under every flyout (see `shadow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlyoutShadow {
    Off,
    /// Soft and a little below, like Windows 11's menus.
    Soft,
    /// Larger and further below: lifted off the desktop.
    Floating,
    /// The same all round.
    Even,
    /// Crisp and offset.
    Sharp,
    /// A halo in the accent colour.
    Glow,
}

/// The mark on a category (and subcategory) icon that says "this opens a
/// flyout".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Indicator {
    /// An accent circle with an arrow pointing where the flyout opens.
    Badge,
    /// Just the arrow.
    Arrow,
    /// A small accent dot.
    Dot,
    /// A folded corner in the accent colour.
    Corner,
    /// An accent bar along the icon's bottom edge.
    Underline,
    /// The user's own picture (`indicator_image`).
    Image,
    /// No mark.
    None,
}

/// An sRGB colour with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }

    pub const fn with_alpha(self, a: u8) -> Rgba {
        Rgba { a, ..self }
    }

    /// Relative luminance in 0..=1 (sRGB, no gamma correction; good enough to
    /// pick a readable text colour).
    pub fn luminance(&self) -> f32 {
        (0.299 * self.r as f32 + 0.587 * self.g as f32 + 0.114 * self.b as f32) / 255.0
    }

    /// `#RRGGBB` or `#RRGGBBAA`.
    pub fn parse(s: &str) -> Option<Rgba> {
        let hex = s.trim().strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            6 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: 255 }),
            8 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: byte(6)? }),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Rgba, D::Error> {
        let s = String::deserialize(d)?;
        Rgba::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("not a colour: {s}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeMode,
    /// Highlight for open/pressed buttons. `None`: the original light blue.
    pub accent: Option<Rgba>,
    /// Bar and flyout background. `None`: the theme's colour.
    pub background: Option<Rgba>,
    /// Background opacity in percent (100 = solid). Icons and text stay solid.
    pub opacity: u8,
    /// Border colour. `None`: a faint line in the theme's text colour.
    pub border: Option<Rgba>,
    /// Border thickness in DIPs (0 = none).
    pub border_width: u32,
    /// Corner radius of the bar in DIPs (0 = square, like the Windows taskbar).
    pub corner_radius: u32,
    /// Gap between the bar and the screen edges in DIPs (0 = docked flush).
    pub margin: u32,
    pub dock_width: DockWidth,
    /// Animation when the pointer moves over the strip's icons.
    pub hover_animation: HoverAnim,
    /// The mark on category icons.
    pub indicator: Indicator,
    /// File name (in the data folder's `icons`) of the picture for
    /// `Indicator::Image`.
    pub indicator_image: Option<String>,
    /// Size of the mark, in percent of the icon.
    pub indicator_size: u32,
    /// Icon size on the bar and in flyout tiles, in DIPs.
    pub icon_size: u32,
    /// App tiles per row in a category flyout.
    pub flyout_columns: u32,
    /// Corner radius of the flyouts in DIPs (0 = square).
    pub flyout_corner_radius: u32,
    /// Border thickness of the flyouts in DIPs (0 = none).
    pub flyout_border_width: u32,
    /// Border colour of the flyouts. `None`: the same as the bar's border.
    pub flyout_border: Option<Rgba>,
    /// How flyouts appear.
    pub flyout_animation: FlyoutAnim,
    /// How long that takes, in milliseconds.
    pub flyout_animation_ms: u32,
    /// Play the opening animation in reverse when a flyout closes.
    pub flyout_close_animation: bool,
    /// The mark under running apps.
    pub running_mark: RunningMark,
    /// Hide the bar until the pointer reaches the screen edge.
    pub auto_hide: AutoHide,
    /// How long after the pointer leaves the bar it hides, in milliseconds.
    pub auto_hide_delay_ms: u32,
    /// The shadow under the flyouts.
    pub flyout_shadow: FlyoutShadow,
    /// Its strength in percent (100 = as designed).
    pub flyout_shadow_strength: u32,
    /// File name (in the data folder's `icons`) of a picture shown on the
    /// *All* button instead of the word. `None`: the word "All".
    pub all_icon: Option<String>,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: ThemeMode::Dark,
            accent: None,
            background: None,
            opacity: 87,
            border: None,
            border_width: 1,
            corner_radius: 0,
            margin: 0,
            dock_width: DockWidth::Full,
            hover_animation: HoverAnim::Magnify,
            indicator: Indicator::Badge,
            indicator_image: None,
            indicator_size: 48,
            icon_size: 32,
            flyout_columns: 4,
            flyout_corner_radius: 6,
            flyout_border_width: 1,
            flyout_border: None,
            flyout_animation: FlyoutAnim::Fade,
            flyout_animation_ms: 180,
            flyout_close_animation: true,
            running_mark: RunningMark::Line,
            auto_hide: AutoHide::Off,
            auto_hide_delay_ms: 600,
            flyout_shadow: FlyoutShadow::Soft,
            flyout_shadow_strength: 100,
            all_icon: None,
        }
    }
}

/// The colours everything is drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub dark: bool,
    pub background: Rgba,
    pub border: Rgba,
    pub flyout_border: Rgba,
    pub text: Rgba,
    pub subtle: Rgba,
    pub hover: Rgba,
    pub pressed: Rgba,
    pub accent: Rgba,
    /// Readable on `accent` (the arrow on a category's badge).
    pub on_accent: Rgba,
}

pub const DEFAULT_ACCENT: Rgba = Rgba::rgb(0x60, 0xCD, 0xFF);
const DARK_BG: Rgba = Rgba::rgb(0x20, 0x20, 0x20);
const LIGHT_BG: Rgba = Rgba::rgb(0xF3, 0xF3, 0xF3);

impl Appearance {
    /// Whether the dark palette applies, given whether Windows is in dark mode.
    pub fn is_dark(&self, system_dark: bool) -> bool {
        match self.theme {
            ThemeMode::System => system_dark,
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
        }
    }

    pub fn colors(&self, system_dark: bool) -> Colors {
        let theme_dark = self.is_dark(system_dark);
        let base = self.background.map(|c| c.with_alpha(255)).unwrap_or(if theme_dark { DARK_BG } else { LIGHT_BG });
        // A custom background decides the text colour by its own brightness.
        let dark = if self.background.is_some() { base.luminance() < 0.5 } else { theme_dark };
        let opacity = self.opacity.min(100) as u32;
        let background = base.with_alpha((opacity * 255 / 100) as u8);
        let text = if dark { Rgba::rgb(0xFF, 0xFF, 0xFF) } else { Rgba::rgb(0x1A, 0x1A, 0x1A) };
        let border = self.border.unwrap_or(text.with_alpha(if dark { 0x33 } else { 0x26 }));
        let accent = self.accent.unwrap_or(DEFAULT_ACCENT).with_alpha(255);
        Colors {
            dark,
            background,
            border,
            flyout_border: self.flyout_border.unwrap_or(border),
            text,
            subtle: text.with_alpha(0xB3),
            hover: text.with_alpha(0x1A),
            pressed: text.with_alpha(0x33),
            accent,
            on_accent: if accent.luminance() > 0.55 {
                Rgba::rgb(0x1A, 0x1A, 0x1A)
            } else {
                Rgba::rgb(0xFF, 0xFF, 0xFF)
            },
        }
    }

    /// Values clamped to the ranges the UI offers (for hand-edited files).
    pub fn clamped(&self) -> Appearance {
        Appearance {
            opacity: self.opacity.min(100),
            border_width: self.border_width.min(6),
            corner_radius: self.corner_radius.min(24),
            margin: self.margin.min(24),
            icon_size: self.icon_size.clamp(16, 48),
            flyout_columns: self.flyout_columns.clamp(1, 12),
            flyout_corner_radius: self.flyout_corner_radius.min(24),
            flyout_border_width: self.flyout_border_width.min(6),
            indicator_size: self.indicator_size.clamp(25, 80),
            flyout_animation_ms: self.flyout_animation_ms.clamp(60, 600),
            flyout_shadow_strength: self.flyout_shadow_strength.clamp(10, 100),
            auto_hide_delay_ms: self.auto_hide_delay_ms.min(3000),
            // A picture that was never chosen falls back to the badge.
            indicator: if self.indicator == Indicator::Image && self.indicator_image.is_none() {
                Indicator::Badge
            } else {
                self.indicator
            },
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_original_bar() {
        let c = Appearance::default().colors(false);
        assert!(c.dark);
        // #DD202020 and a #33FFFFFF border, as in the .NET Theme.xaml.
        assert_eq!(c.background, Rgba { r: 0x20, g: 0x20, b: 0x20, a: 221 });
        assert_eq!(c.border, Rgba { r: 0xFF, g: 0xFF, b: 0xFF, a: 0x33 });
        assert_eq!(c.hover, Rgba { r: 0xFF, g: 0xFF, b: 0xFF, a: 0x1A });
        assert_eq!(c.accent, DEFAULT_ACCENT);
        // Dark arrows on the light blue accent; light ones on a dark accent.
        assert_eq!(c.on_accent, Rgba::rgb(0x1A, 0x1A, 0x1A));
        let dark = Appearance { accent: Some(Rgba::rgb(0x20, 0x30, 0x80)), ..Default::default() };
        assert_eq!(dark.colors(true).on_accent, Rgba::rgb(0xFF, 0xFF, 0xFF));
        // Flyouts: the original's 6 px corners and 1 px border, in the bar's colour.
        let a = Appearance::default();
        assert_eq!((a.flyout_corner_radius, a.flyout_border_width), (6, 1));
        assert_eq!(c.flyout_border, c.border);
    }

    #[test]
    fn flyout_border_colour() {
        let bar = Rgba::rgb(1, 2, 3);
        let a = Appearance { border: Some(bar), ..Default::default() };
        assert_eq!(a.colors(true).flyout_border, bar); // follows the bar by default
        let own = Rgba::rgb(9, 9, 9);
        let a = Appearance { border: Some(bar), flyout_border: Some(own), ..Default::default() };
        assert_eq!(a.colors(true).flyout_border, own);
        let a = Appearance { flyout_corner_radius: 99, flyout_border_width: 40, ..Default::default() }.clamped();
        assert_eq!((a.flyout_corner_radius, a.flyout_border_width), (24, 6));
    }

    #[test]
    fn theme_modes() {
        let mut a = Appearance { theme: ThemeMode::System, ..Default::default() };
        assert!(a.colors(true).dark);
        assert!(!a.colors(false).dark);
        a.theme = ThemeMode::Light;
        let c = a.colors(true);
        assert!(!c.dark);
        assert_eq!(c.text, Rgba::rgb(0x1A, 0x1A, 0x1A));
    }

    #[test]
    fn custom_background_picks_readable_text_and_applies_opacity() {
        let a = Appearance { background: Some(Rgba::rgb(250, 240, 200)), opacity: 50, ..Default::default() };
        let c = a.colors(true);
        assert!(!c.dark); // light background: dark text, even with the dark theme
        assert_eq!(c.background.a, 127);
        let a = Appearance { background: Some(Rgba::rgb(10, 20, 80)), opacity: 100, ..Default::default() };
        assert!(a.colors(false).dark);
        assert_eq!(a.colors(false).background.a, 255);
    }

    #[test]
    fn colour_parsing_and_json() {
        assert_eq!(Rgba::parse("#60CDFF"), Some(DEFAULT_ACCENT));
        assert_eq!(Rgba::parse("#ffffff33"), Some(Rgba { r: 255, g: 255, b: 255, a: 0x33 }));
        assert_eq!(Rgba::parse("60CDFF"), None);
        assert_eq!(Rgba::parse("#12345"), None);
        let a = Appearance { accent: Some(Rgba::rgb(1, 2, 3)), ..Default::default() };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"accent\":\"#010203\""));
        assert!(json.contains("\"theme\":\"dark\""));
        let back: Appearance = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
        let partial: Appearance = serde_json::from_str(r#"{"opacity":40,"dock_width":"fit"}"#).unwrap();
        assert_eq!(partial.opacity, 40);
        assert_eq!(partial.dock_width, DockWidth::Fit);
        assert_eq!(partial.icon_size, 32);
    }

    #[test]
    fn indicator_settings() {
        let a = Appearance::default();
        assert_eq!((a.indicator, a.indicator_size), (Indicator::Badge, 48));
        let a = Appearance { indicator: Indicator::Image, indicator_size: 200, ..Default::default() }.clamped();
        assert_eq!((a.indicator, a.indicator_size), (Indicator::Badge, 80)); // no picture chosen
        let a = Appearance { indicator: Indicator::Image, indicator_image: Some("i.png".into()), ..Default::default() };
        assert_eq!(a.clamped().indicator, Indicator::Image);
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"indicator\":\"image\"") && json.contains("\"indicator_image\":\"i.png\""));
    }

    #[test]
    fn flyout_animation_and_all_icon() {
        let a = Appearance::default();
        assert_eq!((a.flyout_animation, a.flyout_animation_ms, a.all_icon.as_deref()), (FlyoutAnim::Fade, 180, None));
        assert_eq!(Appearance { flyout_animation_ms: 5000, ..Default::default() }.clamped().flyout_animation_ms, 600);
        let a = Appearance { flyout_animation: FlyoutAnim::Genie, all_icon: Some("all.png".into()), ..a };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"flyout_animation\":\"genie\"") && json.contains("\"all_icon\":\"all.png\""));
        // Older files without these keys get the defaults.
        let old: Appearance = serde_json::from_str("{\"opacity\":50}").unwrap();
        assert_eq!((old.flyout_animation, old.all_icon), (FlyoutAnim::Fade, None));
    }

    #[test]
    fn flyout_shadow_settings() {
        let a = Appearance::default();
        assert_eq!((a.flyout_shadow, a.flyout_shadow_strength), (FlyoutShadow::Soft, 100));
        let a = Appearance { flyout_shadow: FlyoutShadow::Glow, flyout_shadow_strength: 0, ..a }.clamped();
        assert_eq!(a.flyout_shadow_strength, 10);
        assert!(serde_json::to_string(&a).unwrap().contains("\"flyout_shadow\":\"glow\""));
        let old: Appearance = serde_json::from_str("{}").unwrap();
        assert_eq!(old.flyout_shadow, FlyoutShadow::Soft);
    }

    #[test]
    fn clamping() {
        let a = Appearance { opacity: 250, icon_size: 2, corner_radius: 99, flyout_columns: 0, ..Default::default() };
        let c = a.clamped();
        assert_eq!((c.opacity, c.icon_size, c.corner_radius, c.flyout_columns), (100, 16, 24, 1));
    }
}
