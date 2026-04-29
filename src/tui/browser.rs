use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use super::app::{App, ListRow};
use super::markdown::markdown_to_text;

/// Percentage of screen width allocated to the doc list pane.
const LIST_PANE_PCT: u16 = 35;
/// Percentage of screen width allocated to the preview pane.
const PREVIEW_PANE_PCT: u16 = 65;

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
        Line::from(vec![Span::styled("   ,___,", owl_style)]),
        Line::from(vec![
            Span::styled("   (o,o)", owl_style),
            Span::raw("   "),
            Span::styled("T o m e", title_style),
        ]),
        Line::from(vec![
            Span::styled("   {`\"'}", owl_style),
            Span::raw("   "),
            Span::styled("docs for humans & AI", Style::default().fg(theme.fg)),
            Span::raw("  "),
            Span::styled(
                concat!("v", env!("CARGO_PKG_VERSION")),
                Style::default().fg(theme.fg_dim),
            ),
        ]),
        Line::from(vec![
            Span::styled("   -\"-\"-", owl_style),
            Span::raw("   "),
            Span::styled("─────────────────────────────────", dim_style),
        ]),
        Line::from(vec![]),
    ]));
    f.render_widget(logo, outer[0]);

    // ── Content: left list + right preview ──────────────────────────────────
    let content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(LIST_PANE_PCT),    // doc list
            Constraint::Percentage(PREVIEW_PANE_PCT), // preview
        ])
        .split(outer[1]);

    draw_doc_list(f, app, content[0]);
    draw_preview(f, app, content[1]);

    // ── Filter bar ──────────────────────────────────────────────────────────
    let filter_text = if app.filtering {
        format!("  / {}_", app.filter)
    } else if !app.filter.is_empty() {
        format!("  / {}  (esc to clear)", app.filter)
    } else {
        String::new()
    };

    f.render_widget(
        Paragraph::new(filter_text).style(theme.filter_style()),
        outer[2],
    );

    // ── Help / status bar ───────────────────────────────────────────────────
    let k = &app.cfg.ui.keys;
    let nav_label = format!("{}/{}", k.navigate_down, k.navigate_up);
    let owned_keys: Vec<(String, &str)> = vec![
        (nav_label, "Navigate"),
        ("Enter".to_string(), "Open"),
        ("Space".to_string(), "Collapse"),
        (k.filter.clone(), "Filter"),
        (k.cycle_theme.clone(), "Theme"),
        ("q".to_string(), "Quit"),
    ];

    let refs: Vec<(&str, &str)> = owned_keys.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    let help_line = if !app.status.is_empty() {
        app.status_or_help(theme, &refs)
    } else if app.filtering {
        theme.help_bar(&[
            ("Type", "Filter"),
            ("Enter", "Confirm"),
            ("Esc", "Cancel"),
        ])
    } else {
        theme.help_bar(&refs)
    };

    f.render_widget(Paragraph::new(help_line), outer[3]);
}

fn draw_doc_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let theme = &app.theme;
    let total = app.doc_aliases.len();
    let rows = app.build_list_rows();

    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            ListRow::Header(cat) => {
                let collapsed = app.collapsed_categories.contains(cat);
                let arrow = if collapsed { "▶" } else { "▼" };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} {}", arrow, cat),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
            }
            ListRow::Doc(alias) => {
                let doc = app.db.find_doc(alias).ok().flatten();
                let tags = doc.as_ref().map(|d| d.tags.join(", ")).unwrap_or_default();
                let namespace = doc.as_ref().and_then(|d| d.namespace.clone());

                let mut spans = vec![
                    Span::raw("  "),
                    Span::styled(
                        alias.clone(),
                        Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                    ),
                ];
                if let Some(ns) = namespace {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        format!("[{}]", ns),
                        Style::default().fg(theme.accent).add_modifier(Modifier::DIM),
                    ));
                }
                if !tags.is_empty() {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(tags, theme.dim_style()));
                }
                ListItem::new(Line::from(spans))
            }
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(if rows.is_empty() {
        None
    } else {
        Some(app.selected.min(rows.len().saturating_sub(1)))
    });

    // Title: show doc count (and filter count when active)
    let doc_count = rows.iter().filter(|r| matches!(r, ListRow::Doc(_))).count();
    let list_title = if !app.filter.is_empty() {
        format!(" {}/{} docs ", doc_count, total)
    } else {
        format!(" {} docs ", total)
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
    use crate::config::KeysConfig;
    let keys = &app.cfg.ui.keys;

    let k_down   = KeysConfig::parse_key(&keys.navigate_down).unwrap_or(KeyCode::Char('j'));
    let k_up     = KeysConfig::parse_key(&keys.navigate_up).unwrap_or(KeyCode::Char('k'));
    let k_filter = KeysConfig::parse_key(&keys.filter).unwrap_or(KeyCode::Char('/'));
    let k_theme  = KeysConfig::parse_key(&keys.cycle_theme).unwrap_or(KeyCode::Char('T'));

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

    let rows = app.build_list_rows();
    let count = rows.len();
    let code = key.code;

    if code == k_down || code == KeyCode::Down {
        if count > 0 {
            app.selected = (app.selected + 1).min(count - 1);
            app.load_preview().await?;
        }
    } else if code == k_up || code == KeyCode::Up {
        app.selected = app.selected.saturating_sub(1);
        app.load_preview().await?;
    } else if code == k_filter {
        app.filtering = true;
        app.filter.clear();
    } else if code == KeyCode::Esc {
        if !app.filter.is_empty() {
            app.filter.clear();
            app.selected = 0;
            app.load_preview().await?;
        }
    } else if code == KeyCode::Enter || code == KeyCode::Char(' ') {
        if let Some(cat) = app.selected_header() {
            app.toggle_category(&cat.clone());
            // Keep selected on the header row; clamp if rows shrunk
            let new_count = app.build_list_rows().len();
            app.selected = app.selected.min(new_count.saturating_sub(1));
            app.load_preview().await?;
        } else if code == KeyCode::Enter {
            if let Some(alias) = app.selected_alias() {
                app.open_doc(&alias).await?;
            }
        }
    } else if code == k_theme {
        app.cycle_theme();
    }

    Ok(())
}
