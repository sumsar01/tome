use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
    let area = f.area();

    // Outer vertical: content area + help bar
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let content_area = vertical[0];
    let help_area = vertical[1];

    // ── Help bar ─────────────────────────────────────────────────────────────
    {
        let theme = &app.theme;
        let k = &app.cfg.ui.keys;

        if app.reader_search_mode {
            let prompt = format!("  / {}_", app.reader_search_query);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(prompt, theme.filter_style()))),
                help_area,
            );
        } else {
            let scroll_label = format!("{}/{}", k.scroll_down, k.scroll_up);
            let toc_action = if app.toc_visible { "Hide ToC" } else { "Show ToC" };
            let mut owned_keys: Vec<(String, &str)> = vec![
                (scroll_label, "Scroll"),
                ("^d/^u".to_string(), "½Page"),
                ("gg/G".to_string(), "Jump"),
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
            if !app.reader_search_matches.is_empty() {
                owned_keys.push(("/n/N".to_string(), "Search"));
            } else {
                owned_keys.push(("/".to_string(), "Search"));
            }
            owned_keys.push(("q/Esc".to_string(), "Back"));
            let refs: Vec<(&str, &str)> = owned_keys.iter().map(|(k, v)| (k.as_str(), *v)).collect();
            f.render_widget(Paragraph::new(app.status_or_help(theme, &refs)), help_area);
        }
    }

    // ── Metadata info overlay ─────────────────────────────────────────────────
    if app.show_info {
        draw_info(f, app, content_area);
        return;
    }

    // ── Content horizontal split ──────────────────────────────────────────────
    // Layout: [gutter] [ToC (optional, max 26)] [reading col (max 100)] [gutter] [scrollbar(1)]
    let show_toc = app.toc_visible && !app.toc.is_empty();

    let reading_col_area = if show_toc {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .flex(Flex::Center)
            .constraints([
                Constraint::Fill(1),    // left gutter
                Constraint::Max(26),    // ToC sidebar
                Constraint::Max(100),   // reading column
                Constraint::Fill(1),    // right gutter
                Constraint::Length(1),  // scrollbar
            ])
            .split(content_area);

        draw_toc(f, app, horizontal[1]);
        draw_scrollbar(f, app, horizontal[4]);
        horizontal[2]
    } else {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .flex(Flex::Center)
            .constraints([
                Constraint::Fill(1),    // left gutter
                Constraint::Max(100),   // reading column
                Constraint::Fill(1),    // right gutter
                Constraint::Length(1),  // scrollbar
            ])
            .split(content_area);

        draw_scrollbar(f, app, horizontal[3]);
        horizontal[1]
    };

    // ── Reading pane ──────────────────────────────────────────────────────────
    let rendered = markdown_to_text(&app.reader_content, &app.theme);
    // Store total lines and viewport height so scrollbar and vim motions work
    app.reader_total_lines = rendered.lines.len() as u16;
    // Subtract 2 for the top/bottom borders of the reading pane block
    app.reader_viewport_height = reading_col_area.height.saturating_sub(2);

    let title = format!(" {} ", app.reader_title);
    let content = Paragraph::new(rendered)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(app.theme.border_style()),
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
        Span::styled("Alias:   ", Style::default().fg(theme.fg_dim)),
        Span::styled(alias.clone(), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
    ]));

    if let Ok(Some(doc)) = app.db.find_doc(alias) {
        lines.push(Line::from(vec![
            Span::styled("Source:  ", Style::default().fg(theme.fg_dim)),
            Span::styled(doc.source.clone(), Style::default().fg(theme.fg)),
        ]));
        if let Some(ref path) = doc.path {
            lines.push(Line::from(vec![
                Span::styled("Path:    ", Style::default().fg(theme.fg_dim)),
                Span::styled(path.clone(), Style::default().fg(theme.fg)),
            ]));
        }
        if let Some(ref page_id) = doc.page_id {
            lines.push(Line::from(vec![
                Span::styled("Page ID: ", Style::default().fg(theme.fg_dim)),
                Span::styled(page_id.clone(), Style::default().fg(theme.fg)),
            ]));
        }
        let tag_str = if doc.tags.is_empty() { "(none)".to_string() } else { doc.tags.join(", ") };
        lines.push(Line::from(vec![
            Span::styled("Tags:    ", Style::default().fg(theme.fg_dim)),
            Span::styled(tag_str, Style::default().fg(theme.fg)),
        ]));
        let size = app.reader_content.len();
        lines.push(Line::from(vec![
            Span::styled("Size:    ", Style::default().fg(theme.fg_dim)),
            Span::styled(format!("{} bytes", size), Style::default().fg(theme.fg)),
        ]));
    }

    // Last fetched from version history
    if let Ok(versions) = app.db.list_versions(alias) {
        if let Some(last) = versions.last() {
            lines.push(Line::from(vec![
                Span::styled("Fetched: ", Style::default().fg(theme.fg_dim)),
                Span::styled(last.fetched_at.clone(), Style::default().fg(theme.fg)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Hash:    ", Style::default().fg(theme.fg_dim)),
                Span::styled(last.content_hash.clone(), Style::default().fg(theme.fg_dim)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("History: ", Style::default().fg(theme.fg_dim)),
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

    // ── Search prompt mode ────────────────────────────────────────────────────
    if app.reader_search_mode {
        match key.code {
            KeyCode::Esc => {
                app.reader_search_mode = false;
                app.status.clear();
            }
            KeyCode::Enter => {
                app.reader_search_mode = false;
                app.status.clear();
                run_search(app);
            }
            KeyCode::Backspace => {
                app.reader_search_query.pop();
            }
            KeyCode::Char(c) => {
                app.reader_search_query.push(c);
            }
            _ => {}
        }
        return Ok(());
    }

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
    let mods = key.modifiers;

    // Any non-g key clears a pending 'g' (stray first press of gg sequence)
    let was_pending_g = app.pending_g;
    if code != KeyCode::Char('g') {
        app.pending_g = false;
    }

    // ── Ctrl-modified keys ────────────────────────────────────────────────────
    if mods.contains(KeyModifiers::CONTROL) {
        let half = app.reader_viewport_height / 2;
        let full = app.reader_viewport_height;
        match code {
            KeyCode::Char('d') => {
                let step = half.max(1);
                app.reader_scroll = app.reader_scroll.saturating_add(step);
            }
            KeyCode::Char('u') => {
                let step = half.max(1);
                app.reader_scroll = app.reader_scroll.saturating_sub(step);
            }
            KeyCode::Char('f') => {
                let step = full.max(1);
                app.reader_scroll = app.reader_scroll.saturating_add(step);
            }
            KeyCode::Char('b') => {
                let step = full.max(1);
                app.reader_scroll = app.reader_scroll.saturating_sub(step);
            }
            _ => {}
        }
        return Ok(());
    }

    // ── Normal keys ───────────────────────────────────────────────────────────
    if code == k_down || code == KeyCode::Down {
        app.reader_scroll = app.reader_scroll.saturating_add(1);
    } else if code == k_up || code == KeyCode::Up {
        app.reader_scroll = app.reader_scroll.saturating_sub(1);
    } else if code == KeyCode::PageDown {
        app.reader_scroll = app.reader_scroll.saturating_add(PAGE_SCROLL_STEP);
    } else if code == KeyCode::PageUp {
        app.reader_scroll = app.reader_scroll.saturating_sub(PAGE_SCROLL_STEP);
    } else if code == KeyCode::Char('G') {
        // Jump to bottom (last screenful)
        app.reader_scroll = app.reader_total_lines.saturating_sub(app.reader_viewport_height);
    } else if code == KeyCode::Char('g') {
        if was_pending_g {
            // gg → jump to top
            app.reader_scroll = 0;
        } else {
            app.pending_g = true;
        }
    } else if code == KeyCode::Char('/') {
        // Enter search mode
        app.reader_search_mode = true;
        app.reader_search_query.clear();
    } else if code == KeyCode::Char('n') {
        // Next search match
        if !app.reader_search_matches.is_empty() {
            app.reader_search_idx = (app.reader_search_idx + 1) % app.reader_search_matches.len();
            app.reader_scroll = app.reader_search_matches[app.reader_search_idx];
            app.set_transient_status(format!(
                "Match {}/{}",
                app.reader_search_idx + 1,
                app.reader_search_matches.len()
            ));
        }
    } else if code == KeyCode::Char('N') {
        // Previous search match
        if !app.reader_search_matches.is_empty() {
            let len = app.reader_search_matches.len();
            app.reader_search_idx = (app.reader_search_idx + len - 1) % len;
            app.reader_scroll = app.reader_search_matches[app.reader_search_idx];
            app.set_transient_status(format!(
                "Match {}/{}",
                app.reader_search_idx + 1,
                app.reader_search_matches.len()
            ));
        }
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
    } else if code == k_theme {
        app.cycle_theme();
    } else if code == KeyCode::Char('q') || code == KeyCode::Esc {
        if app.show_info {
            app.show_info = false;
        } else {
            app.status.clear();
            app.go_back();
        }
    }
    Ok(())
}

// ── Search helpers ────────────────────────────────────────────────────────────

/// Scan reader_content for lines matching reader_search_query (case-insensitive).
/// Populates reader_search_matches and jumps to the first match.
fn run_search(app: &mut App) {
    let query = app.reader_search_query.to_lowercase();
    if query.is_empty() {
        app.reader_search_matches.clear();
        return;
    }

    app.reader_search_matches = app
        .reader_content
        .lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(&query))
        .map(|(i, _)| i as u16)
        .collect();

    app.reader_search_idx = 0;

    if app.reader_search_matches.is_empty() {
        app.set_transient_status(format!("No matches for '{}'", app.reader_search_query));
    } else {
        app.reader_scroll = app.reader_search_matches[0];
        app.set_transient_status(format!(
            "{} match{} for '{}'",
            app.reader_search_matches.len(),
            if app.reader_search_matches.len() == 1 { "" } else { "es" },
            app.reader_search_query
        ));
    }
}
