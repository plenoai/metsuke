use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;

use crate::blocking::run_blocking;
use crate::validation::validate_git_ref;

use super::JobEvent;
use super::WebState;
use super::helpers::*;
use super::jobs::{spawn_sync_pulls_job, spawn_sync_releases_job};

#[derive(Deserialize)]
pub(super) struct VerifyQuery {
    policy: Option<String>,
}

pub(super) async fn api_get_latest_verification(
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

pub(super) async fn api_verify_repo(
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
        libverify_github::verify_repo(
            &client,
            &owner_c,
            &repo_c,
            "HEAD",
            policy.as_deref(),
            false,
            vec![],
        )
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
                        "manual",
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

pub(super) async fn api_list_pulls(
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

pub(super) async fn api_list_releases(
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

#[derive(Deserialize)]
pub(super) struct VerifyReleaseQuery {
    base_tag: String,
    head_tag: String,
    policy: Option<String>,
}

pub(super) async fn api_verify_release(
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
            vec![],
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
                        "manual",
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

pub(super) async fn api_verify_pr(
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
            vec![],
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
                        "manual",
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

pub(super) async fn api_get_latest_pr_verification(
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

pub(super) async fn api_get_latest_release_verification_by_ref(
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

pub(super) async fn api_get_latest_release_verifications(
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
                .map(|s| {
                    serde_json::json!({
                        "target_ref": s.target_ref,
                        "pass": s.pass_count,
                        "fail": s.fail_count,
                        "review": s.review_count,
                        "na": s.na_count,
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
