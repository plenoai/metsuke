use askama_web::WebTemplateExt;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::blocking::run_blocking;

use super::WebState;
use super::helpers::*;
use super::jobs::spawn_sync_repos_job;
use super::templates::{LandingTemplate, SettingsTemplate};

// ---------------------------------------------------------------------------
// Landing page
// ---------------------------------------------------------------------------

pub(super) async fn index(headers: HeaderMap, State(state): State<WebState>) -> Response {
    if require_user(&state.db, &headers).await.is_some() {
        return Redirect::to("/repos").into_response();
    }

    LandingTemplate.into_web_template().into_response()
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
        return error_page("内部エラー", "インストール情報の保存に失敗しました。");
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
