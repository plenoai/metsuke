use askama_web::WebTemplateExt;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Json, Redirect, Response};
use serde::Serialize;

use crate::blocking::run_blocking;

use super::WebState;
use super::helpers::*;
use super::jobs::spawn_sync_repos_job;
use super::policy_options;
use super::templates::{RepoDetailTemplate, ReposTemplate};

#[derive(Serialize)]
struct ComplianceEntry {
    owner: String,
    repo: String,
    pass: i64,
    fail: i64,
    review: i64,
}

pub(super) async fn api_repos(headers: HeaderMap, State(state): State<WebState>) -> Response {
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

/// Returns the latest repo-level compliance summary for all repos.
pub(super) async fn api_compliance(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let summaries: Vec<crate::db::RepoComplianceSummary> =
        run_blocking(move || db.get_all_repo_compliance(user_id))
            .await
            .unwrap_or_default();

    let entries: Vec<ComplianceEntry> = summaries
        .into_iter()
        .map(|s| ComplianceEntry {
            owner: s.owner,
            repo: s.repo,
            pass: s.pass_count,
            fail: s.fail_count,
            review: s.review_count,
        })
        .collect();

    Json(entries).into_response()
}

pub(super) async fn sync_installations(state: &WebState, user_id: i64) {
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

pub(super) async fn repos_page(headers: HeaderMap, State(state): State<WebState>) -> Response {
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

pub(super) async fn repo_detail_page(
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
        policy_options: policy_options(),
    }
    .into_web_template()
    .into_response()
}

pub(super) async fn api_readme(
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

    let url = format!(
        "https://{}/repos/{owner}/{repo}/readme",
        state.github_api_host
    );
    let client = reqwest::Client::new();

    let resp = fetch_readme(&client, &url, Some(&token)).await;

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

async fn fetch_readme(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut req = client
        .get(url)
        .header("Accept", "application/vnd.github.html+json")
        .header("User-Agent", "metsuke")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    req.send().await
}
