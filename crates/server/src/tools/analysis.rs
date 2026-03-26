use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use libverify_core::profile::GateDecision;

use crate::blocking::run_blocking;
use crate::server::MetsukeServer;
use crate::validation::{validate_github_name, validate_policy};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GapAnalysisArgs {
    #[schemars(description = "GitHub repository owner")]
    pub owner: String,
    #[schemars(description = "GitHub repository name")]
    pub repo: String,
    #[schemars(description = "Pull request number")]
    pub pr_number: u32,
    #[schemars(description = "Policy preset (default: soc2)")]
    pub policy: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompliancePostureArgs {
    #[schemars(description = "GitHub organization or owner")]
    pub owner: String,
    #[schemars(description = "List of repository names to assess")]
    pub repos: Vec<String>,
    #[schemars(description = "Git reference (default: HEAD)")]
    pub reference: Option<String>,
    #[schemars(description = "Policy preset (default: soc2)")]
    pub policy: Option<String>,
}

#[derive(Serialize)]
struct GapEntry {
    control_id: String,
    status: String,
    severity: String,
    decision: String,
    rationale: String,
    tsc_criteria: Vec<String>,
    remediation: String,
}

fn mcp_err(msg: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(msg.to_string(), None)
}

impl MetsukeServer {
    #[tool(
        description = "Analyze compliance gaps for a PR — returns only failures/reviews with remediation guidance"
    )]
    pub async fn gap_analysis(
        &self,
        Parameters(args): Parameters<GapAnalysisArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_github_name(&args.owner, "owner")?;
        validate_github_name(&args.repo, "repo")?;
        if let Some(ref p) = args.policy {
            validate_policy(p)?;
        }
        let token = self.github_token().map_err(mcp_err)?;
        let owner = args.owner;
        let repo = args.repo;
        let pr_number = args.pr_number;
        let policy = args.policy.unwrap_or_else(|| "soc2".into());

        let result = run_blocking(move || {
            let config = libverify_github::GitHubConfig {
                token,
                repo: format!("{owner}/{repo}"),
                host: "api.github.com".into(),
            };
            let client = libverify_github::GitHubClient::new(&config)?;
            libverify_github::verify_pr(&client, &owner, &repo, pr_number, Some(&policy), false)
        })
        .await
        .map_err(mcp_err)?;

        let registry = libverify_core::registry::ControlRegistry::builtin();
        let controls = registry.controls();

        let gaps: Vec<GapEntry> = result
            .report
            .outcomes
            .iter()
            .filter(|o| o.decision != GateDecision::Pass)
            .map(|o| {
                let control = controls.iter().find(|c| c.id() == o.control_id);
                let tsc = control
                    .map(|c| c.tsc_criteria().iter().map(|s| s.to_string()).collect())
                    .unwrap_or_default();
                let desc = control
                    .map(|c| c.description())
                    .unwrap_or("Unknown control");

                GapEntry {
                    control_id: o.control_id.to_string(),
                    status: format!("{:?}", o.severity),
                    severity: format!("{:?}", o.severity),
                    decision: format!("{:?}", o.decision),
                    rationale: o.rationale.clone(),
                    tsc_criteria: tsc,
                    remediation: format!(
                        "Control '{}': {}. Review the rationale and address the gap.",
                        o.control_id, desc
                    ),
                }
            })
            .collect();

        let json = serde_json::to_string_pretty(&gaps).map_err(mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Fleet-level compliance posture assessment across multiple repositories")]
    pub async fn compliance_posture(
        &self,
        Parameters(args): Parameters<CompliancePostureArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_github_name(&args.owner, "owner")?;
        for r in &args.repos {
            validate_github_name(r, "repos[]")?;
        }
        if let Some(ref p) = args.policy {
            validate_policy(p)?;
        }
        let token = self.github_token().map_err(mcp_err)?;
        let owner = args.owner;
        let repos = args.repos;
        let reference = args.reference.unwrap_or_else(|| "HEAD".into());
        let policy = args.policy.unwrap_or_else(|| "soc2".into());

        let result = run_blocking(move || {
            let mut results = Vec::new();
            for repo_name in &repos {
                let config = libverify_github::GitHubConfig {
                    token: token.clone(),
                    repo: format!("{owner}/{repo_name}"),
                    host: "api.github.com".into(),
                };
                let client = libverify_github::GitHubClient::new(&config)?;
                match libverify_github::verify_repo(
                    &client,
                    &owner,
                    repo_name,
                    &reference,
                    Some(&policy),
                    false,
                ) {
                    Ok(vr) => {
                        let pass = vr
                            .report
                            .outcomes
                            .iter()
                            .filter(|o| o.decision == GateDecision::Pass)
                            .count();
                        let fail = vr
                            .report
                            .outcomes
                            .iter()
                            .filter(|o| o.decision == GateDecision::Fail)
                            .count();
                        let review = vr
                            .report
                            .outcomes
                            .iter()
                            .filter(|o| o.decision == GateDecision::Review)
                            .count();
                        results.push(serde_json::json!({
                            "repo": format!("{owner}/{repo_name}"),
                            "pass": pass,
                            "review": review,
                            "fail": fail,
                            "overall": if fail > 0 { "fail" } else if review > 0 { "review" } else { "pass" },
                        }));
                    }
                    Err(e) => {
                        results.push(serde_json::json!({
                            "repo": format!("{owner}/{repo_name}"),
                            "error": format!("{e:#}"),
                        }));
                    }
                }
            }
            Ok(results)
        })
        .await
        .map_err(mcp_err)?;

        let json = serde_json::to_string_pretty(&result).map_err(mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
