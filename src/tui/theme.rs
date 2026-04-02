use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};

/// All built-in theme names.
///
/// Serialises to/from kebab-case strings for `config.toml` (e.g. `"solarized-dark"`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
    Catppuccin,
    Gruvbox,
    Nord,
    SolarizedDark,
}

impl ThemeName {
    /// Return the display label shown in the help bar.
    pub fn display(&self) -> &'static str {
        match self {
            ThemeName::Dark => "dark",
            ThemeName::Light => "light",
            ThemeName::Catppuccin => "catppuccin",
            ThemeName::Gruvbox => "gruvbox",
            ThemeName::Nord => "nord",
            ThemeName::SolarizedDark => "solarized-dark",
        }
    }

    /// Parse a theme name from a string (case-insensitive, accepts hyphens or underscores).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('_', "-").as_str() {
            "dark" => Some(ThemeName::Dark),
            "light" => Some(ThemeName::Light),
            "catppuccin" => Some(ThemeName::Catppuccin),
            "gruvbox" => Some(ThemeName::Gruvbox),
            "nord" => Some(ThemeName::Nord),
            "solarized-dark" | "solarized" => Some(ThemeName::SolarizedDark),
            _ => None,
        }
    }

    /// Build the `Theme` value for this name.
    pub fn to_theme(&self) -> Theme {
        match self {
            ThemeName::Dark => Theme::default_dark(),
            ThemeName::Light => Theme::light(),
            ThemeName::Catppuccin => Theme::catppuccin(),
            ThemeName::Gruvbox => Theme::gruvbox(),
            ThemeName::Nord => Theme::nord(),
            ThemeName::SolarizedDark => Theme::solarized_dark(),
        }
    }

    /// Advance to the next theme in the rotation cycle.
    pub fn next(&self) -> Self {
        match self {
            ThemeName::Dark => ThemeName::Light,
            ThemeName::Light => ThemeName::Catppuccin,
            ThemeName::Catppuccin => ThemeName::Gruvbox,
            ThemeName::Gruvbox => ThemeName::Nord,
            ThemeName::Nord => ThemeName::SolarizedDark,
            ThemeName::SolarizedDark => ThemeName::Dark,
        }
    }
}

/// Central palette / style factory for the tome TUI.
///
/// All draw functions should reference this struct instead of inline
/// `Color::*` literals so that adding new themes later only requires
/// creating a new `Theme` value and passing it through.
#[derive(Clone, Debug)]
pub struct Theme {
    // ── Accent colours ────────────────────────────────────────────────────────
    /// Primary accent — used for borders, selection highlights, list bullets.
    pub accent: Color,
    /// Lighter variant — H3 headings, secondary highlights.
    pub accent_light: Color,

    // ── Text colours ──────────────────────────────────────────────────────────
    /// Primary foreground — normal readable text.
    pub fg: Color,
    /// Dimmed — metadata, separators, help text.
    pub fg_dim: Color,
    /// Source badge colour (cyan-ish).
    pub source: Color,
    /// Filter bar / active input colour.
    pub filter: Color,
    /// Blockquote text colour.
    pub quote: Color,
    /// Status / notification message colour.
    pub status: Color,

    // ── Code block colours ────────────────────────────────────────────────────
    /// Foreground for code (inline and block).
    pub code_fg: Color,
    /// Background for fenced code blocks.
    pub code_bg: Color,
    /// Background for inline code spans.
    pub code_span_bg: Color,

    // ── Key-badge (help bar) colours ──────────────────────────────────────────
    /// Background of the `[Key]` pill.
    pub badge_bg: Color,
    /// Foreground of the `[Key]` pill.
    pub badge_fg: Color,
    /// Colour of the description text after the badge.
    pub badge_desc: Color,
}

impl Theme {
    // ── Built-in themes ────────────────────────────────────────────────────────

    /// The default magenta-accented dark theme.
    pub fn default_dark() -> Self {
        Self {
            accent: Color::Magenta,
            accent_light: Color::LightMagenta,

            fg: Color::White,
            fg_dim: Color::DarkGray,
            source: Color::Cyan,
            filter: Color::Yellow,
            quote: Color::Yellow,
            status: Color::Yellow,

            code_fg: Color::Rgb(180, 230, 140),
            code_bg: Color::Rgb(24, 24, 36),
            code_span_bg: Color::Rgb(40, 40, 55),

            badge_bg: Color::Rgb(60, 60, 60),
            badge_fg: Color::White,
            badge_desc: Color::DarkGray,
        }
    }

    /// Light theme — blue accent on a light terminal background.
    pub fn light() -> Self {
        Self {
            accent: Color::Blue,
            accent_light: Color::LightBlue,

            fg: Color::Black,
            fg_dim: Color::Gray,
            source: Color::Cyan,
            filter: Color::Magenta,
            quote: Color::Magenta,
            status: Color::Magenta,

            code_fg: Color::DarkGray,
            code_bg: Color::Rgb(230, 230, 230),
            code_span_bg: Color::Rgb(215, 215, 215),

            badge_bg: Color::Rgb(200, 200, 200),
            badge_fg: Color::Black,
            badge_desc: Color::Gray,
        }
    }

    /// Catppuccin Mocha — pastel dark theme.
    pub fn catppuccin() -> Self {
        Self {
            // Mauve
            accent: Color::Rgb(203, 166, 247),
            // Pink
            accent_light: Color::Rgb(245, 194, 231),

            // Text
            fg: Color::Rgb(205, 214, 244),
            // Subtext0
            fg_dim: Color::Rgb(127, 132, 156),

            // Sapphire
            source: Color::Rgb(116, 199, 236),
            // Yellow
            filter: Color::Rgb(249, 226, 175),
            quote: Color::Rgb(249, 226, 175),
            status: Color::Rgb(249, 226, 175),

            // Green
            code_fg: Color::Rgb(166, 227, 161),
            // Mantle
            code_bg: Color::Rgb(24, 24, 37),
            // Base
            code_span_bg: Color::Rgb(30, 30, 46),

            // Surface0
            badge_bg: Color::Rgb(49, 50, 68),
            badge_fg: Color::Rgb(205, 214, 244),
            badge_desc: Color::Rgb(127, 132, 156),
        }
    }

    /// Gruvbox Dark — warm, earthy tones.
    pub fn gruvbox() -> Self {
        Self {
            // Orange bright
            accent: Color::Rgb(254, 128, 25),
            // Yellow bright
            accent_light: Color::Rgb(250, 189, 47),

            // fg0
            fg: Color::Rgb(235, 219, 178),
            // fg4 / gray
            fg_dim: Color::Rgb(146, 131, 116),

            // Aqua
            source: Color::Rgb(142, 192, 124),
            // Yellow
            filter: Color::Rgb(250, 189, 47),
            quote: Color::Rgb(250, 189, 47),
            status: Color::Rgb(250, 189, 47),

            // Green bright
            code_fg: Color::Rgb(184, 187, 38),
            // bg1
            code_bg: Color::Rgb(60, 56, 54),
            // bg2
            code_span_bg: Color::Rgb(80, 73, 69),

            // bg2
            badge_bg: Color::Rgb(80, 73, 69),
            badge_fg: Color::Rgb(235, 219, 178),
            badge_desc: Color::Rgb(146, 131, 116),
        }
    }

    /// Nord — arctic, bluish dark theme.
    pub fn nord() -> Self {
        Self {
            // Frost: ice blue
            accent: Color::Rgb(136, 192, 208),
            // Frost: sky blue
            accent_light: Color::Rgb(129, 161, 193),

            // Snow Storm: brightest
            fg: Color::Rgb(236, 239, 244),
            // Polar Night: comment-grey
            fg_dim: Color::Rgb(76, 86, 106),

            // Frost: teal
            source: Color::Rgb(143, 188, 187),
            // Aurora: yellow
            filter: Color::Rgb(235, 203, 139),
            quote: Color::Rgb(235, 203, 139),
            status: Color::Rgb(235, 203, 139),

            // Aurora: green
            code_fg: Color::Rgb(163, 190, 140),
            // Polar Night 2
            code_bg: Color::Rgb(39, 44, 54),
            // Polar Night 1
            code_span_bg: Color::Rgb(46, 52, 64),

            // Polar Night 3
            badge_bg: Color::Rgb(59, 66, 82),
            badge_fg: Color::Rgb(236, 239, 244),
            badge_desc: Color::Rgb(76, 86, 106),
        }
    }

    /// Solarized Dark — classic Ethan Schoonover palette.
    pub fn solarized_dark() -> Self {
        Self {
            // Cyan
            accent: Color::Rgb(42, 161, 152),
            // Blue
            accent_light: Color::Rgb(38, 139, 210),

            // base0
            fg: Color::Rgb(131, 148, 150),
            // base01
            fg_dim: Color::Rgb(88, 110, 117),

            // Blue
            source: Color::Rgb(38, 139, 210),
            // Yellow
            filter: Color::Rgb(181, 137, 0),
            quote: Color::Rgb(181, 137, 0),
            status: Color::Rgb(181, 137, 0),

            // Green
            code_fg: Color::Rgb(133, 153, 0),
            // base03
            code_bg: Color::Rgb(0, 43, 54),
            // base02
            code_span_bg: Color::Rgb(7, 54, 66),

            // base02
            badge_bg: Color::Rgb(7, 54, 66),
            badge_fg: Color::Rgb(131, 148, 150),
            badge_desc: Color::Rgb(88, 110, 117),
        }
    }

    // ── Style factories ────────────────────────────────────────────────────────

    pub fn border_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    pub fn selection_style(&self) -> Style {
        Style::default()
            .bg(self.accent)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    }

    pub fn title_style(&self) -> Style {
        Style::default().fg(self.fg).add_modifier(Modifier::BOLD)
    }

    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.fg_dim)
    }

    pub fn source_style(&self) -> Style {
        Style::default().fg(self.source)
    }

    pub fn filter_style(&self) -> Style {
        Style::default().fg(self.filter)
    }

    pub fn status_style(&self) -> Style {
        Style::default().fg(self.status)
    }

    // ── Help-bar helpers ───────────────────────────────────────────────────────

    /// Build a single `[key] desc` segment as a `Vec<Span<'static>>`.
    pub fn key_badge<'a>(&self, key: &'a str, desc: &'a str) -> Vec<Span<'a>> {
        vec![
            Span::styled(" ", Style::default().fg(self.badge_desc)),
            Span::styled(
                format!("[{}]", key),
                Style::default()
                    .bg(self.badge_bg)
                    .fg(self.badge_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {} ", desc), Style::default().fg(self.badge_desc)),
        ]
    }

    /// Build a complete help bar `Line` from a list of `(key, description)` pairs.
    pub fn help_bar<'a>(&self, bindings: &[(&'a str, &'a str)]) -> Line<'a> {
        let mut spans: Vec<Span<'a>> = Vec::new();
        for (key, desc) in bindings {
            spans.extend(self.key_badge(key, desc));
        }
        Line::from(spans)
    }
}
