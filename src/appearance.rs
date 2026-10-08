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

/// Windows 11's blurred backdrop behind the bar, instead of the plain
/// translucency a layered window gives.
///
/// This is not a colour but a different kind of window: DWM draws the
/// backdrop behind the whole window and the bar is painted over it. Windows
/// 10 has no such backdrop, and nor do Windows 11 builds before 22H2, so
/// `win::backdrop` falls back to `Off` where it isn't available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    /// Plain translucency: the bar is a layered window, as it has always been.
    #[default]
    Off,
    /// Mica: the desktop wallpaper, heavily blurred and barely moving.
    Mica,
    /// Acrylic: what is behind the bar, blurred and tinted. More obvious than
    /// Mica, and it follows windows moving underneath.
    Acrylic,
    /// Mica Alt: like Mica, with more of the wallpaper's colour.
    MicaAlt,
}

impl Backdrop {
    /// What `DWMWA_SYSTEMBACKDROP_TYPE` wants (`DWMSBT_*`).
    pub fn dwm_value(self) -> u32 {
        match self {
            Backdrop::Off => 1,     // DWMSBT_NONE
            Backdrop::Mica => 2,    // DWMSBT_MAINWINDOW
            Backdrop::Acrylic => 3, // DWMSBT_TRANSIENTWINDOW
            Backdrop::MicaAlt => 4, // DWMSBT_TABBEDWINDOW
        }
    }

    pub fn on(self) -> bool {
        self != Backdrop::Off
    }
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
    /// Windows 11's blurred backdrop behind the bar, instead of plain
    /// translucency.
    pub backdrop: Backdrop,
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
            backdrop: Backdrop::Off,
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
/// What DWM shows its backdrop through, inside a window whose frame has been
/// extended: nothing drawn at all. The window is cleared to black first and
/// the picture laid over it, so a pixel the picture leaves alone keeps the
/// black and the zero alpha that together mean glass (see `win::canvas`).
pub const GLASS: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };
const DARK_BG: Rgba = Rgba::rgb(0x20, 0x20, 0x20);
const LIGHT_BG: Rgba = Rgba::rgb(0xF3, 0xF3, 0xF3);

impl Colors {
    /// The palette for a bar with a backdrop behind it. Only the background
    /// changes: it becomes the glass DWM shows its blur through, since GDI
    /// has no alpha channel to tint a backdrop with. Icons, text, the border
    /// and the hover and pressed fills are drawn over it as usual.
    pub fn over_backdrop(&self) -> Colors {
        Colors { background: GLASS, ..*self }
    }
}

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

/// Windows' own colours while a high-contrast theme is on (`GetSysColor`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemColors {
    pub window: Rgba,
    pub text: Rgba,
    pub highlight: Rgba,
    pub highlight_text: Rgba,
    pub gray_text: Rgba,
}

impl Colors {
    /// Everything in the high-contrast theme's colours, opaque: its window
    /// colour behind, its text colour for text and borders, its highlight
    /// for hover, open buttons and marks.
    pub fn high_contrast(sc: &SystemColors) -> Colors {
        let opaque = |c: Rgba| c.with_alpha(255);
        let (window, text) = (opaque(sc.window), opaque(sc.text));
        Colors {
            dark: window.luminance() < 0.5,
            background: window,
            border: text,
            flyout_border: text,
            text,
            subtle: opaque(sc.gray_text),
            // See-through over the window colour, so text stays readable on it.
            hover: sc.highlight.with_alpha(0x55),
            pressed: sc.highlight.with_alpha(0x99),
            accent: opaque(sc.highlight),
            on_accent: opaque(sc.highlight_text),
        }
    }
}

impl Appearance {
    /// What a high-contrast theme needs: an opaque bar with borders, no
    /// shadows, and the built-in marks (a picture may not stand out).
    pub fn for_high_contrast(&self) -> Appearance {
        Appearance {
            opacity: 100,
            // High contrast asks for flat, solid colours: no blurred backdrop.
            backdrop: Backdrop::Off,
            border_width: self.border_width.max(1),
            flyout_border_width: self.flyout_border_width.max(1),
            flyout_shadow: FlyoutShadow::Off,
            indicator: if self.indicator == Indicator::Image { Indicator::Badge } else { self.indicator },
            ..self.clone()
        }
    }

    /// What the bar should look like with a backdrop behind it.
    ///
    /// DWM draws a backdrop behind the *whole window*, which settles three
    /// things that the plain layered bar does for itself:
    ///
    /// - the bar's own fill can only tint the blur; at the usual opacity it
    ///   would hide it entirely;
    /// - the floating margin was clear space inside the window, and a
    ///   backdrop would fill it with a blurred band around the bar, so the
    ///   bar sits flush and takes the whole window;
    /// - fading needs a layered window, which is the one thing a backdrop
    ///   cannot be, so *Fade* auto-hide slides instead.
    ///
    /// The corners stay as they are: DWM rounds the window itself to match
    /// (see `win::backdrop`).
    ///
    /// `Off` — and a Windows that can't draw one — leaves the look alone, so
    /// nothing changes for anyone who hasn't asked for a backdrop.
    pub fn over_backdrop(&self, available: bool) -> Appearance {
        if !available || !self.backdrop.on() {
            return Appearance { backdrop: Backdrop::Off, ..self.clone() };
        }
        Appearance {
            // The backdrop is the background. GDI has no alpha to tint it
            // with (see `Colors::over_backdrop`), so the fill goes entirely.
            opacity: 0,
            margin: 0,
            dock_width: DockWidth::Full,
            auto_hide: match self.auto_hide {
                AutoHide::Fade => AutoHide::Slide,
                other => other,
            },
            ..self.clone()
        }
    }

    /// Windows' *Animation effects* are off: nothing moves.
    pub fn without_motion(&self) -> Appearance {
        Appearance {
            hover_animation: HoverAnim::Off,
            flyout_animation: FlyoutAnim::Off,
            flyout_close_animation: false,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backdrop_replaces_the_bars_own_fill() {
        let a = Appearance { opacity: 95, backdrop: Backdrop::Acrylic, ..Default::default() };
        let over = a.over_backdrop(true);
        assert_eq!(over.opacity, 0, "any fill at all would hide the blur");
        assert_eq!(over.backdrop, Backdrop::Acrylic);
    }

    #[test]
    fn the_background_becomes_the_glass_dwm_shows_through() {
        let colors = Appearance::default().colors(true);
        let glass = colors.over_backdrop();
        assert_eq!(glass.background, GLASS, "nothing drawn is what DWM shows its backdrop through");
        assert_eq!(glass.background.a, 0, "and that means fully clear, not black paint");
        assert_eq!(glass.text, colors.text, "everything else is drawn as usual");
        assert_eq!(glass.accent, colors.accent);
        assert_eq!(glass.border, colors.border);
    }

    #[test]
    fn a_backdrop_fills_the_window_so_the_bar_sits_flush() {
        let a = Appearance { backdrop: Backdrop::Mica, margin: 12, dock_width: DockWidth::Fit, ..Default::default() };
        let over = a.over_backdrop(true);
        assert_eq!(over.margin, 0, "a margin would be a blurred band around the bar");
        assert_eq!(over.dock_width, DockWidth::Full);
    }

    #[test]
    fn fading_becomes_sliding_because_a_backdrop_cannot_fade() {
        let a = Appearance { backdrop: Backdrop::Acrylic, auto_hide: AutoHide::Fade, ..Default::default() };
        assert_eq!(a.over_backdrop(true).auto_hide, AutoHide::Slide);
        let sliding = Appearance { backdrop: Backdrop::Acrylic, auto_hide: AutoHide::Slide, ..Default::default() };
        assert_eq!(sliding.over_backdrop(true).auto_hide, AutoHide::Slide);
        let never = Appearance { backdrop: Backdrop::Acrylic, auto_hide: AutoHide::Off, ..Default::default() };
        assert_eq!(never.over_backdrop(true).auto_hide, AutoHide::Off);
    }

    #[test]
    fn a_fading_bar_without_a_backdrop_still_fades() {
        let a = Appearance { backdrop: Backdrop::Off, auto_hide: AutoHide::Fade, margin: 12, ..Default::default() };
        let over = a.over_backdrop(true);
        assert_eq!(over.auto_hide, AutoHide::Fade);
        assert_eq!(over.margin, 12);
    }

    #[test]
    fn without_a_backdrop_the_look_is_untouched() {
        let a = Appearance { opacity: 95, backdrop: Backdrop::Off, ..Default::default() };
        assert_eq!(a.over_backdrop(true), a, "nothing changes for anyone who asked for none");
    }

    #[test]
    fn a_backdrop_windows_cant_draw_falls_back_to_none() {
        let a = Appearance { opacity: 95, backdrop: Backdrop::Acrylic, ..Default::default() };
        let over = a.over_backdrop(false);
        assert_eq!(over.backdrop, Backdrop::Off, "Windows 10 gets the plain layered bar");
        assert_eq!(over.opacity, 95, "and its own opacity back");
    }

    #[test]
    fn high_contrast_turns_the_backdrop_off() {
        let a = Appearance { backdrop: Backdrop::Acrylic, ..Default::default() };
        assert_eq!(a.for_high_contrast().backdrop, Backdrop::Off);
    }

    #[test]
    fn each_backdrop_asks_dwm_for_its_own_kind() {
        let values: Vec<u32> = [Backdrop::Off, Backdrop::Mica, Backdrop::Acrylic, Backdrop::MicaAlt]
            .iter()
            .map(|b| b.dwm_value())
            .collect();
        assert_eq!(values, [1, 2, 3, 4], "DWMSBT_NONE, MAINWINDOW, TRANSIENTWINDOW, TABBEDWINDOW");
        assert!(!Backdrop::Off.on());
        assert!(Backdrop::Mica.on() && Backdrop::Acrylic.on() && Backdrop::MicaAlt.on());
    }

    #[test]
    fn high_contrast() {
        // High Contrast Black.
        let sc = SystemColors {
            window: Rgba::rgb(0, 0, 0),
            text: Rgba::rgb(255, 255, 255),
            highlight: Rgba::rgb(0x1A, 0xEB, 0xFF),
            highlight_text: Rgba::rgb(0, 0, 0),
            gray_text: Rgba::rgb(0x3F, 0xF2, 0x3F),
        };
        let c = Colors::high_contrast(&sc);
        assert!(c.dark);
        assert_eq!((c.background, c.text, c.border, c.flyout_border), (sc.window, sc.text, sc.text, sc.text));
        assert_eq!((c.accent, c.on_accent, c.subtle), (sc.highlight, sc.highlight_text, sc.gray_text));
        assert_eq!((c.hover.a, c.pressed.a), (0x55, 0x99));
        // Whatever the look, the bar is opaque, bordered and without shadows.
        let look = Appearance {
            opacity: 40,
            border_width: 0,
            flyout_border_width: 0,
            flyout_shadow: FlyoutShadow::Soft,
            indicator: Indicator::Image,
            indicator_image: Some("x.png".into()),
            ..Default::default()
        }
        .for_high_contrast();
        assert_eq!((look.opacity, look.border_width, look.flyout_border_width), (100, 1, 1));
        assert_eq!((look.flyout_shadow, look.indicator), (FlyoutShadow::Off, Indicator::Badge));
        let thick = Appearance { border_width: 3, indicator: Indicator::Dot, ..Default::default() }.for_high_contrast();
        assert_eq!((thick.border_width, thick.indicator), (3, Indicator::Dot));
        // White: light.
        let white = SystemColors { window: Rgba::rgb(255, 255, 255), text: Rgba::rgb(0, 0, 0), ..sc };
        assert!(!Colors::high_contrast(&white).dark);
    }

    #[test]
    fn reduced_motion() {
        let look = Appearance {
            hover_animation: HoverAnim::Magnify,
            flyout_animation: FlyoutAnim::Genie,
            flyout_close_animation: true,
            ..Default::default()
        }
        .without_motion();
        assert_eq!(
            (look.hover_animation, look.flyout_animation, look.flyout_close_animation),
            (HoverAnim::Off, FlyoutAnim::Off, false)
        );
    }

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
