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

/// Convert Confluence storage format (HTML-like) to plain markdown.
/// This is a best-effort conversion for common elements.
fn html_to_markdown(title: &str, html: &str) -> String {
    let mut md = format!("# {title}\n\n");

    // Strip XML/HTML tags with a simple approach - good enough for display
    let mut result = String::new();
    let mut in_tag = false;
    let mut _in_code = false;
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '<' => {
                in_tag = true;
                // Peek at tag name for structural elements
                let tag_start = i + 1;
                let tag_end = chars[tag_start..]
                    .iter()
                    .position(|&c| c == '>' || c == ' ')
                    .map(|p| tag_start + p)
                    .unwrap_or(chars.len());
                let tag_name: String = chars[tag_start..tag_end]
                    .iter()
                    .collect::<String>()
                    .to_lowercase();

                match tag_name.trim_start_matches('/') {
                    "h1" => { if !tag_name.starts_with('/') { result.push_str("\n# "); } else { result.push('\n'); } }
                    "h2" => { if !tag_name.starts_with('/') { result.push_str("\n## "); } else { result.push('\n'); } }
                    "h3" => { if !tag_name.starts_with('/') { result.push_str("\n### "); } else { result.push('\n'); } }
                    "h4" | "h5" | "h6" => { if !tag_name.starts_with('/') { result.push_str("\n#### "); } else { result.push('\n'); } }
                    "p" => { if tag_name.starts_with('/') { result.push_str("\n\n"); } }
                    "br" => result.push('\n'),
                    "li" => { if !tag_name.starts_with('/') { result.push_str("\n- "); } }
                    "code" => {
                        if !tag_name.starts_with('/') { result.push('`'); _in_code = true; }
                        else { result.push('`'); _in_code = false; }
                    }
                    "pre" => {
                        result.push_str("\n```\n");
                    }
                    "strong" | "b" => { result.push_str("**"); }
                    "em" | "i" => { result.push('*'); }
                    _ => {}
                }
                i = tag_end;
            }
            '>' if in_tag => {
                in_tag = false;
            }
            '&' => {
                // Basic HTML entity decoding
                let rest: String = chars[i..].iter().take(7).collect();
                if rest.starts_with("&amp;") { result.push('&'); i += 4; }
                else if rest.starts_with("&lt;") { result.push('<'); i += 3; }
                else if rest.starts_with("&gt;") { result.push('>'); i += 3; }
                else if rest.starts_with("&nbsp;") { result.push(' '); i += 5; }
                else if rest.starts_with("&quot;") { result.push('"'); i += 5; }
                else { result.push(chars[i]); }
            }
            c if !in_tag => result.push(c),
            _ => {}
        }
        i += 1;
    }

    md.push_str(&result);
    md
}
