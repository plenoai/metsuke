use std::sync::Arc;

use askama_web::WebTemplateExt;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::blocking::run_blocking;
use crate::db::Database;
use crate::validation;

use super::templates::ErrorTemplate;

pub(super) fn validate_repo_params(owner: &str, repo: &str) -> Option<Response> {
    if let Err(e) = validation::validate_github_name(owner, "owner") {
        return Some((axum::http::StatusCode::BAD_REQUEST, e.message).into_response());
    }
    if let Err(e) = validation::validate_github_name(repo, "repo") {
        return Some((axum::http::StatusCode::BAD_REQUEST, e.message).into_response());
    }
    None
}

pub(super) fn validate_policy_param(policy: Option<&str>) -> Option<Response> {
    if let Some(p) = policy
        && let Err(e) = validation::validate_policy(p)
    {
        return Some((axum::http::StatusCode::BAD_REQUEST, e.message).into_response());
    }
    None
}

pub(super) fn error_page(title: &str, message: &str) -> Response {
    ErrorTemplate {
        title: title.to_string(),
        message: message.to_string(),
    }
    .into_web_template()
    .into_response()
}

pub(super) fn get_session_from_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| c.trim().strip_prefix("session=").map(|s| s.to_string()))
}

pub(super) fn session_cookie(session_id: &str, max_age: i64) -> String {
    format!("session={session_id}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age}")
}

/// Consolidates session-cookie extraction + DB user lookup behind `run_blocking`.
pub(super) async fn require_user(db: &Arc<Database>, headers: &HeaderMap) -> Option<(i64, String)> {
    let session_id = get_session_from_cookie(headers)?;
    let db = db.clone();
    run_blocking(move || db.get_user_by_session(&session_id))
        .await
        .ok()
        .flatten()
}

pub(super) fn count_findings(json: &str) -> (i64, i64, i64, i64) {
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
