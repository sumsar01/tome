use anyhow::{bail, Context, Result};

const KEYCHAIN_SERVICE: &str = "tome";
const CONFLUENCE_TOKEN_KEY: &str = "confluence_token";
const CONFLUENCE_EMAIL_KEY: &str = "confluence_email";
const GITHUB_TOKEN_KEY: &str = "github_token";

/// Prompt for and store a Confluence API token in the OS keychain.
/// The token is NEVER written to disk.
pub fn store_confluence_token(email: &str) -> Result<()> {
    let token = rpassword_prompt("Enter your Confluence API token (input hidden): ")?;
    if token.is_empty() {
        bail!("Token cannot be empty");
    }

    let token_entry = keyring::Entry::new(KEYCHAIN_SERVICE, CONFLUENCE_TOKEN_KEY)
        .context("Failed to create keychain entry for Confluence token")?;
    token_entry
        .set_password(&token)
        .context("Failed to store Confluence token in keychain")?;

    let email_entry = keyring::Entry::new(KEYCHAIN_SERVICE, CONFLUENCE_EMAIL_KEY)
        .context("Failed to create keychain entry for Confluence email")?;
    email_entry
        .set_password(email)
        .context("Failed to store Confluence email in keychain")?;

    println!("Confluence credentials stored securely in OS keychain.");
    println!("Email: {email}");
    println!("Token: stored (not displayed)");
    Ok(())
}

/// Retrieve stored Confluence credentials from the OS keychain.
pub fn get_confluence_credentials() -> Result<(String, String)> {
    let token_entry = keyring::Entry::new(KEYCHAIN_SERVICE, CONFLUENCE_TOKEN_KEY)
        .context("Failed to access keychain")?;
    let token = token_entry.get_password().context(
        "No Confluence token found. Run `tome auth confluence --email you@example.com` first.",
    )?;

    let email_entry = keyring::Entry::new(KEYCHAIN_SERVICE, CONFLUENCE_EMAIL_KEY)
        .context("Failed to access keychain")?;
    let email = email_entry.get_password().context(
        "No Confluence email found. Run `tome auth confluence --email you@example.com` first.",
    )?;

    Ok((email, token))
}

/// Store a GitHub token in the OS keychain.
/// Prefers the existing `gh` CLI token if available.
pub fn store_github_token() -> Result<()> {
    // Try to reuse gh CLI token first
    if let Ok(gh_token) = get_gh_cli_token() {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, GITHUB_TOKEN_KEY)
            .context("Failed to create keychain entry for GitHub token")?;
        entry
            .set_password(&gh_token)
            .context("Failed to store GitHub token in keychain")?;
        println!("GitHub token imported from gh CLI and stored in OS keychain.");
        return Ok(());
    }

    println!("Could not find a gh CLI token. Enter a GitHub Personal Access Token instead.");
    let token = rpassword_prompt("Enter your GitHub token (input hidden): ")?;
    if token.is_empty() {
        bail!("Token cannot be empty");
    }

    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, GITHUB_TOKEN_KEY)
        .context("Failed to create keychain entry for GitHub token")?;
    entry
        .set_password(&token)
        .context("Failed to store GitHub token in keychain")?;

    println!("GitHub token stored securely in OS keychain.");
    Ok(())
}

/// Retrieve the GitHub token — first from keychain, then from gh CLI.
pub fn get_github_token() -> Result<String> {
    // Try keychain first
    if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, GITHUB_TOKEN_KEY) {
        if let Ok(token) = entry.get_password() {
            return Ok(token);
        }
    }

    // Fall back to gh CLI
    get_gh_cli_token().context(
        "No GitHub token found. Run `tome auth github` or ensure you are logged in via `gh auth login`.",
    )
}

fn get_gh_cli_token() -> Result<String> {
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .context("Failed to run `gh auth token`")?;

    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    bail!("gh CLI returned no token")
}

/// Prompt for a password without echoing to the terminal.
fn rpassword_prompt(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    let token = rpassword::read_password().context("Failed to read token from terminal")?;
    Ok(token)
}
