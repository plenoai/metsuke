use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    ErrorData, GetPromptResult, PromptMessage, PromptMessageContent, PromptMessageRole,
};
use rmcp::prompt;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::server::MetsukeServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrReviewArgs {
    #[schemars(description = "GitHub repository owner")]
    pub owner: String,
    #[schemars(description = "GitHub repository name")]
    pub repo: String,
    #[schemars(description = "Pull request number")]
    pub pr_number: u32,
}

impl MetsukeServer {
    #[prompt(
        name = "pr-review",
        description = "Guided PR quality and compliance review"
    )]
    pub async fn pr_review(
        &self,
        Parameters(args): Parameters<PrReviewArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        Ok(GetPromptResult {
            description: Some(format!(
                "PR review for {}/{}#{}",
                args.owner, args.repo, args.pr_number
            )),
            messages: vec![PromptMessage {
                role: PromptMessageRole::User,
                content: PromptMessageContent::Text {
                    text: format!(
                        "Please review PR #{} in {}/{}.\n\n\
                         Steps:\n\
                         1. Use `verify_pr` with policy='default' to get full assessment\n\
                         2. Use `gap_analysis` to identify failures and reviews\n\
                         3. For each gap, explain what's wrong and how to fix it\n\
                         4. Provide an overall summary: is this PR ready to merge?",
                        args.pr_number, args.owner, args.repo
                    ),
                },
            }],
        })
    }
}
