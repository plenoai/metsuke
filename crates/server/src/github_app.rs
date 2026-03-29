use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct GitHubApp {
    app_id: u64,
    private_key: EncodingKey,
    client_id: String,
    client_secret: String,
    http: reqwest::Client,
    jwt_cache: Arc<std::sync::RwLock<Option<(String, std::time::Instant)>>>,
    token_cache: Arc<std::sync::RwLock<HashMap<i64, (String, std::time::Instant)>>>,
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

#[derive(Deserialize)]
struct UserInstallationsResponse {
    total_count: i64,
    installations: Vec<UserInstallation>,
}

#[derive(Deserialize)]
pub struct UserInstallation {
    pub id: i64,
    pub account: InstallationAccount,
}

#[derive(Deserialize)]
struct RepoListResponse {
    total_count: i64,
    repositories: Vec<Repository>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Repository {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub description: Option<String>,
    pub default_branch: Option<String>,
    pub language: Option<String>,
    pub updated_at: Option<String>,
    pub pushed_at: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub state: String,
    pub user: PullRequestUser,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub draft: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PullRequestUser {
    pub login: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Release {
    pub id: i64,
    pub tag_name: String,
    pub name: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    pub published_at: Option<String>,
    pub author: ReleaseAuthor,
    pub html_url: String,
    pub body: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ReleaseAuthor {
    pub login: String,
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
        let http = reqwest::Client::builder()
            .user_agent("metsuke")
            .pool_max_idle_per_host(20)
            .build()
            .context("Failed to build HTTP client")?;
        Ok(Self {
            app_id,
            private_key,
            client_id,
            client_secret,
            http,
            jwt_cache: Arc::new(std::sync::RwLock::new(None)),
            token_cache: Arc::new(std::sync::RwLock::new(HashMap::new())),
        })
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    fn generate_jwt(&self) -> Result<String> {
        const JWT_TTL: std::time::Duration = std::time::Duration::from_secs(9 * 60);

        // Check cache (read lock)
        {
            let cache = self.jwt_cache.read().unwrap();
            if let Some((ref token, created_at)) = *cache {
                if created_at.elapsed() < JWT_TTL {
                    return Ok(token.clone());
                }
            }
        }

        // Cache miss — generate a new JWT
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        let claims = JwtClaims {
            iat: now - 60,
            exp: now + (10 * 60),
            iss: self.app_id.to_string(),
        };
        let token = encode(&Header::new(Algorithm::RS256), &claims, &self.private_key)
            .context("Failed to encode JWT")?;

        // Store in cache (write lock)
        {
            let mut cache = self.jwt_cache.write().unwrap();
            *cache = Some((token.clone(), std::time::Instant::now()));
        }

        Ok(token)
    }

    pub async fn create_installation_token(&self, installation_id: i64) -> Result<String> {
        const TOKEN_TTL: std::time::Duration = std::time::Duration::from_secs(50 * 60);

        // Check cache (read lock)
        {
            let cache = self.token_cache.read().unwrap();
            if let Some((token, created_at)) = cache.get(&installation_id) {
                if created_at.elapsed() < TOKEN_TTL {
                    return Ok(token.clone());
                }
            }
        }

        // Cache miss — fetch from GitHub API
        let jwt = self.generate_jwt()?;
        let resp: InstallationTokenResponse = self
            .http
            .post(format!(
                "https://api.github.com/app/installations/{installation_id}/access_tokens"
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()
            .context("Failed to create installation token")?
            .json()
            .await?;

        // Store in cache (write lock)
        {
            let mut cache = self.token_cache.write().unwrap();
            cache.insert(
                installation_id,
                (resp.token.clone(), std::time::Instant::now()),
            );
        }

        Ok(resp.token)
    }

    pub async fn get_installation(&self, installation_id: i64) -> Result<Installation> {
        let jwt = self.generate_jwt()?;
        self.http
            .get(format!(
                "https://api.github.com/app/installations/{installation_id}"
            ))
            .header("Authorization", format!("Bearer {jwt}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("Failed to get installation")
    }

    pub async fn exchange_code(&self, code: &str) -> Result<OAuthTokenResponse> {
        self.http
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

    pub async fn list_installation_repos(&self, installation_id: i64) -> Result<Vec<Repository>> {
        let token = self.create_installation_token(installation_id).await?;
        let mut repos = Vec::new();
        let mut page = 1u32;
        loop {
            let resp: RepoListResponse = self
                .http
                .get("https://api.github.com/installation/repositories")
                .query(&[("per_page", "100"), ("page", &page.to_string())])
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/vnd.github+json")
                .send()
                .await?
                .error_for_status()
                .context("Failed to list installation repositories")?
                .json()
                .await?;
            let count = resp.repositories.len();
            repos.extend(resp.repositories);
            if count < 100 || repos.len() as i64 >= resp.total_count {
                break;
            }
            page += 1;
        }
        Ok(repos)
    }

    /// List all GitHub App installations accessible to the authenticated user.
    pub async fn list_user_installations(&self, user_token: &str) -> Result<Vec<UserInstallation>> {
        let mut installations = Vec::new();
        let mut page = 1u32;
        loop {
            let resp: UserInstallationsResponse = self
                .http
                .get("https://api.github.com/user/installations")
                .query(&[("per_page", "100"), ("page", &page.to_string())])
                .header("Authorization", format!("Bearer {user_token}"))
                .header("Accept", "application/vnd.github+json")
                .send()
                .await?
                .error_for_status()
                .context("Failed to list user installations")?
                .json()
                .await?;
            let count = resp.installations.len();
            installations.extend(resp.installations);
            if count < 100 || installations.len() as i64 >= resp.total_count {
                break;
            }
            page += 1;
        }
        Ok(installations)
    }

    pub async fn list_pull_requests(
        &self,
        installation_id: i64,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<PullRequest>> {
        let token = self.create_installation_token(installation_id).await?;
        self.http
            .get(format!("https://api.github.com/repos/{owner}/{repo}/pulls"))
            .query(&[("state", "all"), ("per_page", "30"), ("sort", "updated")])
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()
            .context("Failed to list pull requests")?
            .json()
            .await
            .context("Failed to parse pull requests")
    }

    pub async fn list_releases(
        &self,
        installation_id: i64,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<Release>> {
        let token = self.create_installation_token(installation_id).await?;
        self.http
            .get(format!(
                "https://api.github.com/repos/{owner}/{repo}/releases"
            ))
            .query(&[("per_page", "30")])
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()
            .context("Failed to list releases")?
            .json()
            .await
            .context("Failed to parse releases")
    }

    /// Create a Check Run on a commit to report verification results.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_check_run(
        &self,
        installation_id: i64,
        owner: &str,
        repo: &str,
        head_sha: &str,
        name: &str,
        conclusion: &str,
        title: &str,
        summary: &str,
    ) -> Result<()> {
        let token = self.create_installation_token(installation_id).await?;
        let body = serde_json::json!({
            "name": name,
            "head_sha": head_sha,
            "status": "completed",
            "conclusion": conclusion,
            "output": {
                "title": title,
                "summary": summary,
            }
        });
        self.http
            .post(format!(
                "https://api.github.com/repos/{owner}/{repo}/check-runs"
            ))
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()
            .await?
            .error_for_status()
            .context("Failed to create check run")?;
        Ok(())
    }

    pub async fn get_user(&self, access_token: &str) -> Result<GitHubUser> {
        self.http
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {access_token}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("Failed to get GitHub user")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    // 2048-bit RSA key generated for testing only (PKCS#1 format)
    const TEST_RSA_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEAqzbtQHmqIhOu650zIypzursOHTZhGltXZQ/aeDV8/XEADzQJ
vQ0wwhs1gae9/31tfQk/osJ93CH3QaBTS7VPHBGvd057hDbTq+Y7QAsVTy1SNIEN
Syh8CI+3oCXMkaZDpDQvFMBKjOYnHunqEQTIE6YRJX3dceFKDnEQ0nU9MLbwCRDW
QFFubzLIIP+BkzF1DC8jwwmU+nHtj+UD7izo6t8tsOeZmT6QjjAufYdsejbww0Ev
sN+mnoag2zHBNBpG6m/fIw9Hcvflo73T7i9iuPPoBpwKdx8COrjo/AxaghatJ7rJ
g3hdMPVBcIVUgmfWNYF11wsnTyLFmP0PWDUw1wIDAQABAoIBABxu2TSZX88b7LMV
Ho5q+OAcO0pPow2Q+LEAUnwfCdw+3U8pCar7G0tI4HhhJnTc3AdlN0ust+EMRPcB
jIOonvQe3cBW6L06q6lC6TkH/ihxctLkUZRXK03yrABs9o2Din0k62KrUlYWzI1e
NDBSVnWo4PUUc2d7jeRbE3uX26sQmHL7cSsNq8gyZQO6snR1t0mgnV8x66l+tsYy
Jt2rKg8bN1yVMUMeXIC+tf2/uhnIPdkAlu7S1/vMhqRfeJK+/56vuW+f5ilH1/12
OQx+jaUbVxuVkgFsiAYkLf202TGJFkK97UazFtrni+2Qp1YrZyiYFIiy/hGK58MI
FEGeNSUCgYEA5cLKtdQyKQ8rCXyTYCatJYC3xo/ZKnDxw4bVZR/cnsE9ueLCA9of
PW/AoDVj06XkDpI1EooP83BKlL02Uzf+YqALYGDGTQ9M+VBBnFNIrkKiYGFIQd99
RiHFPv2z/ix7ixs2vWHy7M6RPkSPzqR491OaIjk39ikvEANpDEy0kOMCgYEAvsR+
pWc4vL1lZFbIlzGvR4EGXXmVR/xKR/URtlsCQyuWmQiG2fbgw/ZigeMAcncNNXQD
9w9dhIWMI/6B27WHrAikEmK1OwvxXV1rDsohcozaTenIjj8bP/T1ZstXgSYY1qpX
zWgvHM+19GW6D9+uCWXzG6nJAzKPcLa8Z2VwZn0CgYAMRwJqAPLFOuhD04JUivyJ
mn03gQxLtklU92mDw9YYLZ9MxY80gX1V3Rjf9rpk3uJ23N01Jmd/zKpPlGTIwZ84
SfERr1opV/32/JDk95ZUqX7fw5MG4hhhnQBbQ1dQ57OaVVPxfsBqYwdj2moM0sEc
Bj2gQop4/u5i3qvIWnjznQKBgG77Q66YaZKsIMOKFXKYbh+MOZbB+A4EAXbxZReQ
xLUtM5TeOA2wKbz3pwFnfcgZ6K5TS0c9QiupwgjitMuMRVzZPhKQKF0sqoOlqHXX
NDQ/K3Wub4YJwqGnsejWnZa+Ai9ItIIEfXwmfvWrBN7dQ5OmIxPR5+abUIXDWcJR
al3FAoGAehWD1fvBiaKjCWdmV8/+ibed+kMGp5GjfBLS+QwD/dRpH/Xk0duuN8a2
DyPFdryNIZr/lOYzmWgXIpbTSufDiaac8ijgVKZZamEGWNrHZ5eu++9tWI9PEPtQ
J9/Y+qX1+dFvHem00HtuVTs2mItUXlLIOAlgtrWHl0pIzYSARxM=
-----END RSA PRIVATE KEY-----";

    fn test_app() -> GitHubApp {
        GitHubApp::new(12345, TEST_RSA_KEY, "Iv1.test".into(), "secret".into()).unwrap()
    }

    #[test]
    fn new_with_valid_key() {
        let app = GitHubApp::new(1, TEST_RSA_KEY, "cid".into(), "cs".into());
        assert!(app.is_ok());
    }

    #[test]
    fn new_with_invalid_key() {
        let app = GitHubApp::new(1, "not-a-pem-key", "cid".into(), "cs".into());
        assert!(app.is_err());
        let err = format!("{:#}", app.err().unwrap());
        assert!(
            err.contains("private key"),
            "error should mention private key: {err}"
        );
    }

    #[test]
    fn client_id_returns_configured_value() {
        let app = test_app();
        assert_eq!(app.client_id(), "Iv1.test");
    }

    #[test]
    fn generate_jwt_produces_valid_rs256_token() {
        let app = test_app();
        let jwt = app.generate_jwt().unwrap();

        // JWT has 3 dot-separated parts
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT should have header.payload.signature");

        // Decode header to verify algorithm
        let header_bytes = base64::prelude::BASE64_URL_SAFE_NO_PAD
            .decode(parts[0])
            .unwrap();
        let header: serde_json::Value = serde_json::from_slice(&header_bytes).unwrap();
        assert_eq!(header["alg"], "RS256");

        // Decode payload to verify claims
        let payload_bytes = base64::prelude::BASE64_URL_SAFE_NO_PAD
            .decode(parts[1])
            .unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&payload_bytes).unwrap();
        assert_eq!(claims["iss"], "12345");

        let iat = claims["iat"].as_i64().unwrap();
        let exp = claims["exp"].as_i64().unwrap();
        // exp should be ~10 minutes after iat (iat is now-60, exp is now+600)
        assert!(exp - iat > 0, "exp must be after iat");
        assert!(
            exp - iat <= 660 + 1,
            "token should expire within ~11 minutes of iat"
        );
    }

    #[test]
    fn github_user_deserializes() {
        let json = serde_json::json!({
            "id": 12345,
            "login": "octocat",
            "avatar_url": "https://avatars.githubusercontent.com/u/12345"
        });
        let user: GitHubUser = serde_json::from_value(json).unwrap();
        assert_eq!(user.id, 12345);
        assert_eq!(user.login, "octocat");
        assert_eq!(
            user.avatar_url,
            Some("https://avatars.githubusercontent.com/u/12345".into())
        );
    }

    #[test]
    fn github_user_deserializes_without_avatar() {
        let json = serde_json::json!({"id": 1, "login": "bot"});
        let user: GitHubUser = serde_json::from_value(json).unwrap();
        assert_eq!(user.avatar_url, None);
    }

    #[test]
    fn installation_deserializes() {
        let json = serde_json::json!({
            "id": 99,
            "account": {"login": "my-org", "type": "Organization"}
        });
        let inst: Installation = serde_json::from_value(json).unwrap();
        assert_eq!(inst.id, 99);
        assert_eq!(inst.account.login, "my-org");
        assert_eq!(inst.account.account_type, "Organization");
    }

    #[test]
    fn repository_deserializes_minimal() {
        let json = serde_json::json!({
            "id": 1,
            "name": "repo",
            "full_name": "org/repo",
            "private": false,
            "description": null,
            "default_branch": null,
            "language": null,
            "updated_at": null,
            "pushed_at": null
        });
        let repo: Repository = serde_json::from_value(json).unwrap();
        assert_eq!(repo.full_name, "org/repo");
        assert!(!repo.private);
    }

    #[test]
    fn pull_request_deserializes() {
        let json = serde_json::json!({
            "number": 42,
            "title": "Add feature",
            "state": "open",
            "user": {"login": "author"},
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-02T00:00:00Z",
            "draft": false
        });
        let pr: PullRequest = serde_json::from_value(json).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.user.login, "author");
        assert_eq!(pr.draft, Some(false));
    }

    #[test]
    fn release_deserializes() {
        let json = serde_json::json!({
            "id": 1,
            "tag_name": "v1.0.0",
            "name": "Release 1.0",
            "draft": false,
            "prerelease": false,
            "created_at": "2024-01-01T00:00:00Z",
            "published_at": "2024-01-01T00:00:00Z",
            "author": {"login": "releaser"},
            "html_url": "https://github.com/org/repo/releases/tag/v1.0.0",
            "body": "Changelog"
        });
        let rel: Release = serde_json::from_value(json).unwrap();
        assert_eq!(rel.tag_name, "v1.0.0");
        assert!(!rel.draft);
        assert_eq!(rel.body, Some("Changelog".into()));
    }
}
