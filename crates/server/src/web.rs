use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::http::header::SET_COOKIE;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::config::AppConfig;
use crate::db::Database;
use crate::github_app::GitHubApp;

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
        .with_state(state)
}

fn error_page(title: &str, message: &str) -> Response {
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Error — Metsuke</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Shippori+Mincho:wght@400;700;800&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<style>
:root {{
  --bg-deep: #0c0e1a;
  --bg-surface: #141627;
  --border: #2a2d47;
  --text-primary: #e8e6e3;
  --text-secondary: #8a8da0;
  --accent-vermillion: #c73e3a;
  --font-display: 'Shippori Mincho', 'Hiragino Mincho ProN', serif;
  --font-mono: 'JetBrains Mono', 'SF Mono', monospace;
}}
*, *::before, *::after {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  font-family: var(--font-display);
  background: var(--bg-deep);
  color: var(--text-primary);
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
}}
.error-card {{
  text-align: center;
  max-width: 440px;
  padding: 2.5rem;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 10px;
}}
.error-mark {{
  font-size: 2.5rem;
  margin-bottom: 0.5rem;
  color: var(--accent-vermillion);
}}
.error-title {{
  font-size: 1.2rem;
  font-weight: 700;
  margin-bottom: 0.75rem;
}}
.error-msg {{
  font-family: var(--font-mono);
  font-size: 0.8rem;
  color: var(--text-secondary);
  line-height: 1.6;
  margin-bottom: 1.5rem;
  word-break: break-all;
}}
.back-link {{
  font-family: var(--font-mono);
  font-size: 0.8rem;
  color: var(--text-secondary);
  text-decoration: none;
  padding: 0.5rem 1rem;
  border: 1px solid var(--border);
  border-radius: 6px;
  transition: all 0.2s ease;
}}
.back-link:hover {{
  color: var(--text-primary);
  border-color: var(--accent-vermillion);
}}
</style>
</head>
<body>
<div class="error-card">
  <div class="error-mark">障</div>
  <div class="error-title">{title}</div>
  <p class="error-msg">{message}</p>
  <a class="back-link" href="/">トップに戻る</a>
</div>
</body>
</html>"#,
    ))
    .into_response()
}

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

    let install_list = if installations.is_empty() {
        "<p class=\"hint\">インストールされたアカウントはありません。GitHub Appをインストールしてください。</p>".to_string()
    } else {
        let items: Vec<String> = installations
            .iter()
            .map(|(id, login, typ)| {
                let tag_class = if typ == "Organization" { "tag tag-org" } else { "tag" };
                format!(
                    r#"<div class="install-item"><span class="install-name">{login}</span><div class="install-meta"><span class="{tag_class}">{typ}</span><span class="install-id">#{id}</span></div></div>"#
                )
            })
            .collect();
        items.join("")
    };

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Dashboard — Metsuke</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Shippori+Mincho:wght@400;700;800&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<style>
:root {{
  --bg-deep: #0c0e1a;
  --bg-surface: #141627;
  --bg-elevated: #1c1f36;
  --border: #2a2d47;
  --border-subtle: #1f2139;
  --text-primary: #e8e6e3;
  --text-secondary: #8a8da0;
  --accent-vermillion: #c73e3a;
  --accent-vermillion-glow: rgba(199, 62, 58, 0.12);
  --accent-gold: #c9a84c;
  --accent-gold-dim: rgba(201, 168, 76, 0.1);
  --accent-indigo: #4a5fd7;
  --font-display: 'Shippori Mincho', 'Hiragino Mincho ProN', serif;
  --font-mono: 'JetBrains Mono', 'SF Mono', monospace;
}}
*, *::before, *::after {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  font-family: var(--font-display);
  background: var(--bg-deep);
  color: var(--text-primary);
  min-height: 100vh;
  position: relative;
}}
body::before {{
  content: '';
  position: fixed;
  inset: 0;
  background-image: url("data:image/svg+xml,%3Csvg width='60' height='60' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M30 0L60 30L30 60L0 30Z' fill='none' stroke='%232a2d47' stroke-width='0.5' opacity='0.3'/%3E%3C/svg%3E");
  background-size: 60px 60px;
  z-index: 0;
  pointer-events: none;
}}
.shell {{
  position: relative;
  z-index: 1;
  max-width: 760px;
  margin: 0 auto;
  padding: 2.5rem 1.5rem 4rem;
}}

/* header */
.header {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 2.5rem;
  padding-bottom: 1.5rem;
  border-bottom: 1px solid var(--border-subtle);
}}
.header-left {{
  display: flex;
  align-items: center;
  gap: 1rem;
}}
.brand {{
  font-size: 1.6rem;
  font-weight: 800;
  letter-spacing: 0.05em;
}}
.brand-sub {{
  font-family: var(--font-mono);
  font-size: 0.65rem;
  letter-spacing: 0.3em;
  text-transform: uppercase;
  color: var(--text-secondary);
}}
.user-badge {{
  display: flex;
  align-items: center;
  gap: 0.6rem;
  font-family: var(--font-mono);
  font-size: 0.85rem;
  color: var(--text-secondary);
}}
.user-badge strong {{
  color: var(--text-primary);
}}
.logout-link {{
  color: var(--text-secondary);
  text-decoration: none;
  font-family: var(--font-mono);
  font-size: 0.75rem;
  letter-spacing: 0.05em;
  padding: 0.35rem 0.7rem;
  border: 1px solid var(--border);
  border-radius: 4px;
  transition: all 0.2s ease;
}}
.logout-link:hover {{
  color: var(--accent-vermillion);
  border-color: var(--accent-vermillion);
}}

/* section */
.section {{
  margin-bottom: 2rem;
  animation: fadeIn 0.5s ease-out both;
}}
.section:nth-child(2) {{ animation-delay: 0.1s; }}
.section:nth-child(3) {{ animation-delay: 0.2s; }}
.section-title {{
  font-size: 0.7rem;
  font-family: var(--font-mono);
  font-weight: 500;
  letter-spacing: 0.25em;
  text-transform: uppercase;
  color: var(--accent-gold);
  margin-bottom: 1rem;
  display: flex;
  align-items: center;
  gap: 0.5rem;
}}
.section-title::before {{
  content: '';
  display: inline-block;
  width: 12px;
  height: 2px;
  background: var(--accent-vermillion);
}}

/* card */
.card {{
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1.25rem 1.5rem;
  margin-bottom: 0.75rem;
  transition: border-color 0.2s ease;
}}
.card:hover {{
  border-color: #3a3d57;
}}

/* installations */
.install-item {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.75rem 0;
}}
.install-item + .install-item {{
  border-top: 1px solid var(--border-subtle);
}}
.install-name {{
  font-weight: 700;
  font-size: 1rem;
}}
.install-meta {{
  display: flex;
  align-items: center;
  gap: 0.6rem;
}}
.tag {{
  font-family: var(--font-mono);
  font-size: 0.7rem;
  letter-spacing: 0.05em;
  padding: 0.2rem 0.55rem;
  border-radius: 4px;
  background: var(--accent-gold-dim);
  color: var(--accent-gold);
  border: 1px solid rgba(201, 168, 76, 0.2);
}}
.tag-org {{
  background: rgba(74, 95, 215, 0.1);
  color: var(--accent-indigo);
  border-color: rgba(74, 95, 215, 0.2);
}}
.install-id {{
  font-family: var(--font-mono);
  font-size: 0.75rem;
  color: var(--text-secondary);
}}
.hint {{
  color: var(--text-secondary);
  font-size: 0.9rem;
  padding: 0.5rem 0;
}}

/* buttons */
.btn {{
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.65rem 1.3rem;
  font-family: var(--font-mono);
  font-size: 0.8rem;
  font-weight: 500;
  text-decoration: none;
  border-radius: 6px;
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text-primary);
  transition: all 0.2s ease;
  cursor: pointer;
  letter-spacing: 0.02em;
}}
.btn:hover {{
  border-color: var(--accent-vermillion);
  box-shadow: 0 0 16px var(--accent-vermillion-glow);
}}
.btn svg {{
  width: 16px;
  height: 16px;
  fill: currentColor;
}}
.btn-row {{
  margin-top: 1rem;
}}

/* code blocks */
.code-wrap {{
  position: relative;
  margin-top: 0.75rem;
}}
.code-block {{
  font-family: var(--font-mono);
  font-size: 0.8rem;
  line-height: 1.6;
  background: var(--bg-deep);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 1rem 1.25rem;
  overflow-x: auto;
  color: var(--text-primary);
  white-space: pre;
}}
.code-label {{
  font-family: var(--font-mono);
  font-size: 0.7rem;
  color: var(--text-secondary);
  margin-bottom: 0.4rem;
  letter-spacing: 0.05em;
}}
.copy-btn {{
  position: absolute;
  top: 0.6rem;
  right: 0.6rem;
  padding: 0.3rem 0.6rem;
  font-family: var(--font-mono);
  font-size: 0.65rem;
  background: var(--bg-elevated);
  color: var(--text-secondary);
  border: 1px solid var(--border);
  border-radius: 4px;
  cursor: pointer;
  transition: all 0.2s ease;
  letter-spacing: 0.05em;
}}
.copy-btn:hover {{
  color: var(--text-primary);
  border-color: var(--accent-gold);
}}
.copy-btn.copied {{
  color: var(--accent-gold);
  border-color: var(--accent-gold);
}}

.mcp-desc {{
  font-size: 0.85rem;
  color: var(--text-secondary);
  margin-bottom: 0.75rem;
  line-height: 1.6;
}}
.mcp-desc code {{
  font-family: var(--font-mono);
  font-size: 0.8rem;
  background: var(--bg-deep);
  padding: 0.15rem 0.4rem;
  border-radius: 3px;
  color: var(--accent-gold);
}}

@keyframes fadeIn {{
  from {{ opacity: 0; transform: translateY(8px); }}
  to {{ opacity: 1; transform: translateY(0); }}
}}
@media (max-width: 600px) {{
  .shell {{ padding: 1.5rem 1rem 3rem; }}
  .header {{ flex-direction: column; align-items: flex-start; gap: 1rem; }}
  .brand {{ font-size: 1.3rem; }}
}}
</style>
</head>
<body>
<div class="shell">
  <header class="header">
    <div class="header-left">
      <div>
        <div class="brand">目付</div>
        <div class="brand-sub">Metsuke</div>
      </div>
    </div>
    <div class="user-badge">
      <strong>{login}</strong>
      <a class="logout-link" href="/auth/logout">logout</a>
    </div>
  </header>

  <div class="section">
    <div class="section-title">Installations</div>
    <div class="card">
      {install_list}
    </div>
    <div class="btn-row">
      <a class="btn" href="https://github.com/apps/pleno-metsuke/installations/new?redirect_url={base_url}/auth/install/callback">
        <svg viewBox="0 0 16 16"><path d="M8 0a8 8 0 110 16A8 8 0 018 0zM1.5 8a6.5 6.5 0 1013 0 6.5 6.5 0 00-13 0z"/><path d="M8 4a.75.75 0 01.75.75v2.5h2.5a.75.75 0 010 1.5h-2.5v2.5a.75.75 0 01-1.5 0v-2.5h-2.5a.75.75 0 010-1.5h2.5v-2.5A.75.75 0 018 4z"/></svg>
        Install GitHub App
      </a>
    </div>
  </div>

  <div class="section">
    <div class="section-title">MCP Connection</div>
    <div class="card">
      <p class="mcp-desc">MCPクライアントは <code>OAuth 2.1</code> で自動認証されます。以下の設定をMCPクライアントに追加してください。</p>

      <div style="margin-top:1.25rem">
        <div class="code-label">Claude Code Settings</div>
        <div class="code-wrap">
          <pre class="code-block" id="config">{{
  "mcpServers": {{
    "metsuke": {{
      "url": "{base_url}/mcp"
    }}
  }}
}}</pre>
          <button class="copy-btn" onclick="copyText('config', this)">COPY</button>
        </div>
      </div>

      <div style="margin-top:1rem">
        <div class="code-label">Discovery Endpoints</div>
        <div class="code-wrap">
          <pre class="code-block" id="endpoints">Protected Resource: {base_url}/.well-known/oauth-protected-resource
Auth Server:        {base_url}/.well-known/oauth-authorization-server</pre>
        </div>
      </div>
    </div>
  </div>
</div>
<script>
function copyText(id, btn) {{
  const t = document.getElementById(id).textContent;
  navigator.clipboard.writeText(t).then(() => {{
    btn.textContent = 'COPIED';
    btn.classList.add('copied');
    setTimeout(() => {{ btn.textContent = 'COPY'; btn.classList.remove('copied'); }}, 1500);
  }});
}}
</script>
</body>
</html>"#,
        login = login,
        install_list = install_list,
        base_url = state.base_url,
    ))
    .into_response()
}
