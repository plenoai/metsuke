use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    ErrorData, GetPromptResult, PromptMessage, PromptMessageContent, PromptMessageRole,
};
use rmcp::prompt;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::server::MetsukeServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ComplianceAuditArgs {
    #[schemars(description = "Target framework: soc2, slsa-l2, etc.")]
    pub framework: String,
    #[schemars(description = "GitHub repository owner")]
    pub owner: String,
    #[schemars(description = "GitHub repository name")]
    pub repo: String,
}

impl MetsukeServer {
    #[prompt(
        name = "compliance-audit",
        description = "Guided compliance audit workflow for SOC2, SLSA, or other frameworks"
    )]
    pub async fn compliance_audit(
        &self,
        Parameters(args): Parameters<ComplianceAuditArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        Ok(GetPromptResult {
            description: Some(format!(
                "Compliance audit for {}/{} against {}",
                args.owner, args.repo, args.framework
            )),
            messages: vec![PromptMessage {
                role: PromptMessageRole::User,
                content: PromptMessageContent::Text {
                    text: format!(
                        "I need a {} compliance audit for the repository {}/{}.\n\n\
                             Please:\n\
                             1. Use the `list_controls` tool to see all available controls\n\
                             2. Use the `verify_repo` tool with policy='{}' to assess the repository\n\
                             3. Use the `gap_analysis` tool on recent PRs to identify specific gaps\n\
                             4. Summarize findings by TSC criteria and provide remediation steps\n\
                             5. Rate the overall compliance posture (pass/review/fail)",
                        args.framework, args.owner, args.repo, args.framework
                    ),
                },
            }],
        })
    }
}
