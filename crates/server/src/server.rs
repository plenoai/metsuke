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
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

fn mcp_err(msg: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(msg.to_string(), None)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyPrArgs {
    #[schemars(description = "GitHub repository owner")]
    pub owner: String,
    #[schemars(description = "GitHub repository name")]
    pub repo: String,
    #[schemars(description = "Pull request number")]
    pub pr_number: u32,
    #[schemars(description = "Policy preset (default, oss, aiops, soc1, soc2, slsa-l1..l4)")]
    pub policy: Option<String>,
    #[schemars(description = "Include raw evidence bundle in output")]
    #[serde(default)]
    pub with_evidence: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyReleaseArgs {
    #[schemars(description = "GitHub repository owner")]
    pub owner: String,
    #[schemars(description = "GitHub repository name")]
    pub repo: String,
    #[schemars(description = "Base tag (e.g. v1.0.0)")]
    pub base_tag: String,
    #[schemars(description = "Head tag (e.g. v1.1.0)")]
    pub head_tag: String,
    #[schemars(description = "Policy preset")]
    pub policy: Option<String>,
    #[serde(default)]
    pub with_evidence: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyRepoArgs {
    #[schemars(description = "GitHub repository owner")]
    pub owner: String,
    #[schemars(description = "GitHub repository name")]
    pub repo: String,
    #[schemars(description = "Git reference (branch, tag, or SHA)")]
    #[serde(default = "default_ref")]
    pub reference: String,
    #[schemars(description = "Policy preset")]
    pub policy: Option<String>,
    #[serde(default)]
    pub with_evidence: bool,
}

fn default_ref() -> String {
    "HEAD".into()
}

#[tool_router]
impl MetsukeServer {
    pub fn new(db: Arc<Database>, github_app: Arc<GitHubApp>) -> Self {
        Self {
            db,
            github_app,
            tool_router: Self::tool_router(),
        }
    }

    async fn get_github_token(&self, owner: &str) -> anyhow::Result<String> {
        let user_id = REQUEST_USER_ID
            .try_with(|id| *id)
            .map_err(|_| anyhow::anyhow!("No authenticated user"))?;
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

    #[tool(description = "Verify a pull request against SDLC controls and a policy preset")]
    pub async fn verify_pr(
        &self,
        Parameters(args): Parameters<VerifyPrArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_github_name(&args.owner, "owner")?;
        validate_github_name(&args.repo, "repo")?;
        if let Some(ref p) = args.policy {
            validate_policy(p)?;
        }
        let token = self.get_github_token(&args.owner).await.map_err(mcp_err)?;
        let owner = args.owner;
        let repo = args.repo;
        let pr_number = args.pr_number;
        let policy = args.policy;
        let with_evidence = args.with_evidence;

        let result = run_blocking(move || {
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
        .map_err(mcp_err)?;

        let json = serde_json::to_string_pretty(&result).map_err(mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Verify a release tag range against SDLC controls")]
    pub async fn verify_release(
        &self,
        Parameters(args): Parameters<VerifyReleaseArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_github_name(&args.owner, "owner")?;
        validate_github_name(&args.repo, "repo")?;
        if let Some(ref p) = args.policy {
            validate_policy(p)?;
        }
        let token = self
            .get_github_token(&args.owner)
            .await
            .map_err(mcp_err)?;
        let owner = args.owner;
        let repo = args.repo;
        let base_tag = args.base_tag;
        let head_tag = args.head_tag;
        let policy = args.policy;
        let with_evidence = args.with_evidence;

        let result = run_blocking(move || {
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
        .map_err(mcp_err)?;

        let json = serde_json::to_string_pretty(&result).map_err(mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Verify repository dependency signatures at a git reference")]
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
        let token = self
            .get_github_token(&args.owner)
            .await
            .map_err(mcp_err)?;
        let owner = args.owner;
        let repo = args.repo;
        let reference = args.reference;
        let policy = args.policy;
        let with_evidence = args.with_evidence;

        let result = run_blocking(move || {
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
        .map_err(mcp_err)?;

        let json = serde_json::to_string_pretty(&result).map_err(mcp_err)?;
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
