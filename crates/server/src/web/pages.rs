use askama_web::WebTemplateExt;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};

use super::WebState;
use super::helpers::*;
use super::policy_options;
use super::templates::{VerifyPrTemplate, VerifyReleaseTemplate};

pub(super) async fn verify_pr_page(
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
        policy_options: policy_options(),
    }
    .into_web_template()
    .into_response()
}

pub(super) async fn verify_release_page(
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
        policy_options: policy_options(),
    }
    .into_web_template()
    .into_response()
}
