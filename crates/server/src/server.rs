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
    github_api_host: String,
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
    pub fn with_api_host(
        db: Arc<Database>,
        github_app: Arc<GitHubApp>,
        github_api_host: &str,
    ) -> Self {
        // Capture user_id from task_local at factory time (before rmcp spawns session task)
        let user_id = REQUEST_USER_ID.try_with(|id| *id).ok();
        Self {
            db,
            github_app,
            user_id,
            tool_router: Self::tool_router(),
            github_api_host: github_api_host.to_string(),
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
}
