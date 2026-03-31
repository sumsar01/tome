# tome

A docs reader for humans and AI. Browse Confluence, GitHub, and local docs from your terminal — or expose them as tools to any AI agent via MCP.

```
tome                            # open TUI browser
tome get kubernetes             # print doc to stdout
tome read migration-guide       # open doc in TUI reader
tome search "migration"         # fuzzy search across all docs
tome mcp                        # start MCP server for AI agents
```

## Install

```bash
cargo install --path .
```

## Setup

### 1. Configure docs

Create `~/.config/tome/config.toml` (auto-created with examples on first run):

```toml
[cache]
enabled = true
ttl_seconds = 3600

# GitHub source
[[sources]]
name = "infra-docs"
type = "github"
repo = "yourorg/infra-docs"
path = "docs/"
ref = "main"

# Confluence source
[[sources]]
name = "confluence"
type = "confluence"
base_url = "https://yourco.atlassian.net"

# Local source
[[sources]]
name = "my-notes"
type = "local"
root = "/Users/you/docs/"

# Register individual docs with aliases
[[docs]]
alias = "kubernetes"
source = "confluence"
page_id = "123456"
tags = ["infra", "k8s"]

[[docs]]
alias = "migration-guide"
source = "infra-docs"
path = "docs/migration.md"
tags = ["migration"]

[[docs]]
alias = "runbook"
source = "my-notes"
path = "runbook.md"
tags = ["ops"]
```

### 2. Authenticate

```bash
# Confluence (token stored in OS keychain — never on disk)
tome auth confluence --email you@company.com

# GitHub (imports from gh CLI if already logged in)
tome auth github
```

> **Security:** Tokens are stored in your OS keychain via the `keyring` crate.
> They are **never** written to `config.toml` or any file on disk.
> The config file is safe to commit — it contains no secrets.

## MCP server (AI agents)

Run `tome mcp` to start an MCP stdio server. Configure it in your AI client:

```json
{
  "mcpServers": {
    "tome": {
      "command": "tome",
      "args": ["mcp"]
    }
  }
}
```

Exposed tools:

| Tool | Description |
|---|---|
| `tome_list` | List all registered doc aliases (optional tag filter) |
| `tome_get` | Fetch full content of a doc by alias |
| `tome_search` | Fuzzy search across aliases and tags |

Then tell your AI: *"use tome to get the kubernetes docs"* and it fetches them directly.

## TUI

### Browser screen
- `j` / `k` — navigate
- `/` — fuzzy filter
- `Enter` — open in reader
- `q` — quit

### Reader screen
- `j` / `k` — scroll
- `y` — copy to clipboard
- `q` / `Esc` — back to browser

## CLI reference

```bash
tome                            # TUI browser
tome read <alias>               # open doc in TUI reader
tome get <alias>                # print to stdout
tome get <alias> --no-cache     # force live fetch
tome list                       # list all aliases
tome search <query>             # fuzzy search
tome auth confluence --email <email>   # store Confluence token
tome auth github                # import/store GitHub token
tome cache clear                # wipe cache
tome cache status               # show cache info
tome mcp                        # start MCP stdio server
```

## Sources

| Type | Auth |
|---|---|
| GitHub | `gh auth token` (reused automatically) or stored via `tome auth github` |
| Confluence | API token stored in OS keychain via `tome auth confluence` |
| Local | No auth required |

## Cache

Docs are cached in `~/.cache/tome/` with a configurable TTL (default 1 hour).
Local files are never cached — they are read directly.

```bash
tome cache clear    # wipe all cached docs
tome cache status   # show cache size and entry count
```
