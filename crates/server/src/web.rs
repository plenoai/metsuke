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
}

pub fn router(db: Arc<Database>, github_app: Arc<GitHubApp>, config: &AppConfig) -> Router {
    let state = WebState {
        db,
        github_app,
        base_url: config.base_url.clone(),
    };

    Router::new()
        .route("/", axum::routing::get(index))
        .route("/dashboard", axum::routing::get(dashboard))
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
            "/repos/{owner}/{repo}/pulls",
            axum::routing::get(verify_pr_page),
        )
        .route("/api/repos", axum::routing::get(api_repos))
        .route(
            "/api/repos/{owner}/{repo}/verify",
            axum::routing::post(api_verify_repo),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-pr/{pr_number}",
            axum::routing::post(api_verify_pr),
        )
        .route(
            "/api/verification-cache",
            axum::routing::get(api_verification_cache),
        )
        .with_state(state)
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
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    login: String,
    active_page: &'static str,
    installations: Vec<(i64, String, String)>,
    install_count: usize,
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
        return Redirect::to("/dashboard").into_response();
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

    let mut resp = Redirect::to("/dashboard").into_response();
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

    Redirect::to("/dashboard").into_response()
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

async fn dashboard(headers: HeaderMap, State(state): State<WebState>) -> Response {
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

    let install_count = installations.len();

    DashboardTemplate {
        login,
        active_page: "dashboard",
        installations,
        install_count,
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

    Json(repos).into_response()
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
            // Cache the result
            if let Ok(json) = serde_json::to_string(&r) {
                let mut pass = 0i64;
                let mut fail = 0i64;
                let mut review = 0i64;
                let mut na = 0i64;
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json)
                    && let Some(findings) = val
                        .get("report")
                        .and_then(|r| r.get("findings"))
                        .and_then(|f| f.as_array())
                {
                    for f in findings {
                        match f.get("status").and_then(|s| s.as_str()) {
                            Some("Satisfied") => pass += 1,
                            Some("Violated") => fail += 1,
                            Some("Indeterminate") => review += 1,
                            _ => na += 1,
                        }
                    }
                }
                let policy_str = policy_used.as_deref().unwrap_or("default");
                let _ = state.db.save_verification_result(
                    user_id, &owner, &repo, policy_str, pass, fail, review, na, &json,
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
// Cached verification results API
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
        .get_verification_results_for_user(user_id)
        .unwrap_or_default();

    let json: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "owner": e.owner,
                "repo": e.repo,
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
        Ok(r) => Json(r).into_response(),
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
