# Metsuke Self-Hosted Deployment Guide

Deploy Metsuke on your own infrastructure for use with GitHub.com or GitHub Enterprise Server (GHES) in air-gapped / closed-network environments.

## Prerequisites

| Requirement | Minimum |
|---|---|
| Docker Engine | 20.10+ |
| Docker Compose | v2.0+ |
| Disk | 1 GB (SQLite data) |
| RAM | 256 MB |
| Network | Outbound HTTPS to your GitHub instance (github.com or GHES) |

## 1. Create a GitHub App

Metsuke authenticates as a GitHub App. Create one on the target GitHub instance.

### 1.1 Navigate to App creation

- **GitHub.com** -- `https://github.com/settings/apps/new`
- **GHES** -- `https://<GHES_HOST>/settings/apps/new`

### 1.2 Fill in App settings

| Field | Value |
|---|---|
| GitHub App name | `metsuke` (or your choice) |
| Homepage URL | `https://<YOUR_METSUKE_HOST>` |
| Webhook URL | `https://<YOUR_METSUKE_HOST>/webhook/github` |
| Webhook secret | Generate a strong random string (save it for `GITHUB_WEBHOOK_SECRET`) |

### 1.3 Permissions

Set the following **Repository permissions**:

| Permission | Access |
|---|---|
| Actions | Read-only |
| Checks | Read & write |
| Contents | Read-only |
| Metadata | Read-only |
| Pull requests | Read & write |
| Commit statuses | Read & write |

### 1.4 Subscribe to events

Enable the following webhook events:

- Pull request
- Check suite
- Push

### 1.5 Generate a private key

After creating the App, click **Generate a private key**. Save the downloaded `.pem` file. You will need its contents for `GITHUB_APP_PRIVATE_KEY`.

### 1.6 Note the App credentials

Record these values from the App settings page:

- **App ID** (numeric) -- for `GITHUB_APP_ID`
- **Client ID** -- for `GITHUB_APP_CLIENT_ID`
- **Client secret** (generate one) -- for `GITHUB_APP_CLIENT_SECRET`

## 2. Configure environment

Create a `.env` file next to `docker-compose.yml`:

```bash
# Required -- GitHub App credentials
GITHUB_APP_ID=123456
GITHUB_APP_CLIENT_ID=Iv1.abcdef1234567890
GITHUB_APP_CLIENT_SECRET=your_client_secret_here
GITHUB_WEBHOOK_SECRET=your_webhook_secret_here

# Private key -- embed the full PEM content (use quotes to preserve newlines)
GITHUB_APP_PRIVATE_KEY="-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA...
...
-----END RSA PRIVATE KEY-----"

# The externally reachable URL of this Metsuke instance
BASE_URL=https://metsuke.internal.example.com

# Default database path inside the container (usually no change needed)
# DATABASE_URL=/data/metsuke.db
```

> **Tip:** Alternatively, mount the PEM file and use a wrapper script to set `GITHUB_APP_PRIVATE_KEY` from the file content at container startup.

## 3. Start Metsuke

```bash
docker compose up -d
```

Verify the service is healthy:

```bash
curl -s http://localhost:8080/health
# Expected: ok

curl -s http://localhost:8080/healthz
# Expected: JSON with DB connectivity status
```

View logs:

```bash
docker compose logs -f metsuke
```

## 4. GHES (GitHub Enterprise Server) Configuration

For on-premises GHES environments, the GitHub App must be created on your GHES instance (see Section 1.1).

Currently, Metsuke uses `github.com` API endpoints by default. For GHES, set the following environment variables in your `.env` or `docker-compose.yml`:

```bash
GITHUB_API_URL=https://github.example.com/api/v3
GITHUB_URL=https://github.example.com
```

> **Note:** GHES API support requires that libverify-github respects these environment variables. Verify compatibility with your Metsuke version before deploying.

Ensure the container can reach your GHES instance over HTTPS. In air-gapped networks, no outbound internet access is required -- only connectivity to the GHES host.

### Custom CA certificates

If your GHES instance uses an internal CA, mount the CA bundle into the container:

```yaml
services:
  metsuke:
    volumes:
      - ./ca-certificates/internal-ca.crt:/usr/local/share/ca-certificates/internal-ca.crt:ro
    environment:
      SSL_CERT_FILE: /usr/local/share/ca-certificates/internal-ca.crt
```

Then rebuild the CA store on startup by adding a command override or custom entrypoint that runs `update-ca-certificates` before the application starts.

## 5. TLS / Reverse Proxy

In production, terminate TLS in front of Metsuke. Below is an nginx example.

### nginx configuration

```nginx
server {
    listen 443 ssl http2;
    server_name metsuke.internal.example.com;

    ssl_certificate     /etc/nginx/ssl/metsuke.crt;
    ssl_certificate_key /etc/nginx/ssl/metsuke.key;

    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # Webhook payloads can be large
        client_max_body_size 10m;
    }
}

server {
    listen 80;
    server_name metsuke.internal.example.com;
    return 301 https://$host$request_uri;
}
```

Set `BASE_URL=https://metsuke.internal.example.com` to match the externally reachable hostname.

## 6. Air-Gapped / Closed-Network Deployment

In environments without internet access:

1. **Pre-pull the image** on a machine with connectivity, then transfer via `docker save` / `docker load`:

   ```bash
   # On a connected machine
   docker compose build
   docker save plenoai/metsuke:latest | gzip > metsuke-image.tar.gz

   # Transfer metsuke-image.tar.gz to the air-gapped host

   # On the air-gapped host
   docker load < metsuke-image.tar.gz
   ```

2. **Update `docker-compose.yml`** to use the `image:` directive instead of `build:`.

3. **DNS resolution** -- ensure the container can resolve and reach your GHES hostname. Use `extra_hosts` in docker-compose if internal DNS is not available:

   ```yaml
   services:
     metsuke:
       extra_hosts:
         - "github.example.com:10.0.1.50"
   ```

4. **No telemetry** -- Metsuke does not phone home. No outbound traffic is required beyond communication with your GitHub instance.

## 7. Backup and Restore

Metsuke stores all persistent data in a single SQLite database at `/data/metsuke.db` inside the container (mapped to the `metsuke_data` Docker volume).

### Backup

```bash
# Stop writes for a consistent snapshot
docker compose stop metsuke

# Copy the database from the volume
docker compose cp metsuke:/data/metsuke.db ./backup/metsuke-$(date +%Y%m%d).db

docker compose start metsuke
```

For zero-downtime backups, use SQLite's backup API:

```bash
docker compose exec metsuke sqlite3 /data/metsuke.db ".backup '/data/metsuke-backup.db'"
docker compose cp metsuke:/data/metsuke-backup.db ./backup/
```

### Restore

```bash
docker compose stop metsuke
docker compose cp ./backup/metsuke-20260331.db metsuke:/data/metsuke.db
docker compose start metsuke
```

### Scheduled backups

Use cron on the host:

```cron
0 2 * * * docker compose -f /opt/metsuke/docker-compose.yml exec -T metsuke sqlite3 /data/metsuke.db ".backup '/data/metsuke-backup.db'" && cp /var/lib/docker/volumes/metsuke_metsuke_data/_data/metsuke-backup.db /backup/metsuke-$(date +\%Y\%m\%d).db
```

## 8. Upgrade

1. Pull the latest image (or rebuild):

   ```bash
   docker compose pull
   # or
   docker compose build --no-cache
   ```

2. Back up the database (see Section 7).

3. Recreate the container:

   ```bash
   docker compose up -d
   ```

4. Verify health:

   ```bash
   curl -s http://localhost:8080/healthz
   ```

The SQLite database schema is migrated automatically on startup. Rollback by restoring the backup and pinning the previous image tag.

## Environment Variable Reference

| Variable | Required | Default | Description |
|---|---|---|---|
| `GITHUB_APP_ID` | Yes | -- | GitHub App numeric ID |
| `GITHUB_APP_CLIENT_ID` | Yes | -- | GitHub App Client ID |
| `GITHUB_APP_CLIENT_SECRET` | Yes | -- | GitHub App Client Secret |
| `GITHUB_APP_PRIVATE_KEY` | Yes | -- | PEM-encoded private key (full content) |
| `BASE_URL` | Yes | `https://metsuke.fly.dev` | Externally reachable URL of this instance |
| `GITHUB_WEBHOOK_SECRET` | Recommended | (none) | Webhook payload signature verification secret |
| `HOST` | No | `0.0.0.0` | Bind address |
| `PORT` | No | `8080` | Listen port |
| `DATABASE_URL` | No | `/data/metsuke.db` | SQLite database path |
| `GITHUB_API_URL` | GHES only | -- | GHES API base URL (e.g. `https://ghes.example.com/api/v3`) |
| `GITHUB_URL` | GHES only | -- | GHES base URL (e.g. `https://ghes.example.com`) |
| `SSL_CERT_FILE` | GHES only | -- | Path to custom CA certificate bundle |
