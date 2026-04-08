use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Visuals};

use crate::tui::theme::ThemeName;

/// Convert a ratatui `Color` to an egui `Color32`.
fn ratatui_to_egui(c: ratatui::style::Color) -> Color32 {
    use ratatui::style::Color as C;
    match c {
        C::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        C::Black => Color32::BLACK,
        C::White => Color32::WHITE,
        C::Gray => Color32::GRAY,
        C::DarkGray => Color32::DARK_GRAY,
        C::Red => Color32::from_rgb(200, 50, 50),
        C::LightRed => Color32::from_rgb(255, 100, 100),
        C::Green => Color32::from_rgb(50, 200, 50),
        C::LightGreen => Color32::from_rgb(100, 255, 100),
        C::Yellow => Color32::from_rgb(200, 200, 0),
        C::LightYellow => Color32::from_rgb(255, 255, 100),
        C::Blue => Color32::from_rgb(50, 100, 220),
        C::LightBlue => Color32::from_rgb(100, 150, 255),
        C::Magenta => Color32::from_rgb(180, 50, 180),
        C::LightMagenta => Color32::from_rgb(220, 100, 220),
        C::Cyan => Color32::from_rgb(0, 180, 200),
        C::LightCyan => Color32::from_rgb(100, 230, 240),
        _ => Color32::from_rgb(180, 180, 180),
    }
}

/// Palette derived from a `Theme` for use in egui widgets.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct GuiTheme {
    pub name: ThemeName,
    /// Window / panel background.
    pub bg: Color32,
    /// Slightly lighter background for panels/sidebars.
    pub bg_panel: Color32,
    /// Card / code-block background.
    pub bg_code: Color32,
    /// Primary foreground text.
    pub fg: Color32,
    /// Dimmed text (metadata, hints).
    pub fg_dim: Color32,
    /// Accent (selection, links, highlights).
    pub accent: Color32,
    /// Lighter accent for secondary highlights.
    pub accent_light: Color32,
    /// Source badge.
    pub source: Color32,
    /// Filter bar text.
    pub filter: Color32,
    /// Status / notification.
    pub status: Color32,
    /// Whether this is a dark theme (controls Visuals base).
    pub is_dark: bool,
}

impl GuiTheme {
    pub fn from_name(name: &ThemeName) -> Self {
        let t = name.to_theme();
        let is_dark = !matches!(name, ThemeName::Light);
        let (bg, bg_panel, bg_code) = if is_dark {
            match name {
                ThemeName::Catppuccin => (
                    Color32::from_rgb(30, 30, 46),
                    Color32::from_rgb(24, 24, 37),
                    Color32::from_rgb(24, 24, 37),
                ),
                ThemeName::Gruvbox => (
                    Color32::from_rgb(40, 40, 40),
                    Color32::from_rgb(50, 47, 45),
                    Color32::from_rgb(60, 56, 54),
                ),
                ThemeName::Nord => (
                    Color32::from_rgb(46, 52, 64),
                    Color32::from_rgb(59, 66, 82),
                    Color32::from_rgb(39, 44, 54),
                ),
                ThemeName::SolarizedDark => (
                    Color32::from_rgb(0, 43, 54),
                    Color32::from_rgb(7, 54, 66),
                    Color32::from_rgb(0, 43, 54),
                ),
                _ => (
                    Color32::from_rgb(18, 18, 28),
                    Color32::from_rgb(24, 24, 36),
                    Color32::from_rgb(22, 22, 34),
                ),
            }
        } else {
            (
                Color32::from_rgb(248, 248, 252),
                Color32::from_rgb(240, 240, 248),
                Color32::from_rgb(230, 230, 235),
            )
        };

        Self {
            name: name.clone(),
            bg,
            bg_panel,
            bg_code,
            fg: ratatui_to_egui(t.fg),
            fg_dim: ratatui_to_egui(t.fg_dim),
            accent: ratatui_to_egui(t.accent),
            accent_light: ratatui_to_egui(t.accent_light),
            source: ratatui_to_egui(t.source),
            filter: ratatui_to_egui(t.filter),
            status: ratatui_to_egui(t.status),
            is_dark,
        }
    }

    /// Build a full egui `Style` from this theme.
    pub fn to_egui_style(&self) -> Style {
        let mut style = Style::default();
        style.visuals = self.to_visuals();

        // Font sizes
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
            (
                TextStyle::Button,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(20.0, FontFamily::Proportional),
            ),
        ]
        .into();

        // Scrollbar style
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.bar_inner_margin = 2.0;
        style.spacing.scroll.bar_outer_margin = 0.0;

        style
    }

    fn to_visuals(&self) -> Visuals {
        let mut v = if self.is_dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };

        // Window / panel backgrounds
        v.window_fill = self.bg;
        v.panel_fill = self.bg_panel;
        v.faint_bg_color = self.bg_code;
        v.extreme_bg_color = self.bg_code;

        // Text
        v.override_text_color = Some(self.fg);

        // Corner radius for window
        v.window_corner_radius = CornerRadius::same(6);

        // Widgets — noninteractive (labels, panels)
        v.widgets.noninteractive.bg_fill = self.bg_panel;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.fg);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.accent.linear_multiply(0.3));
        v.widgets.noninteractive.corner_radius = CornerRadius::same(4);

        // Widgets — inactive (buttons not hovered)
        v.widgets.inactive.bg_fill = self.bg_panel;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, self.fg_dim);
        v.widgets.inactive.corner_radius = CornerRadius::same(4);

        // Widgets — hovered
        v.widgets.hovered.bg_fill = self.accent.linear_multiply(0.15);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, self.fg);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent.linear_multiply(0.6));
        v.widgets.hovered.corner_radius = CornerRadius::same(4);

        // Widgets — active (pressed)
        v.widgets.active.bg_fill = self.accent.linear_multiply(0.25);
        v.widgets.active.fg_stroke = Stroke::new(1.0, self.fg);
        v.widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        v.widgets.active.corner_radius = CornerRadius::same(4);

        // Widgets — open (combo boxes etc.)
        v.widgets.open.bg_fill = self.accent.linear_multiply(0.2);
        v.widgets.open.fg_stroke = Stroke::new(1.0, self.accent_light);

        // Selection
        v.selection.bg_fill = self.accent.linear_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, self.accent);

        // Hyperlinks
        v.hyperlink_color = self.accent_light;

        v
    }
}

/// Helper — convert a `ThemeName` directly to an egui `Style`.
pub fn theme_style(name: &ThemeName) -> Style {
    GuiTheme::from_name(name).to_egui_style()
}
