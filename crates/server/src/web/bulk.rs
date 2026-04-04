use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use tokio::sync::Semaphore;

use crate::blocking::run_blocking;
use crate::bulk::{
    BulkJob, BulkJobStatus, BulkTarget, BulkTargetResult, MAX_CONCURRENCY, MAX_TARGETS,
};
use crate::db::Database;
use crate::github_app::GitHubApp;
use crate::validation::{validate_git_ref, validate_github_name, validate_policy};

use super::helpers::*;
use super::{JobEvent, WebState};

#[derive(Deserialize)]
pub(super) struct BulkVerifyRequest {
    targets: Vec<BulkTarget>,
    #[serde(default)]
    policy: Option<String>,
}

pub(super) async fn api_bulk_verify(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<BulkVerifyRequest>,
) -> Response {
    let (user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    if body.targets.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "targets must not be empty",
        )
            .into_response();
    }
    if body.targets.len() > MAX_TARGETS {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            format!("too many targets (max {MAX_TARGETS})"),
        )
            .into_response();
    }

    if let Some(ref p) = body.policy
        && let Err(e) = validate_policy(p)
    {
        return (axum::http::StatusCode::BAD_REQUEST, e.message).into_response();
    }

    // Validate all targets upfront
    for target in &body.targets {
        if let Err(e) = validate_github_name(target.owner(), "owner") {
            return (axum::http::StatusCode::BAD_REQUEST, e.message).into_response();
        }
        if let Err(e) = validate_github_name(target.repo(), "repo") {
            return (axum::http::StatusCode::BAD_REQUEST, e.message).into_response();
        }
        if let BulkTarget::Release {
            base_tag, head_tag, ..
        } = target
        {
            if let Err(e) = validate_git_ref(base_tag) {
                return (axum::http::StatusCode::BAD_REQUEST, e.message).into_response();
            }
            if let Err(e) = validate_git_ref(head_tag) {
                return (axum::http::StatusCode::BAD_REQUEST, e.message).into_response();
            }
        }
    }

    let job_id = uuid::Uuid::new_v4().to_string();
    let total = body.targets.len();

    // Create job entry
    {
        let mut jobs = state.bulk_jobs.write().await;
        jobs.insert(
            job_id.clone(),
            BulkJob {
                id: job_id.clone(),
                status: BulkJobStatus::Running,
                total,
                completed: 0,
                results: Vec::with_capacity(total),
            },
        );
    }

    // Spawn background job
    let job_id_bg = job_id.clone();
    let bulk_jobs = state.bulk_jobs.clone();
    let db = state.db.clone();
    let github_app = state.github_app.clone();
    let api_host = state.github_api_host.clone();
    let events_tx = state.events_tx.clone();
    let policy = body.policy;
    let targets = body.targets;

    let ctx = BulkJobContext {
        job_id: job_id_bg,
        bulk_jobs,
        db,
        github_app,
        api_host,
        events_tx,
        user_id,
        policy,
    };
    tokio::spawn(async move {
        run_bulk_job(ctx, targets).await;
    });

    (
        axum::http::StatusCode::ACCEPTED,
        Json(serde_json::json!({ "job_id": job_id })),
    )
        .into_response()
}

pub(super) async fn api_bulk_verify_status(
    headers: HeaderMap,
    Path(job_id): Path<String>,
    State(state): State<WebState>,
) -> Response {
    let (_user_id, _login) = match require_user(&state.db, &headers).await {
        Some(u) => u,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
    };

    let jobs = state.bulk_jobs.read().await;
    match jobs.get(&job_id) {
        Some(job) => Json(job.clone()).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "Job not found").into_response(),
    }
}

struct BulkJobContext {
    job_id: String,
    bulk_jobs: crate::bulk::BulkJobStore,
    db: Arc<Database>,
    github_app: Arc<GitHubApp>,
    api_host: String,
    events_tx: tokio::sync::broadcast::Sender<JobEvent>,
    user_id: i64,
    policy: Option<String>,
}

async fn run_bulk_job(ctx: BulkJobContext, targets: Vec<BulkTarget>) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENCY));
    let mut handles = Vec::with_capacity(targets.len());

    for target in targets {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let db = ctx.db.clone();
        let github_app = ctx.github_app.clone();
        let api_host = ctx.api_host.clone();
        let events_tx = ctx.events_tx.clone();
        let policy = ctx.policy.clone();
        let bulk_jobs = ctx.bulk_jobs.clone();
        let job_id = ctx.job_id.clone();
        let user_id = ctx.user_id;

        let handle = tokio::spawn(async move {
            let result = verify_single_target(
                &db,
                &github_app,
                &api_host,
                user_id,
                &target,
                policy.clone(),
            )
            .await;

            let target_result = match result {
                Ok((json_value, json_str)) => {
                    let (pass, fail, review, na) = count_findings(&json_str);
                    let policy_str = policy.as_deref().unwrap_or("default").to_string();
                    let v_type = target.verification_type().to_string();
                    let owner = target.owner().to_string();
                    let repo = target.repo().to_string();
                    let target_ref = target.target_ref();
                    let db_c = db.clone();
                    let _ = run_blocking(move || {
                        db_c.append_audit_entry(
                            user_id,
                            &v_type,
                            &owner,
                            &repo,
                            &target_ref,
                            &policy_str,
                            pass,
                            fail,
                            review,
                            na,
                            &json_str,
                            "api",
                        )
                    })
                    .await;
                    let _ = events_tx.send(JobEvent::VerificationComplete {
                        user_id,
                        owner: target.owner().to_string(),
                        repo: target.repo().to_string(),
                    });

                    BulkTargetResult {
                        target,
                        result: Some(json_value),
                        error: None,
                    }
                }
                Err(e) => BulkTargetResult {
                    target,
                    result: None,
                    error: Some(e.to_string()),
                },
            };

            {
                let mut jobs = bulk_jobs.write().await;
                if let Some(job) = jobs.get_mut(&job_id) {
                    job.completed += 1;
                    job.results.push(target_result);
                    if job.completed == job.total {
                        job.status = BulkJobStatus::Completed;
                    }
                }
            }

            drop(permit);
        });

        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        let _ = handle.await;
    }
}

async fn verify_single_target(
    db: &Arc<Database>,
    github_app: &Arc<GitHubApp>,
    api_host: &str,
    user_id: i64,
    target: &BulkTarget,
    policy: Option<String>,
) -> anyhow::Result<(serde_json::Value, String)> {
    let owner = target.owner().to_string();
    let db_c = db.clone();
    let owner_q = owner.clone();
    let installation_id = run_blocking(move || db_c.get_installation_for_owner(user_id, &owner_q))
        .await?
        .ok_or_else(|| anyhow::anyhow!("No installation found for '{owner}'"))?;

    let token = github_app
        .create_installation_token(installation_id)
        .await?;

    let target = target.clone();
    let api_host = api_host.to_string();

    let json_str = run_blocking(move || {
        let config = libverify_github::GitHubConfig {
            token,
            repo: format!("{}/{}", target.owner(), target.repo()),
            host: api_host,
        };
        let client = libverify_github::GitHubClient::new(&config)?;

        let policy_ref = policy.as_deref();
        let result = match &target {
            BulkTarget::Repo { owner, repo } => {
                libverify_github::verify_repo(&client, owner, repo, "HEAD", policy_ref, false)
            }
            BulkTarget::Pr {
                owner,
                repo,
                pr_number,
            } => libverify_github::verify_pr(&client, owner, repo, *pr_number, policy_ref, false),
            BulkTarget::Release {
                owner,
                repo,
                base_tag,
                head_tag,
            } => libverify_github::verify_release(
                &client, owner, repo, base_tag, head_tag, policy_ref, false,
            ),
        }?;

        serde_json::to_string(&result).map_err(|e| anyhow::anyhow!(e))
    })
    .await?;

    let json_value: serde_json::Value = serde_json::from_str(&json_str)?;
    Ok((json_value, json_str))
}
