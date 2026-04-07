use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, Options as ParseOptions, Parser, Tag, TagEnd,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};
use syntect::{easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet};

use super::theme::Theme;

// ── Syntax highlighting globals ───────────────────────────────────────────────

thread_local! {
    static SYNTAX_SET: SyntaxSet = SyntaxSet::load_defaults_newlines();
    static THEME_SET: ThemeSet = ThemeSet::load_defaults();
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Render markdown input into a styled ratatui `Text` value.
pub fn markdown_to_text(input: &str, theme: &Theme) -> Text<'static> {
    Renderer::new(theme).render(input)
}

/// Extract `(heading_level, heading_text)` pairs from markdown for ToC use.
/// Levels are 1-indexed (1 = H1, 2 = H2, …).
pub fn extract_headings(input: &str) -> Vec<(u16, String)> {
    let opts = ParseOptions::ENABLE_STRIKETHROUGH
        | ParseOptions::ENABLE_TABLES
        | ParseOptions::ENABLE_SMART_PUNCTUATION;

    let parser = Parser::new_ext(input, opts);
    let mut headings: Vec<(u16, String)> = Vec::new();
    let mut in_heading: Option<u16> = None;
    let mut buf = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                in_heading = Some(heading_level_to_u16(level));
                buf.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(lvl) = in_heading.take() {
                    headings.push((lvl, buf.trim().to_string()));
                }
            }
            Event::Text(t) if in_heading.is_some() => {
                buf.push_str(&t);
            }
            _ => {}
        }
    }

    headings
}

// ── Internal renderer ─────────────────────────────────────────────────────────

/// Maximum render width (columns) for code blocks, headings, and horizontal rules.
const RENDER_WIDTH: usize = 76;

struct Renderer {
    // Output
    lines: Vec<Line<'static>>,

    // Inline state
    spans: Vec<Span<'static>>,
    bold: bool,
    italic: bool,
    strikethrough: bool,

    // Block state
    in_code_block: bool,
    code_lang: String,
    code_lines: Vec<String>,

    // List state: stack of (ordered start counter | None for unordered)
    list_stack: Vec<Option<u64>>,

    // Blockquote depth
    blockquote_depth: usize,

    // Table accumulation for two-pass aligned rendering
    in_table: bool,
    in_table_header: bool,
    /// Collected rows: each row is a Vec of (plain_text, styled_spans) per cell
    #[allow(clippy::type_complexity)]
    table_rows: Vec<(bool, Vec<(String, Vec<Span<'static>>)>)>,
    table_current_row: Vec<(String, Vec<Span<'static>>)>,
    table_cell_text: String,
    table_cell_spans: Vec<Span<'static>>,

    // Cached styles (cloned from theme)
    accent: Color,
    accent_light: Color,
    fg_dim: Color,
    quote: Color,
    code_fg: Color,
    code_span_bg: Color,
    code_rule: Color,
    syntect_theme: &'static str,
}

impl Renderer {
    fn new(theme: &Theme) -> Self {
        Self {
            lines: Vec::new(),
            spans: Vec::new(),
            bold: false,
            italic: false,
            strikethrough: false,
            in_code_block: false,
            code_lang: String::new(),
            code_lines: Vec::new(),
            list_stack: Vec::new(),
            blockquote_depth: 0,
            in_table: false,
            in_table_header: false,
            table_rows: Vec::new(),
            table_current_row: Vec::new(),
            table_cell_text: String::new(),
            table_cell_spans: Vec::new(),
            accent: theme.accent,
            accent_light: theme.accent_light,
            fg_dim: theme.fg_dim,
            quote: theme.quote,
            code_fg: theme.code_fg,
            code_span_bg: theme.code_span_bg,
            code_rule: theme.code_rule,
            syntect_theme: theme.syntect_theme,
        }
    }

    fn flush_line(&mut self) {
        let mut line_spans = std::mem::take(&mut self.spans);
        if self.blockquote_depth > 0 {
            let prefix = Span::styled(
                "│ ".repeat(self.blockquote_depth),
                Style::default().fg(self.quote),
            );
            line_spans.insert(0, prefix);
        }
        self.lines.push(Line::from(line_spans));
    }

    fn push_blank(&mut self) {
        if self.blockquote_depth > 0 {
            self.lines.push(Line::from(Span::styled(
                "│".repeat(self.blockquote_depth),
                Style::default().fg(self.quote),
            )));
        } else {
            self.lines.push(Line::default());
        }
    }

    fn render_table(&mut self) {
        let rows = std::mem::take(&mut self.table_rows);
        if rows.is_empty() {
            return;
        }

        // Pass 1: compute per-column maximum display widths
        let num_cols = rows.iter().map(|(_, r)| r.len()).max().unwrap_or(0);
        let mut col_widths: Vec<usize> = vec![0; num_cols];
        for (_, row) in &rows {
            for (col_idx, (text, _)) in row.iter().enumerate() {
                let w = text.chars().count();
                if w > col_widths[col_idx] {
                    col_widths[col_idx] = w;
                }
            }
        }

        // Compute total separator width: col widths + " │ " (3 chars) between each col
        let total_w =
            col_widths.iter().sum::<usize>() + if num_cols > 1 { (num_cols - 1) * 3 } else { 0 };

        let sep_style = Style::default().fg(self.fg_dim);

        // Top border of the table
        self.lines.push(Line::from(Span::styled(
            "─".repeat(total_w.max(1)),
            sep_style,
        )));

        // Pass 2: render each row with padding
        for (is_header, row) in rows {
            let mut line_spans: Vec<Span<'static>> = Vec::new();

            for (col_idx, (text, cell_spans)) in row.into_iter().enumerate() {
                // Column separator between cells
                if col_idx > 0 {
                    line_spans.push(Span::styled(" │ ", sep_style));
                }

                // Emit the styled spans for this cell
                let text_width = text.chars().count();
                let pad = col_widths
                    .get(col_idx)
                    .copied()
                    .unwrap_or(0)
                    .saturating_sub(text_width);

                line_spans.extend(cell_spans);

                // Trailing padding spaces to align columns
                if pad > 0 {
                    line_spans.push(Span::raw(" ".repeat(pad)));
                }
            }

            self.lines.push(Line::from(line_spans));

            if is_header {
                // Separator after header
                self.lines.push(Line::from(Span::styled(
                    "─".repeat(total_w.max(1)),
                    sep_style,
                )));
            }
        }
    }

    fn render(mut self, input: &str) -> Text<'static> {
        let opts = ParseOptions::ENABLE_STRIKETHROUGH
            | ParseOptions::ENABLE_TABLES
            | ParseOptions::ENABLE_SMART_PUNCTUATION;

        let parser = Parser::new_ext(input, opts);

        for event in parser {
            self.handle_event(event);
        }

        // Flush any trailing content
        if !self.spans.is_empty() {
            self.flush_line();
        }

        Text::from(self.lines)
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => self.start_code_block(kind),
            Event::End(TagEnd::CodeBlock) => self.end_code_block(),
            Event::Text(t) if self.in_code_block => {
                for line in t.lines() {
                    self.code_lines.push(line.to_string());
                }
            }
            Event::Start(Tag::Heading { .. }) => self.start_heading(),
            Event::End(TagEnd::Heading(level)) => self.end_heading(level),
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                self.flush_line();
                self.push_blank();
            }
            Event::Start(Tag::BlockQuote(_)) => self.blockquote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                if self.blockquote_depth > 0 {
                    self.blockquote_depth -= 1;
                }
            }
            Event::Start(Tag::List(start)) => self.list_stack.push(start),
            Event::End(TagEnd::List(_)) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.push_blank();
                }
            }
            Event::Start(Tag::Item) => self.start_list_item(),
            Event::End(TagEnd::Item) => self.flush_line(),
            Event::Start(Tag::Strong) => self.bold = true,
            Event::End(TagEnd::Strong) => self.bold = false,
            Event::Start(Tag::Emphasis) => self.italic = true,
            Event::End(TagEnd::Emphasis) => self.italic = false,
            Event::Start(Tag::Strikethrough) => self.strikethrough = true,
            Event::End(TagEnd::Strikethrough) => self.strikethrough = false,
            Event::Code(t) => self.handle_inline_code(t),
            Event::Start(Tag::Link { .. }) => {}
            Event::End(TagEnd::Link) => {}
            Event::Start(Tag::Image { dest_url, .. }) => self.handle_image(dest_url),
            Event::End(TagEnd::Image) => {}
            Event::Start(Tag::Table(_)) => self.start_table(),
            Event::End(TagEnd::Table) => self.end_table(),
            Event::Start(Tag::TableHead) => {
                self.in_table_header = true;
                self.table_current_row.clear();
            }
            Event::End(TagEnd::TableHead) => {
                let row = std::mem::take(&mut self.table_current_row);
                self.table_rows.push((true, row));
                self.in_table_header = false;
            }
            Event::Start(Tag::TableRow) => self.table_current_row.clear(),
            Event::End(TagEnd::TableRow) => {
                let row = std::mem::take(&mut self.table_current_row);
                self.table_rows.push((false, row));
            }
            Event::Start(Tag::TableCell) => {
                self.table_cell_text.clear();
                self.table_cell_spans.clear();
            }
            Event::End(TagEnd::TableCell) => {
                let text = std::mem::take(&mut self.table_cell_text);
                let spans = std::mem::take(&mut self.table_cell_spans);
                self.table_current_row.push((text, spans));
            }
            Event::Rule => self.handle_rule(),
            Event::SoftBreak => self.spans.push(Span::raw(" ")),
            Event::HardBreak => self.flush_line(),
            Event::Text(t) if self.in_table => self.handle_table_text(t),
            Event::Text(t) => self.handle_text(t),
            Event::Html(t) | Event::InlineHtml(t) => {
                let s = t.trim().to_string();
                if !s.is_empty() {
                    self.spans
                        .push(Span::styled(s, Style::default().fg(self.fg_dim)));
                }
            }
            _ => {}
        }
    }

    // ── Code block helpers ─────────────────────────────────────────────────────

    fn start_code_block(&mut self, kind: CodeBlockKind) {
        self.in_code_block = true;
        self.code_lang = match kind {
            CodeBlockKind::Fenced(lang) => lang.to_string(),
            CodeBlockKind::Indented => String::new(),
        };
        self.code_lines.clear();
    }

    fn end_code_block(&mut self) {
        self.in_code_block = false;
        const CODE_WIDTH: usize = RENDER_WIDTH;
        let rule = Span::styled("─".repeat(CODE_WIDTH), Style::default().fg(self.code_rule));
        self.lines.push(Line::from(rule.clone()));

        if !self.code_lang.is_empty() {
            self.lines.push(Line::from(Span::styled(
                format!("  {}", self.code_lang),
                Style::default()
                    .fg(self.fg_dim)
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        let syntect_theme_name = self.syntect_theme;
        let code_lang = self.code_lang.clone();
        let code_lines = std::mem::take(&mut self.code_lines);

        SYNTAX_SET.with(|ss| {
            THEME_SET.with(|ts| {
                let syntax = if code_lang.is_empty() {
                    None
                } else {
                    ss.find_syntax_by_token(&code_lang)
                };
                let theme = ts
                    .themes
                    .get(syntect_theme_name)
                    .or_else(|| ts.themes.get("base16-ocean.dark"));
                let mut highlighter = syntax
                    .zip(theme)
                    .map(|(syn, thm)| HighlightLines::new(syn, thm));

                for cl in &code_lines {
                    let spans = match highlighter.as_mut() {
                        Some(hl) => {
                            let line_with_newline = format!("{}\n", cl);
                            match hl.highlight_line(&line_with_newline, ss) {
                                Ok(tokens) => {
                                    let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
                                    for (style, text) in tokens {
                                        if text.is_empty() || text == "\n" {
                                            continue;
                                        }
                                        let fg = syntect_color_to_ratatui(style.foreground);
                                        let mut ratatui_style = Style::default().fg(fg);
                                        use syntect::highlighting::FontStyle;
                                        if style.font_style.contains(FontStyle::BOLD) {
                                            ratatui_style =
                                                ratatui_style.add_modifier(Modifier::BOLD);
                                        }
                                        if style.font_style.contains(FontStyle::ITALIC) {
                                            ratatui_style =
                                                ratatui_style.add_modifier(Modifier::ITALIC);
                                        }
                                        if style.font_style.contains(FontStyle::UNDERLINE) {
                                            ratatui_style =
                                                ratatui_style.add_modifier(Modifier::UNDERLINED);
                                        }
                                        let text_owned = text.trim_end_matches('\n').to_string();
                                        spans.push(Span::styled(text_owned, ratatui_style));
                                    }
                                    spans
                                }
                                Err(_) => vec![Span::styled(
                                    format!("  {}", cl),
                                    Style::default().fg(self.code_fg),
                                )],
                            }
                        }
                        None => vec![Span::styled(
                            format!("  {}", cl),
                            Style::default().fg(self.code_fg),
                        )],
                    };
                    self.lines.push(Line::from(spans));
                }
            });
        });

        self.lines.push(Line::from(rule));
        self.push_blank();
        self.code_lang.clear();
    }

    // ── Heading helpers ────────────────────────────────────────────────────────

    fn start_heading(&mut self) {
        if !self.lines.is_empty() {
            self.push_blank();
        }
    }

    fn end_heading(&mut self, level: HeadingLevel) {
        const HEADING_WIDTH: usize = RENDER_WIDTH;
        let title: String = self.spans.iter().map(|s| s.content.as_ref()).collect();

        match level {
            HeadingLevel::H1 => {
                let style = Style::default()
                    .fg(Color::Black)
                    .bg(self.accent)
                    .add_modifier(Modifier::BOLD);
                let pad = HEADING_WIDTH.saturating_sub(title.chars().count() + 1);
                let mut spans: Vec<Span<'static>> = vec![Span::styled(" ".to_string(), style)];
                spans.extend(
                    self.spans
                        .drain(..)
                        .map(|s| Span::styled(s.content, style.patch(s.style))),
                );
                spans.push(Span::styled(" ".repeat(pad), style));
                self.lines.push(Line::from(spans));
            }
            HeadingLevel::H2 => {
                let title_style = Style::default()
                    .fg(self.accent)
                    .add_modifier(Modifier::BOLD);
                let dim_style = Style::default().fg(self.fg_dim);
                let prefix = "── ";
                let suffix = " ";
                let used = prefix.chars().count() + title.chars().count() + suffix.chars().count();
                let fill = HEADING_WIDTH.saturating_sub(used);
                let mut spans: Vec<Span<'static>> = vec![Span::styled(prefix, dim_style)];
                spans.extend(
                    self.spans
                        .drain(..)
                        .map(|s| Span::styled(s.content, title_style.patch(s.style))),
                );
                spans.push(Span::styled(suffix, dim_style));
                spans.push(Span::styled("─".repeat(fill), dim_style));
                self.lines.push(Line::from(spans));
            }
            _ => {
                let (style, prefix) = self.heading_style(level);
                let mut spans: Vec<Span<'static>> = vec![Span::raw(prefix)];
                spans.extend(
                    self.spans
                        .drain(..)
                        .map(|s| Span::styled(s.content, style.patch(s.style))),
                );
                self.lines.push(Line::from(spans));
            }
        }
        self.push_blank();
    }

    // ── List helpers ───────────────────────────────────────────────────────────

    fn start_list_item(&mut self) {
        let depth = self.list_stack.len().saturating_sub(1);
        let indent = "  ".repeat(depth);
        let bullet = match self.list_stack.last_mut() {
            Some(Some(n)) => {
                let s = format!("{indent}{}. ", n);
                *n += 1;
                s
            }
            _ => format!("{indent}• "),
        };
        self.spans
            .push(Span::styled(bullet, Style::default().fg(self.accent)));
    }

    // ── Inline formatting helpers ──────────────────────────────────────────────

    fn handle_inline_code(&mut self, t: pulldown_cmark::CowStr) {
        let s = t.into_string();
        let span = Span::styled(
            format!(" {} ", s),
            Style::default().fg(self.code_fg).bg(self.code_span_bg),
        );
        if self.in_table {
            self.table_cell_text.push_str(&s);
            self.table_cell_spans.push(span);
        } else {
            self.spans.push(span);
        }
    }

    fn handle_image(&mut self, dest_url: pulldown_cmark::CowStr) {
        self.spans.push(Span::styled(
            format!("[image: {}]", dest_url),
            Style::default()
                .fg(self.fg_dim)
                .add_modifier(Modifier::ITALIC),
        ));
    }

    // ── Table helpers ──────────────────────────────────────────────────────────

    fn start_table(&mut self) {
        self.push_blank();
        self.in_table = true;
        self.in_table_header = false;
        self.table_rows.clear();
        self.table_current_row.clear();
        self.table_cell_text.clear();
        self.table_cell_spans.clear();
    }

    fn end_table(&mut self) {
        self.in_table = false;
        self.render_table();
        self.push_blank();
    }

    fn handle_table_text(&mut self, t: pulldown_cmark::CowStr) {
        let s = t.into_string();
        self.table_cell_text.push_str(&s);
        let mut style = Style::default();
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strikethrough {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.in_table_header {
            style = style.add_modifier(Modifier::BOLD);
        }
        self.table_cell_spans.push(Span::styled(s, style));
    }

    // ── Rule and text helpers ──────────────────────────────────────────────────

    fn handle_rule(&mut self) {
        const RULE_WIDTH: usize = RENDER_WIDTH;
        self.push_blank();
        self.lines.push(Line::from(Span::styled(
            "─".repeat(RULE_WIDTH),
            Style::default().fg(self.fg_dim),
        )));
        self.push_blank();
    }

    fn handle_text(&mut self, t: pulldown_cmark::CowStr) {
        let mut style = Style::default();
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strikethrough {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.blockquote_depth > 0 {
            style = style.fg(self.quote).add_modifier(Modifier::ITALIC);
        }
        self.spans.push(Span::styled(t.into_string(), style));
    }

    fn heading_style(&self, level: HeadingLevel) -> (Style, &'static str) {
        match level {
            HeadingLevel::H1 => (
                Style::default()
                    .fg(Color::Black)
                    .bg(self.accent)
                    .add_modifier(Modifier::BOLD),
                " ",
            ),
            HeadingLevel::H2 => (
                Style::default()
                    .fg(self.accent)
                    .add_modifier(Modifier::BOLD),
                "── ",
            ),
            HeadingLevel::H3 => (
                Style::default()
                    .fg(self.accent_light)
                    .add_modifier(Modifier::BOLD),
                "› ",
            ),
            HeadingLevel::H4 => (
                Style::default()
                    .fg(self.accent_light)
                    .add_modifier(Modifier::ITALIC),
                "› ",
            ),
            _ => (
                Style::default()
                    .fg(self.fg_dim)
                    .add_modifier(Modifier::ITALIC),
                "",
            ),
        }
    }
}

/// Convert a syntect `Color` to a ratatui `Color::Rgb`.
fn syntect_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

fn heading_level_to_u16(level: HeadingLevel) -> u16 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}
