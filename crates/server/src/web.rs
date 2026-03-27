use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::header::SET_COOKIE;
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Router;
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

fn get_session_from_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| c.trim().strip_prefix("session=").map(|s| s.to_string()))
}

fn session_cookie(session_id: &str, max_age: i64) -> String {
    format!(
        "session={session_id}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age}"
    )
}

async fn index(headers: HeaderMap, State(state): State<WebState>) -> Response {
    if let Some(session_id) = get_session_from_cookie(&headers) {
        if state.db.get_user_by_session(&session_id).ok().flatten().is_some() {
            return Redirect::to("/dashboard").into_response();
        }
    }

    Html(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Metsuke</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:system-ui,sans-serif;background:#0d1117;color:#c9d1d9;display:flex;align-items:center;justify-content:center;min-height:100vh}
.card{text-align:center;padding:3rem;border:1px solid #30363d;border-radius:12px;background:#161b22;max-width:400px}
h1{font-size:2rem;margin-bottom:0.5rem}
p{color:#8b949e;margin-bottom:2rem}
a.btn{display:inline-block;padding:0.75rem 1.5rem;background:#238636;color:#fff;text-decoration:none;border-radius:6px;font-weight:600}
a.btn:hover{background:#2ea043}
</style>
</head>
<body>
<div class="card">
  <h1>目付 Metsuke</h1>
  <p>SDLC Process Inspector</p>
  <a class="btn" href="/auth/login">Login with GitHub</a>
</div>
</body>
</html>"#,
    )
    .into_response()
}

#[derive(Deserialize)]
struct AuthCallback {
    code: String,
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
    let token_resp = match state.github_app.exchange_code(&params.code).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("OAuth exchange failed: {e:#}");
            return Html(format!("Authentication failed: {e}")).into_response();
        }
    };

    let user = match GitHubApp::get_user(&token_resp.access_token).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Failed to get user: {e:#}");
            return Html(format!("Failed to get user info: {e}")).into_response();
        }
    };

    let user_id = match state
        .db
        .upsert_user(user.id, &user.login, user.avatar_url.as_deref())
    {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("DB error: {e:#}");
            return Html("Internal error".to_string()).into_response();
        }
    };

    let session_id = match state.db.create_session(user_id) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Session creation failed: {e:#}");
            return Html("Internal error".to_string()).into_response();
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
    resp.headers_mut().insert(
        SET_COOKIE,
        session_cookie("", 0).parse().unwrap(),
    );
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

    let installation = match state.github_app.get_installation(params.installation_id).await {
        Ok(i) => i,
        Err(e) => {
            tracing::error!("Failed to get installation: {e:#}");
            return Html(format!("Failed to verify installation: {e}")).into_response();
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
        "<p class=\"hint\">No installations yet. Install the GitHub App on your account or organization.</p>".to_string()
    } else {
        let items: Vec<String> = installations
            .iter()
            .map(|(id, login, typ)| {
                format!("<li><strong>{login}</strong> <span class=\"tag\">{typ}</span> <code>#{id}</code></li>")
            })
            .collect();
        format!("<ul>{}</ul>", items.join(""))
    };

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Dashboard - Metsuke</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{font-family:system-ui,sans-serif;background:#0d1117;color:#c9d1d9;padding:2rem;max-width:720px;margin:0 auto}}
h1{{margin-bottom:0.5rem}}
h2{{margin-top:2rem;margin-bottom:1rem;color:#58a6ff}}
.hint{{color:#8b949e}}
.tag{{background:#30363d;padding:0.15rem 0.5rem;border-radius:4px;font-size:0.85rem}}
ul{{list-style:none}} li{{padding:0.5rem 0;border-bottom:1px solid #21262d}}
code{{background:#161b22;padding:0.2rem 0.5rem;border-radius:4px;font-size:0.9rem}}
pre{{background:#161b22;padding:1rem;border-radius:8px;overflow-x:auto;margin-top:0.5rem}}
a.btn{{display:inline-block;padding:0.5rem 1rem;background:#238636;color:#fff;text-decoration:none;border-radius:6px;font-weight:600;margin-right:0.5rem}}
a.btn:hover{{background:#2ea043}}
a.btn.secondary{{background:#30363d}}
a.btn.secondary:hover{{background:#484f58}}
.actions{{margin-top:2rem}}
</style>
</head>
<body>
<h1>Dashboard</h1>
<p>Logged in as <strong>{login}</strong></p>

<h2>Installations</h2>
{install_list}

<div class="actions">
  <a class="btn" href="https://github.com/apps/pleno-metsuke/installations/new?redirect_url={base_url}/auth/install/callback">Install GitHub App</a>
</div>

<h2>MCP Connection</h2>
<p>Use this session token as <code>Bearer</code> token for MCP clients:</p>
<pre>{session_id}</pre>

<p style="margin-top:1rem">Claude Code config:</p>
<pre>{{
  "mcpServers": {{
    "metsuke": {{
      "url": "{base_url}/mcp",
      "headers": {{
        "Authorization": "Bearer {session_id}"
      }}
    }}
  }}
}}</pre>

<div class="actions">
  <a class="btn secondary" href="/auth/logout">Logout</a>
</div>
</body>
</html>"#,
        login = login,
        install_list = install_list,
        session_id = session_id,
        base_url = state.base_url,
    ))
    .into_response()
}
