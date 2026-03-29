use anyhow::{Context, Result};

#[derive(Clone, Debug)]
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
        Self::from_getter(|key| std::env::var(key).ok())
    }

    pub(crate) fn from_getter<F>(get: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let host = get("HOST").unwrap_or_else(|| "0.0.0.0".into());
        let port: u16 = get("PORT")
            .unwrap_or_else(|| "8080".into())
            .parse()
            .context("PORT must be a valid u16")?;
        let github_app_id: u64 = get("GITHUB_APP_ID")
            .context("GITHUB_APP_ID required")?
            .parse()
            .context("GITHUB_APP_ID must be a number")?;
        let github_app_client_id =
            get("GITHUB_APP_CLIENT_ID").context("GITHUB_APP_CLIENT_ID required")?;
        let github_app_client_secret =
            get("GITHUB_APP_CLIENT_SECRET").context("GITHUB_APP_CLIENT_SECRET required")?;
        let github_app_private_key =
            get("GITHUB_APP_PRIVATE_KEY").context("GITHUB_APP_PRIVATE_KEY required")?;
        let database_url = get("DATABASE_URL").unwrap_or_else(|| "/data/metsuke.db".into());
        let base_url =
            get("BASE_URL").unwrap_or_else(|| "https://metsuke.fly.dev".into());
        let github_webhook_secret = get("GITHUB_WEBHOOK_SECRET");

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_map(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    fn required_vars() -> Vec<(&'static str, &'static str)> {
        vec![
            ("GITHUB_APP_ID", "12345"),
            ("GITHUB_APP_CLIENT_ID", "Iv1.abc"),
            ("GITHUB_APP_CLIENT_SECRET", "secret"),
            ("GITHUB_APP_PRIVATE_KEY", "-----BEGIN RSA PRIVATE KEY-----"),
        ]
    }

    #[test]
    fn from_getter_with_all_defaults() {
        let cfg = AppConfig::from_getter(env_map(&required_vars())).unwrap();
        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.database_url, "/data/metsuke.db");
        assert_eq!(cfg.base_url, "https://metsuke.fly.dev");
        assert_eq!(cfg.github_webhook_secret, None);
        assert_eq!(cfg.github_app_id, 12345);
    }

    #[test]
    fn from_getter_with_overrides() {
        let mut vars = required_vars();
        vars.extend([
            ("HOST", "127.0.0.1"),
            ("PORT", "3000"),
            ("DATABASE_URL", "/tmp/test.db"),
            ("BASE_URL", "http://localhost:3000"),
            ("GITHUB_WEBHOOK_SECRET", "whsec"),
        ]);
        let cfg = AppConfig::from_getter(env_map(&vars)).unwrap();
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 3000);
        assert_eq!(cfg.database_url, "/tmp/test.db");
        assert_eq!(cfg.base_url, "http://localhost:3000");
        assert_eq!(cfg.github_webhook_secret, Some("whsec".into()));
    }

    #[test]
    fn from_getter_missing_required_var() {
        // Missing GITHUB_APP_ID
        let cfg = AppConfig::from_getter(|_| None);
        assert!(cfg.is_err());
        let msg = cfg.unwrap_err().to_string();
        assert!(msg.contains("GITHUB_APP_ID"), "error should name the missing var: {msg}");
    }

    #[test]
    fn from_getter_invalid_port() {
        let mut vars = required_vars();
        vars.push(("PORT", "not-a-number"));
        let cfg = AppConfig::from_getter(env_map(&vars));
        assert!(cfg.is_err());
        assert!(cfg.unwrap_err().to_string().contains("PORT"));
    }

    #[test]
    fn from_getter_invalid_app_id() {
        let vars = vec![
            ("GITHUB_APP_ID", "not-a-number"),
            ("GITHUB_APP_CLIENT_ID", "Iv1.abc"),
            ("GITHUB_APP_CLIENT_SECRET", "secret"),
            ("GITHUB_APP_PRIVATE_KEY", "key"),
        ];
        let cfg = AppConfig::from_getter(env_map(&vars));
        assert!(cfg.is_err());
        assert!(cfg.unwrap_err().to_string().contains("GITHUB_APP_ID"));
    }

    #[test]
    fn bind_address_formats_correctly() {
        let cfg = AppConfig::from_getter(env_map(&required_vars())).unwrap();
        assert_eq!(cfg.bind_address(), "0.0.0.0:8080");
    }
}
