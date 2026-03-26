# Metsuke (目付)

SDLC process inspector — Remote MCP Server + GitHub App powered by [libverify](https://github.com/HikaruEgashira/libverify).

## What is Metsuke?

Metsuke is a Remote MCP Server that provides SDLC compliance verification as tools that AI agents (Claude Code, etc.) can invoke. Named after the Edo-period inspector officials (目付), it continuously monitors and evaluates your organization's development processes.

## Features

- **10 MCP Tools**: PR/release/repo verification, gap analysis, compliance posture, policy comparison
- **3 MCP Prompts**: Guided workflows for compliance audits, PR reviews, and policy onboarding
- **28 SDLC Controls**: SLSA v1.2 (Source/Build/Dependencies) + SOC2 CC7/CC8
- **9 Policy Presets**: default, oss, aiops, soc1, soc2, slsa-l1 through slsa-l4
- **GitHub App**: Install on your org for automatic PR/release verification via webhooks
- **Secure by Design**: Input validation, HMAC webhook verification, no token logging

## Quick Start

```bash
# Run with a GitHub token
GH_TOKEN=ghp_xxx cargo run

# Or with Docker
docker run -p 8080:8080 -e GH_TOKEN=ghp_xxx ghcr.io/hikaruegashira/metsuke

# Connect from Claude Code
# Add to your MCP config:
# { "url": "http://localhost:8080/mcp" }
```

## GitHub App Setup

1. Create a GitHub App at https://github.com/settings/apps/new
2. Set permissions: `contents:read`, `pull_requests:read`, `checks:read+write`, `statuses:read`, `metadata:read`
3. Subscribe to events: `pull_request`, `release`, `installation`
4. Generate a private key
5. Deploy with environment variables:

```bash
GITHUB_APP_ID=123456
GITHUB_APP_PRIVATE_KEY=$(base64 < private-key.pem)
GITHUB_WEBHOOK_SECRET=your-secret
```

## Build

```bash
# Cargo
cargo build --release

# Nix Flakes
nix build .#default     # binary
nix build .#docker      # Docker image

# Docker
docker build -t metsuke .
```

## License

MIT
