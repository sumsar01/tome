use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::Client;
use serde::Deserialize;

use super::Source;
use crate::config::auth;

pub struct GitHubSource {
    owner: String,
    repo: String,
    git_ref: String,
    client: Client,
}

impl GitHubSource {
    pub fn new(repo: &str, git_ref: &str) -> Result<Self> {
        let (owner, repo_name) = repo
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("GitHub repo must be in 'owner/repo' format, got '{repo}'"))?;

        let client = Client::builder()
            .user_agent("tome/0.1")
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            owner: owner.to_string(),
            repo: repo_name.to_string(),
            git_ref: git_ref.to_string(),
            client,
        })
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GitHubContentResponse {
    #[serde(rename = "type")]
    kind: String,
    content: Option<String>,
    encoding: Option<String>,
    name: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GitHubDirEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    path: String,
}

#[async_trait]
impl Source for GitHubSource {
    async fn fetch_content(&self, path: &str) -> Result<String> {
        let token = auth::get_github_token()
            .context("Failed to retrieve GitHub token")?;

        let url = format!(
            "https://api.github.com/repos/{}/{}/contents/{}?ref={}",
            self.owner, self.repo, path, self.git_ref
        );

        let response = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .with_context(|| format!("Failed to fetch GitHub content at '{path}'"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("GitHub API error {status} for '{path}': {body}");
        }

        // GitHub returns either a file object or an array (directory)
        let text = response.text().await.context("Failed to read GitHub response")?;

        if text.trim_start().starts_with('[') {
            // Directory listing
            let entries: Vec<GitHubDirEntry> =
                serde_json::from_str(&text).context("Failed to parse GitHub directory listing")?;
            return format_directory_listing(&self.owner, &self.repo, path, &entries);
        }

        let file: GitHubContentResponse =
            serde_json::from_str(&text).context("Failed to parse GitHub file response")?;

        if file.kind == "dir" {
            anyhow::bail!("Path '{path}' is a directory. Use a file path or configure a source with a subdirectory.");
        }

        let encoding = file.encoding.as_deref().unwrap_or("none");
        let content = file.content.unwrap_or_default();

        if encoding == "base64" {
            let cleaned: String = content.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = STANDARD
                .decode(&cleaned)
                .context("Failed to decode base64 content from GitHub")?;
            String::from_utf8(bytes).context("GitHub file content is not valid UTF-8")
        } else {
            Ok(content)
        }
    }
}

fn format_directory_listing(
    owner: &str,
    repo: &str,
    path: &str,
    entries: &[GitHubDirEntry],
) -> Result<String> {
    let mut out = format!(
        "# {owner}/{repo}/{path}\n\nDirectory contents:\n\n"
    );
    for entry in entries {
        let icon = if entry.kind == "dir" { "📁" } else { "📄" };
        out.push_str(&format!("- {icon} `{}`\n", entry.name));
    }
    Ok(out)
}
