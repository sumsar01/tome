use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    text::Text,
};

use super::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(f.area());

    let title = format!(" {} ", app.reader_title);
    let content = Paragraph::new(Text::raw(&app.reader_content))
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.reader_scroll, 0));

    f.render_widget(content, chunks[0]);

    let help = Paragraph::new("  j/k scroll   y copy   q/esc back")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[1]);
}

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
        KeyCode::Char('y') => {
            // Copy to clipboard
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(app.reader_content.clone());
                app.status = "Copied to clipboard!".to_string();
            }
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            app.go_back();
        }
        _ => {}
    }
    Ok(())
}
