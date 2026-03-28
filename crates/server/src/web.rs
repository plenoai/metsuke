use std::sync::Arc;

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
        .route("/api/repos", axum::routing::get(api_repos))
        .route(
            "/api/repos/{owner}/{repo}/verify",
            axum::routing::post(api_verify_repo),
        )
        .route(
            "/api/verification-cache",
            axum::routing::get(api_verification_cache),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Shared HTML helpers
// ---------------------------------------------------------------------------

/// Returns the full `<head>` section including fonts, CSS variables, and shared styles.
/// `title` is the page title shown in the browser tab.
fn common_head(title: &str) -> String {
    format!(
        r#"<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — Metsuke</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Shippori+Mincho:wght@400;700;800&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
<link rel="stylesheet" href="/static/style.css">
</head>"#,
    )
}

/// Returns the `<header>` element with brand, nav links, user badge, and logout.
/// `login` is the GitHub username.
/// `active_page` is `"dashboard"` or `"repos"`.
fn nav_header(login: &str, active_page: &str) -> String {
    let dash_class = if active_page == "dashboard" {
        "nav-link active"
    } else {
        "nav-link"
    };
    let repos_class = if active_page == "repos" {
        "nav-link active"
    } else {
        "nav-link"
    };
    format!(
        r##"<a class="skip-link" href="#main">コンテンツへスキップ</a>
  <header class="header" role="banner">
    <div class="header-left">
      <div>
        <a class="brand" href="/dashboard" aria-label="Metsuke ホーム">目付</a>
        <div class="brand-sub">Metsuke</div>
      </div>
      <nav class="nav-links" aria-label="メインナビゲーション">
        <a class="{dash_class}" href="/dashboard">Dashboard</a>
        <a class="{repos_class}" href="/repos">Repos</a>
      </nav>
    </div>
    <div class="user-badge">
      <strong>{login}</strong>
      <a class="logout-link" href="/auth/logout">logout</a>
    </div>
  </header>
  <main id="main" role="main">"##,
    )
}

/// Policy `<option>` tags for the policy selector dropdown.
fn policy_options() -> &'static str {
    r#"<option value="default">default</option>
<option value="oss">oss</option>
<option value="aiops">aiops</option>
<option value="soc1">soc1</option>
<option value="soc2">soc2</option>
<option value="slsa-l1">slsa-l1</option>
<option value="slsa-l2">slsa-l2</option>
<option value="slsa-l3">slsa-l3</option>
<option value="slsa-l4">slsa-l4</option>"#
}

// ---------------------------------------------------------------------------
// Error page (unique layout, no shared helpers)
// ---------------------------------------------------------------------------

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
// Landing page (unique layout, no shared helpers)
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

    let install_count = installations.len();
    let head = common_head("Dashboard");
    let header = nav_header(&login, "dashboard");

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="ja">
{head}
<body>
<div class="shell">
  {header}

  <div class="section">
    <div class="section-title">Overview</div>
    <div class="card" id="overview-card">
      <div style="display:flex;gap:1.5rem;flex-wrap:wrap;align-items:center">
        <div style="text-align:center;min-width:80px">
          <div id="ov-repos" style="font-size:2rem;font-weight:800;color:var(--text-primary)">—</div>
          <div style="font-family:var(--font-mono);font-size:0.7rem;color:var(--text-secondary);letter-spacing:0.1em">REPOS</div>
        </div>
        <div style="text-align:center;min-width:80px">
          <div id="ov-installs" style="font-size:2rem;font-weight:800;color:var(--accent-indigo)">{install_count}</div>
          <div style="font-family:var(--font-mono);font-size:0.7rem;color:var(--text-secondary);letter-spacing:0.1em">INSTALLS</div>
        </div>
        <div style="text-align:center;min-width:80px">
          <div id="ov-verified" style="font-size:2rem;font-weight:800;color:var(--accent-green)">—</div>
          <div style="font-family:var(--font-mono);font-size:0.7rem;color:var(--text-secondary);letter-spacing:0.1em">VERIFIED</div>
        </div>
        <div style="text-align:center;min-width:80px">
          <div id="ov-failing" style="font-size:2rem;font-weight:800;color:var(--accent-vermillion)">—</div>
          <div style="font-family:var(--font-mono);font-size:0.7rem;color:var(--text-secondary);letter-spacing:0.1em">FAILING</div>
        </div>
        <div style="flex:1;display:flex;justify-content:flex-end">
          <a class="btn" href="/repos" style="font-size:0.75rem">リポジトリ一覧を見る →</a>
        </div>
      </div>
      <div id="ov-bar" style="margin-top:0.75rem;height:6px;background:var(--bg-deep);border-radius:3px;overflow:hidden;display:none">
        <div style="display:flex;height:100%">
          <div id="ov-bar-pass" style="background:var(--accent-green);height:100%;transition:width 0.5s ease"></div>
          <div id="ov-bar-review" style="background:var(--accent-gold);height:100%;transition:width 0.5s ease"></div>
          <div id="ov-bar-fail" style="background:var(--accent-vermillion);height:100%;transition:width 0.5s ease"></div>
        </div>
      </div>
    </div>
  </div>

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
  </main>
</div>
<style>
/* dashboard-specific styles */
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
</style>
<script>
function copyText(id, btn) {{
  const t = document.getElementById(id).textContent;
  navigator.clipboard.writeText(t).then(() => {{
    btn.textContent = 'COPIED';
    btn.classList.add('copied');
    setTimeout(() => {{ btn.textContent = 'COPY'; btn.classList.remove('copied'); }}, 1500);
  }});
}}
Promise.all([
  fetch('/api/repos').then(r => r.ok ? r.json() : []),
  fetch('/api/verification-cache').then(r => r.ok ? r.json() : []),
]).then(([repos, cache]) => {{
  document.getElementById('ov-repos').textContent = repos.length;
  const verified = new Set(cache.map(c => `${{c.owner}}/${{c.repo}}`));
  document.getElementById('ov-verified').textContent = verified.size;
  const failing = new Set(cache.filter(c => c.fail > 0).map(c => `${{c.owner}}/${{c.repo}}`));
  document.getElementById('ov-failing').textContent = failing.size;
  let totalP = 0, totalR = 0, totalF = 0;
  cache.forEach(c => {{ totalP += c.pass; totalR += c.review; totalF += c.fail; }});
  const total = totalP + totalR + totalF;
  if (total > 0) {{
    const bar = document.getElementById('ov-bar');
    bar.style.display = '';
    document.getElementById('ov-bar-pass').style.width = `${{(totalP/total*100).toFixed(1)}}%`;
    document.getElementById('ov-bar-review').style.width = `${{(totalR/total*100).toFixed(1)}}%`;
    document.getElementById('ov-bar-fail').style.width = `${{(totalF/total*100).toFixed(1)}}%`;
  }}
}}).catch(() => {{}});
</script>
</body>
</html>"#,
        base_url = state.base_url,
    ))
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

    let head = common_head("Repositories");
    let header = nav_header(&login, "repos");
    let options = policy_options();

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="ja">
{head}
<body>
<div class="shell">
  {header}

  <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:1rem;flex-wrap:wrap;gap:0.5rem">
    <div class="section-title" id="repos-title" style="margin-bottom:0">Repositories</div>
    <div style="display:flex;gap:0.5rem;align-items:center;flex-wrap:wrap" id="toolbar" class="toolbar-hidden">
      <select class="policy-select" id="sort-select" onchange="sortRepos(this.value)" aria-label="ソート順">
        <option value="name">名前順</option>
        <option value="fail-desc">FAIL多い順</option>
        <option value="fail-asc">FAIL少ない順</option>
        <option value="unverified">未検証優先</option>
      </select>
      <button class="verify-btn" id="export-csv-btn" onclick="exportCSV()" style="display:none">CSV出力</button>
      <button class="btn" id="verify-all-btn" onclick="verifyAll()" style="font-size:0.72rem;display:none">全リポジトリを検証</button>
    </div>
  </div>
  <div id="verify-all-progress" style="display:none;margin-bottom:1rem">
    <div style="background:var(--bg-surface);border:1px solid var(--border);border-radius:8px;padding:0.75rem 1rem;font-family:var(--font-mono);font-size:0.75rem;color:var(--text-secondary)">
      <span id="progress-text">0 / 0</span>
      <div style="margin-top:0.5rem;height:4px;background:var(--bg-deep);border-radius:2px;overflow:hidden">
        <div id="progress-bar" style="height:100%;background:var(--accent-vermillion);width:0%;transition:width 0.3s ease"></div>
      </div>
    </div>
  </div>
  <div id="repo-list">
    <div class="loading">リポジトリを取得中</div>
  </div>
  </main>
</div>
<style>
.repo-grid {{
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}}
.repo-card {{
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1rem 1.25rem;
  display: flex;
  align-items: center;
  justify-content: space-between;
  transition: border-color 0.2s ease;
}}
.repo-card:hover {{
  border-color: #3a3d57;
}}
.repo-info {{
  flex: 1;
  min-width: 0;
}}
.repo-name {{
  font-weight: 700;
  font-size: 0.95rem;
  margin-bottom: 0.25rem;
}}
.repo-name a {{
  color: var(--text-primary);
  text-decoration: none;
}}
.repo-name a:hover {{
  color: var(--accent-gold);
}}
.repo-meta {{
  font-family: var(--font-mono);
  font-size: 0.72rem;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
}}
.repo-actions {{
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-shrink: 0;
  margin-left: 1rem;
}}
.result-summary {{
  display: flex;
  align-items: center;
  gap: 0.35rem;
}}
.empty-state {{
  text-align: center;
  padding: 3rem;
  color: var(--text-secondary);
  font-size: 0.9rem;
}}
@media (max-width: 600px) {{
  .repo-card {{ flex-direction: column; align-items: flex-start; gap: 0.75rem; }}
  .repo-actions {{ margin-left: 0; }}
}}
</style>

<script>
const POLICY_OPTIONS = `{options}`;

function renderBadges(pass, fail, review) {{
  let b = '';
  if (pass > 0) b += `<span class="badge badge-pass">PASS ${{pass}}</span>`;
  if (review > 0) b += `<span class="badge badge-review">REVIEW ${{review}}</span>`;
  if (fail > 0) b += `<span class="badge badge-fail">FAIL ${{fail}}</span>`;
  return b;
}}

function filterRepos() {{
  const q = (document.getElementById('search-input')?.value || '').toLowerCase();
  document.querySelectorAll('.repo-card').forEach(card => {{
    const name = card.dataset.name || '';
    const desc = card.dataset.desc || '';
    const lang = card.dataset.lang || '';
    const match = name.includes(q) || desc.includes(q) || lang.includes(q);
    card.style.display = match ? '' : 'none';
  }});
}}

async function loadRepos() {{
  try {{
    const [repoResp, cacheResp] = await Promise.all([
      fetch('/api/repos'),
      fetch('/api/verification-cache'),
    ]);
    const repos = repoResp.ok ? await repoResp.json() : [];
    const cache = cacheResp.ok ? await cacheResp.json() : [];

    const cacheMap = {{}};
    for (const c of cache) {{
      cacheMap[`${{c.owner}}/${{c.repo}}`] = c;
    }}

    const container = document.getElementById('repo-list');
    document.getElementById('repos-title').textContent = `Repositories (${{repos.length}})`;

    if (repos.length === 0) {{
      container.innerHTML = '<div class="empty-state">リポジトリが見つかりません。GitHub Appをインストールしてください。</div>';
      return;
    }}

    const searchBar = `<div style="margin-bottom:0.75rem">
      <input type="search" id="search-input" class="policy-select" style="width:100%;padding:0.5rem 0.75rem;font-size:0.8rem" placeholder="リポジトリを検索…" oninput="filterRepos()" aria-label="リポジトリを検索">
    </div>`;

    const cards = repos.map(r => {{
      const cached = cacheMap[r.full_name];
      const cachedBadges = cached ? renderBadges(cached.pass, cached.fail, cached.review) : '';
      const cachedTime = cached ? `<span style="font-family:var(--font-mono);font-size:0.6rem;color:var(--text-secondary)" title="最終検証">${{cached.verified_at}}</span>` : '';
      const desc = r.description ? `<div style="font-family:var(--font-mono);font-size:0.7rem;color:var(--text-secondary);margin-top:0.2rem;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:500px">${{r.description}}</div>` : '';

      return `<div class="repo-card" id="repo-${{r.full_name.replace('/', '-')}}" data-name="${{r.full_name.toLowerCase()}}" data-desc="${{(r.description || '').toLowerCase()}}" data-lang="${{(r.language || '').toLowerCase()}}">
        <div class="repo-info">
          <div class="repo-name">
            <a href="/repos/${{r.owner}}/${{r.name}}">${{r.full_name}}</a>
            <a class="gh-link" href="https://github.com/${{r.full_name}}" target="_blank" rel="noopener" title="GitHub で開く">
              <svg viewBox="0 0 16 16"><path d="M3.75 2h3.5a.75.75 0 010 1.5H4.56l6.22 6.22a.75.75 0 11-1.06 1.06L3.5 4.56v2.69a.75.75 0 01-1.5 0v-3.5A1.75 1.75 0 013.75 2z"/><path d="M9.25 3.5a.75.75 0 010-1.5h3A1.75 1.75 0 0114 3.75v8.5A1.75 1.75 0 0112.25 14h-8.5A1.75 1.75 0 012 12.25v-3a.75.75 0 011.5 0v3c0 .138.112.25.25.25h8.5a.25.25 0 00.25-.25v-8.5a.25.25 0 00-.25-.25h-3z"/></svg>
            </a>
          </div>
          ${{desc}}
          <div class="repo-meta">
            ${{r.private ? '<span class="badge badge-private">private</span>' : ''}}
            ${{r.language ? `<span>${{r.language}}</span>` : ''}}
            ${{r.default_branch ? `<span>${{r.default_branch}}</span>` : ''}}
            ${{cachedTime}}
          </div>
        </div>
        <div class="repo-actions">
          <div class="result-summary" id="result-${{r.full_name.replace('/', '-')}}">${{cachedBadges}}</div>
          <select class="policy-select" id="policy-${{r.full_name.replace('/', '-')}}">${{POLICY_OPTIONS}}</select>
          <button class="verify-btn" onclick="verifyRepo('${{r.owner}}', '${{r.name}}', this)">${{cached ? '再検証' : '検証'}}</button>
        </div>
      </div>`;
    }}).join('');

    container.innerHTML = searchBar + '<div class="repo-grid">' + cards + '</div>';
    allRepos = repos;
    allCache = cacheMap;
    if (repos.length > 0) {{
      document.getElementById('verify-all-btn').style.display = '';
      document.getElementById('export-csv-btn').style.display = '';
    }}
  }} catch (e) {{
    document.getElementById('repo-list').innerHTML =
      '<div class="empty-state">リポジトリの取得に失敗しました。</div>';
  }}
}}

async function verifyRepo(owner, repo, btn) {{
  btn.disabled = true;
  btn.classList.add('running');
  btn.textContent = '検証中…';
  const resultEl = document.getElementById(`result-${{owner}}-${{repo}}`);
  resultEl.innerHTML = '';

  const policyEl = document.getElementById(`policy-${{owner}}-${{repo}}`);
  const policy = policyEl ? policyEl.value : 'default';

  try {{
    const resp = await fetch(`/api/repos/${{owner}}/${{repo}}/verify?policy=${{encodeURIComponent(policy)}}`, {{ method: 'POST' }});
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();

    let pass = 0, fail = 0, review = 0;
    if (data.report && data.report.findings) {{
      for (const f of data.report.findings) {{
        if (f.status === 'Satisfied') pass++;
        else if (f.status === 'Violated') fail++;
        else if (f.status === 'Indeterminate') review++;
      }}
    }}
    resultEl.innerHTML = renderBadges(pass, fail, review);

    btn.textContent = '再検証';
  }} catch (e) {{
    resultEl.innerHTML = '<span class="badge badge-fail">ERROR</span>';
    btn.textContent = '再試行';
  }}
  btn.disabled = false;
  btn.classList.remove('running');
}}

let allRepos = [];
let allCache = {{}};

function sortRepos(mode) {{
  const grid = document.querySelector('.repo-grid');
  if (!grid) return;
  const cards = Array.from(grid.children);
  cards.sort((a, b) => {{
    const nameA = a.dataset.name || '';
    const nameB = b.dataset.name || '';
    const cA = allCache[nameA.replace('-', '/')] || {{}};
    const cB = allCache[nameB.replace('-', '/')] || {{}};
    if (mode === 'fail-desc') return (cB.fail || 0) - (cA.fail || 0);
    if (mode === 'fail-asc') return (cA.fail || 0) - (cB.fail || 0);
    if (mode === 'unverified') {{
      const vA = cA.verified_at ? 1 : 0;
      const vB = cB.verified_at ? 1 : 0;
      return vA - vB;
    }}
    return nameA.localeCompare(nameB);
  }});
  cards.forEach(c => grid.appendChild(c));
}}

function exportCSV() {{
  const rows = [['Repository','Language','Private','Policy','Pass','Fail','Review','N/A','Verified At']];
  allRepos.forEach(r => {{
    const c = allCache[r.full_name] || {{}};
    rows.push([
      r.full_name,
      r.language || '',
      r.private ? 'yes' : 'no',
      c.policy || '',
      c.pass || 0,
      c.fail || 0,
      c.review || 0,
      c.na || 0,
      c.verified_at || 'not verified',
    ]);
  }});
  const csv = rows.map(r => r.map(v => `"${{String(v).replace(/"/g, '""')}}"`).join(',')).join('\n');
  const blob = new Blob([csv], {{ type: 'text/csv;charset=utf-8;' }});
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `metsuke-verification-${{new Date().toISOString().slice(0,10)}}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}}

async function verifyAll() {{
  const btn = document.getElementById('verify-all-btn');
  const progressWrap = document.getElementById('verify-all-progress');
  const progressText = document.getElementById('progress-text');
  const progressBar = document.getElementById('progress-bar');
  btn.disabled = true;
  btn.textContent = '検証中…';
  progressWrap.style.display = 'block';
  let done = 0;
  const total = allRepos.length;
  progressText.textContent = `0 / ${{total}}`;
  for (const r of allRepos) {{
    const verifyBtn = document.querySelector(`#repo-${{r.full_name.replace('/', '-')}} .verify-btn`);
    if (verifyBtn) {{
      await verifyRepo(r.owner, r.name, verifyBtn);
    }}
    done++;
    progressText.textContent = `${{done}} / ${{total}}`;
    progressBar.style.width = `${{(done / total * 100).toFixed(1)}}%`;
  }}
  btn.disabled = false;
  btn.textContent = '全リポジトリを検証';
}}

loadRepos();
</script>
</body>
</html>"#,
    ))
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

    let head = common_head(&format!("{owner}/{repo}"));
    let header = nav_header(&login, "repos");
    let options = policy_options();

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="ja">
{head}
<body>
<div class="shell">
  {header}

  <div class="section">
    <div style="display:flex;align-items:center;justify-content:space-between;flex-wrap:wrap;gap:1rem;margin-bottom:1.5rem">
      <div>
        <div style="font-size:1.3rem;font-weight:800;letter-spacing:0.02em">{owner}<span style="color:var(--text-secondary);font-weight:400"> / </span>{repo}</div>
        <a class="gh-link" href="https://github.com/{owner}/{repo}" target="_blank" rel="noopener" style="font-family:var(--font-mono);font-size:0.75rem;margin-left:0">
          <svg viewBox="0 0 16 16"><path d="M3.75 2h3.5a.75.75 0 010 1.5H4.56l6.22 6.22a.75.75 0 11-1.06 1.06L3.5 4.56v2.69a.75.75 0 01-1.5 0v-3.5A1.75 1.75 0 013.75 2z"/><path d="M9.25 3.5a.75.75 0 010-1.5h3A1.75 1.75 0 0114 3.75v8.5A1.75 1.75 0 0112.25 14h-8.5A1.75 1.75 0 012 12.25v-3a.75.75 0 011.5 0v3c0 .138.112.25.25.25h8.5a.25.25 0 00.25-.25v-8.5a.25.25 0 00-.25-.25h-3z"/></svg>
          GitHub で開く
        </a>
      </div>
      <div style="display:flex;align-items:center;gap:0.5rem">
        <select class="policy-select" id="policy-select">{options}</select>
        <button class="btn" id="verify-btn" onclick="runVerify()">検証を実行</button>
      </div>
    </div>
  </div>

  <div id="result-area"></div>
  </main>
</div>

<style>
.findings-table {{
  width: 100%;
  border-collapse: collapse;
  font-family: var(--font-mono);
  font-size: 0.78rem;
}}
.findings-table th {{
  text-align: left;
  font-size: 0.68rem;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  color: var(--text-secondary);
  padding: 0.6rem 0.75rem;
  border-bottom: 1px solid var(--border);
  background: var(--bg-deep);
}}
.findings-table td {{
  padding: 0.6rem 0.75rem;
  border-bottom: 1px solid var(--border-subtle);
  vertical-align: top;
  color: var(--text-primary);
}}
.findings-table tr:hover td {{
  background: var(--bg-elevated);
}}
.findings-table .rationale {{
  font-size: 0.72rem;
  color: var(--text-secondary);
  max-width: 420px;
  line-height: 1.5;
}}
.summary-bar {{
  display: flex;
  gap: 0.75rem;
  margin-bottom: 1.25rem;
  flex-wrap: wrap;
}}
.summary-stat {{
  font-family: var(--font-mono);
  font-size: 0.8rem;
  display: flex;
  align-items: center;
  gap: 0.35rem;
}}
</style>

<script>
const OWNER = '{owner}';
const REPO = '{repo}';

async function runVerify() {{
  const btn = document.getElementById('verify-btn');
  const area = document.getElementById('result-area');
  const policyEl = document.getElementById('policy-select');
  const policy = policyEl.value;

  btn.disabled = true;
  btn.textContent = '検証中…';
  btn.style.borderColor = 'var(--accent-gold)';
  btn.style.color = 'var(--accent-gold)';
  area.innerHTML = '<div class="loading">検証を実行中</div>';

  try {{
    const resp = await fetch(`/api/repos/${{OWNER}}/${{REPO}}/verify?policy=${{encodeURIComponent(policy)}}`, {{ method: 'POST' }});
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();

    const findings = (data.report && data.report.findings) || [];
    const profileName = (data.report && data.report.profile_name) || policy;

    let pass = 0, fail = 0, review = 0, na = 0;
    for (const f of findings) {{
      if (f.status === 'Satisfied') pass++;
      else if (f.status === 'Violated') fail++;
      else if (f.status === 'Indeterminate') review++;
      else if (f.status === 'NotApplicable') na++;
    }}

    function statusBadge(status) {{
      if (status === 'Satisfied') return '<span class="badge badge-pass">PASS</span>';
      if (status === 'Violated') return '<span class="badge badge-fail">FAIL</span>';
      if (status === 'Indeterminate') return '<span class="badge badge-review">REVIEW</span>';
      if (status === 'NotApplicable') return '<span class="badge badge-na">N/A</span>';
      return '<span class="badge">' + status + '</span>';
    }}

    let html = `<div class="section-title">Results — ${{profileName}}</div>`;
    html += '<div class="summary-bar">';
    html += `<div class="summary-stat"><span class="badge badge-pass">PASS</span> ${{pass}}</div>`;
    html += `<div class="summary-stat"><span class="badge badge-fail">FAIL</span> ${{fail}}</div>`;
    html += `<div class="summary-stat"><span class="badge badge-review">REVIEW</span> ${{review}}</div>`;
    html += `<div class="summary-stat"><span class="badge badge-na">N/A</span> ${{na}}</div>`;
    html += '</div>';

    html += '<div class="card" style="padding:0;overflow:hidden">';
    html += '<table class="findings-table"><thead><tr><th>Control</th><th>Status</th><th>Rationale</th></tr></thead><tbody>';
    for (const f of findings) {{
      const rationale = f.rationale || '';
      const escaped = rationale.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
      html += `<tr><td style="white-space:nowrap">${{f.control_id}}</td><td>${{statusBadge(f.status)}}</td><td class="rationale">${{escaped}}</td></tr>`;
    }}
    html += '</tbody></table></div>';

    area.innerHTML = html;
  }} catch (e) {{
    area.innerHTML = `<div class="card" style="color:var(--accent-vermillion)"><strong>検証エラー:</strong> ${{e.message}}</div>`;
  }}

  btn.disabled = false;
  btn.textContent = '検証を実行';
  btn.style.borderColor = '';
  btn.style.color = '';
}}
</script>
</body>
</html>"#,
    ))
    .into_response()
}
