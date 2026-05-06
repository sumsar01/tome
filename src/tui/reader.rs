use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Flex, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
};

use super::app::App;
use super::markdown::markdown_to_text;

// ── Draw ──────────────────────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &mut App) {
    let theme = &app.theme;
    let area = f.area();

    // Outer vertical: content area + help bar
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let content_area = vertical[0];
    let help_area = vertical[1];

    // ── Help bar ─────────────────────────────────────────────────────────────
    let k = &app.cfg.ui.keys;
    let scroll_label = format!("{}/{}", k.scroll_down, k.scroll_up);
    let toc_action = if app.toc_visible { "Hide ToC" } else { "Show ToC" };
    // Build owned key strings so they live long enough for help_bar borrows
    let mut owned_keys: Vec<(String, &str)> = vec![
        (scroll_label, "Scroll"),
        ("PgDn/PgUp".to_string(), "Page"),
    ];
    if !app.toc.is_empty() {
        owned_keys.push((k.toggle_toc.clone(), toc_action));
    }
    owned_keys.push((k.copy.clone(), "Copy"));
    owned_keys.push((k.history.clone(), "History"));
    owned_keys.push((k.open_url.clone(), "Open URL"));
    owned_keys.push((k.info.clone(), "Info"));
    owned_keys.push((k.cycle_theme.clone(), "Theme"));
    owned_keys.push(("q/Esc".to_string(), "Back"));

    let refs: Vec<(&str, &str)> = owned_keys.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    f.render_widget(Paragraph::new(app.status_or_help(theme, &refs)), help_area);

    // ── Metadata info overlay ─────────────────────────────────────────────────
    if app.show_info {
        draw_info(f, app, content_area);
        return;
    }

    // ── Content horizontal split ──────────────────────────────────────────────
    let show_toc = app.toc_visible && !app.toc.is_empty();

    // Always compute the centered reading column the same way — ToC or not.
    // The reading column is always Max(100) centered in the full content area.
    // When the ToC is visible it is carved out of the left gutter so the
    // reading column position never moves.
    let centered = Layout::default()
        .direction(Direction::Horizontal)
        .flex(Flex::Center)
        .constraints([
            Constraint::Fill(1),    // left gutter (ToC lives here when visible)
            Constraint::Max(125),   // reading column — always centered
            Constraint::Fill(1),    // right gutter
            Constraint::Length(1),  // scrollbar
        ])
        .split(content_area);

    let reading_col_area = centered[1];
    draw_scrollbar(f, app, centered[3]);

    if show_toc {
        // Place the ToC flush against the reading column inside the left gutter.
        let toc_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Fill(1),  // margin to the left of ToC
                Constraint::Max(26),  // ToC sidebar, right-aligned in gutter
            ])
            .split(centered[0]);

        draw_toc(f, app, toc_area[1]);
    }

    // ── Reading pane ──────────────────────────────────────────────────────────
    let rendered = markdown_to_text(&app.reader_content, &app.theme);
    // Store total lines so scrollbar can compute position
    app.reader_total_lines = rendered.lines.len() as u16;

    let title = format!(" {} ", app.reader_title);
    let content = Paragraph::new(rendered)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(theme.border_style()),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.reader_scroll, 0));

    f.render_widget(content, reading_col_area);
}

fn draw_info(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = &app.theme;
    let alias = &app.reader_title;

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Alias:     ", Style::default().fg(theme.fg_dim)),
        Span::styled(alias.clone(), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
    ]));

    if let Ok(Some(doc)) = app.db.find_doc(alias) {
        lines.push(Line::from(vec![
            Span::styled("Source:    ", Style::default().fg(theme.fg_dim)),
            Span::styled(doc.source.clone(), Style::default().fg(theme.fg)),
        ]));
        if let Some(ref path) = doc.path {
            lines.push(Line::from(vec![
                Span::styled("Path:      ", Style::default().fg(theme.fg_dim)),
                Span::styled(path.clone(), Style::default().fg(theme.fg)),
            ]));
        }
        if let Some(ref page_id) = doc.page_id {
            lines.push(Line::from(vec![
                Span::styled("Page ID:   ", Style::default().fg(theme.fg_dim)),
                Span::styled(page_id.clone(), Style::default().fg(theme.fg)),
            ]));
        }
        let cat_str = doc.category.clone().unwrap_or_else(|| "(none)".to_string());
        lines.push(Line::from(vec![
            Span::styled("Category:  ", Style::default().fg(theme.fg_dim)),
            Span::styled(cat_str, Style::default().fg(theme.fg)),
        ]));
        let ns_str = doc.namespace.clone().unwrap_or_else(|| "(none)".to_string());
        lines.push(Line::from(vec![
            Span::styled("Namespace: ", Style::default().fg(theme.fg_dim)),
            Span::styled(ns_str, Style::default().fg(theme.fg)),
        ]));
        let tag_str = if doc.tags.is_empty() { "(none)".to_string() } else { doc.tags.join(", ") };
        lines.push(Line::from(vec![
            Span::styled("Tags:      ", Style::default().fg(theme.fg_dim)),
            Span::styled(tag_str, Style::default().fg(theme.fg)),
        ]));
        let size = app.reader_content.len();
        lines.push(Line::from(vec![
            Span::styled("Size:      ", Style::default().fg(theme.fg_dim)),
            Span::styled(format!("{} bytes", size), Style::default().fg(theme.fg)),
        ]));
    }

    // Last fetched from version history
    if let Ok(versions) = app.db.list_versions(alias) {
        if let Some(last) = versions.last() {
            lines.push(Line::from(vec![
                Span::styled("Fetched:   ", Style::default().fg(theme.fg_dim)),
                Span::styled(last.fetched_at.clone(), Style::default().fg(theme.fg)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Hash:      ", Style::default().fg(theme.fg_dim)),
                Span::styled(last.content_hash.clone(), Style::default().fg(theme.fg_dim)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("History:   ", Style::default().fg(theme.fg_dim)),
                Span::styled(format!("{} version(s)", versions.len()), Style::default().fg(theme.fg)),
            ]));
        }
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled("Press 'i' or Esc to close", theme.dim_style())));

    let para = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" Doc Info ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(theme.border_style()),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn draw_toc(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = &app.theme;

    let items: Vec<ListItem> = app
        .toc
        .iter()
        .map(|(level, text)| {
            let indent = match level {
                1 => "",
                2 => "  ",
                3 => "    ",
                _ => "      ",
            };
            let style = match level {
                1 => Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
                2 => Style::default().fg(theme.fg),
                _ => Style::default().fg(theme.fg_dim),
            };
            // Truncate to fit the narrow pane (max 22 cols, minus indent and border)
            let max_len = (area.width as usize).saturating_sub(indent.len() + 4);
            let label = if text.len() > max_len && max_len > 1 {
                format!("{}{}…", indent, &text[..max_len.saturating_sub(1)])
            } else {
                format!("{}{}", indent, text)
            };
            ListItem::new(Line::from(Span::styled(label, style)))
        })
        .collect();

    let toc_widget = List::new(items).block(
        Block::default()
            .title(" ToC ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(theme.border_style()),
    );

    f.render_widget(toc_widget, area);
}

fn draw_scrollbar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = &app.theme;
    let total = app.reader_total_lines;
    if total == 0 || area.height < 3 {
        return;
    }

    // Height available for the scroll track (leave 1 row top marker, 1 row for pct label)
    let track_height = area.height.saturating_sub(2);
    let max_scroll = total.saturating_sub(track_height);

    let thumb_pos = if max_scroll == 0 {
        0u16
    } else {
        let ratio = app.reader_scroll.min(max_scroll) as f64 / max_scroll as f64;
        (ratio * track_height as f64).round() as u16
    };

    let pct = if max_scroll == 0 {
        100u16
    } else {
        let ratio = app.reader_scroll.min(max_scroll) as f64 / max_scroll as f64;
        (ratio * 100.0).round() as u16
    };

    let mut lines: Vec<Line> = Vec::new();

    // Top arrow
    lines.push(Line::from(Span::styled("▴", theme.dim_style())));

    // Track
    for i in 0..track_height {
        let (ch, style) = if i == thumb_pos {
            ("█", Style::default().fg(theme.accent))
        } else {
            ("▕", theme.dim_style())
        };
        lines.push(Line::from(Span::styled(ch, style)));
    }

    // Percentage — only show if scrolled at all
    let pct_str = format!("{:>3}%", pct.min(100));
    lines.push(Line::from(Span::styled(pct_str, theme.dim_style())));

    f.render_widget(Paragraph::new(Text::from(lines)), area);
}

// ── Key handling ──────────────────────────────────────────────────────────────

/// Number of lines scrolled by PageDown / PageUp.
const PAGE_SCROLL_STEP: u16 = 20;

pub async fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use crate::config::KeysConfig;
    let keys = &app.cfg.ui.keys;

    // Resolve configured key codes (errors impossible here — validated at startup)
    let k_down       = KeysConfig::parse_key(&keys.scroll_down).unwrap_or(KeyCode::Char('j'));
    let k_up         = KeysConfig::parse_key(&keys.scroll_up).unwrap_or(KeyCode::Char('k'));
    let k_toc        = KeysConfig::parse_key(&keys.toggle_toc).unwrap_or(KeyCode::Char('t'));
    let k_copy       = KeysConfig::parse_key(&keys.copy).unwrap_or(KeyCode::Char('y'));
    let k_history    = KeysConfig::parse_key(&keys.history).unwrap_or(KeyCode::Char('h'));
    let k_open       = KeysConfig::parse_key(&keys.open_url).unwrap_or(KeyCode::Char('o'));
    let k_info       = KeysConfig::parse_key(&keys.info).unwrap_or(KeyCode::Char('i'));
    let k_theme      = KeysConfig::parse_key(&keys.cycle_theme).unwrap_or(KeyCode::Char('T'));

    let code = key.code;

    if code == k_down || code == KeyCode::Down {
        app.reader_scroll = app.reader_scroll.saturating_add(1);
    } else if code == k_up || code == KeyCode::Up {
        app.reader_scroll = app.reader_scroll.saturating_sub(1);
    } else if code == KeyCode::PageDown {
        app.reader_scroll = app.reader_scroll.saturating_add(PAGE_SCROLL_STEP);
    } else if code == KeyCode::PageUp {
        app.reader_scroll = app.reader_scroll.saturating_sub(PAGE_SCROLL_STEP);
    } else if code == k_toc {
        app.toc_visible = !app.toc_visible;
    } else if code == k_copy {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(app.reader_content.clone());
            app.set_transient_status("Copied to clipboard!".to_string());
        }
    } else if code == k_history {
        app.open_history();
    } else if code == k_open {
        app.open_source_in_browser();
    } else if code == k_info {
        app.show_info = !app.show_info;
        if app.show_info {
            app.info_from_browser = false;
        }
    } else if code == k_theme {
        app.cycle_theme();
    } else if code == KeyCode::Char('q') || code == KeyCode::Esc {
        if app.show_info {
            app.show_info = false;
            if app.info_from_browser {
                app.info_from_browser = false;
                app.status.clear();
                app.go_back();
            }
        } else {
            app.status.clear();
            app.go_back();
        }
    }
    Ok(())
}
