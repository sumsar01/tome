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
    let help_line = if !app.status.is_empty() {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                app.status.clone(),
                theme.status_style(),
            ),
        ])
    } else {
        let mut bindings: Vec<(&str, &str)> = vec![("j/k", "Scroll"), ("PgDn/PgUp", "Page")];
        if !app.toc.is_empty() {
            if app.toc_visible {
                bindings.push(("t", "Hide ToC"));
            } else {
                bindings.push(("t", "Show ToC"));
            }
        }
        bindings.push(("y", "Copy"));
        bindings.push(("h", "History"));
        bindings.push(("T", "Theme"));
        bindings.push(("q/Esc", "Back"));
        theme.help_bar(&bindings)
    };
    f.render_widget(Paragraph::new(help_line), help_area);

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

pub async fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.reader_scroll = app.reader_scroll.saturating_add(1);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.reader_scroll = app.reader_scroll.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.reader_scroll = app.reader_scroll.saturating_add(20);
        }
        KeyCode::PageUp => {
            app.reader_scroll = app.reader_scroll.saturating_sub(20);
        }
        KeyCode::Char('t') => {
            app.toc_visible = !app.toc_visible;
        }
        KeyCode::Char('y') => {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(app.reader_content.clone());
                app.set_transient_status("Copied to clipboard!".to_string());
            }
        }
        KeyCode::Char('h') => {
            app.open_history();
        }
        KeyCode::Char('T') => {
            app.cycle_theme();
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            app.status.clear();
            app.go_back();
        }
        _ => {}
    }
    Ok(())
}
