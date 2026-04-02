use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use super::app::App;
use super::markdown::markdown_to_text;

pub fn draw(f: &mut Frame, app: &App) {
    let theme = &app.theme;
    let area = f.area();

    // ── Outer vertical split ────────────────────────────────────────────────
    // Row 0: owl header  (5 lines)
    // Row 1: content pane (list + preview, fills)
    // Row 2: filter bar  (1 line)
    // Row 3: help bar    (1 line)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // header
            Constraint::Min(3),    // content
            Constraint::Length(1), // filter
            Constraint::Length(1), // help
        ])
        .split(area);

    // ── Header ──────────────────────────────────────────────────────────────
    let owl_style = Style::default().fg(theme.accent);
    let title_style = theme.title_style();
    let dim_style = theme.dim_style();

    let logo = Paragraph::new(Text::from(vec![
        Line::from(vec![Span::styled("    ,___,", owl_style)]),
        Line::from(vec![
            Span::styled("   (o,o)", owl_style),
            Span::raw("    "),
            Span::styled("t o m e", title_style),
        ]),
        Line::from(vec![
            Span::styled("   {`\"'}", owl_style),
            Span::raw("    "),
            Span::styled("─────────────────────────────────────────", dim_style),
        ]),
        Line::from(vec![
            Span::styled("   -\"-\"-", owl_style),
            Span::raw("    "),
            Span::styled("docs for humans & AI", dim_style),
        ]),
        Line::from(vec![]),
    ]));
    f.render_widget(logo, outer[0]);

    // ── Content: left list + right preview ──────────────────────────────────
    let content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35), // doc list
            Constraint::Percentage(65), // preview
        ])
        .split(outer[1]);

    draw_doc_list(f, app, content[0]);
    draw_preview(f, app, content[1]);

    // ── Filter bar ──────────────────────────────────────────────────────────
    let filter_text = if app.filtering {
        format!("/ {}_", app.filter)
    } else if !app.filter.is_empty() {
        format!("/ {} (esc to clear)", app.filter)
    } else {
        String::new()
    };

    f.render_widget(
        Paragraph::new(filter_text).style(theme.filter_style()),
        outer[2],
    );

    // ── Help / status bar ───────────────────────────────────────────────────
    let help_line = if !app.status.is_empty() {
        Line::from(Span::styled(
            format!("  {}", app.status),
            theme.status_style(),
        ))
    } else if app.filtering {
        theme.help_bar(&[
            ("Type", "Filter"),
            ("Enter", "Confirm"),
            ("Esc", "Cancel"),
        ])
    } else {
        theme.help_bar(&[
            ("j/k", "Navigate"),
            ("Enter", "Open"),
            ("/", "Filter"),
            ("T", "Theme"),
            ("q", "Quit"),
        ])
    };

    f.render_widget(Paragraph::new(help_line), outer[3]);
}

fn draw_doc_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = &app.theme;
    let aliases = app.filtered_aliases();
    let total = app.doc_aliases.len();
    let shown = aliases.len();

    let items: Vec<ListItem> = aliases
        .iter()
        .map(|a| {
            let doc = app.db.find_doc(a).ok().flatten();
            let source = doc
                .as_ref()
                .map(|d| d.source.clone())
                .unwrap_or_else(|| "?".to_string());
            let tags = doc.as_ref().map(|d| d.tags.join(", ")).unwrap_or_default();

            let mut spans = vec![
                Span::styled(
                    *a,
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(source, theme.source_style()),
            ];
            if !tags.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(tags, theme.dim_style()));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(if aliases.is_empty() {
        None
    } else {
        Some(app.selected.min(aliases.len().saturating_sub(1)))
    });

    // Title shows filtered count vs total
    let list_title = if shown < total {
        format!(" tome  {}/{} ", shown, total)
    } else {
        format!(" tome  {} ", total)
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(list_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(theme.border_style()),
        )
        .highlight_style(theme.selection_style())
        .highlight_symbol(" ▶ ");

    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_preview(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = &app.theme;

    let title = match &app.preview_alias {
        Some(alias) => format!(" {} ", alias),
        None => " preview ".to_string(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.border_style());

    match &app.preview_content {
        Some(md) => {
            let widget = Paragraph::new(markdown_to_text(md, theme))
                .block(block)
                .wrap(Wrap { trim: false });
            f.render_widget(widget, area);
        }
        None => {
            let msg = if app.preview_alias.is_some() {
                "  Loading…"
            } else {
                "  Select a doc to preview"
            };
            let widget = Paragraph::new(Line::from(Span::styled(msg, theme.dim_style())))
                .block(block);
            f.render_widget(widget, area);
        }
    }
}

// ── Key handling ──────────────────────────────────────────────────────────────

pub async fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if app.filtering {
        match key.code {
            KeyCode::Esc => {
                app.filtering = false;
                app.filter.clear();
                app.selected = 0;
                app.load_preview().await?;
            }
            KeyCode::Enter => {
                app.filtering = false;
            }
            KeyCode::Backspace => {
                app.filter.pop();
                app.load_preview().await?;
            }
            KeyCode::Char(c) => {
                app.filter.push(c);
                app.selected = 0;
                app.load_preview().await?;
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
                app.load_preview().await?;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.selected = app.selected.saturating_sub(1);
            app.load_preview().await?;
        }
        KeyCode::Char('/') => {
            app.filtering = true;
            app.filter.clear();
        }
        KeyCode::Esc => {
            if !app.filter.is_empty() {
                app.filter.clear();
                app.selected = 0;
                app.load_preview().await?;
            }
        }
        KeyCode::Enter => {
            let aliases = app.filtered_aliases();
            if let Some(alias) = aliases.get(app.selected) {
                let alias = alias.to_string();
                app.open_doc(&alias).await?;
            }
        }
        KeyCode::Char('T') => {
            app.cycle_theme();
        }
        _ => {}
    }

    Ok(())
}
