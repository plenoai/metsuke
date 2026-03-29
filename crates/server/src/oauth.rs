use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Redirect, Response};
use base64::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::AppConfig;
use crate::db::Database;
use crate::github_app::GitHubApp;

#[derive(Clone)]
struct OAuthState {
    db: Arc<Database>,
    github_app: Arc<GitHubApp>,
    base_url: String,
}

const ACCESS_TOKEN_TTL: i64 = 3600; // 1 hour
const REFRESH_TOKEN_TTL: i64 = 30 * 24 * 3600; // 30 days

pub fn router(db: Arc<Database>, github_app: Arc<GitHubApp>, config: &AppConfig) -> Router {
    let state = OAuthState {
        db,
        github_app,
        base_url: config.base_url.clone(),
    };

    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            axum::routing::get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(authorization_server_metadata),
        )
        .route("/oauth/authorize", axum::routing::get(authorize))
        .route("/oauth/token", axum::routing::post(token))
        .route("/oauth/register", axum::routing::post(register))
        .with_state(state)
}

/// RFC 9728: Protected Resource Metadata
async fn protected_resource_metadata(State(state): State<OAuthState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "resource": format!("{}/mcp", state.base_url),
        "authorization_servers": [state.base_url],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["mcp"]
    }))
}

/// RFC 8414: Authorization Server Metadata
async fn authorization_server_metadata(State(state): State<OAuthState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "issuer": state.base_url,
        "authorization_endpoint": format!("{}/oauth/authorize", state.base_url),
        "token_endpoint": format!("{}/oauth/token", state.base_url),
        "registration_endpoint": format!("{}/oauth/register", state.base_url),
        "scopes_supported": ["mcp"],
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_post"],
        "code_challenge_methods_supported": ["S256"],
        "service_documentation": format!("{}/", state.base_url)
    }))
}

// --- Dynamic Client Registration (RFC 7591) ---

#[derive(Deserialize)]
struct RegisterRequest {
    redirect_uris: Vec<String>,
    client_name: Option<String>,
    token_endpoint_auth_method: Option<String>,
    grant_types: Option<Vec<String>>,
}

#[derive(Serialize)]
struct RegisterResponse {
    client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
    client_name: Option<String>,
    redirect_uris: Vec<String>,
    grant_types: Vec<String>,
    token_endpoint_auth_method: String,
}

async fn register(State(state): State<OAuthState>, Json(req): Json<RegisterRequest>) -> Response {
    if req.redirect_uris.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_client_metadata", "error_description": "redirect_uris is required"})),
        )
            .into_response();
    }

    // Validate redirect URIs
    for uri in &req.redirect_uris {
        if uri.contains('#') {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid_redirect_uri", "error_description": "Fragment not allowed in redirect_uri"})),
            )
                .into_response();
        }
    }

    let auth_method = req
        .token_endpoint_auth_method
        .unwrap_or_else(|| "none".into());

    let client_id = generate_random_token();
    let client_secret = if auth_method == "client_secret_post" {
        Some(generate_random_token())
    } else {
        None
    };

    if let Err(e) = state.db.register_oauth_client(
        &client_id,
        client_secret.as_deref(),
        req.client_name.as_deref(),
        &req.redirect_uris,
        &auth_method,
    ) {
        tracing::error!("Failed to register client: {e:#}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "server_error"})),
        )
            .into_response();
    }

    let grant_types = req
        .grant_types
        .unwrap_or_else(|| vec!["authorization_code".into(), "refresh_token".into()]);

    (
        StatusCode::CREATED,
        Json(RegisterResponse {
            client_id,
            client_secret,
            client_name: req.client_name,
            redirect_uris: req.redirect_uris,
            grant_types,
            token_endpoint_auth_method: auth_method,
        }),
    )
        .into_response()
}

// --- Authorization Endpoint ---

#[derive(Deserialize)]
struct AuthorizeParams {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

async fn authorize(
    Query(params): Query<AuthorizeParams>,
    State(state): State<OAuthState>,
) -> Response {
    // Validate response_type
    if params.response_type != "code" {
        return oauth_error_redirect(
            &params.redirect_uri,
            "unsupported_response_type",
            params.state.as_deref(),
        );
    }

    // Validate PKCE (S256 required)
    if params.code_challenge_method != "S256" {
        return oauth_error_redirect(
            &params.redirect_uri,
            "invalid_request",
            params.state.as_deref(),
        );
    }

    // Validate client
    let client = match state.db.get_oauth_client(&params.client_id) {
        Ok(Some(c)) => c,
        _ => {
            return oauth_error_redirect(
                &params.redirect_uri,
                "invalid_client",
                params.state.as_deref(),
            );
        }
    };

    // Validate redirect_uri
    if !client.redirect_uris().contains(&params.redirect_uri) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_redirect_uri"})),
        )
            .into_response();
    }

    let scope = params.scope.unwrap_or_else(|| "mcp".into());

    // Generate internal state that maps to the client's OAuth params
    let internal_state = generate_random_token();

    if let Err(e) = state.db.create_oauth_state(
        &internal_state,
        &params.client_id,
        &params.redirect_uri,
        &params.code_challenge,
        &scope,
    ) {
        tracing::error!("Failed to create OAuth state: {e:#}");
        return oauth_error_redirect(
            &params.redirect_uri,
            "server_error",
            params.state.as_deref(),
        );
    }

    // Encode the client state into our internal state so we can return it after GitHub callback
    let combined_state = if let Some(ref client_state) = params.state {
        format!(
            "{internal_state}:{}",
            BASE64_URL_SAFE_NO_PAD.encode(client_state.as_bytes())
        )
    } else {
        internal_state
    };

    // Redirect to GitHub OAuth (reuse registered callback URL)
    let github_redirect = format!("{}/auth/callback", state.base_url);
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user&state={}",
        state.github_app.client_id(),
        urlencoding::encode(&github_redirect),
        urlencoding::encode(&combined_state),
    );
    Redirect::temporary(&url).into_response()
}

// --- OAuth Callback (GitHub → Metsuke → MCP Client) ---
// Called from web.rs auth_callback when state parameter is present.

pub async fn handle_oauth_callback(
    code: &str,
    combined_state: &str,
    db: &Database,
    github_app: &GitHubApp,
) -> Response {
    // Split internal_state and client_state
    let (internal_state, client_state) = if let Some(idx) = combined_state.find(':') {
        let (is, cs_b64) = combined_state.split_at(idx);
        let cs_b64 = &cs_b64[1..]; // skip ':'
        let cs = BASE64_URL_SAFE_NO_PAD
            .decode(cs_b64)
            .ok()
            .and_then(|b| String::from_utf8(b).ok());
        (is.to_string(), cs)
    } else {
        (combined_state.to_string(), None)
    };

    // Retrieve stored OAuth state
    let oauth_state = match db.consume_oauth_state(&internal_state) {
        Ok(Some(s)) => s,
        _ => {
            return (StatusCode::BAD_REQUEST, "Invalid or expired state").into_response();
        }
    };

    // Exchange GitHub code for token and get user
    let token_resp = match github_app.exchange_code(code).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("GitHub OAuth exchange failed: {e:#}");
            return oauth_error_redirect(
                &oauth_state.redirect_uri,
                "server_error",
                client_state.as_deref(),
            );
        }
    };

    let user = match GitHubApp::get_user(&token_resp.access_token).await {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Failed to get GitHub user: {e:#}");
            return oauth_error_redirect(
                &oauth_state.redirect_uri,
                "server_error",
                client_state.as_deref(),
            );
        }
    };

    let user_id = match db.upsert_user(
        user.id,
        &user.login,
        user.avatar_url.as_deref(),
        Some(&token_resp.access_token),
    ) {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("DB error: {e:#}");
            return oauth_error_redirect(
                &oauth_state.redirect_uri,
                "server_error",
                client_state.as_deref(),
            );
        }
    };

    // Generate authorization code
    let auth_code = generate_random_token();
    if let Err(e) = db.create_authorization_code(
        &auth_code,
        &oauth_state.client_id,
        user_id,
        &oauth_state.redirect_uri,
        &oauth_state.code_challenge,
        &oauth_state.scope,
    ) {
        tracing::error!("Failed to create authorization code: {e:#}");
        return oauth_error_redirect(
            &oauth_state.redirect_uri,
            "server_error",
            client_state.as_deref(),
        );
    }

    // Redirect to client with authorization code
    let sep = if oauth_state.redirect_uri.contains('?') {
        "&"
    } else {
        "?"
    };
    let mut redirect_url = format!(
        "{}{}code={}",
        oauth_state.redirect_uri,
        sep,
        urlencoding::encode(&auth_code)
    );
    if let Some(cs) = client_state {
        redirect_url.push_str(&format!("&state={}", urlencoding::encode(&cs)));
    }

    Redirect::temporary(&redirect_url).into_response()
}

// --- Token Endpoint ---

#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    scope: String,
}

async fn token(
    State(state): State<OAuthState>,
    axum::Form(req): axum::Form<TokenRequest>,
) -> Response {
    match req.grant_type.as_str() {
        "authorization_code" => handle_authorization_code_grant(state, req).await,
        "refresh_token" => handle_refresh_token_grant(state, req).await,
        _ => token_error("unsupported_grant_type", "Unsupported grant_type"),
    }
}

async fn handle_authorization_code_grant(state: OAuthState, req: TokenRequest) -> Response {
    let code = match req.code {
        Some(c) => c,
        None => return token_error("invalid_request", "Missing code"),
    };
    let code_verifier = match req.code_verifier {
        Some(v) => v,
        None => return token_error("invalid_request", "Missing code_verifier (PKCE required)"),
    };
    let redirect_uri = match req.redirect_uri {
        Some(r) => r,
        None => return token_error("invalid_request", "Missing redirect_uri"),
    };
    let client_id = match req.client_id {
        Some(c) => c,
        None => return token_error("invalid_request", "Missing client_id"),
    };

    // Consume authorization code
    let auth_code = match state.db.consume_authorization_code(&code) {
        Ok(Some(ac)) => ac,
        _ => return token_error("invalid_grant", "Invalid or expired authorization code"),
    };

    // Validate client_id matches
    if auth_code.client_id != client_id {
        return token_error("invalid_grant", "client_id mismatch");
    }

    // Validate redirect_uri matches
    if auth_code.redirect_uri != redirect_uri {
        return token_error("invalid_grant", "redirect_uri mismatch");
    }

    // Validate PKCE: S256
    if !verify_pkce_s256(&auth_code.code_challenge, &code_verifier) {
        return token_error("invalid_grant", "PKCE verification failed");
    }

    // Validate client_secret if client uses client_secret_post
    if let Ok(Some(client)) = state.db.get_oauth_client(&client_id)
        && client.token_endpoint_auth_method == "client_secret_post"
    {
        match (&client.client_secret, &req.client_secret) {
            (Some(expected), Some(provided)) if expected == provided => {}
            _ => return token_error("invalid_client", "Invalid client_secret"),
        }
    }

    // Issue tokens
    let access_token = generate_random_token();
    let refresh_token = generate_random_token();

    if let Err(e) = state.db.create_oauth_token(
        &access_token,
        &refresh_token,
        &client_id,
        auth_code.user_id,
        &auth_code.scope,
        ACCESS_TOKEN_TTL,
        REFRESH_TOKEN_TTL,
    ) {
        tracing::error!("Failed to create token: {e:#}");
        return token_error("server_error", "Internal server error");
    }

    Json(TokenResponse {
        access_token,
        token_type: "Bearer".into(),
        expires_in: ACCESS_TOKEN_TTL,
        refresh_token: Some(refresh_token),
        scope: auth_code.scope,
    })
    .into_response()
}

async fn handle_refresh_token_grant(state: OAuthState, req: TokenRequest) -> Response {
    let old_refresh = match req.refresh_token {
        Some(r) => r,
        None => return token_error("invalid_request", "Missing refresh_token"),
    };

    let new_access = generate_random_token();
    let new_refresh = generate_random_token();

    match state.db.refresh_oauth_token(
        &old_refresh,
        &new_access,
        &new_refresh,
        ACCESS_TOKEN_TTL,
        REFRESH_TOKEN_TTL,
    ) {
        Ok(Some(refreshed)) => Json(TokenResponse {
            access_token: new_access,
            token_type: "Bearer".into(),
            expires_in: ACCESS_TOKEN_TTL,
            refresh_token: Some(new_refresh),
            scope: refreshed.scope,
        })
        .into_response(),
        Ok(None) => token_error("invalid_grant", "Invalid or expired refresh_token"),
        Err(e) => {
            tracing::error!("Refresh token error: {e:#}");
            token_error("server_error", "Internal server error")
        }
    }
}

// --- Helpers ---

fn generate_random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}

fn verify_pkce_s256(code_challenge: &str, code_verifier: &str) -> bool {
    let digest = Sha256::digest(code_verifier.as_bytes());
    let computed = BASE64_URL_SAFE_NO_PAD.encode(digest);
    computed == code_challenge
}

fn token_error(error: &str, description: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": error,
            "error_description": description
        })),
    )
        .into_response()
}

fn oauth_error_redirect(redirect_uri: &str, error: &str, state: Option<&str>) -> Response {
    let sep = if redirect_uri.contains('?') { "&" } else { "?" };
    let mut url = format!("{redirect_uri}{sep}error={error}");
    if let Some(s) = state {
        url.push_str(&format!("&state={}", urlencoding::encode(s)));
    }
    Redirect::temporary(&url).into_response()
}
