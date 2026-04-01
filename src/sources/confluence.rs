use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use super::Source;
use crate::config::auth;

/// Confluence API v2 source
pub struct ConfluenceSource {
    base_url: String,
    client: Client,
}

impl ConfluenceSource {
    pub fn new(base_url: &str) -> Result<Self> {
        let client = Client::builder()
            .user_agent("tome/0.1")
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    fn api_url(&self, endpoint: &str) -> String {
        format!("{}/wiki/api/v2/{}", self.base_url, endpoint.trim_start_matches('/'))
    }
}

#[derive(Deserialize)]
struct PageResponse {
    title: String,
    body: Option<PageBody>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct PageBody {
    storage: Option<StorageBody>,
    atlas_doc_format: Option<AtlasBody>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct AtlasBody {
    value: String,
}

#[derive(Deserialize)]
struct StorageBody {
    value: String,
}

#[async_trait]
impl Source for ConfluenceSource {
    async fn fetch_content(&self, page_id: &str) -> Result<String> {
        let (email, token) = auth::get_confluence_credentials()
            .context("Failed to retrieve Confluence credentials")?;

        let url = self.api_url(&format!(
            "pages/{}?body-format=storage",
            page_id
        ));

        let response = self
            .client
            .get(&url)
            .basic_auth(&email, Some(&token))
            .send()
            .await
            .with_context(|| format!("Failed to fetch Confluence page {page_id}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "Confluence API error {status} for page {page_id}: {body}"
            );
        }

        let page: PageResponse = response
            .json()
            .await
            .context("Failed to parse Confluence page response")?;

        let html = page
            .body
            .as_ref()
            .and_then(|b| b.storage.as_ref())
            .map(|s| s.value.as_str())
            .unwrap_or("");

        let markdown = html_to_markdown(&page.title, html);
        Ok(markdown)
    }
}

/// Convert Confluence storage format (XHTML) to plain markdown.
///
/// Strategy (two-pass):
/// 1. Pre-process: replace Confluence code macros (which contain CDATA) with
///    fenced markdown code blocks; strip all remaining `<ac:...>` tags.
/// 2. Walk the resulting HTML char-by-char converting standard HTML elements.
fn html_to_markdown(title: &str, html: &str) -> String {
    // Pass 1 – handle Confluence-specific constructs
    let preprocessed = preprocess_confluence(html);

    // Pass 2 – convert standard HTML
    let raw_md = html_walk(&preprocessed);

    // Pass 3 – clean up whitespace
    let cleaned = post_process(&raw_md);

    format!("# {title}\n\n{}", cleaned.trim())
}

/// Pre-process Confluence storage XML:
/// - Extract `<ac:structured-macro ac:name="code">` blocks → fenced code
/// - Strip all remaining `<ac:...>` and `<ri:...>` tags (and their content for
///   known noise-only macros like `toc`)
fn preprocess_confluence(html: &str) -> String {
    let mut out = String::new();
    let mut remaining = html;

    while !remaining.is_empty() {
        // Look for an ac:structured-macro opening tag
        if let Some(macro_pos) = remaining.find("<ac:structured-macro") {
            // Emit everything before the macro tag as-is (will be walked later)
            out.push_str(&remaining[..macro_pos]);
            remaining = &remaining[macro_pos..];

            // Find where the opening tag ends (could be self-closing or have body)
            let tag_close = remaining.find('>').unwrap_or(remaining.len() - 1);
            let opening_tag = &remaining[..=tag_close];

            // Is it a code macro?
            let is_code = opening_tag.contains("ac:name=\"code\"");

            if opening_tag.ends_with("/>") {
                // Self-closing — no body, skip it
                remaining = &remaining[tag_close + 1..];
                continue;
            }

            // Find the matching </ac:structured-macro>
            let end_marker = "</ac:structured-macro>";
            let body_start = tag_close + 1;
            if let Some(end_pos) = remaining[body_start..].find(end_marker) {
                let macro_body = &remaining[body_start..body_start + end_pos];
                remaining = &remaining[body_start + end_pos + end_marker.len()..];

                if is_code {
                    // Extract language hint from <ac:parameter ac:name="language">LANG</ac:parameter>
                    let lang = extract_ac_parameter(macro_body, "language").unwrap_or_default();
                    // Extract CDATA content from ac:plain-text-body
                    let code = extract_cdata(macro_body);
                    out.push_str("\n\n```");
                    out.push_str(lang);
                    out.push('\n');
                    out.push_str(code.trim_matches('\n'));
                    out.push_str("\n```\n\n");
                }
                // Non-code macros (toc etc.) are silently dropped
            } else {
                // No closing tag found — skip rest
                break;
            }
        } else {
            // No more macros — emit the rest
            out.push_str(remaining);
            break;
        }
    }

    // Strip all remaining ac:/ri: tags (leave their text children if any)
    strip_ac_tags(&out)
}

/// Extract the content inside <![CDATA[...]]> within the given string.
fn extract_cdata(s: &str) -> &str {
    if let Some(start) = s.find("<![CDATA[") {
        let content_start = start + 9; // len("<![CDATA[")
        if let Some(end) = s[content_start..].find("]]>") {
            return &s[content_start..content_start + end];
        }
    }
    s
}

/// Extract the text content of an `<ac:parameter ac:name="NAME">VALUE</ac:parameter>` element.
fn extract_ac_parameter<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("ac:name=\"{name}\">");
    let close = "</ac:parameter>";
    let start = s.find(open.as_str())? + open.len();
    let end = s[start..].find(close)? + start;
    Some(s[start..end].trim())
}

/// Strip tags whose name starts with "ac:" or "ri:" (Confluence/Atlassian
/// custom elements). Their text content is preserved; attributes are dropped.
fn strip_ac_tags(html: &str) -> String {
    let mut out = String::new();
    let mut remaining = html;

    while !remaining.is_empty() {
        if let Some(open) = remaining.find('<') {
            out.push_str(&remaining[..open]);
            remaining = &remaining[open..];

            // Read tag name
            let tag_name_start = if remaining.starts_with("</") { 2 } else { 1 };
            let tag_name_end = remaining[tag_name_start..]
                .find(|c: char| c == '>' || c == ' ' || c == '/')
                .map(|p| tag_name_start + p)
                .unwrap_or(remaining.len());
            let tag_name = &remaining[tag_name_start..tag_name_end];

            if tag_name.starts_with("ac:") || tag_name.starts_with("ri:") {
                // Skip the entire tag (up to closing >)
                if let Some(close) = remaining.find('>') {
                    remaining = &remaining[close + 1..];
                } else {
                    break;
                }
            } else {
                // Normal tag — keep the '<' and continue
                out.push('<');
                remaining = &remaining[1..];
            }
        } else {
            out.push_str(remaining);
            break;
        }
    }

    out
}

/// Walk standard HTML and emit markdown equivalents.
fn html_walk(html: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    let mut in_tag = false;

    while i < chars.len() {
        match chars[i] {
            '<' => {
                in_tag = true;
                let tag_start = i + 1;
                let tag_end = chars[tag_start..]
                    .iter()
                    .position(|&c| c == '>' || c == ' ' || c == '/')
                    .map(|p| tag_start + p)
                    .unwrap_or(chars.len());
                let tag_name_raw: String = chars[tag_start..tag_end].iter().collect();
                let tag_name = tag_name_raw.to_lowercase();
                let is_closing = tag_name.starts_with('/');
                let base = tag_name.trim_start_matches('/');

                match base {
                    "h1" => { if !is_closing { result.push_str("\n\n# "); } else { result.push('\n'); } }
                    "h2" => { if !is_closing { result.push_str("\n\n## "); } else { result.push('\n'); } }
                    "h3" => { if !is_closing { result.push_str("\n\n### "); } else { result.push('\n'); } }
                    "h4" | "h5" | "h6" => { if !is_closing { result.push_str("\n\n#### "); } else { result.push('\n'); } }
                    "p" => { if is_closing { result.push_str("\n\n"); } }
                    "br" => result.push('\n'),
                    "ul" | "ol" => { if is_closing { result.push('\n'); } }
                    "li" => { if !is_closing { result.push_str("\n- "); } }
                    "code" => result.push('`'),
                    "pre" => {
                        if !is_closing { result.push_str("\n\n```\n"); }
                        else { result.push_str("\n```\n\n"); }
                    }
                    "strong" | "b" => result.push_str("**"),
                    "em" | "i" => result.push('*'),
                    _ => {}
                }
                i = tag_end;
            }
            '>' if in_tag => { in_tag = false; }
            '&' if !in_tag => {
                // Decode named HTML entities (including curly quotes from Confluence)
                let rest: String = chars[i..].iter().take(9).collect();
                let (ch, skip) = decode_entity(&rest);
                result.push(ch);
                i += skip;
            }
            c if !in_tag => result.push(c),
            _ => {}
        }
        i += 1;
    }

    result
}

/// Decode a single HTML entity at the start of `s`. Returns (char, bytes_consumed - 1)
/// so the caller can do `i += skip` before the usual `i += 1`.
fn decode_entity(s: &str) -> (char, usize) {
    // Named entities — extend as needed
    let entities: &[(&str, char)] = &[
        ("&amp;",   '&'),
        ("&lt;",    '<'),
        ("&gt;",    '>'),
        ("&nbsp;",  ' '),
        ("&quot;",  '"'),
        ("&apos;",  '\''),
        ("&ldquo;", '"'),
        ("&rdquo;", '"'),
        ("&lsquo;", '\''),
        ("&rsquo;", '\''),
        ("&ndash;", '–'),
        ("&mdash;", '—'),
        ("&hellip;",'…'),
    ];
    for &(entity, ch) in entities {
        if s.starts_with(entity) {
            return (ch, entity.len() - 1);
        }
    }
    // Numeric entity &#NNN; or &#xHH;
    if s.starts_with("&#") {
        if let Some(semi) = s.find(';') {
            let inner = &s[2..semi];
            let code_point = if inner.starts_with('x') || inner.starts_with('X') {
                u32::from_str_radix(&inner[1..], 16).ok()
            } else {
                inner.parse::<u32>().ok()
            };
            if let Some(cp) = code_point.and_then(char::from_u32) {
                return (cp, semi);
            }
        }
    }
    ('&', 0)
}

/// Post-process: collapse excess blank lines, drop empty heading lines.
fn post_process(md: &str) -> String {
    let mut out = String::new();
    let mut blank_count = 0usize;
    let mut in_fence = false;

    for line in md.lines() {
        let trimmed = line.trim();

        // Track fenced code blocks — don't modify their content
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            out.push_str(line);
            out.push('\n');
            blank_count = 0;
            continue;
        }

        if in_fence {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // Drop heading lines with no text after the hashes
        if matches!(trimmed, "#" | "##" | "###" | "####") {
            continue;
        }

        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                out.push('\n');
            }
        } else {
            blank_count = 0;
            out.push_str(line);  // preserve leading whitespace (list indents, etc.)
            out.push('\n');
        }
    }

    out
}
