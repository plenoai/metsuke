# Metsuke (目付) — SDLC Process Inspector

Remote MCP Server + GitHub App powered by libverify.

## Commands

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
cargo build --release
```

## Architecture

Single-crate workspace (`crates/server`) producing the `metsuke` binary.

```
axum HTTP Server
├─ /mcp     → rmcp StreamableHttpService (10 tools, 3 prompts, resources)
├─ /webhook → GitHub App webhook handler (HMAC-SHA256 verified)
└─ /health  → health check
```

### MCP Tools

| Tool | Purpose |
|------|---------|
| `verify_pr` | PR verification against SDLC controls |
| `verify_release` | Release tag range verification |
| `verify_repo` | Repository dependency signature verification |
| `gap_analysis` | Gap analysis with remediation guidance |
| `compliance_posture` | Fleet-level compliance assessment |
| `policy_diff` | Compare two policy presets |
| `list_controls` | List 28 built-in controls |
| `explain_control` | Detailed control explanation |
| `list_policies` | List 9 policy presets |
| `format_sarif` | SARIF output conversion |

### Key Types

| Type | Module | Purpose |
|------|--------|---------|
| `MetsukeServer` | server.rs | MCP server struct with ToolRouter + PromptRouter |
| `AppConfig` | config.rs | Environment-based configuration |
| `WebhookState` | github_app/webhook.rs | Webhook signature verification |
| `InstallationTokenCache` | github_app/installation.rs | DashMap-backed token cache |

### Security

- All user inputs validated via `validation.rs` before processing
- GitHub names: alphanumeric + `-_. ` only, no path traversal
- Policy names: whitelist of 9 known presets only (blocks file path injection)
- Git refs: no `..` or null bytes
- Webhook: rejects requests when `GITHUB_WEBHOOK_SECRET` is not configured
- Tokens: `AppConfig` has no Debug derive to prevent accidental logging

### Sync-Async Bridge

libverify uses `reqwest::blocking`. All tool handlers use `tokio::task::spawn_blocking`.

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `GH_TOKEN` | Yes* | GitHub token (dev mode) |
| `GITHUB_APP_ID` | Yes* | GitHub App ID (production) |
| `GITHUB_APP_PRIVATE_KEY` | Yes* | PEM or base64-encoded private key |
| `GITHUB_WEBHOOK_SECRET` | Recommended | Webhook HMAC secret |
| `HOST` | No | Bind address (default: 0.0.0.0) |
| `PORT` | No | Port (default: 8080) |

*Either `GH_TOKEN` or `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY`.

## Build

```bash
# Nix
nix build .#default     # binary
nix build .#docker      # Docker image

# Cargo
cargo build --release

# Docker
docker build -t metsuke .
```
