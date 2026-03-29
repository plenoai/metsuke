use anyhow::{Context, Result};

#[derive(Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub github_app_id: u64,
    pub github_app_client_id: String,
    pub github_app_client_secret: String,
    pub github_app_private_key: String,
    pub database_url: String,
    pub base_url: String,
    pub github_webhook_secret: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port: u16 = std::env::var("PORT")
            .unwrap_or_else(|_| "8080".into())
            .parse()
            .context("PORT must be a valid u16")?;
        let github_app_id: u64 = std::env::var("GITHUB_APP_ID")
            .context("GITHUB_APP_ID required")?
            .parse()
            .context("GITHUB_APP_ID must be a number")?;
        let github_app_client_id =
            std::env::var("GITHUB_APP_CLIENT_ID").context("GITHUB_APP_CLIENT_ID required")?;
        let github_app_client_secret = std::env::var("GITHUB_APP_CLIENT_SECRET")
            .context("GITHUB_APP_CLIENT_SECRET required")?;
        let github_app_private_key =
            std::env::var("GITHUB_APP_PRIVATE_KEY").context("GITHUB_APP_PRIVATE_KEY required")?;
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "/data/metsuke.db".into());
        let base_url =
            std::env::var("BASE_URL").unwrap_or_else(|_| "https://metsuke.fly.dev".into());
        let github_webhook_secret = std::env::var("GITHUB_WEBHOOK_SECRET").ok();

        Ok(Self {
            host,
            port,
            github_app_id,
            github_app_client_id,
            github_app_client_secret,
            github_app_private_key,
            database_url,
            base_url,
            github_webhook_secret,
        })
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
