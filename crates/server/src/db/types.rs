use serde::Serialize;

pub struct VerificationSummary {
    pub target_ref: String,
    pub pass_count: i64,
    pub fail_count: i64,
    pub review_count: i64,
    pub na_count: i64,
}

pub struct RepoComplianceSummary {
    pub owner: String,
    pub repo: String,
    pub pass_count: i64,
    pub fail_count: i64,
    pub review_count: i64,
}

pub struct AuditEntry {
    pub id: i64,
    pub verification_type: String,
    pub owner: String,
    pub repo: String,
    pub target_ref: String,
    pub policy: String,
    pub pass_count: i64,
    pub fail_count: i64,
    pub review_count: i64,
    pub na_count: i64,
    pub verified_at: String,
}

#[derive(Serialize)]
pub struct RepoRow {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub description: Option<String>,
    pub language: Option<String>,
    pub default_branch: Option<String>,
    pub pushed_at: Option<String>,
    pub synced_at: String,
}

#[derive(Serialize)]
pub struct CachedPullRow {
    pub pr_number: i64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub draft: bool,
}

#[derive(Serialize)]
pub struct CachedReleaseRow {
    pub release_id: i64,
    pub tag_name: String,
    pub name: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    pub published_at: Option<String>,
    pub author: String,
    pub html_url: String,
    pub body: Option<String>,
}

pub struct OAuthClient {
    pub client_secret: Option<String>,
    pub(crate) redirect_uris_json: String,
    pub token_endpoint_auth_method: String,
}

impl OAuthClient {
    pub fn redirect_uris(&self) -> Vec<String> {
        serde_json::from_str(&self.redirect_uris_json).unwrap_or_default()
    }
}

pub struct AuthorizationCode {
    pub client_id: String,
    pub user_id: i64,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub scope: String,
}

pub struct RefreshedToken {
    pub scope: String,
}

pub struct OAuthState {
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub scope: String,
}
