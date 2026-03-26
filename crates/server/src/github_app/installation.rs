use anyhow::{Context, Result};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::jwt::generate_jwt;

struct CachedToken {
    token: String,
    expires_at: Instant,
}

pub struct InstallationTokenCache {
    app_id: u64,
    private_key: Vec<u8>,
    tokens: DashMap<u64, CachedToken>,
    http: reqwest::Client,
    /// Cached mapping from "owner/repo" to installation_id.
    repo_installations: DashMap<String, u64>,
}

#[derive(serde::Deserialize)]
struct InstallationTokenResponse {
    token: String,
    expires_at: String,
}

#[derive(serde::Deserialize)]
struct RepoInstallation {
    id: u64,
}

impl InstallationTokenCache {
    pub fn new(app_id: u64, private_key: Vec<u8>) -> Self {
        Self {
            app_id,
            private_key,
            tokens: DashMap::new(),
            http: reqwest::Client::new(),
            repo_installations: DashMap::new(),
        }
    }

    /// Get an installation token for a repo, resolving installation_id if needed.
    pub async fn get_token(&self, owner: &str, repo: &str) -> Result<String> {
        let repo_full = format!("{owner}/{repo}");

        // Resolve installation_id
        let installation_id = if let Some(id) = self.repo_installations.get(&repo_full) {
            *id
        } else {
            let id = self.resolve_installation_id(owner, repo).await?;
            self.repo_installations.insert(repo_full, id);
            id
        };

        // Check cache
        if let Some(cached) = self.tokens.get(&installation_id)
            && cached.expires_at > Instant::now() + Duration::from_secs(300)
        {
            return Ok(cached.token.clone());
        }

        // Fetch new token
        let jwt = generate_jwt(self.app_id, &self.private_key)?;
        let resp: InstallationTokenResponse = self
            .http
            .post(format!(
                "https://api.github.com/app/installations/{installation_id}/access_tokens"
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "metsuke")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .context("failed to request installation token")?
            .error_for_status()
            .context("GitHub API returned error for installation token")?
            .json()
            .await
            .context("failed to parse installation token response")?;

        let token = resp.token.clone();

        // Parse expiry (ISO 8601) — approximate with 1 hour from now as fallback
        let expires_at = Instant::now() + Duration::from_secs(3600);

        self.tokens.insert(
            installation_id,
            CachedToken {
                token: resp.token,
                expires_at,
            },
        );

        Ok(token)
    }

    async fn resolve_installation_id(&self, owner: &str, repo: &str) -> Result<u64> {
        let jwt = generate_jwt(self.app_id, &self.private_key)?;
        let resp: RepoInstallation = self
            .http
            .get(format!(
                "https://api.github.com/repos/{owner}/{repo}/installation"
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "metsuke")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .context("failed to resolve installation")?
            .error_for_status()
            .context("GitHub App not installed on this repository")?
            .json()
            .await
            .context("failed to parse installation response")?;

        Ok(resp.id)
    }
}

/// Shared state for GitHub App authentication.
pub type SharedInstallationCache = Arc<InstallationTokenCache>;
