use anyhow::{Context, Result};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct GitHubApp {
    app_id: u64,
    private_key: EncodingKey,
    client_id: String,
    client_secret: String,
}

#[derive(Serialize)]
struct JwtClaims {
    iat: i64,
    exp: i64,
    iss: String,
}

#[derive(Deserialize)]
struct InstallationTokenResponse {
    token: String,
}

#[derive(Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
}

#[derive(Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    pub avatar_url: Option<String>,
}

#[derive(Deserialize)]
pub struct Installation {
    pub id: i64,
    pub account: InstallationAccount,
}

#[derive(Deserialize)]
pub struct InstallationAccount {
    pub login: String,
    #[serde(rename = "type")]
    pub account_type: String,
}

impl GitHubApp {
    pub fn new(
        app_id: u64,
        private_key_pem: &str,
        client_id: String,
        client_secret: String,
    ) -> Result<Self> {
        let private_key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
            .context("Failed to parse GitHub App private key")?;
        Ok(Self {
            app_id,
            private_key,
            client_id,
            client_secret,
        })
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    fn generate_jwt(&self) -> Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        let claims = JwtClaims {
            iat: now - 60,
            exp: now + (10 * 60),
            iss: self.app_id.to_string(),
        };
        encode(&Header::new(Algorithm::RS256), &claims, &self.private_key)
            .context("Failed to encode JWT")
    }

    pub async fn create_installation_token(&self, installation_id: i64) -> Result<String> {
        let jwt = self.generate_jwt()?;
        let client = reqwest::Client::new();
        let resp: InstallationTokenResponse = client
            .post(format!(
                "https://api.github.com/app/installations/{installation_id}/access_tokens"
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "metsuke")
            .send()
            .await?
            .error_for_status()
            .context("Failed to create installation token")?
            .json()
            .await?;
        Ok(resp.token)
    }

    pub async fn get_installation(&self, installation_id: i64) -> Result<Installation> {
        let jwt = self.generate_jwt()?;
        let client = reqwest::Client::new();
        client
            .get(format!(
                "https://api.github.com/app/installations/{installation_id}"
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "metsuke")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("Failed to get installation")
    }

    pub async fn exchange_code(&self, code: &str) -> Result<OAuthTokenResponse> {
        let client = reqwest::Client::new();
        client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code", code),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("Failed to exchange OAuth code")
    }

    pub async fn get_user(access_token: &str) -> Result<GitHubUser> {
        let client = reqwest::Client::new();
        client
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", "metsuke")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("Failed to get GitHub user")
    }
}
