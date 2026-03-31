# Metsuke Security & Data Handling Whitepaper

**Version:** 1.0
**Last Updated:** 2026-03-31
**Audience:** Enterprise security teams, SOC 2 auditors, compliance reviewers

---

## 1. Architecture Overview

Metsuke is an SDLC compliance verification service deployed as a Remote MCP Server and GitHub App. It inspects development process metadata (not source code) and reports verification results.

```
                         +-----------------------+
                         |    GitHub.com API      |
                         |  (api.github.com)      |
                         +-----+----------+------+
                               |          |
                   Webhook     |          | REST API
                  (push)       |          | (pull)
                               v          v
+-------------+       +------------------------+       +----------------+
|  MCP Client | OAuth |     Metsuke Server     |       |  Fly.io  (nrt) |
| (Claude Code| 2.1   |  +-----------------+   | mount |  +----------+  |
|  etc.)      +------>|  | axum HTTP server |   +------+->| /data    |  |
+-------------+ HTTPS |  +-----------------+   |       |  | (volume) |  |
                      |  | libverify engine |   |       |  +----------+  |
                      |  +-----------------+   |       +----------------+
                      |  | SQLite (WAL)     |   |
                      |  +-----------------+   |
                      +------------------------+

Data Flow:
  1. GitHub sends webhook (PR opened/synced, release published)
     --> HMAC-SHA256 signature verified
     --> Delivery ID deduplication
  2. Metsuke fetches PR/release metadata via GitHub API
     (installation token, scoped to metadata-only permissions)
  3. libverify engine evaluates controls against metadata
  4. Results posted as GitHub Check Run
  5. Results optionally served to MCP clients via OAuth 2.1
```

## 2. Authentication & Authorization

### 2.1 OAuth 2.1 (RFC 9728 / RFC 8414 / RFC 7591)

Metsuke implements a full OAuth 2.1 authorization server for MCP client access.

**Discovery Endpoints:**

| Endpoint | RFC | Purpose |
|---|---|---|
| `/.well-known/oauth-protected-resource` | RFC 9728 | Resource server metadata |
| `/.well-known/oauth-authorization-server` | RFC 8414 | Authorization server metadata |

**Supported Flows:**

- **Grant Types:** `authorization_code`, `refresh_token`
- **Response Types:** `code`
- **PKCE:** Required. Only `S256` is accepted (plain is rejected)
- **Dynamic Client Registration:** RFC 7591 via `POST /oauth/register`
- **Token Endpoint Auth Methods:** `none`, `client_secret_post`

**Authorization Flow:**

1. MCP client dynamically registers via `/oauth/register` with `redirect_uris`
2. Client initiates authorization at `/oauth/authorize` with PKCE `code_challenge` (S256)
3. Metsuke redirects user to GitHub OAuth (`github.com/login/oauth/authorize`) with `read:user` scope
4. GitHub authenticates user and redirects back to Metsuke callback
5. Metsuke exchanges GitHub code for GitHub access token, fetches user identity
6. Metsuke generates an authorization code and redirects to the MCP client's `redirect_uri`
7. Client exchanges authorization code + `code_verifier` at `/oauth/token`
8. Metsuke verifies PKCE S256 challenge, issues access + refresh tokens

**Token Lifecycle:**

| Token Type | TTL | Storage |
|---|---|---|
| Access Token | 1 hour (3600s) | SQLite `oauth_tokens` table |
| Refresh Token | 30 days (2,592,000s) | SQLite `oauth_tokens` table |
| Authorization Code | Short-lived, single-use | SQLite `authorization_codes` table, `used` flag |

**Token Properties:**
- 256-bit cryptographically random (32 bytes, base64url-encoded, 43 characters)
- Generated via `rand::Rng` with system CSPRNG
- Refresh token rotation: each refresh issues a new access + refresh token pair (old refresh token is invalidated)

### 2.2 GitHub App Permissions (Least Privilege)

Metsuke operates as a GitHub App with minimal permissions:

| Permission | Access Level | Purpose |
|---|---|---|
| Pull Requests | Read | Fetch PR metadata for verification |
| Checks | Write | Post verification results as Check Runs |
| Metadata | Read | Repository metadata (implicit, always granted) |
| Contents | None | Source code is never read |
| Issues | None | Not accessed |
| Administration | None | Not accessed |

**GitHub OAuth User Scope:** `read:user` only (used for identity during MCP OAuth flow).

**Installation Tokens:**
- Scoped to the repositories the GitHub App is installed on
- Cached in-memory with ~50 minute TTL (GitHub installation tokens expire in 1 hour)
- JWT for App authentication uses RS256 with a cached 9-minute TTL

### 2.3 Request Authentication (MCP Endpoints)

All MCP tool endpoints are protected by the `OAuthAuthLayer` middleware:

- Extracts `Bearer` token from the `Authorization` header
- Validates the token against the `oauth_tokens` table (checks expiry)
- Sets the authenticated `user_id` in a task-local for downstream handlers
- Returns `401 Unauthorized` with `WWW-Authenticate: Bearer resource_metadata="..."` (RFC 9728) on failure

## 3. Data Handling

### 3.1 Data Collected

Metsuke collects and stores only SDLC process metadata:

| Data Category | Examples | Storage |
|---|---|---|
| User identity | GitHub user ID, login, avatar URL | `users` table |
| Installation info | Installation ID, account login, account type | `installations` table |
| PR metadata (cached) | PR number, title, state, author, timestamps, draft status | `cached_pulls` table |
| Release metadata (cached) | Tag name, release name, author, timestamps, draft/prerelease flags | `cached_releases` table |
| Repository metadata (cached) | Repo name, language, default branch, visibility | `repositories` table |
| Verification results | Pass/fail/review/NA counts, control-level results (JSON) | `audit_log` table |
| OAuth client registrations | Client ID, redirect URIs, auth method | `oauth_clients` table |
| OAuth tokens | Access token, refresh token, scope, expiry | `oauth_tokens` table |

### 3.2 Data NOT Collected

- **Source code:** Metsuke never reads file contents, diffs, or blobs
- **Commit contents:** Only commit metadata (SHA, author, timestamps) is used by libverify
- **Secrets or credentials:** No scanning of repository secrets
- **Issue/PR body text:** Not stored (only titles for cache display)
- **User emails:** Not fetched from GitHub (only `read:user` scope)

### 3.3 Data Retention Policy

| Data Type | Retention |
|---|---|
| OAuth access tokens | Expire after 1 hour; removed on refresh |
| OAuth refresh tokens | Expire after 30 days |
| Authorization codes | Single-use; marked `used` after consumption |
| OAuth states | Short-lived; consumed on callback |
| Cached PR/release/repo data | Refreshed on sync; no automatic purge |
| Audit log | Retained indefinitely for compliance traceability |
| Webhook delivery ID dedup cache | In-memory only; cleared when cache exceeds 1000 entries or on restart |

### 3.4 Encryption

**In Transit:**
- All external communication uses TLS (HTTPS enforced via `force_https = true` in Fly.io)
- GitHub API calls use HTTPS exclusively (`https://api.github.com`)
- GitHub OAuth flow uses HTTPS (`https://github.com/login/oauth/`)

**At Rest:**
- SQLite database stored on Fly.io persistent volume (`/data`)
- Fly.io volumes use encrypted-at-rest block storage
- GitHub user tokens stored in the `users.github_token` column (encrypted at the volume level)
- OAuth client secrets stored in `oauth_clients.client_secret` column

**Key Management:**
- GitHub App private key: Provided via environment variable (`GITHUB_APP_PRIVATE_KEY`), never written to disk in application code
- Webhook secret: Provided via environment variable (`GITHUB_WEBHOOK_SECRET`)
- No application-level encryption keys are generated or managed (relies on infrastructure-level encryption)

## 4. Infrastructure

### 4.1 Fly.io Deployment

| Property | Value |
|---|---|
| Application | `metsuke` |
| Primary Region | `nrt` (Tokyo, Japan) |
| VM Size | `shared-cpu-1x`, 256 MB RAM |
| Internal Port | 8080 |
| HTTPS | Forced (`force_https = true`) |
| Auto-scaling | `auto_stop_machines = suspend`, `auto_start_machines = true` |
| Minimum Machines | 1 |
| Health Check | `GET /health` every 30s, 5s timeout, 10s grace period |

### 4.2 SQLite Persistence

| Property | Value |
|---|---|
| Storage | Fly.io persistent volume mounted at `/data` |
| Journal Mode | WAL (Write-Ahead Logging) |
| Synchronous Mode | NORMAL |
| Connection Pool | 1 writer + 4 read-only readers (round-robin) |
| Foreign Keys | Enforced (`PRAGMA foreign_keys=ON`) |
| Busy Timeout | 5000ms |
| Cache Size | 20 MB (writer), 5 MB (reader) |

### 4.3 Network Configuration

- Ingress: Fly.io edge proxy terminates TLS, forwards to port 8080
- No direct database access from the network (SQLite is embedded, file-based)
- Outbound: HTTPS to `api.github.com` and `github.com` only
- No other external service dependencies

### 4.4 Container Security

- Base image: `debian:bookworm-slim` (minimal attack surface)
- Non-root user: `metsuke` (UID 1000)
- No shell utilities beyond `ca-certificates`
- Multi-stage build: build dependencies are not present in the runtime image

## 5. Webhook Security

### 5.1 HMAC-SHA256 Signature Verification

When `GITHUB_WEBHOOK_SECRET` is configured:

1. Metsuke extracts the `X-Hub-Signature-256` header from incoming webhook requests
2. Computes HMAC-SHA256 over the raw request body using the shared secret
3. Performs constant-time comparison (via `hmac::Mac::verify_slice`) against the provided signature
4. Rejects the request with `401 Unauthorized` if verification fails

### 5.2 Delivery ID Deduplication

- Each webhook includes an `X-GitHub-Delivery` header with a unique UUID
- Metsuke maintains an in-memory `HashSet` of seen delivery IDs
- Duplicate deliveries are acknowledged (`200 OK`) but not processed
- The dedup cache is capped at 1000 entries to bound memory usage; when full, it is cleared

### 5.3 Event Filtering

Only specific events are processed:

| Event | Actions Processed | Action Taken |
|---|---|---|
| `pull_request` | `opened`, `synchronize`, `reopened` | Run PR verification, post Check Run |
| `release` | `published` | Run release verification |
| `ping` | N/A | Acknowledged, logged |
| All others | N/A | Ignored |

## 6. Self-Hosting Security Considerations

For organizations deploying Metsuke on their own infrastructure:

### 6.1 Required Environment Variables

| Variable | Sensitivity | Description |
|---|---|---|
| `GITHUB_APP_ID` | Low | GitHub App numeric ID |
| `GITHUB_APP_CLIENT_ID` | Low | OAuth client ID for the GitHub App |
| `GITHUB_APP_CLIENT_SECRET` | **High** | OAuth client secret |
| `GITHUB_APP_PRIVATE_KEY` | **Critical** | RSA private key (PEM) for JWT signing |
| `GITHUB_WEBHOOK_SECRET` | **High** | Shared secret for webhook HMAC verification |
| `DATABASE_URL` | Medium | Path to SQLite database file |
| `BASE_URL` | Low | Public URL of the Metsuke instance |

### 6.2 Recommendations

1. **Always configure `GITHUB_WEBHOOK_SECRET`:** Without it, webhook signature verification is skipped, and any party that can reach your webhook endpoint can trigger verification runs.
2. **Restrict network access:** The webhook endpoint (`/webhook`) should only accept connections from [GitHub's webhook IP ranges](https://api.github.com/meta).
3. **Protect the database file:** SQLite stores OAuth tokens and GitHub tokens. Ensure file permissions restrict access to the application user only.
4. **Use a secrets manager:** Store `GITHUB_APP_PRIVATE_KEY` and `GITHUB_APP_CLIENT_SECRET` in a vault (e.g., HashiCorp Vault, AWS Secrets Manager) rather than plain environment variables.
5. **Run as non-root:** The provided Dockerfile creates a dedicated `metsuke` user (UID 1000).
6. **Enable TLS termination:** Place the service behind a reverse proxy with TLS (nginx, Caddy, cloud load balancer). The application itself does not terminate TLS.
7. **Monitor audit logs:** The `audit_log` table provides a full trail of all verification executions with timestamps, users, repositories, and results.
8. **Backup the database volume:** SQLite WAL mode supports hot backups. Schedule regular backups of the `/data` volume.

## 7. Incident Response

### 7.1 Credential Compromise

| Scenario | Response |
|---|---|
| GitHub App private key leaked | Regenerate the key in GitHub App settings; update `GITHUB_APP_PRIVATE_KEY`; all existing JWTs become invalid immediately |
| Webhook secret leaked | Rotate in GitHub App settings and `GITHUB_WEBHOOK_SECRET`; redeploy |
| OAuth client secret leaked | Delete the affected row in `oauth_clients`; client must re-register |
| Database file exfiltrated | Rotate GitHub App private key and client secret; invalidate all OAuth tokens by clearing `oauth_tokens` table; notify affected users |

### 7.2 Token Revocation

- **Access tokens** can be invalidated by deleting from the `oauth_tokens` table (TTL is 1 hour, so natural expiry is fast)
- **Refresh tokens** can be invalidated by deleting the corresponding row
- **GitHub installation tokens** are cached in-memory with ~50 minute TTL; restarting the service clears the cache
- There is no external token revocation endpoint at this time (tokens are opaque and validated server-side only)

### 7.3 Audit Trail

The `audit_log` table records every verification execution:

- `user_id`: Who triggered the verification
- `verification_type`: PR or release verification
- `owner`, `repo`, `target_ref`: What was verified
- `policy`: Which policy preset was applied
- `pass_count`, `fail_count`, `review_count`, `na_count`: Summary counts
- `result_json`: Full control-level results
- `verified_at`: Timestamp

This provides the evidence trail needed for SOC 2 CC7.2 (monitoring) and CC7.3 (response) controls.

## 8. Compliance Considerations

### 8.1 SOC 2

| Trust Service Criteria | Metsuke Alignment |
|---|---|
| **CC6.1** Logical Access | OAuth 2.1 with PKCE; GitHub identity federation; Bearer token validation |
| **CC6.3** Role-Based Access | GitHub App installation scoping; per-user token isolation |
| **CC7.1** System Monitoring | Health check endpoint; structured logging via `tracing` |
| **CC7.2** Anomaly Detection | Webhook signature verification; delivery ID deduplication; audit log |
| **CC7.3** Incident Response | Token revocation procedures; credential rotation runbooks (Section 7) |
| **CC8.1** Change Management | Metsuke itself verifies SDLC controls (SLSA Source, Build, Dependencies) |

### 8.2 GDPR

| Requirement | Status |
|---|---|
| **Lawful Basis** | Legitimate interest (SDLC compliance monitoring); consent obtained via GitHub OAuth flow |
| **Data Minimization** | Only process metadata required for verification; no source code access |
| **Right to Access** | User data is tied to GitHub identity; can be queried from `users` and `audit_log` tables |
| **Right to Erasure** | Delete user record and associated audit logs, tokens, and cached data |
| **Data Processor** | When self-hosted, the organization is the data controller; Pleno AI is not a sub-processor |
| **Cross-border Transfer** | Managed service runs in `nrt` (Tokyo); self-hosted customers control data location |

### 8.3 SLSA (Supply-chain Levels for Software Artifacts)

Metsuke verifies SLSA v1.2 controls across three tracks:

- **Source:** Branch protection, code review, signed commits
- **Build:** CI/CD pipeline provenance, build reproducibility
- **Dependencies:** Dependency review, vulnerability scanning, license compliance

Policy presets available: `slsa-l1`, `slsa-l2`, `slsa-l3`, `slsa-l4`.

### 8.4 ISMAP (Information system Security Management and Assessment Program)

For Japanese government cloud procurement:

- **Data Residency:** Primary region is `nrt` (Tokyo, Japan)
- **Access Control:** OAuth 2.1 with GitHub identity federation
- **Audit Logging:** Full verification audit trail in `audit_log`
- **Encryption:** TLS in transit; volume-level encryption at rest
- **Self-hosting option:** Organizations can deploy on ISMAP-certified infrastructure (e.g., AWS Tokyo, Azure Japan East) for full control

---

## Appendix A: Database Schema Summary

| Table | Purpose | PII |
|---|---|---|
| `users` | GitHub identity mapping | GitHub ID, login, avatar URL |
| `installations` | GitHub App installations | Account login |
| `sessions` | Web UI sessions | User ID reference |
| `oauth_clients` | Dynamic client registration | None |
| `authorization_codes` | OAuth auth codes (single-use) | User ID reference |
| `oauth_tokens` | Access/refresh tokens | User ID reference |
| `oauth_states` | PKCE flow state | None |
| `audit_log` | Verification results | User ID, repo owner |
| `repositories` | Cached repo metadata | None |
| `cached_pulls` | Cached PR metadata | PR author login |
| `cached_releases` | Cached release metadata | Release author login |

## Appendix B: Third-Party Dependencies (Security-Relevant)

| Crate | Purpose |
|---|---|
| `axum` | HTTP server framework |
| `rusqlite` | SQLite database driver |
| `hmac` + `sha2` | Webhook HMAC-SHA256 verification |
| `jsonwebtoken` | GitHub App JWT generation (RS256) |
| `rand` | Cryptographic random token generation |
| `reqwest` | HTTPS client for GitHub API |
| `libverify-*` | SDLC verification engine (core, github, policy, output) |
