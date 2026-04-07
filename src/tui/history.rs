use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
};

use super::app::App;

// ── Draw ──────────────────────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &mut App) {
    let theme = &app.theme;
    let area = f.area();

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let content_area = vertical[0];
    let help_area = vertical[1];

    // Help bar
    let bindings: Vec<(&str, &str)> = if app.diff_content.is_some() {
        vec![("j/k", "Scroll"), ("Esc", "Back to list")]
    } else {
        vec![
            ("j/k", "Select"),
            ("d", "Diff vs prev"),
            ("Enter", "Diff vs prev"),
            ("q/Esc", "Back"),
        ]
    };
    f.render_widget(Paragraph::new(app.status_or_help(theme, &bindings)), help_area);

    if let Some(ref diff) = app.diff_content.clone() {
        // Show diff pane
        let title = format!(" Diff — {} ", app.reader_title);
        let text: Vec<Line> = diff
            .lines()
            .map(|line| {
                let style = if line.starts_with('+') && !line.starts_with("+++") {
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    Style::default().fg(ratatui::style::Color::Red)
                } else if line.starts_with("@@") {
                    Style::default().fg(theme.fg_dim).add_modifier(Modifier::ITALIC)
                } else {
                    Style::default().fg(theme.fg)
                };
                Line::from(Span::styled(line.to_string(), style))
            })
            .collect();
        let para = Paragraph::new(Text::from(text))
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(theme.border_style()),
            )
            .wrap(Wrap { trim: false })
            .scroll((app.reader_scroll, 0));
        f.render_widget(para, content_area);
    } else {
        // Show history list
        let title = format!(" History — {} ", app.reader_title);
        let items: Vec<ListItem> = app
            .history_entries
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let selected = i == app.history_selected;
                let label = format!(
                    " v{:<3} {}  [{}]",
                    v.version, v.fetched_at, v.content_hash
                );
                let style = if selected {
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else {
                    Style::default().fg(theme.fg)
                };
                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(theme.border_style()),
        );
        f.render_widget(list, content_area);
    }
}

// ── Key handling ──────────────────────────────────────────────────────────────

/// Number of lines scrolled by PageDown / PageUp.
const PAGE_SCROLL_STEP: u16 = 20;

pub async fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use crate::config::KeysConfig;
    let keys = &app.cfg.ui.keys;
    let k_down = KeysConfig::parse_key(&keys.navigate_down).unwrap_or(KeyCode::Char('j'));
    let k_up   = KeysConfig::parse_key(&keys.navigate_up).unwrap_or(KeyCode::Char('k'));

    let code = key.code;

    if app.diff_content.is_some() {
        // In diff view
        if code == k_down || code == KeyCode::Down {
            app.reader_scroll = app.reader_scroll.saturating_add(1);
        } else if code == k_up || code == KeyCode::Up {
            app.reader_scroll = app.reader_scroll.saturating_sub(1);
        } else if code == KeyCode::PageDown {
            app.reader_scroll = app.reader_scroll.saturating_add(PAGE_SCROLL_STEP);
        } else if code == KeyCode::PageUp {
            app.reader_scroll = app.reader_scroll.saturating_sub(PAGE_SCROLL_STEP);
        } else if code == KeyCode::Esc || code == KeyCode::Char('q') {
            app.diff_content = None;
            app.reader_scroll = 0;
        }
    } else {
        // In history list view
        if code == k_down || code == KeyCode::Down {
            let max = app.history_entries.len().saturating_sub(1);
            app.history_selected = (app.history_selected + 1).min(max);
        } else if code == k_up || code == KeyCode::Up {
            app.history_selected = app.history_selected.saturating_sub(1);
        } else if code == KeyCode::Char('d') || code == KeyCode::Enter {
            app.reader_scroll = 0;
            app.show_diff_for_selected();
        } else if code == KeyCode::Char('q') || code == KeyCode::Esc {
            app.go_back();
        }
    }
    Ok(())
}
