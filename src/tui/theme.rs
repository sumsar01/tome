use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

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

            code_fg: Color::Rgb(180, 230, 140),
            code_bg: Color::Rgb(24, 24, 36),
            code_span_bg: Color::Rgb(40, 40, 55),

            badge_bg: Color::Rgb(60, 60, 60),
            badge_fg: Color::White,
            badge_desc: Color::DarkGray,
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
