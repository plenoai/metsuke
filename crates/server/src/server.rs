use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Semaphore;

use crate::auth::REQUEST_USER_ID;
use crate::blocking::run_blocking;
use crate::bulk::{
    BulkJob, BulkJobStatus, BulkJobStore, BulkTarget, BulkTargetResult, MAX_CONCURRENCY,
    MAX_TARGETS,
};
use crate::db::Database;
use crate::github_app::GitHubApp;
use crate::validation::{validate_git_ref, validate_github_name, validate_policy};
use crate::web::helpers::count_findings;

#[derive(Clone)]
pub struct MetsukeServer {
    db: Arc<Database>,
    github_app: Arc<GitHubApp>,
    user_id: Option<i64>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    github_api_host: String,
    bulk_jobs: BulkJobStore,
}

fn tool_error(msg: impl std::fmt::Display) -> CallToolResult {
    let mut result = CallToolResult::success(vec![Content::text(msg.to_string())]);
    result.is_error = Some(true);
    result
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyPrArgs {
    #[schemars(description = "GitHub repository owner (e.g. 'octocat')")]
    pub owner: String,
    #[schemars(description = "GitHub repository name (e.g. 'hello-world')")]
    pub repo: String,
    #[schemars(description = "Pull request number to verify")]
    pub pr_number: u32,
    #[schemars(description = "Policy preset that selects which controls to enforce. \
        default=basic SDLC hygiene, oss=open-source best practices, \
        soc2=SOC 2 compliance controls, slsa-l1..l4=SLSA supply-chain levels 1-4, \
        aiops=AI/ML operations controls, soc1=SOC 1 controls. \
        Omit to use 'default'.")]
    pub policy: Option<String>,
    #[schemars(
        description = "When true, includes raw GitHub API responses in the output \
        as an evidence bundle. Significantly increases response size. \
        Useful for audit trails and debugging false positives."
    )]
    #[serde(default)]
    pub with_evidence: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyReleaseArgs {
    #[schemars(description = "GitHub repository owner (e.g. 'octocat')")]
    pub owner: String,
    #[schemars(description = "GitHub repository name (e.g. 'hello-world')")]
    pub repo: String,
    #[schemars(description = "Base tag — the older release to compare from (e.g. 'v1.0.0')")]
    pub base_tag: String,
    #[schemars(description = "Head tag — the newer release to compare to (e.g. 'v1.1.0')")]
    pub head_tag: String,
    #[schemars(description = "Policy preset that selects which controls to enforce. \
        default=basic SDLC hygiene, oss=open-source best practices, \
        soc2=SOC 2 compliance controls, slsa-l1..l4=SLSA supply-chain levels 1-4, \
        aiops=AI/ML operations controls, soc1=SOC 1 controls. \
        Omit to use 'default'.")]
    pub policy: Option<String>,
    #[schemars(
        description = "When true, includes raw GitHub API responses in the output \
        as an evidence bundle. Significantly increases response size. \
        Useful for audit trails and debugging false positives."
    )]
    #[serde(default)]
    pub with_evidence: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyRepoArgs {
    #[schemars(description = "GitHub repository owner (e.g. 'octocat')")]
    pub owner: String,
    #[schemars(description = "GitHub repository name (e.g. 'hello-world')")]
    pub repo: String,
    #[schemars(
        description = "Git reference to verify at — branch name, tag, or commit SHA. \
        Defaults to HEAD."
    )]
    #[serde(default = "default_ref")]
    pub reference: String,
    #[schemars(description = "Policy preset that selects which controls to enforce. \
        default=basic SDLC hygiene, oss=open-source best practices, \
        soc2=SOC 2 compliance controls, slsa-l1..l4=SLSA supply-chain levels 1-4, \
        aiops=AI/ML operations controls, soc1=SOC 1 controls. \
        Omit to use 'default'.")]
    pub policy: Option<String>,
    #[schemars(
        description = "When true, includes raw GitHub API responses in the output \
        as an evidence bundle. Significantly increases response size. \
        Useful for audit trails and debugging false positives."
    )]
    #[serde(default)]
    pub with_evidence: bool,
}

fn default_ref() -> String {
    "HEAD".into()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BulkVerifyArgs {
    #[schemars(
        description = "Array of verification targets. Each target must have a 'type' field \
        ('repo', 'pr', or 'release') plus the fields for that type. \
        Max 50 targets per request."
    )]
    pub targets: Vec<BulkTarget>,
    #[schemars(description = "Policy preset applied to all targets. \
        default=basic SDLC hygiene, oss=open-source best practices, \
        soc2=SOC 2 compliance controls, slsa-l1..l4=SLSA supply-chain levels 1-4, \
        aiops=AI/ML operations controls, soc1=SOC 1 controls. \
        Omit to use 'default'.")]
    pub policy: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BulkVerifyStatusArgs {
    #[schemars(description = "Job ID returned by bulk_verify")]
    pub job_id: String,
}

#[tool_router]
impl MetsukeServer {
    pub fn with_api_host(
        db: Arc<Database>,
        github_app: Arc<GitHubApp>,
        github_api_host: &str,
        bulk_jobs: BulkJobStore,
    ) -> Self {
        // Capture user_id from task_local at factory time (before rmcp spawns session task)
        let user_id = REQUEST_USER_ID.try_with(|id| *id).ok();
        Self {
            db,
            github_app,
            user_id,
            tool_router: Self::tool_router(),
            github_api_host: github_api_host.to_string(),
            bulk_jobs,
        }
    }

    async fn get_github_token(&self, owner: &str) -> anyhow::Result<String> {
        let user_id = self
            .user_id
            .ok_or_else(|| anyhow::anyhow!("No authenticated user"))?;
        let installation_id = self
            .db
            .get_installation_for_owner(user_id, owner)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No GitHub App installation found for '{owner}'. Install the app at https://github.com/apps/pleno-metsuke"
                )
            })?;
        self.github_app
            .create_installation_token(installation_id)
            .await
    }

    #[tool(
        description = "Verify a pull request against SDLC controls including code review approval, \
        CI status checks, commit signing, and branch protection. Returns JSON with a top-level \
        verdict and a findings array where each entry has control_id, status \
        (satisfied/violated/indeterminate/not_applicable), and rationale fields. \
        Use the policy parameter to select which controls are enforced."
    )]
    pub async fn verify_pr(
        &self,
        Parameters(args): Parameters<VerifyPrArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_github_name(&args.owner, "owner")?;
        validate_github_name(&args.repo, "repo")?;
        if let Some(ref p) = args.policy {
            validate_policy(p)?;
        }
        let token = match self.get_github_token(&args.owner).await {
            Ok(t) => t,
            Err(e) => return Ok(tool_error(e)),
        };
        let api_host = self.github_api_host.clone();
        let owner = args.owner;
        let repo = args.repo;
        let pr_number = args.pr_number;
        let policy = args.policy;
        let with_evidence = args.with_evidence;

        let result = match run_blocking(move || {
            let config = libverify_github::GitHubConfig {
                token,
                repo: format!("{owner}/{repo}"),
                host: api_host,
            };
            let client = libverify_github::GitHubClient::new(&config)?;
            libverify_github::verify_pr(
                &client,
                &owner,
                &repo,
                pr_number,
                policy.as_deref(),
                with_evidence,
                vec![],
            )
        })
        .await
        {
            Ok(r) => r,
            Err(e) => return Ok(tool_error(e)),
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Verify changes between two release tags against SDLC controls. \
        Evaluates all commits and PRs in the base_tag..head_tag range for code review, \
        CI status, commit signing, and release integrity. Returns JSON with a top-level \
        verdict and a findings array where each entry has control_id, status \
        (satisfied/violated/indeterminate/not_applicable), and rationale fields. \
        Use the policy parameter to select which controls are enforced."
    )]
    pub async fn verify_release(
        &self,
        Parameters(args): Parameters<VerifyReleaseArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_github_name(&args.owner, "owner")?;
        validate_github_name(&args.repo, "repo")?;
        if let Some(ref p) = args.policy {
            validate_policy(p)?;
        }
        let token = match self.get_github_token(&args.owner).await {
            Ok(t) => t,
            Err(e) => return Ok(tool_error(e)),
        };
        let api_host = self.github_api_host.clone();
        let owner = args.owner;
        let repo = args.repo;
        let base_tag = args.base_tag;
        let head_tag = args.head_tag;
        let policy = args.policy;
        let with_evidence = args.with_evidence;

        let result = match run_blocking(move || {
            let config = libverify_github::GitHubConfig {
                token,
                repo: format!("{owner}/{repo}"),
                host: api_host,
            };
            let client = libverify_github::GitHubClient::new(&config)?;
            libverify_github::verify_release(
                &client,
                &owner,
                &repo,
                &base_tag,
                &head_tag,
                policy.as_deref(),
                with_evidence,
                vec![],
            )
        })
        .await
        {
            Ok(r) => r,
            Err(e) => return Ok(tool_error(e)),
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Verify repository-level SDLC controls at a specific git reference, \
        including branch protection rules, dependency signature verification, and \
        security policy presence. Returns JSON with a top-level verdict and a findings \
        array where each entry has control_id, status \
        (satisfied/violated/indeterminate/not_applicable), and rationale fields. \
        Use the policy parameter to select which controls are enforced."
    )]
    pub async fn verify_repo(
        &self,
        Parameters(args): Parameters<VerifyRepoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_github_name(&args.owner, "owner")?;
        validate_github_name(&args.repo, "repo")?;
        validate_git_ref(&args.reference)?;
        if let Some(ref p) = args.policy {
            validate_policy(p)?;
        }
        let token = match self.get_github_token(&args.owner).await {
            Ok(t) => t,
            Err(e) => return Ok(tool_error(e)),
        };
        let api_host = self.github_api_host.clone();
        let owner = args.owner;
        let repo = args.repo;
        let reference = args.reference;
        let policy = args.policy;
        let with_evidence = args.with_evidence;

        let result = match run_blocking(move || {
            let config = libverify_github::GitHubConfig {
                token,
                repo: format!("{owner}/{repo}"),
                host: api_host,
            };
            let client = libverify_github::GitHubClient::new(&config)?;
            libverify_github::verify_repo(
                &client,
                &owner,
                &repo,
                &reference,
                policy.as_deref(),
                with_evidence,
                vec![],
            )
        })
        .await
        {
            Ok(r) => r,
            Err(e) => return Ok(tool_error(e)),
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Submit a batch of verification targets (repos, PRs, releases) for \
        concurrent processing. Returns a job_id immediately. Use bulk_verify_status to \
        poll for results. Max 50 targets per request. Each target is verified independently \
        with up to 4 concurrent verifications. Results are stored in the audit log."
    )]
    pub async fn bulk_verify(
        &self,
        Parameters(args): Parameters<BulkVerifyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.targets.is_empty() {
            return Err(ErrorData::invalid_params("targets must not be empty", None));
        }
        if args.targets.len() > MAX_TARGETS {
            return Err(ErrorData::invalid_params(
                format!("too many targets (max {MAX_TARGETS})"),
                None,
            ));
        }
        if let Some(ref p) = args.policy {
            validate_policy(p)?;
        }
        for target in &args.targets {
            validate_github_name(target.owner(), "owner")?;
            validate_github_name(target.repo(), "repo")?;
            if let BulkTarget::Release {
                base_tag, head_tag, ..
            } = target
            {
                validate_git_ref(base_tag)?;
                validate_git_ref(head_tag)?;
            }
        }

        let user_id = self
            .user_id
            .ok_or_else(|| ErrorData::internal_error("No authenticated user", None))?;

        let job_id = uuid::Uuid::new_v4().to_string();
        let total = args.targets.len();

        // Create job entry
        {
            let mut jobs = self.bulk_jobs.write().await;
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
        let bulk_jobs = self.bulk_jobs.clone();
        let db = self.db.clone();
        let github_app = self.github_app.clone();
        let api_host = self.github_api_host.clone();
        let targets = args.targets;
        let policy = args.policy;

        tokio::spawn(async move {
            let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENCY));
            let mut handles = Vec::with_capacity(targets.len());

            for target in targets {
                let permit = semaphore.clone().acquire_owned().await.unwrap();
                let db = db.clone();
                let github_app = github_app.clone();
                let api_host = api_host.clone();
                let policy = policy.clone();
                let bulk_jobs = bulk_jobs.clone();
                let job_id = job_id_bg.clone();

                let handle = tokio::spawn(async move {
                    let result = mcp_verify_single(
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
                                    "mcp",
                                )
                            })
                            .await;
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

            for handle in handles {
                let _ = handle.await;
            }
        });

        let response = serde_json::json!({ "job_id": job_id });
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap(),
        )]))
    }

    #[tool(
        description = "Check the status and results of a bulk verification job. \
        Returns the job status (running/completed), progress (completed/total), \
        and any results collected so far."
    )]
    pub async fn bulk_verify_status(
        &self,
        Parameters(args): Parameters<BulkVerifyStatusArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let jobs = self.bulk_jobs.read().await;
        match jobs.get(&args.job_id) {
            Some(job) => {
                let json = serde_json::to_string_pretty(job)
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            None => Ok(tool_error(format!("Job '{}' not found", args.job_id))),
        }
    }
}

/// Run a single verification for MCP bulk jobs.
async fn mcp_verify_single(
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
            BulkTarget::Repo { owner, repo } => libverify_github::verify_repo(
                &client,
                owner,
                repo,
                "HEAD",
                policy_ref,
                false,
                vec![],
            ),
            BulkTarget::Pr {
                owner,
                repo,
                pr_number,
            } => libverify_github::verify_pr(
                &client,
                owner,
                repo,
                *pr_number,
                policy_ref,
                false,
                vec![],
            ),
            BulkTarget::Release {
                owner,
                repo,
                base_tag,
                head_tag,
            } => libverify_github::verify_release(
                &client,
                owner,
                repo,
                base_tag,
                head_tag,
                policy_ref,
                false,
                vec![],
            ),
        }?;

        serde_json::to_string(&result).map_err(|e| anyhow::anyhow!(e))
    })
    .await?;

    let json_value: serde_json::Value = serde_json::from_str(&json_str)?;
    Ok((json_value, json_str))
}

#[tool_handler]
impl ServerHandler for MetsukeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "metsuke".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "Metsuke (目付) — SDLC process inspector powered by libverify.".into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_error_sets_is_error_flag() {
        let result = tool_error("something went wrong");
        assert_eq!(result.is_error, Some(true));
        assert!(!result.content.is_empty());
    }

    #[test]
    fn verify_pr_args_deserializes_minimal() {
        let json = serde_json::json!({
            "owner": "octocat",
            "repo": "hello-world",
            "pr_number": 42
        });
        let args: VerifyPrArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.owner, "octocat");
        assert_eq!(args.repo, "hello-world");
        assert_eq!(args.pr_number, 42);
        assert_eq!(args.policy, None);
        assert!(!args.with_evidence);
    }

    #[test]
    fn verify_pr_args_deserializes_full() {
        let json = serde_json::json!({
            "owner": "org",
            "repo": "repo",
            "pr_number": 1,
            "policy": "soc2",
            "with_evidence": true
        });
        let args: VerifyPrArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.policy, Some("soc2".into()));
        assert!(args.with_evidence);
    }

    #[test]
    fn verify_release_args_deserializes_minimal() {
        let json = serde_json::json!({
            "owner": "org",
            "repo": "repo",
            "base_tag": "v1.0.0",
            "head_tag": "v1.1.0"
        });
        let args: VerifyReleaseArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.base_tag, "v1.0.0");
        assert_eq!(args.head_tag, "v1.1.0");
        assert_eq!(args.policy, None);
        assert!(!args.with_evidence);
    }

    #[test]
    fn verify_repo_args_defaults_reference_to_head() {
        let json = serde_json::json!({
            "owner": "org",
            "repo": "repo"
        });
        let args: VerifyRepoArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.reference, "HEAD");
        assert_eq!(args.policy, None);
        assert!(!args.with_evidence);
    }

    #[test]
    fn verify_repo_args_custom_reference() {
        let json = serde_json::json!({
            "owner": "org",
            "repo": "repo",
            "reference": "refs/heads/main",
            "policy": "slsa-l2",
            "with_evidence": true
        });
        let args: VerifyRepoArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.reference, "refs/heads/main");
        assert_eq!(args.policy, Some("slsa-l2".into()));
        assert!(args.with_evidence);
    }

    #[test]
    fn verify_pr_args_rejects_missing_required() {
        let json = serde_json::json!({"owner": "org"});
        assert!(serde_json::from_value::<VerifyPrArgs>(json).is_err());
    }

    #[test]
    fn bulk_verify_args_deserializes_mixed_targets() {
        let json = serde_json::json!({
            "targets": [
                {"type": "repo", "owner": "org", "repo": "r1"},
                {"type": "pr", "owner": "org", "repo": "r2", "pr_number": 10},
                {"type": "release", "owner": "org", "repo": "r3", "base_tag": "v1.0", "head_tag": "v1.1"}
            ],
            "policy": "soc2"
        });
        let args: BulkVerifyArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.targets.len(), 3);
        assert_eq!(args.policy, Some("soc2".into()));
    }

    #[test]
    fn bulk_verify_args_defaults_policy_to_none() {
        let json = serde_json::json!({
            "targets": [{"type": "repo", "owner": "o", "repo": "r"}]
        });
        let args: BulkVerifyArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.policy, None);
    }

    #[test]
    fn bulk_verify_args_rejects_empty_targets() {
        let json = serde_json::json!({"targets": []});
        let args: BulkVerifyArgs = serde_json::from_value(json).unwrap();
        assert!(args.targets.is_empty());
    }

    #[test]
    fn bulk_target_methods() {
        let repo = BulkTarget::Repo {
            owner: "o".into(),
            repo: "r".into(),
        };
        assert_eq!(repo.owner(), "o");
        assert_eq!(repo.repo(), "r");
        assert_eq!(repo.verification_type(), "repo");
        assert_eq!(repo.target_ref(), "HEAD");

        let pr = BulkTarget::Pr {
            owner: "o".into(),
            repo: "r".into(),
            pr_number: 42,
        };
        assert_eq!(pr.verification_type(), "pr");
        assert_eq!(pr.target_ref(), "#42");

        let release = BulkTarget::Release {
            owner: "o".into(),
            repo: "r".into(),
            base_tag: "v1".into(),
            head_tag: "v2".into(),
        };
        assert_eq!(release.verification_type(), "release");
        assert_eq!(release.target_ref(), "v1..v2");
    }

    #[test]
    fn bulk_verify_status_args_deserializes() {
        let json = serde_json::json!({"job_id": "abc-123"});
        let args: BulkVerifyStatusArgs = serde_json::from_value(json).unwrap();
        assert_eq!(args.job_id, "abc-123");
    }
}
