use anyhow::{Context, Result};

#[derive(Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    /// Direct GitHub token (for development without GitHub App).
    pub github_token: Option<String>,
    /// GitHub App ID (for production with GitHub App auth).
    pub github_app_id: Option<u64>,
    /// GitHub App private key PEM (base64-encoded or raw).
    pub github_app_private_key: Option<Vec<u8>>,
    /// GitHub webhook secret for signature verification.
    pub github_webhook_secret: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port: u16 = std::env::var("PORT")
            .unwrap_or_else(|_| "8080".into())
            .parse()
            .context("PORT must be a valid u16")?;

        let github_token = std::env::var("GH_TOKEN").ok();

        let github_app_id = std::env::var("GITHUB_APP_ID")
            .ok()
            .map(|s| s.parse::<u64>())
            .transpose()
            .context("GITHUB_APP_ID must be a valid u64")?;

        let github_app_private_key = std::env::var("GITHUB_APP_PRIVATE_KEY").ok().map(|s| {
            // Support base64-encoded PEM (for env vars that can't hold newlines)
            if s.starts_with("-----BEGIN") {
                s.into_bytes()
            } else {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(&s)
                    .unwrap_or_else(|_| s.into_bytes())
            }
        });

        let github_webhook_secret = std::env::var("GITHUB_WEBHOOK_SECRET").ok();

        Ok(Self {
            host,
            port,
            github_token,
            github_app_id,
            github_app_private_key,
            github_webhook_secret,
        })
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
