use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
};

use super::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Logo header — owl mascot
    let owl = Style::default().fg(Color::Magenta);
    let title = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let logo = Paragraph::new(Text::from(vec![
        Line::from(vec![Span::styled("    ,___,", owl)]),
        Line::from(vec![
            Span::styled("   (o,o)", owl),
            Span::raw("    "),
            Span::styled("t o m e", title),
        ]),
        Line::from(vec![
            Span::styled("   {`\"'}", owl),
            Span::raw("    "),
            Span::styled("\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}", dim),
        ]),
        Line::from(vec![
            Span::styled("   -\"-\"-", owl),
            Span::raw("    "),
            Span::styled("docs for humans & AI", dim),
        ]),
        Line::from(vec![]),
    ]));
    f.render_widget(logo, chunks[0]);

    // Doc list
    let aliases = app.filtered_aliases();
    let items: Vec<ListItem> = aliases
        .iter()
        .map(|a| {
            let doc = app.db.find_doc(a).ok().flatten();
            let source = doc.as_ref().map(|d| d.source.clone()).unwrap_or_else(|| "?".to_string());
            let tags = doc.as_ref().map(|d| d.tags.join(", ")).unwrap_or_default();

            let line = Line::from(vec![
                Span::styled(*a, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(source, Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled(tags, Style::default().fg(Color::DarkGray)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(if aliases.is_empty() { None } else { Some(app.selected.min(aliases.len().saturating_sub(1))) });

    let list = List::new(items)
        .block(
            Block::default()
                .title(" tome ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .highlight_style(Style::default().bg(Color::Magenta).fg(Color::Black).add_modifier(Modifier::BOLD))
        .highlight_symbol(" > ");

    f.render_stateful_widget(list, chunks[1], &mut list_state);

    // Filter bar
    let filter_text = if app.filtering {
        format!("/ {}_", app.filter)
    } else if !app.filter.is_empty() {
        format!("/ {} (esc to clear)", app.filter)
    } else {
        String::new()
    };

    let filter_bar = Paragraph::new(filter_text)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(filter_bar, chunks[2]);

    // Status / help bar
    let help = if app.status.is_empty() {
        "  j/k navigate   enter open   / filter   q quit".to_string()
    } else {
        app.status.clone()
    };
    let status = Paragraph::new(help)
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(status, chunks[3]);
}

pub async fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if app.filtering {
        match key.code {
            KeyCode::Esc => {
                app.filtering = false;
                app.filter.clear();
                app.selected = 0;
            }
            KeyCode::Enter => {
                app.filtering = false;
            }
            KeyCode::Backspace => {
                app.filter.pop();
            }
            KeyCode::Char(c) => {
                app.filter.push(c);
                app.selected = 0;
            }
            _ => {}
        }
        return Ok(());
    }

    let aliases = app.filtered_aliases();
    let count = aliases.len();

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if count > 0 {
                app.selected = (app.selected + 1).min(count - 1);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.selected = app.selected.saturating_sub(1);
        }
        KeyCode::Char('/') => {
            app.filtering = true;
            app.filter.clear();
        }
        KeyCode::Esc => {
            if !app.filter.is_empty() {
                app.filter.clear();
                app.selected = 0;
            }
        }
        KeyCode::Enter => {
            let aliases = app.filtered_aliases();
            if let Some(alias) = aliases.get(app.selected) {
                let alias = alias.to_string();
                app.open_doc(&alias).await?;
            }
        }
        _ => {}
    }

    Ok(())
}
