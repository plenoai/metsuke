use std::sync::Arc;

use askama::Template;
use askama_web::WebTemplateExt;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::http::header::SET_COOKIE;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Json, Redirect, Response};
use serde::{Deserialize, Serialize};

use crate::blocking::run_blocking;
use crate::config::AppConfig;
use crate::db::{CachedPullRow, CachedReleaseRow, Database, RepoRow};
use crate::github_app::GitHubApp;
use crate::validation::{self, validate_git_ref};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const POLICY_OPTIONS: &[&str] = &[
    "default", "oss", "aiops", "soc1", "soc2", "slsa-l1", "slsa-l2", "slsa-l3", "slsa-l4",
];

// ---------------------------------------------------------------------------
// Job events (broadcast to SSE clients)
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(tag = "type")]
pub enum JobEvent {
    #[serde(rename = "repos_synced")]
    ReposSynced { user_id: i64 },
    #[serde(rename = "pulls_synced")]
    PullsSynced {
        user_id: i64,
        owner: String,
        repo: String,
    },
    #[serde(rename = "releases_synced")]
    ReleasesSynced {
        user_id: i64,
        owner: String,
        repo: String,
    },
    #[serde(rename = "verification_complete")]
    VerificationComplete {
        user_id: i64,
        owner: String,
        repo: String,
    },
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct WebState {
    db: Arc<Database>,
    github_app: Arc<GitHubApp>,
    base_url: String,
    events_tx: tokio::sync::broadcast::Sender<JobEvent>,
    github_web_base_url: String,
    github_api_host: String,
}

pub fn router(db: Arc<Database>, github_app: Arc<GitHubApp>, config: &AppConfig) -> Router {
    let (events_tx, _) = tokio::sync::broadcast::channel::<JobEvent>(256);
    let state = WebState {
        db,
        github_app: github_app.clone(),
        base_url: config.base_url.clone(),
        events_tx,
        github_web_base_url: github_app.web_base_url().to_string(),
        github_api_host: config.github_api_host.clone(),
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
            axum::routing::get(api_get_latest_verification).post(api_verify_repo),
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
            "/api/repos/{owner}/{repo}/verify-release/latest",
            axum::routing::get(api_get_latest_release_verifications),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-release/latest/{target_ref}",
            axum::routing::get(api_get_latest_release_verification_by_ref),
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
            "/api/repos/{owner}/{repo}/verify-pr/{pr_number}/latest",
            axum::routing::get(api_get_latest_pr_verification),
        )
        .route(
            "/api/repos/{owner}/{repo}/readme",
            axum::routing::get(api_readme),
        )
        .route("/api/events", axum::routing::get(api_events))
        .route("/api/audit-history", axum::routing::get(api_audit_history))
        .route(
            "/api/audit-history/export",
            axum::routing::get(api_audit_export_csv),
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

/// Consolidates session-cookie extraction + DB user lookup behind `run_blocking`.
async fn require_user(db: &Arc<Database>, headers: &HeaderMap) -> Option<(i64, String)> {
    let session_id = get_session_from_cookie(headers)?;
    let db = db.clone();
    run_blocking(move || db.get_user_by_session(&session_id))
        .await
        .ok()
        .flatten()
}

// ---------------------------------------------------------------------------
// Landing page (unique layout, inline HTML)
// ---------------------------------------------------------------------------

async fn index(headers: HeaderMap, State(state): State<WebState>) -> Response {
    if require_user(&state.db, &headers).await.is_some() {
        return Redirect::to("/repos").into_response();
    }

    Html(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="description" content="Metsuke — SDLC準拠検証ツール。GitHub連携でリポジトリのセキュリティ・コンプライアンスを自動検証。">
<meta property="og:title" content="Metsuke — SDLC Process Inspector">
<meta property="og:description" content="GitHubリポジトリのSDLC準拠を自動検証。SLSA、SOC2、セキュリティコントロールに対応。">
<meta property="og:type" content="website">
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

async fn login(State(state): State<WebState>) -> Response {
    let redirect_uri = format!("{}/auth/callback", state.base_url);
    let csrf_state = uuid::Uuid::new_v4().to_string();
    let url = format!(
        "{}/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user&state=web:{}",
        state.github_web_base_url,
        state.github_app.client_id(),
        redirect_uri,
        csrf_state,
    );
    let mut resp = Redirect::temporary(&url).into_response();
    resp.headers_mut().insert(
        SET_COOKIE,
        format!("csrf_state={csrf_state}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=600")
            .parse()
            .unwrap(),
    );
    resp
}

async fn auth_callback(
    headers: HeaderMap,
    Query(params): Query<AuthCallback>,
    State(state): State<WebState>,
) -> Response {
    // Dispatch based on state parameter format:
    //   - "web:<csrf>" => web login flow with CSRF verification
    //   - other non-empty state => MCP OAuth 2.1 callback
    //   - None => legacy web login (should not happen after CSRF addition)
    if let Some(ref oauth_state) = params.state {
        if let Some(csrf_token) = oauth_state.strip_prefix("web:") {
            // Web login flow — verify CSRF cookie matches
            let cookie_csrf = headers
                .get(axum::http::header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|cookies| {
                    cookies
                        .split(';')
                        .find_map(|c| c.trim().strip_prefix("csrf_state=").map(|s| s.to_string()))
                });
            match cookie_csrf {
                Some(ref expected) if expected == csrf_token => { /* CSRF valid */ }
                _ => {
                    return error_page(
                        "認証エラー",
                        "CSRF検証に失敗しました。もう一度ログインしてください。",
                    );
                }
            }
            // Fall through to web login flow below; clear csrf_state cookie
        } else {
            // MCP OAuth 2.1 callback
            return crate::oauth::handle_oauth_callback(
                &params.code,
                oauth_state,
                &state.db,
                &state.github_app,
            )
            .await;
        }
    }

    // Standard web login flow
    let token_resp = match state.github_app.exchange_code(&params.code).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("OAuth exchange failed: {e:#}");
            return error_page("認証に失敗しました", &format!("{e}"));
        }
    };

    let user = match state.github_app.get_user(&token_resp.access_token).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Failed to get user: {e:#}");
            return error_page("ユーザー情報の取得に失敗しました", &format!("{e}"));
        }
    };

    let db = state.db.clone();
    let login = user.login.clone();
    let avatar = user.avatar_url.clone();
    let access_token = token_resp.access_token.clone();
    let uid = user.id;
    let (user_id, session_id) = match run_blocking(move || {
        db.upsert_user_and_create_session(uid, &login, avatar.as_deref(), Some(&access_token))
    })
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("DB error: {e:#}");
            return error_page(
                "内部エラー",
                "予期しないエラーが発生しました。しばらく経ってから再度お試しください。",
            );
        }
    };

    // Proactively sync repos in background so data is ready when user lands on /repos
    spawn_sync_repos_job(&state, user_id);

    let mut resp = Redirect::to("/repos").into_response();
    resp.headers_mut().append(
        SET_COOKIE,
        session_cookie(&session_id, 30 * 24 * 3600).parse().unwrap(),
    );
    // Clear the CSRF cookie now that login is complete
    resp.headers_mut().append(
        SET_COOKIE,
        "csrf_state=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0"
            .parse()
            .unwrap(),
    );
    resp
}

async fn logout(headers: HeaderMap, State(state): State<WebState>) -> Response {
    if let Some(session_id) = get_session_from_cookie(&headers) {
        let db = state.db.clone();
        let _ = run_blocking(move || db.delete_session(&session_id)).await;
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
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return Redirect::to("/auth/login").into_response(),
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

    let db = state.db.clone();
    let account_login = installation.account.login.clone();
    let account_type = installation.account.account_type.clone();
    let inst_id = installation.id;
    if let Err(e) =
        run_blocking(move || db.save_installation(inst_id, user_id, &account_login, &account_type))
            .await
    {
        tracing::error!("Failed to save installation: {e:#}");
        return Html("Internal error".to_string()).into_response();
    }

    // Proactively sync repos for the new installation
    spawn_sync_repos_job(&state, user_id);

    Redirect::to("/settings").into_response()
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

async fn settings(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let (user_id, login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return Redirect::to("/").into_response(),
    };

    let db = state.db.clone();
    let installations = run_blocking(move || db.get_installations_for_user(user_id))
        .await
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

async fn api_repos(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let (repos, stale) = run_blocking(move || db.get_repos_with_staleness(user_id))
        .await
        .unwrap_or_default();

    if repos.is_empty() || stale {
        spawn_sync_repos_job(&state, user_id);
    }

    Json(repos).into_response()
}

async fn sync_installations(state: &WebState, user_id: i64) {
    let db = state.db.clone();
    let token = match run_blocking(move || db.get_github_token(user_id)).await {
        Ok(Some(t)) => t,
        _ => return,
    };
    let user_installations = match state.github_app.list_user_installations(&token).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to list user installations: {e:#}");
            return;
        }
    };
    let installs: Vec<_> = user_installations
        .into_iter()
        .map(|i| (i.id, i.account.login, i.account.account_type))
        .collect();
    let db = state.db.clone();
    if let Err(e) = run_blocking(move || db.batch_save_installations(user_id, &installs)).await {
        tracing::warn!("Failed to sync installations: {e:#}");
    }
}

// ---------------------------------------------------------------------------
// SSE events endpoint
// ---------------------------------------------------------------------------

async fn api_events(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let mut rx = state.events_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let event_user_id = match &event {
                        JobEvent::ReposSynced { user_id, .. } => *user_id,
                        JobEvent::PullsSynced { user_id, .. } => *user_id,
                        JobEvent::ReleasesSynced { user_id, .. } => *user_id,
                        JobEvent::VerificationComplete { user_id, .. } => *user_id,
                    };
                    if event_user_id == user_id {
                        let data = serde_json::to_string(&event).unwrap_or_default();
                        yield Ok::<_, std::convert::Infallible>(
                            Event::default().event("job").data(data)
                        );
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

// ---------------------------------------------------------------------------
// Background sync jobs
// ---------------------------------------------------------------------------

fn spawn_sync_repos_job(state: &WebState, user_id: i64) {
    let state = state.clone();
    tokio::spawn(async move {
        sync_installations(&state, user_id).await;

        let db = state.db.clone();
        let installations = run_blocking(move || db.get_installations_for_user(user_id))
            .await
            .unwrap_or_default();

        let mut join_set = tokio::task::JoinSet::new();
        for (installation_id, account_login, _account_type) in installations {
            let github_app = state.github_app.clone();
            join_set.spawn(async move {
                match github_app.list_installation_repos(installation_id).await {
                    Ok(repos) => repos
                        .into_iter()
                        .map(|r| RepoRow {
                            owner: account_login.clone(),
                            name: r.name,
                            full_name: r.full_name,
                            private: r.private,
                            description: r.description,
                            language: r.language,
                            default_branch: r.default_branch,
                            pushed_at: r.pushed_at,
                            synced_at: String::new(), // set by DB
                        })
                        .collect::<Vec<_>>(),
                    Err(e) => {
                        tracing::warn!("Failed to list repos for {account_login}: {e:#}");
                        Vec::new()
                    }
                }
            });
        }

        let mut all_repos = Vec::new();
        while let Some(result) = join_set.join_next().await {
            if let Ok(batch) = result {
                all_repos.extend(batch);
            }
        }

        let db = state.db.clone();
        if let Err(e) = run_blocking(move || db.upsert_repositories(user_id, &all_repos)).await {
            tracing::warn!("Failed to save repos to DB: {e:#}");
            return;
        }

        let _ = state.events_tx.send(JobEvent::ReposSynced { user_id });
    });
}

fn spawn_sync_pulls_job(state: &WebState, user_id: i64, owner: String, repo: String) {
    let state = state.clone();
    tokio::spawn(async move {
        let db = state.db.clone();
        let owner_q = owner.clone();
        let installation_id =
            match run_blocking(move || db.get_installation_for_owner(user_id, &owner_q)).await {
                Ok(Some(id)) => id,
                _ => return,
            };

        let pulls = match state
            .github_app
            .list_pull_requests(installation_id, &owner, &repo)
            .await
        {
            Ok(prs) => prs
                .into_iter()
                .map(|p| CachedPullRow {
                    pr_number: p.number as i64,
                    title: p.title,
                    state: p.state,
                    author: p.user.login,
                    created_at: p.created_at,
                    updated_at: p.updated_at,
                    merged_at: p.merged_at,
                    draft: p.draft.unwrap_or(false),
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!("Failed to list pulls for {owner}/{repo}: {e:#}");
                return;
            }
        };

        let db = state.db.clone();
        let o = owner.clone();
        let r = repo.clone();
        if let Err(e) = run_blocking(move || db.upsert_cached_pulls(user_id, &o, &r, &pulls)).await
        {
            tracing::warn!("Failed to save pulls to DB: {e:#}");
            return;
        }

        let _ = state.events_tx.send(JobEvent::PullsSynced {
            user_id,
            owner,
            repo,
        });
    });
}

fn spawn_sync_releases_job(state: &WebState, user_id: i64, owner: String, repo: String) {
    let state = state.clone();
    tokio::spawn(async move {
        let db = state.db.clone();
        let owner_q = owner.clone();
        let installation_id =
            match run_blocking(move || db.get_installation_for_owner(user_id, &owner_q)).await {
                Ok(Some(id)) => id,
                _ => return,
            };

        let releases = match state
            .github_app
            .list_releases(installation_id, &owner, &repo)
            .await
        {
            Ok(rels) => rels
                .into_iter()
                .map(|r| CachedReleaseRow {
                    release_id: r.id,
                    tag_name: r.tag_name,
                    name: r.name,
                    draft: r.draft,
                    prerelease: r.prerelease,
                    created_at: r.created_at,
                    published_at: r.published_at,
                    author: r.author.login,
                    html_url: r.html_url,
                    body: r.body,
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!("Failed to list releases for {owner}/{repo}: {e:#}");
                return;
            }
        };

        let db = state.db.clone();
        let o = owner.clone();
        let r = repo.clone();
        if let Err(e) =
            run_blocking(move || db.upsert_cached_releases(user_id, &o, &r, &releases)).await
        {
            tracing::warn!("Failed to save releases to DB: {e:#}");
            return;
        }

        let _ = state.events_tx.send(JobEvent::ReleasesSynced {
            user_id,
            owner,
            repo,
        });
    });
}

#[derive(Deserialize)]
struct VerifyQuery {
    policy: Option<String>,
}

async fn api_get_latest_verification(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let owner_c = owner.clone();
    let repo_c = repo.clone();
    match run_blocking(move || db.get_latest_repo_verification(user_id, &owner_c, &repo_c)).await {
        Ok(Some(json)) => (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            json,
        )
            .into_response(),
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "No verification found").into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
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

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let owner_q = owner.clone();
    let installation_id =
        match run_blocking(move || db.get_installation_for_owner(user_id, &owner_q)).await {
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
            host: state.github_api_host.clone(),
        };
        let client = libverify_github::GitHubClient::new(&config)?;
        libverify_github::verify_repo(&client, &owner_c, &repo_c, "HEAD", policy.as_deref(), false)
    })
    .await;

    match result {
        Ok(r) => {
            if let Ok(json) = serde_json::to_string(&r) {
                let (pass, fail, review, na) = count_findings(&json);
                let policy_str = policy_used.unwrap_or_else(|| "default".to_string());
                let db = state.db.clone();
                let owner_a = owner.clone();
                let repo_a = repo.clone();
                let _ = run_blocking(move || {
                    db.append_audit_entry(
                        user_id,
                        "repo",
                        &owner_a,
                        &repo_a,
                        "HEAD",
                        &policy_str,
                        pass,
                        fail,
                        review,
                        na,
                        &json,
                    )
                })
                .await;
                let _ = state.events_tx.send(JobEvent::VerificationComplete {
                    user_id,
                    owner: owner.clone(),
                    repo: repo.clone(),
                });
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

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let o = owner.clone();
    let r = repo.clone();
    let (pulls, stale) = run_blocking(move || db.get_pulls_with_staleness(user_id, &o, &r))
        .await
        .unwrap_or_default();

    if pulls.is_empty() || stale {
        spawn_sync_pulls_job(&state, user_id, owner, repo);
    }

    Json(pulls).into_response()
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

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let o = owner.clone();
    let r = repo.clone();
    let (releases, stale) = run_blocking(move || db.get_releases_with_staleness(user_id, &o, &r))
        .await
        .unwrap_or_default();

    if releases.is_empty() || stale {
        spawn_sync_releases_job(&state, user_id, owner, repo);
    }

    Json(releases).into_response()
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

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let owner_q = owner.clone();
    let installation_id =
        match run_blocking(move || db.get_installation_for_owner(user_id, &owner_q)).await {
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
            host: state.github_api_host.clone(),
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
                let policy_str = policy_used.unwrap_or_else(|| "default".to_string());
                let target_ref = format!("{}..{}", query.base_tag, query.head_tag);
                let db = state.db.clone();
                let owner_a = owner.clone();
                let repo_a = repo.clone();
                let _ = run_blocking(move || {
                    db.append_audit_entry(
                        user_id,
                        "release",
                        &owner_a,
                        &repo_a,
                        &target_ref,
                        &policy_str,
                        pass,
                        fail,
                        review,
                        na,
                        &json,
                    )
                })
                .await;
                let _ = state.events_tx.send(JobEvent::VerificationComplete {
                    user_id,
                    owner: owner.clone(),
                    repo: repo.clone(),
                });
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
    from_date: Option<String>,
    to_date: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn api_audit_history(
    headers: HeaderMap,
    Query(query): Query<AuditHistoryQuery>,
    State(state): State<WebState>,
) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let vtype = query.verification_type.clone();
    let qowner = query.owner.clone();
    let qrepo = query.repo.clone();
    let from = query.from_date.clone();
    let to = query.to_date.clone();
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    let entries = run_blocking(move || {
        db.get_audit_history(
            user_id,
            vtype.as_deref(),
            qowner.as_deref(),
            qrepo.as_deref(),
            from.as_deref(),
            to.as_deref(),
            limit,
            offset,
        )
    })
    .await
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

async fn api_audit_export_csv(
    headers: HeaderMap,
    Query(query): Query<AuditHistoryQuery>,
    State(state): State<WebState>,
) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let vtype = query.verification_type.clone();
    let qowner = query.owner.clone();
    let qrepo = query.repo.clone();
    let from = query.from_date.clone();
    let to = query.to_date.clone();
    let entries = run_blocking(move || {
        db.get_audit_history(
            user_id,
            vtype.as_deref(),
            qowner.as_deref(),
            qrepo.as_deref(),
            from.as_deref(),
            to.as_deref(),
            10000,
            0,
        )
    })
    .await
    .unwrap_or_default();

    let mut csv = String::from("Date,Type,Owner,Repo,Target,Policy,Pass,Fail,Review,N/A\n");
    for e in &entries {
        csv.push_str(&format!(
            "\"{}\",\"{}\",\"{}\",\"{}\",\"{}\",\"{}\",{},{},{},{}\n",
            e.verified_at,
            e.verification_type,
            e.owner,
            e.repo,
            e.target_ref,
            e.policy,
            e.pass_count,
            e.fail_count,
            e.review_count,
            e.na_count,
        ));
    }

    (
        [
            (axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"metsuke-audit.csv\"",
            ),
        ],
        csv,
    )
        .into_response()
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

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let owner_q = owner.clone();
    let installation_id =
        match run_blocking(move || db.get_installation_for_owner(user_id, &owner_q)).await {
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
            host: state.github_api_host.clone(),
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
                let policy_str = policy_used.unwrap_or_else(|| "default".to_string());
                let target_ref = format!("#{pr_number}");
                let db = state.db.clone();
                let owner_a = owner.clone();
                let repo_a = repo.clone();
                let _ = run_blocking(move || {
                    db.append_audit_entry(
                        user_id,
                        "pr",
                        &owner_a,
                        &repo_a,
                        &target_ref,
                        &policy_str,
                        pass,
                        fail,
                        review,
                        na,
                        &json,
                    )
                })
                .await;
                let _ = state.events_tx.send(JobEvent::VerificationComplete {
                    user_id,
                    owner: owner.clone(),
                    repo: repo.clone(),
                });
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

async fn api_get_latest_pr_verification(
    headers: HeaderMap,
    Path((owner, repo, pr_number)): Path<(String, String, u32)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let target_ref = format!("#{pr_number}");
    match run_blocking(move || {
        db.get_latest_verification_by_ref(user_id, &owner, &repo, &target_ref)
    })
    .await
    {
        Ok(Some(json)) => (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            json,
        )
            .into_response(),
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "No verification found").into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

async fn api_get_latest_release_verification_by_ref(
    headers: HeaderMap,
    Path((owner, repo, target_ref)): Path<(String, String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    match run_blocking(move || {
        db.get_latest_verification_by_ref(user_id, &owner, &repo, &target_ref)
    })
    .await
    {
        Ok(Some(json)) => (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            json,
        )
            .into_response(),
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "No verification found").into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

async fn api_get_latest_release_verifications(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    match run_blocking(move || {
        db.get_latest_verifications_by_type(user_id, "release", &owner, &repo)
    })
    .await
    {
        Ok(rows) => {
            let json: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(target_ref, pass, fail, review, na, _result_json)| {
                    serde_json::json!({
                        "target_ref": target_ref,
                        "pass": pass,
                        "fail": fail,
                        "review": review,
                        "na": na,
                    })
                })
                .collect();
            Json(json).into_response()
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

    let (_user_id, login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return Redirect::to("/").into_response(),
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

    let (_user_id, login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return Redirect::to("/").into_response(),
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
    let (_user_id, login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return Redirect::to("/").into_response(),
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
    let (_user_id, login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return Redirect::to("/").into_response(),
    };

    ReposTemplate {
        login,
        active_page: "repos",
    }
    .into_web_template()
    .into_response()
}

// ---------------------------------------------------------------------------
// README API
// ---------------------------------------------------------------------------

async fn api_readme(
    headers: HeaderMap,
    Path((owner, repo)): Path<(String, String)>,
    State(state): State<WebState>,
) -> Response {
    if let Some(r) = validate_repo_params(&owner, &repo) {
        return r;
    }

    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let token = run_blocking(move || db.get_github_token(user_id))
        .await
        .ok()
        .flatten();

    let url = format!(
        "https://{}/repos/{owner}/{repo}/readme",
        state.github_api_host
    );
    let client = reqwest::Client::new();
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github.html+json")
        .header("User-Agent", "metsuke")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = req.send().await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let html = r.text().await.unwrap_or_default();
            (
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                html,
            )
                .into_response()
        }
        Ok(r) if r.status().as_u16() == 404 => {
            (axum::http::StatusCode::NOT_FOUND, "README not found").into_response()
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            (
                axum::http::StatusCode::from_u16(status.as_u16())
                    .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
                body,
            )
                .into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
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

    let (_user_id, login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return Redirect::to("/").into_response(),
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
        };
        let html = t.render().unwrap();
        assert!(html.contains("Repositories"));
        assert!(html.contains("testuser"));
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
        assert!(html.contains("pr-policy"));
        assert!(html.contains("Pull Request"));
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
        assert!(html.contains("リリース一覧"));
    }

    #[test]
    fn audit_template_renders() {
        let t = AuditTemplate {
            login: "testuser".into(),
            active_page: "audit",
        };
        let html = t.render().unwrap();
        assert!(html.contains("監査ログ"));
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
