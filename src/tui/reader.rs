use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options as ParseOptions, Parser, Tag, TagEnd};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Flex, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

use super::app::App;

// ── Markdown → ratatui Text ──────────────────────────────────────────────────

fn markdown_to_text(input: &str) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Inline span state
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut bold = false;
    let mut italic = false;
    let mut strikethrough = false;
    let mut in_code_span = false;

    // Block state
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_lines: Vec<String> = Vec::new();

    // List state: stack of (ordered, counter)
    let mut list_stack: Vec<Option<u64>> = Vec::new();

    // Blockquote depth
    let mut blockquote_depth: usize = 0;

    let opts = ParseOptions::ENABLE_STRIKETHROUGH
        | ParseOptions::ENABLE_TABLES
        | ParseOptions::ENABLE_SMART_PUNCTUATION;

    let parser = Parser::new_ext(input, opts);

    // Helper: flush current spans into lines vector
    macro_rules! flush_line {
        () => {{
            let mut line_spans = std::mem::take(&mut spans);
            if blockquote_depth > 0 {
                let prefix = Span::styled(
                    "│ ".repeat(blockquote_depth),
                    Style::default().fg(Color::Yellow),
                );
                line_spans.insert(0, prefix);
            }
            lines.push(Line::from(line_spans));
        }};
    }

    macro_rules! push_blank {
        () => {{
            if blockquote_depth > 0 {
                lines.push(Line::from(Span::styled(
                    "│".repeat(blockquote_depth),
                    Style::default().fg(Color::Yellow),
                )));
            } else {
                lines.push(Line::default());
            }
        }};
    }

    for event in parser {
        match event {
            // ── Code blocks ──────────────────────────────────────────────
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code_lines.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                // Blank line before
                push_blank!();
                // Language label
                if !code_lang.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!(" {} ", code_lang),
                        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                    )));
                }
                // Code lines
                for cl in &code_lines {
                    let span = Span::styled(
                        format!(" {} ", cl),
                        Style::default().fg(Color::Green).bg(Color::Rgb(30, 30, 30)),
                    );
                    lines.push(Line::from(span));
                }
                // Blank line after
                push_blank!();
                code_lang.clear();
                code_lines.clear();
            }
            Event::Text(t) if in_code_block => {
                // pulldown-cmark emits the whole block as one Text event with embedded newlines
                for line in t.lines() {
                    code_lines.push(line.to_string());
                }
                // preserve trailing newline as an empty line
                if t.ends_with('\n') && !t.trim_end_matches('\n').is_empty() {
                    // handled by the loop above
                }
            }

            // ── Headings ─────────────────────────────────────────────────
            Event::Start(Tag::Heading { level, .. }) => {
                // blank line before heading (except at very start)
                if !lines.is_empty() {
                    push_blank!();
                }
                let _ = level; // consumed in End
            }
            Event::End(TagEnd::Heading(level)) => {
                let (style, prefix) = heading_style(level);
                // Build the heading line from current spans
                let mut heading_spans: Vec<Span<'static>> = vec![Span::raw(prefix)];
                heading_spans.extend(spans.drain(..).map(|s| {
                    // apply heading style on top of any existing style
                    Span::styled(s.content, style.patch(s.style))
                }));
                lines.push(Line::from(heading_spans));
                // blank line after heading
                push_blank!();
            }

            // ── Paragraphs ───────────────────────────────────────────────
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                flush_line!();
                push_blank!();
            }

            // ── Blockquotes ──────────────────────────────────────────────
            Event::Start(Tag::BlockQuote(_)) => {
                blockquote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                if blockquote_depth > 0 {
                    blockquote_depth -= 1;
                }
            }

            // ── Lists ─────────────────────────────────────────────────────
            Event::Start(Tag::List(start)) => {
                list_stack.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
                if list_stack.is_empty() {
                    push_blank!();
                }
            }
            Event::Start(Tag::Item) => {
                let depth = list_stack.len().saturating_sub(1);
                let indent = "  ".repeat(depth);
                let bullet = match list_stack.last_mut() {
                    Some(Some(n)) => {
                        let s = format!("{indent}{}. ", n);
                        *n += 1;
                        s
                    }
                    _ => format!("{indent}• "),
                };
                spans.push(Span::styled(bullet, Style::default().fg(Color::Magenta)));
            }
            Event::End(TagEnd::Item) => {
                flush_line!();
            }

            // ── Inline formatting ─────────────────────────────────────────
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Start(Tag::Strikethrough) => strikethrough = true,
            Event::End(TagEnd::Strikethrough) => strikethrough = false,
            Event::Code(t) => {
                spans.push(Span::styled(
                    format!(" {} ", t.into_string()),
                    Style::default().fg(Color::Green).bg(Color::Rgb(40, 40, 40)),
                ));
            }

            // ── Links ─────────────────────────────────────────────────────
            Event::Start(Tag::Link { .. }) => {}
            Event::End(TagEnd::Link) => {}

            // ── Images ────────────────────────────────────────────────────
            Event::Start(Tag::Image { dest_url, .. }) => {
                spans.push(Span::styled(
                    format!("[image: {}]", dest_url),
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                ));
            }
            Event::End(TagEnd::Image) => {}

            // ── Tables ────────────────────────────────────────────────────
            Event::Start(Tag::Table(_)) => { push_blank!(); }
            Event::End(TagEnd::Table) => { push_blank!(); }
            Event::Start(Tag::TableHead) => {}
            Event::End(TagEnd::TableHead) => {
                // Draw a separator line after the header row
                let sep = Span::styled(
                    "─".repeat(60),
                    Style::default().fg(Color::DarkGray),
                );
                lines.push(Line::from(sep));
            }
            Event::Start(Tag::TableRow) => {}
            Event::End(TagEnd::TableRow) => { flush_line!(); }
            Event::Start(Tag::TableCell) => {
                spans.push(Span::raw("  "));
            }
            Event::End(TagEnd::TableCell) => {
                spans.push(Span::raw("  │"));
            }

            // ── Horizontal rule ───────────────────────────────────────────
            Event::Rule => {
                push_blank!();
                lines.push(Line::from(Span::styled(
                    "─".repeat(80),
                    Style::default().fg(Color::DarkGray),
                )));
                push_blank!();
            }

            // ── Soft / hard breaks ────────────────────────────────────────
            Event::SoftBreak => {
                spans.push(Span::raw(" "));
            }
            Event::HardBreak => {
                flush_line!();
            }

            // ── Plain text ────────────────────────────────────────────────
            Event::Text(t) => {
                if in_code_span {
                    spans.push(Span::styled(
                        t.into_string(),
                        Style::default().fg(Color::Green).bg(Color::Rgb(40, 40, 40)),
                    ));
                    in_code_span = false;
                } else {
                    let mut style = Style::default();
                    if bold { style = style.add_modifier(Modifier::BOLD); }
                    if italic { style = style.add_modifier(Modifier::ITALIC); }
                    if strikethrough { style = style.add_modifier(Modifier::CROSSED_OUT); }
                    if blockquote_depth > 0 {
                        style = style.fg(Color::Yellow).add_modifier(Modifier::ITALIC);
                    }
                    spans.push(Span::styled(t.into_string(), style));
                }
            }

            // ── HTML (pass-through as dim text) ───────────────────────────
            Event::Html(t) | Event::InlineHtml(t) => {
                let s = t.trim().to_string();
                if !s.is_empty() {
                    spans.push(Span::styled(s, Style::default().fg(Color::DarkGray)));
                }
            }

            _ => {}
        }
    }

    // Flush any remaining spans
    if !spans.is_empty() {
        flush_line!();
    }

    Text::from(lines)
}

fn heading_style(level: HeadingLevel) -> (Style, &'static str) {
    match level {
        HeadingLevel::H1 => (
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            " ",
        ),
        HeadingLevel::H2 => (
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            "",
        ),
        HeadingLevel::H3 => (
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
            "  ",
        ),
        HeadingLevel::H4 => (
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::ITALIC),
            "    ",
        ),
        _ => (
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            "      ",
        ),
    }
}

// ── Draw ─────────────────────────────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Outer vertical split: content area + help bar
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let content_area = vertical[0];
    let help_area = vertical[1];

    // Center the reading column — max 100 cols, gutters fill the rest
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .flex(Flex::Center)
        .constraints([
            Constraint::Fill(1),
            Constraint::Max(100),
            Constraint::Fill(1),
        ])
        .split(content_area);

    let reading_col = horizontal[1];

    let title = format!(" {} ", app.reader_title);
    let content = Paragraph::new(markdown_to_text(&app.reader_content))
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.reader_scroll, 0));

    f.render_widget(content, reading_col);

    let help = Paragraph::new("  j/k scroll   y copy   q/esc back")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, help_area);
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
        KeyCode::Char('y') => {
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
