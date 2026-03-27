# Metsuke (目付)

SDLC process inspector — Remote MCP Server + GitHub App powered by [libverify](https://github.com/HikaruEgashira/libverify).

## What is Metsuke?

Metsuke is a Remote MCP Server that provides SDLC compliance verification as tools that AI agents (Claude Code, etc.) can invoke. Named after the Edo-period inspector officials (目付), it continuously monitors and evaluates your organization's development processes.

## Features

- **3 MCP Tools**: `verify_pr`, `verify_release`, `verify_repo`
- **28 SDLC Controls**: SLSA v1.2 (Source/Build/Dependencies) + SOC2 CC7/CC8
- **9 Policy Presets**: default, oss, aiops, soc1, soc2, slsa-l1 through slsa-l4
- **GitHub App**: [Install on your org](https://github.com/apps/pleno-metsuke)
- **Agent Skills**: `/verify-pr`, `/verify-release`, `/verify-repo`

## Connect

**Remote MCP Server**:
```
https://metsuke.fly.dev/mcp
```

**Claude Code** (`~/.claude/settings.json`):
```json
{
  "mcpServers": {
    "metsuke": {
      "type": "url",
      "url": "https://metsuke.fly.dev/mcp"
    }
  }
}
```

**Agent Skills** (`.claude/skills/` をコピー):
- `/verify-pr owner/repo#123` — PR検証
- `/verify-release owner/repo v1.0..v1.1` — リリース検証
- `/verify-repo owner/repo` — リポジトリ依存性検証

## GitHub App

Install: https://github.com/apps/pleno-metsuke

## Build

```bash
cargo build --release
nix build .#default     # binary
nix build .#docker      # Docker image
docker build -t metsuke .
```

## License

MIT
