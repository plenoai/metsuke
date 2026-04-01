use askama_web::WebTemplateExt;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Json, Redirect, Response};
use serde::Deserialize;

use crate::blocking::run_blocking;

use super::WebState;
use super::helpers::*;
use super::templates::AuditTemplate;

#[derive(Deserialize)]
pub(super) struct AuditHistoryQuery {
    #[serde(rename = "type")]
    verification_type: Option<String>,
    owner: Option<String>,
    repo: Option<String>,
    from_date: Option<String>,
    to_date: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

pub(super) async fn api_audit_history(
    headers: HeaderMap,
    Query(query): Query<AuditHistoryQuery>,
    State(state): State<WebState>,
) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let vtype = query.verification_type.clone();
    let qowner = query.owner.clone();
    let qrepo = query.repo.clone();
    let from = query.from_date.clone();
    let to = query.to_date.clone();
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    let entries = run_blocking(move || {
        db.get_audit_history(
            user_id,
            vtype.as_deref(),
            qowner.as_deref(),
            qrepo.as_deref(),
            from.as_deref(),
            to.as_deref(),
            limit,
            offset,
        )
    })
    .await
    .unwrap_or_default();

    let json: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "type": e.verification_type,
                "owner": e.owner,
                "repo": e.repo,
                "target_ref": e.target_ref,
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

pub(super) async fn api_audit_export_csv(
    headers: HeaderMap,
    Query(query): Query<AuditHistoryQuery>,
    State(state): State<WebState>,
) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let db = state.db.clone();
    let vtype = query.verification_type.clone();
    let qowner = query.owner.clone();
    let qrepo = query.repo.clone();
    let from = query.from_date.clone();
    let to = query.to_date.clone();
    let entries = run_blocking(move || {
        db.get_audit_history(
            user_id,
            vtype.as_deref(),
            qowner.as_deref(),
            qrepo.as_deref(),
            from.as_deref(),
            to.as_deref(),
            10000,
            0,
        )
    })
    .await
    .unwrap_or_default();

    let mut csv = String::from("Date,Type,Owner,Repo,Target,Policy,Pass,Fail,Review,N/A\n");
    for e in &entries {
        csv.push_str(&format!(
            "\"{}\",\"{}\",\"{}\",\"{}\",\"{}\",\"{}\",{},{},{},{}\n",
            e.verified_at,
            e.verification_type,
            e.owner,
            e.repo,
            e.target_ref,
            e.policy,
            e.pass_count,
            e.fail_count,
            e.review_count,
            e.na_count,
        ));
    }

    (
        [
            (axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"metsuke-audit.csv\"",
            ),
        ],
        csv,
    )
        .into_response()
}

pub(super) async fn audit_page(headers: HeaderMap, State(state): State<WebState>) -> Response {
    let (_user_id, login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return Redirect::to("/").into_response(),
    };

    AuditTemplate {
        login,
        active_page: "audit",
    }
    .into_web_template()
    .into_response()
}
