use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::auth::REQUEST_USER_ID;
use crate::blocking::run_blocking;
use crate::db::Database;
use crate::github_app::GitHubApp;
use crate::validation::{validate_git_ref, validate_github_name, validate_policy};

#[derive(Clone)]
pub struct MetsukeServer {
    db: Arc<Database>,
    github_app: Arc<GitHubApp>,
    user_id: Option<i64>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
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

#[tool_router]
impl MetsukeServer {
    pub fn new(db: Arc<Database>, github_app: Arc<GitHubApp>) -> Self {
        // Capture user_id from task_local at factory time (before rmcp spawns session task)
        let user_id = REQUEST_USER_ID.try_with(|id| *id).ok();
        Self {
            db,
            github_app,
            user_id,
            tool_router: Self::tool_router(),
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
        let owner = args.owner;
        let repo = args.repo;
        let pr_number = args.pr_number;
        let policy = args.policy;
        let with_evidence = args.with_evidence;

        let result = match run_blocking(move || {
            let config = libverify_github::GitHubConfig {
                token,
                repo: format!("{owner}/{repo}"),
                host: "api.github.com".into(),
            };
            let client = libverify_github::GitHubClient::new(&config)?;
            libverify_github::verify_pr(
                &client,
                &owner,
                &repo,
                pr_number,
                policy.as_deref(),
                with_evidence,
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
                host: "api.github.com".into(),
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
        let owner = args.owner;
        let repo = args.repo;
        let reference = args.reference;
        let policy = args.policy;
        let with_evidence = args.with_evidence;

        let result = match run_blocking(move || {
            let config = libverify_github::GitHubConfig {
                token,
                repo: format!("{owner}/{repo}"),
                host: "api.github.com".into(),
            };
            let client = libverify_github::GitHubClient::new(&config)?;
            libverify_github::verify_repo(
                &client,
                &owner,
                &repo,
                &reference,
                policy.as_deref(),
                with_evidence,
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
