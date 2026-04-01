use askama_web::WebTemplateExt;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::http::header::SET_COOKIE;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::blocking::run_blocking;

use super::WebState;
use super::helpers::*;
use super::jobs::spawn_sync_repos_job;
use super::templates::SettingsTemplate;

// ---------------------------------------------------------------------------
// Landing page (unique layout, inline HTML)
// ---------------------------------------------------------------------------

pub(super) async fn index(headers: HeaderMap, State(state): State<WebState>) -> Response {
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
pub(super) struct AuthCallback {
    code: String,
    #[serde(default)]
    state: Option<String>,
}

pub(super) async fn login(State(state): State<WebState>) -> Response {
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

pub(super) async fn auth_callback(
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

pub(super) async fn logout(headers: HeaderMap, State(state): State<WebState>) -> Response {
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
pub(super) struct InstallCallback {
    installation_id: i64,
}

pub(super) async fn install_callback(
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

pub(super) async fn settings(headers: HeaderMap, State(state): State<WebState>) -> Response {
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
