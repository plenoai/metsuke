use anyhow::{Context, Result};

#[derive(Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub github_token: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port: u16 = std::env::var("PORT")
            .unwrap_or_else(|_| "8080".into())
            .parse()
            .context("PORT must be a valid u16")?;
        let github_token = std::env::var("GH_TOKEN").ok();
        Ok(Self {
            host,
            port,
            github_token,
        })
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
