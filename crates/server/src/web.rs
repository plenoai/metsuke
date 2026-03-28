use std::future::Future;
use std::sync::Arc;

use askama::Template;
use askama_web::WebTemplateExt;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::header::SET_COOKIE;
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use serde::{Deserialize, Serialize};

use crate::blocking::run_blocking;
use crate::config::AppConfig;
use crate::db::Database;
use crate::github_app::GitHubApp;
use crate::swr_cache::{CacheStatus, SwrCache};
use crate::validation::{self, validate_git_ref};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const POLICY_OPTIONS: &[&str] = &[
    "default", "oss", "aiops", "soc1", "soc2", "slsa-l1", "slsa-l2", "slsa-l3", "slsa-l4",
];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct WebState {
    db: Arc<Database>,
    github_app: Arc<GitHubApp>,
    base_url: String,
    cache: SwrCache,
}

pub fn router(db: Arc<Database>, github_app: Arc<GitHubApp>, config: &AppConfig) -> Router {
    let state = WebState {
        db,
        github_app,
        base_url: config.base_url.clone(),
        cache: SwrCache::new(
            std::time::Duration::from_secs(60),
            std::time::Duration::from_secs(300),
        ),
    };

    Router::new()
        .route("/", axum::routing::get(index))
        .route("/settings", axum::routing::get(settings))
        .route("/auth/login", axum::routing::get(login))
        .route("/auth/callback", axum::routing::get(auth_callback))
        .route("/auth/logout", axum::routing::get(logout))
        .route(
            "/auth/install/callback",
            axum::routing::get(install_callback),
        )
        .route("/repos", axum::routing::get(repos_page))
        .route(
            "/repos/{owner}/{repo}",
            axum::routing::get(repo_detail_page),
        )
        .route(
            "/repos/{owner}/{repo}/releases",
            axum::routing::get(verify_release_page),
        )
        .route(
            "/repos/{owner}/{repo}/pulls",
            axum::routing::get(verify_pr_page),
        )
        .route("/audit", axum::routing::get(audit_page))
        .route("/api/repos", axum::routing::get(api_repos))
        .route(
            "/api/repos/{owner}/{repo}/verify",
            axum::routing::post(api_verify_repo),
        )
        .route(
            "/api/repos/{owner}/{repo}/releases",
            axum::routing::get(api_list_releases),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-release",
            axum::routing::post(api_verify_release),
        )
        .route(
            "/api/repos/{owner}/{repo}/pulls",
            axum::routing::get(api_list_pulls),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-pr/{pr_number}",
            axum::routing::post(api_verify_pr),
        )
        .route(
            "/api/verification-cache",
            axum::routing::get(api_verification_cache),
        )
        .route("/api/audit-history", axum::routing::get(api_audit_history))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// SWR cache helper
// ---------------------------------------------------------------------------

/// Shared SWR response logic for GitHub API endpoints.
/// On Fresh: return cached. On Stale: return cached + spawn background refresh.
/// On Miss: fetch synchronously and cache.
async fn swr_respond<F, Fut, T>(cache: &SwrCache, key: String, label: &str, fetch_fn: F) -> Response
where
    F: FnOnce() -> Fut + Clone + Send + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send,
    T: Serialize + Send + 'static,
{
    match cache.get(&key).await {
        CacheStatus::Fresh(v) => Json(v).into_response(),
        CacheStatus::Stale(v) => {
            let cache = cache.clone();
            let ck = key.clone();
            let label = label.to_owned();
            let f = fetch_fn.clone();
            cache.mark_revalidating(&ck).await;
            tokio::spawn(async move {
                match f().await {
                    Ok(fresh) => cache.set(ck, fresh).await,
                    Err(e) => {
                        tracing::warn!("SWR background revalidation failed for {label}: {e:#}")
                    }
                }
            });
            Json(v).into_response()
        }
        CacheStatus::Miss => match fetch_fn().await {
            Ok(fresh) => {
                cache.set(key, &fresh).await;
                Json(fresh).into_response()
            }
            Err(e) => {
                tracing::warn!("SWR fetch failed for {label}: {e:#}");
                Json(serde_json::Value::Array(vec![])).into_response()
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Template structs
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate {
    title: String,
    message: String,
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate {
    login: String,
    active_page: &'static str,
    installations: Vec<(i64, String, String)>,
    base_url: String,
}

#[derive(Template)]
#[template(path = "repos.html")]
struct ReposTemplate {
    login: String,
    active_page: &'static str,
    policy_options: &'static [&'static str],
}

#[derive(Template)]
#[template(path = "repo_detail.html")]
struct RepoDetailTemplate {
    login: String,
    active_page: &'static str,
    owner: String,
    repo: String,
    policy_options: &'static [&'static str],
}

#[derive(Template)]
#[template(path = "verify_pr.html")]
struct VerifyPrTemplate {
    login: String,
    active_page: &'static str,
    owner: String,
    repo: String,
    policy_options: &'static [&'static str],
}

#[derive(Template)]
#[template(path = "verify_release.html")]
struct VerifyReleaseTemplate {
    login: String,
    active_page: &'static str,
    owner: String,
    repo: String,
    policy_options: &'static [&'static str],
}

#[derive(Template)]
#[template(path = "audit.html")]
struct AuditTemplate {
    login: String,
    active_page: &'static str,
}

// ---------------------------------------------------------------------------
// Input validation helpers
// ---------------------------------------------------------------------------

fn validate_repo_params(owner: &str, repo: &str) -> Option<Response> {
    if let Err(e) = validation::validate_github_name(owner, "owner") {
        return Some((axum::http::StatusCode::BAD_REQUEST, e.message).into_response());
    }
    if let Err(e) = validation::validate_github_name(repo, "repo") {
        return Some((axum::http::StatusCode::BAD_REQUEST, e.message).into_response());
    }
    None
}

fn validate_policy_param(policy: Option<&str>) -> Option<Response> {
    if let Some(p) = policy
        && let Err(e) = validation::validate_policy(p)
    {
        return Some((axum::http::StatusCode::BAD_REQUEST, e.message).into_response());
    }
    None
}

// ---------------------------------------------------------------------------
// Error page
// ---------------------------------------------------------------------------

fn error_page(title: &str, message: &str) -> Response {
    ErrorTemplate {
        title: title.to_string(),
        message: message.to_string(),
    }
    .into_web_template()
    .into_response()
}

// ---------------------------------------------------------------------------
// Session helpers
// ---------------------------------------------------------------------------

fn get_session_from_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| c.trim().strip_prefix("session=").map(|s| s.to_string()))
}

fn session_cookie(session_id: &str, max_age: i64) -> String {
    format!("session={session_id}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age}")
}

// ---------------------------------------------------------------------------
// Landing page (unique layout, inline HTML)
// ---------------------------------------------------------------------------

async fn index(headers: HeaderMap, State(state): State<WebState>) -> Response {
    if let Some(session_id) = get_session_from_cookie(&headers)
        && state
            .db
            .get_user_by_session(&session_id)
            .ok()
            .flatten()
            .is_some()
    {
        return Redirect::to("/repos").into_response();
    }

    Html(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Metsuke — SDLC Process Inspector</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Shippori+Mincho:wght@400;700;800&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<style>
:root {
  --bg-deep: #0c0e1a;
  --bg-surface: #141627;
  --bg-elevated: #1c1f36;
  --border: #2a2d47;
  --text-primary: #e8e6e3;
  --text-secondary: #8a8da0;
  --accent-vermillion: #c73e3a;
  --accent-vermillion-glow: rgba(199, 62, 58, 0.15);
  --accent-gold: #c9a84c;
  --accent-indigo: #4a5fd7;
  --font-display: 'Shippori Mincho', 'Hiragino Mincho ProN', serif;
  --font-mono: 'JetBrains Mono', 'SF Mono', monospace;
}
*, *::before, *::after { margin: 0; padding: 0; box-sizing: border-box; }
body {
  font-family: var(--font-display);
  background: var(--bg-deep);
  color: var(--text-primary);
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  position: relative;
}
/* kumiko lattice SVG background */
body::before {
  content: '';
  position: fixed;
  inset: 0;
  background-image: url("data:image/svg+xml,%3Csvg width='60' height='60' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M30 0L60 30L30 60L0 30Z' fill='none' stroke='%232a2d47' stroke-width='0.5' opacity='0.4'/%3E%3Cpath d='M30 10L50 30L30 50L10 30Z' fill='none' stroke='%232a2d47' stroke-width='0.3' opacity='0.25'/%3E%3C/svg%3E");
  background-size: 60px 60px;
  z-index: 0;
}
/* radial glow */
body::after {
  content: '';
  position: fixed;
  top: 30%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 600px;
  height: 600px;
  background: radial-gradient(circle, var(--accent-vermillion-glow) 0%, transparent 70%);
  z-index: 0;
  pointer-events: none;
}
.landing {
  position: relative;
  z-index: 1;
  text-align: center;
  max-width: 480px;
  padding: 2rem;
}
.mon {
  font-size: 6rem;
  font-weight: 800;
  letter-spacing: 0.08em;
  color: var(--text-primary);
  line-height: 1;
  margin-bottom: 0.25rem;
  opacity: 0;
  animation: brushReveal 0.8s cubic-bezier(0.22, 1, 0.36, 1) forwards;
}
.logotype {
  font-family: var(--font-mono);
  font-size: 0.85rem;
  font-weight: 500;
  letter-spacing: 0.35em;
  text-transform: uppercase;
  color: var(--text-secondary);
  margin-bottom: 2.5rem;
  opacity: 0;
  animation: fadeUp 0.6s 0.3s ease-out forwards;
}
.divider {
  width: 48px;
  height: 2px;
  background: var(--accent-vermillion);
  margin: 0 auto 2rem;
  opacity: 0;
  animation: scaleX 0.5s 0.5s ease-out forwards;
  transform-origin: center;
}
.tagline {
  font-size: 1.05rem;
  color: var(--text-secondary);
  line-height: 1.7;
  margin-bottom: 3rem;
  opacity: 0;
  animation: fadeUp 0.6s 0.6s ease-out forwards;
}
.tagline strong {
  color: var(--accent-gold);
  font-weight: 700;
}
.cta {
  display: inline-flex;
  align-items: center;
  gap: 0.6rem;
  padding: 0.9rem 2rem;
  background: var(--bg-elevated);
  color: var(--text-primary);
  text-decoration: none;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-family: var(--font-mono);
  font-size: 0.9rem;
  font-weight: 500;
  letter-spacing: 0.02em;
  transition: all 0.25s ease;
  opacity: 0;
  animation: fadeUp 0.6s 0.8s ease-out forwards;
}
.cta:hover {
  border-color: var(--accent-vermillion);
  box-shadow: 0 0 24px var(--accent-vermillion-glow), inset 0 0 12px var(--accent-vermillion-glow);
  transform: translateY(-1px);
}
.cta svg {
  width: 20px;
  height: 20px;
  fill: currentColor;
}
.footer-note {
  margin-top: 3rem;
  font-family: var(--font-mono);
  font-size: 0.7rem;
  color: var(--text-secondary);
  opacity: 0;
  animation: fadeUp 0.6s 1s ease-out forwards;
  letter-spacing: 0.05em;
}
@keyframes brushReveal {
  from { opacity: 0; transform: scale(0.92); filter: blur(4px); }
  to { opacity: 1; transform: scale(1); filter: blur(0); }
}
@keyframes fadeUp {
  from { opacity: 0; transform: translateY(12px); }
  to { opacity: 1; transform: translateY(0); }
}
@keyframes scaleX {
  from { opacity: 0; transform: scaleX(0); }
  to { opacity: 1; transform: scaleX(1); }
}
</style>
</head>
<body>
<div class="landing">
  <div class="mon">目付</div>
  <div class="logotype">Metsuke</div>
  <div class="divider"></div>
  <p class="tagline">
    SDLCプロセスの<strong>遵守</strong>を監察する。<br>
    コンプライアンス検証を、コードレビューの隣に。
  </p>
  <a class="cta" href="/auth/login">
    <svg viewBox="0 0 16 16"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>
    GitHub でログイン
  </a>
  <p class="footer-note">Remote MCP Server for SDLC Compliance</p>
</div>
</body>
</html>"#,
    )
    .into_response()
}

// ---------------------------------------------------------------------------
// Auth handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AuthCallback {
    code: String,
    #[serde(default)]
    state: Option<String>,
}

async fn login(State(state): State<WebState>) -> Redirect {
    let redirect_uri = format!("{}/auth/callback", state.base_url);
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user",
        state.github_app.client_id(),
        redirect_uri
    );
    Redirect::temporary(&url)
}

async fn auth_callback(
    Query(params): Query<AuthCallback>,
    State(state): State<WebState>,
) -> Response {
    // If state parameter is present, this is an MCP OAuth 2.1 callback
    if let Some(ref oauth_state) = params.state {
        return crate::oauth::handle_oauth_callback(
            &params.code,
            oauth_state,
            &state.db,
            &state.github_app,
        )
        .await;
    }

    // Otherwise, this is the standard web login flow
    let token_resp = match state.github_app.exchange_code(&params.code).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("OAuth exchange failed: {e:#}");
            return error_page("認証に失敗しました", &format!("{e}"));
        }
    };

    let user = match GitHubApp::get_user(&token_resp.access_token).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Failed to get user: {e:#}");
            return error_page("ユーザー情報の取得に失敗しました", &format!("{e}"));
        }
    };

    let user_id = match state
        .db
        .upsert_user(user.id, &user.login, user.avatar_url.as_deref())
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("DB error: {e:#}");
            return error_page(
                "内部エラー",
                "予期しないエラーが発生しました。しばらく経ってから再度お試しください。",
            );
        }
    };

    let session_id = match state.db.create_session(user_id) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Session creation failed: {e:#}");
            return error_page(
                "内部エラー",
                "予期しないエラーが発生しました。しばらく経ってから再度お試しください。",
            );
        }
    };

    let mut resp = Redirect::to("/repos").into_response();
    resp.headers_mut().insert(
        SET_COOKIE,
        session_cookie(&session_id, 30 * 24 * 3600).parse().unwrap(),
    );
    resp
}

async fn logout(headers: HeaderMap, State(state): State<WebState>) -> Response {
    if let Some(session_id) = get_session_from_cookie(&headers) {
        let _ = state.db.delete_session(&session_id);
    }
    let mut resp = Redirect::to("/").into_response();
    resp.headers_mut()
        .insert(SET_COOKIE, session_cookie("", 0).parse().unwrap());
    resp
}

#[derive(Deserialize)]
struct InstallCallback {
    installation_id: i64,
}

async fn install_callback(
    headers: HeaderMap,
    Query(params): Query<InstallCallback>,
    State(state): State<WebState>,
) -> Response {
    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return Redirect::to("/auth/login").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return Redirect::to("/auth/login").into_response(),
    };

    let installation = match state
        .github_app
        .get_installation(params.installation_id)
        .await
    {
        Ok(i) => i,
        Err(e) => {
            tracing::error!("Failed to get installation: {e:#}");
            return error_page("インストールの検証に失敗しました", &format!("{e}"));
        }
    };

    if let Err(e) = state.db.save_installation(
        installation.id,
        user_id,
        &installation.account.login,
        &installation.account.account_type,
    ) {
        tracing::error!("Failed to save installation: {e:#}");
        return Html("Internal error".to_string()).into_response();
    }

    Redirect::to("/settings").into_response()
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

async fn settings(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return Redirect::to("/").into_response(),
    };

    let (user_id, login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return Redirect::to("/").into_response(),
    };

    let installations = state
        .db
        .get_installations_for_user(user_id)
        .unwrap_or_default();

    SettingsTemplate {
        login,
        active_page: "settings",
        installations,
        base_url: state.base_url.clone(),
    }
    .into_web_template()
    .into_response()
}

// ---------------------------------------------------------------------------
// API Endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RepoWithOwner {
    owner: String,
    name: String,
    full_name: String,
    private: bool,
    description: Option<String>,
    language: Option<String>,
    default_branch: Option<String>,
    updated_at: Option<String>,
}

async fn api_repos(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let cache_key = format!("repos:user:{user_id}");
    let st = state.clone();
    swr_respond(&state.cache, cache_key, "repos", move || async move {
        Ok(fetch_repos(&st, user_id).await)
    })
    .await
}

async fn fetch_repos(state: &WebState, user_id: i64) -> Vec<RepoWithOwner> {
    let installations = state
        .db
        .get_installations_for_user(user_id)
        .unwrap_or_default();

    let mut repos: Vec<RepoWithOwner> = Vec::new();
    for (installation_id, account_login, _account_type) in &installations {
        match state
            .github_app
            .list_installation_repos(*installation_id)
            .await
        {
            Ok(repo_list) => {
                for r in repo_list {
                    repos.push(RepoWithOwner {
                        owner: account_login.clone(),
                        name: r.name,
                        full_name: r.full_name,
                        private: r.private,
                        description: r.description,
                        language: r.language,
                        default_branch: r.default_branch,
                        updated_at: r.updated_at,
                    });
                }
            }
            Err(e) => {
                tracing::warn!("Failed to list repos for {account_login}: {e:#}");
            }
        }
    }
    repos
}

#[derive(Deserialize)]
struct VerifyQuery {
    policy: Option<String>,
}

async fn api_verify_repo(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    Query(query): Query<VerifyQuery>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }
    if let Some(r) = validate_policy_param(query.policy.as_deref()) {
        return r;
    }

    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let installation_id = match state.db.get_installation_for_owner(user_id, &owner) {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                "No installation found for this owner",
            )
                .into_response();
        }
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response();
        }
    };

    let token = match state
        .github_app
        .create_installation_token(installation_id)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response();
        }
    };

    let policy = query.policy;
    let policy_used = policy.clone();
    let owner_c = owner.clone();
    let repo_c = repo.clone();

    let result = run_blocking(move || {
        let config = libverify_github::GitHubConfig {
            token,
            repo: format!("{owner_c}/{repo_c}"),
            host: "api.github.com".into(),
        };
        let client = libverify_github::GitHubClient::new(&config)?;
        libverify_github::verify_repo(&client, &owner_c, &repo_c, "HEAD", policy.as_deref(), false)
    })
    .await;

    match result {
        Ok(r) => {
            if let Ok(json) = serde_json::to_string(&r) {
                let (pass, fail, review, na) = count_findings(&json);
                let policy_str = policy_used.as_deref().unwrap_or("default");
                let _ = state.db.append_audit_entry(
                    user_id, "repo", &owner, &repo, "HEAD", policy_str, pass, fail, review, na,
                    &json,
                );
            }
            Json(r).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

fn count_findings(json: &str) -> (i64, i64, i64, i64) {
    let (mut pass, mut fail, mut review, mut na) = (0i64, 0i64, 0i64, 0i64);
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json) {
        let findings = val
            .get("report")
            .and_then(|r| r.get("findings"))
            .and_then(|f| f.as_array())
            .or_else(|| val.get("findings").and_then(|f| f.as_array()));
        if let Some(findings) = findings {
            for f in findings {
                match f.get("status").and_then(|s| s.as_str()) {
                    Some("Satisfied" | "satisfied") => pass += 1,
                    Some("Violated" | "violated") => fail += 1,
                    Some("Indeterminate" | "indeterminate") => review += 1,
                    _ => na += 1,
                }
            }
        }
    }
    (pass, fail, review, na)
}

// ---------------------------------------------------------------------------
// Cached verification results API (reads from audit_log)
// ---------------------------------------------------------------------------

async fn api_verification_cache(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let entries = state
        .db
        .get_latest_verifications_for_user(user_id)
        .unwrap_or_default();

    let json: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "owner": e.owner,
                "repo": e.repo,
                "type": e.verification_type,
                "target_ref": e.target_ref,
                "policy": e.policy,
                "pass": e.pass_count,
                "fail": e.fail_count,
                "review": e.review_count,
                "na": e.na_count,
                "verified_at": e.verified_at,
            })
        })
        .collect();

    Json(json).into_response()
}

// ---------------------------------------------------------------------------
// PR list API
// ---------------------------------------------------------------------------

async fn api_list_pulls(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let installation_id = match state.db.get_installation_for_owner(user_id, &owner) {
        Ok(Some(id)) => id,
        Ok(None) => return Json(Vec::<serde_json::Value>::new()).into_response(),
        Err(_) => return Json(Vec::<serde_json::Value>::new()).into_response(),
    };

    let cache_key = format!("pulls:{owner}/{repo}:inst:{installation_id}");
    let app = state.github_app.clone();
    let o = owner.clone();
    let r = repo.clone();
    swr_respond(&state.cache, cache_key, "pulls", move || async move {
        app.list_pull_requests(installation_id, &o, &r).await
    })
    .await
}

// ---------------------------------------------------------------------------
// Release list API
// ---------------------------------------------------------------------------

async fn api_list_releases(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let installation_id = match state.db.get_installation_for_owner(user_id, &owner) {
        Ok(Some(id)) => id,
        Ok(None) => return Json(Vec::<serde_json::Value>::new()).into_response(),
        Err(_) => return Json(Vec::<serde_json::Value>::new()).into_response(),
    };

    let cache_key = format!("releases:{owner}/{repo}:inst:{installation_id}");
    let app = state.github_app.clone();
    let o = owner.clone();
    let r = repo.clone();
    swr_respond(&state.cache, cache_key, "releases", move || async move {
        app.list_releases(installation_id, &o, &r).await
    })
    .await
}

// ---------------------------------------------------------------------------
// Release verification API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct VerifyReleaseQuery {
    base_tag: String,
    head_tag: String,
    policy: Option<String>,
}

async fn api_verify_release(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    Query(query): Query<VerifyReleaseQuery>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }
    if let Some(r) = validate_policy_param(query.policy.as_deref()) {
        return r;
    }
    if let Err(e) = validate_git_ref(&query.base_tag) {
        return (axum::http::StatusCode::BAD_REQUEST, e.message).into_response();
    }
    if let Err(e) = validate_git_ref(&query.head_tag) {
        return (axum::http::StatusCode::BAD_REQUEST, e.message).into_response();
    }

    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let installation_id = match state.db.get_installation_for_owner(user_id, &owner) {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                "No installation found for this owner",
            )
                .into_response();
        }
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response();
        }
    };

    let token = match state
        .github_app
        .create_installation_token(installation_id)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response();
        }
    };

    let policy = query.policy;
    let policy_used = policy.clone();
    let base_tag = query.base_tag.clone();
    let head_tag = query.head_tag.clone();
    let owner_c = owner.clone();
    let repo_c = repo.clone();

    let result = run_blocking(move || {
        let config = libverify_github::GitHubConfig {
            token,
            repo: format!("{owner_c}/{repo_c}"),
            host: "api.github.com".into(),
        };
        let client = libverify_github::GitHubClient::new(&config)?;
        libverify_github::verify_release(
            &client,
            &owner_c,
            &repo_c,
            &base_tag,
            &head_tag,
            policy.as_deref(),
            false,
        )
    })
    .await;

    match result {
        Ok(r) => {
            if let Ok(json) = serde_json::to_string(&r) {
                let (pass, fail, review, na) = count_findings(&json);
                let policy_str = policy_used.as_deref().unwrap_or("default");
                let target_ref = format!("{}..{}", query.base_tag, query.head_tag);
                let _ = state.db.append_audit_entry(
                    user_id,
                    "release",
                    &owner,
                    &repo,
                    &target_ref,
                    policy_str,
                    pass,
                    fail,
                    review,
                    na,
                    &json,
                );
            }
            Json(r).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Audit history API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AuditHistoryQuery {
    #[serde(rename = "type")]
    verification_type: Option<String>,
    owner: Option<String>,
    repo: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn api_audit_history(
    headers: HeaderMap,
    Query(query): Query<AuditHistoryQuery>,
    State(state): State<WebState>,
) -> Response {
    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let entries = state
        .db
        .get_audit_history(
            user_id,
            query.verification_type.as_deref(),
            query.owner.as_deref(),
            query.repo.as_deref(),
            query.limit.unwrap_or(50),
            query.offset.unwrap_or(0),
        )
        .unwrap_or_default();

    let json: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "type": e.verification_type,
                "owner": e.owner,
                "repo": e.repo,
                "target_ref": e.target_ref,
                "policy": e.policy,
                "pass": e.pass_count,
                "fail": e.fail_count,
                "review": e.review_count,
                "na": e.na_count,
                "verified_at": e.verified_at,
            })
        })
        .collect();

    Json(json).into_response()
}

// ---------------------------------------------------------------------------
// PR verification API
// ---------------------------------------------------------------------------

async fn api_verify_pr(
    headers: HeaderMap,
    Path((owner, repo, pr_number)): Path<(String, String, u32)>,
    Query(query): Query<VerifyQuery>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }
    if let Some(r) = validate_policy_param(query.policy.as_deref()) {
        return r;
    }

    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let (user_id, _login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let installation_id = match state.db.get_installation_for_owner(user_id, &owner) {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                "No installation found for this owner",
            )
                .into_response();
        }
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response();
        }
    };

    let token = match state
        .github_app
        .create_installation_token(installation_id)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response();
        }
    };

    let policy = query.policy;
    let policy_used = policy.clone();
    let owner_c = owner.clone();
    let repo_c = repo.clone();

    let result = run_blocking(move || {
        let config = libverify_github::GitHubConfig {
            token,
            repo: format!("{owner_c}/{repo_c}"),
            host: "api.github.com".into(),
        };
        let client = libverify_github::GitHubClient::new(&config)?;
        libverify_github::verify_pr(
            &client,
            &owner_c,
            &repo_c,
            pr_number,
            policy.as_deref(),
            false,
        )
    })
    .await;

    match result {
        Ok(r) => {
            if let Ok(json) = serde_json::to_string(&r) {
                let (pass, fail, review, na) = count_findings(&json);
                let policy_str = policy_used.as_deref().unwrap_or("default");
                let target_ref = format!("#{pr_number}");
                let _ = state.db.append_audit_entry(
                    user_id,
                    "pr",
                    &owner,
                    &repo,
                    &target_ref,
                    policy_str,
                    pass,
                    fail,
                    review,
                    na,
                    &json,
                );
            }
            Json(r).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// PR verification page
// ---------------------------------------------------------------------------

async fn verify_pr_page(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return Redirect::to("/").into_response(),
    };

    let (_user_id, login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return Redirect::to("/").into_response(),
    };

    VerifyPrTemplate {
        login,
        active_page: "repos",
        owner,
        repo,
        policy_options: POLICY_OPTIONS,
    }
    .into_web_template()
    .into_response()
}

// ---------------------------------------------------------------------------
// Release verification page
// ---------------------------------------------------------------------------

async fn verify_release_page(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return Redirect::to("/").into_response(),
    };

    let (_user_id, login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return Redirect::to("/").into_response(),
    };

    VerifyReleaseTemplate {
        login,
        active_page: "repos",
        owner,
        repo,
        policy_options: POLICY_OPTIONS,
    }
    .into_web_template()
    .into_response()
}

// ---------------------------------------------------------------------------
// Audit dashboard page
// ---------------------------------------------------------------------------

async fn audit_page(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return Redirect::to("/").into_response(),
    };

    let (_user_id, login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return Redirect::to("/").into_response(),
    };

    AuditTemplate {
        login,
        active_page: "audit",
    }
    .into_web_template()
    .into_response()
}

// ---------------------------------------------------------------------------
// Repos list page
// ---------------------------------------------------------------------------

async fn repos_page(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return Redirect::to("/").into_response(),
    };

    let (_user_id, login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return Redirect::to("/").into_response(),
    };

    ReposTemplate {
        login,
        active_page: "repos",
        policy_options: POLICY_OPTIONS,
    }
    .into_web_template()
    .into_response()
}

// ---------------------------------------------------------------------------
// Repo detail page
// ---------------------------------------------------------------------------

async fn repo_detail_page(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let session_id = match get_session_from_cookie(&headers) {
        Some(s) => s,
        None => return Redirect::to("/").into_response(),
    };

    let (_user_id, login) = match state.db.get_user_by_session(&session_id) {
        Ok(Some(u)) => u,
        _ => return Redirect::to("/").into_response(),
    };

    let _ = &state; // keep state alive for future use

    RepoDetailTemplate {
        login,
        active_page: "repos",
        owner,
        repo,
        policy_options: POLICY_OPTIONS,
    }
    .into_web_template()
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_template_renders() {
        let t = ErrorTemplate {
            title: "テスト".into(),
            message: "エラーメッセージ".into(),
        };
        let html = t.render().unwrap();
        assert!(html.contains("テスト"));
        assert!(html.contains("エラーメッセージ"));
        assert!(html.contains("style.css"));
    }

    #[test]
    fn settings_template_renders() {
        let t = SettingsTemplate {
            login: "testuser".into(),
            active_page: "settings",
            installations: vec![(1, "myorg".into(), "Organization".into())],
            base_url: "https://example.com".into(),
        };
        let html = t.render().unwrap();
        assert!(html.contains("testuser"));
        assert!(html.contains("myorg"));
        assert!(html.contains("Organization"));
        assert!(html.contains("https://example.com/mcp"));
    }

    #[test]
    fn repos_template_renders() {
        let t = ReposTemplate {
            login: "testuser".into(),
            active_page: "repos",
            policy_options: POLICY_OPTIONS,
        };
        let html = t.render().unwrap();
        assert!(html.contains("Repositories"));
        assert!(html.contains("testuser"));
        assert!(html.contains("slsa-l4"));
    }

    #[test]
    fn repo_detail_template_renders() {
        let t = RepoDetailTemplate {
            login: "testuser".into(),
            active_page: "repos",
            owner: "myorg".into(),
            repo: "myrepo".into(),
            policy_options: POLICY_OPTIONS,
        };
        let html = t.render().unwrap();
        assert!(html.contains("myorg"));
        assert!(html.contains("myrepo"));
        assert!(html.contains("リリース検証"));
        assert!(html.contains("PR検証"));
    }

    #[test]
    fn verify_pr_template_renders() {
        let t = VerifyPrTemplate {
            login: "testuser".into(),
            active_page: "repos",
            owner: "myorg".into(),
            repo: "myrepo".into(),
            policy_options: POLICY_OPTIONS,
        };
        let html = t.render().unwrap();
        assert!(html.contains("PR検証"));
        assert!(html.contains("pr-number"));
        assert!(html.contains("Open Pull Requests"));
    }

    #[test]
    fn verify_release_template_renders() {
        let t = VerifyReleaseTemplate {
            login: "testuser".into(),
            active_page: "repos",
            owner: "myorg".into(),
            repo: "myrepo".into(),
            policy_options: POLICY_OPTIONS,
        };
        let html = t.render().unwrap();
        assert!(html.contains("Release 検証"));
        assert!(html.contains("base-tag"));
        assert!(html.contains("head-tag"));
        assert!(html.contains("Releases"));
    }

    #[test]
    fn audit_template_renders() {
        let t = AuditTemplate {
            login: "testuser".into(),
            active_page: "audit",
        };
        let html = t.render().unwrap();
        assert!(html.contains("Audit Log"));
        assert!(html.contains("filter-type"));
        assert!(html.contains(r#"nav-link active" href="/audit"#));
    }

    #[test]
    fn nav_highlights_active_page() {
        let settings = SettingsTemplate {
            login: "u".into(),
            active_page: "settings",
            installations: vec![],
            base_url: "https://x.com".into(),
        };
        let html = settings.render().unwrap();
        assert!(html.contains(r#"nav-link active" href="/settings"#));

        let repos = ReposTemplate {
            login: "u".into(),
            active_page: "repos",
            policy_options: POLICY_OPTIONS,
        };
        let html = repos.render().unwrap();
        assert!(html.contains(r#"nav-link active" href="/repos"#));

        let audit = AuditTemplate {
            login: "u".into(),
            active_page: "audit",
        };
        let html = audit.render().unwrap();
        assert!(html.contains(r#"nav-link active" href="/audit"#));
    }

    #[test]
    fn policy_options_constant() {
        assert!(POLICY_OPTIONS.contains(&"default"));
        assert!(POLICY_OPTIONS.contains(&"slsa-l4"));
        assert_eq!(POLICY_OPTIONS.len(), 9);
    }

    #[test]
    fn session_cookie_format() {
        let cookie = session_cookie("abc123", 3600);
        assert!(cookie.contains("session=abc123"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Lax"));
    }
}
