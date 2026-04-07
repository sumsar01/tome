//! Shared HTTP client construction.

use anyhow::{Context, Result};
use reqwest::Client;

/// User-agent sent with all outbound HTTP requests.
pub const USER_AGENT: &str = concat!("tome/", env!("CARGO_PKG_VERSION"));

/// Build a [`reqwest::Client`] with the standard tome user-agent.
pub fn build_http_client() -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .context("Failed to build HTTP client")
}
