use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    ErrorData, GetPromptResult, PromptMessage, PromptMessageContent, PromptMessageRole,
};
use rmcp::prompt;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::server::MetsukeServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OnboardingArgs {
    #[schemars(description = "Organization name")]
    pub org: String,
    #[schemars(description = "Compliance goals: soc2, slsa, both (default: both)")]
    pub compliance_goals: Option<String>,
}

impl MetsukeServer {
    #[prompt(
        name = "onboarding",
        description = "Help an organization choose and configure the right policy preset"
    )]
    pub async fn onboarding(
        &self,
        Parameters(args): Parameters<OnboardingArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let goals = args.compliance_goals.unwrap_or_else(|| "both".into());
        Ok(GetPromptResult {
            description: Some(format!(
                "Policy onboarding for {} (goals: {})",
                args.org, goals
            )),
            messages: vec![PromptMessage {
                role: PromptMessageRole::User,
                content: PromptMessageContent::Text {
                    text: format!(
                        "Help me set up Metsuke for the '{}' organization.\n\
                         Our compliance goals: {}.\n\n\
                         Please:\n\
                         1. Use `list_policies` to show all available presets\n\
                         2. Use `policy_diff` to compare the most relevant presets for our goals\n\
                         3. Recommend the best preset with rationale\n\
                         4. Run a trial `verify_repo` on one of our repos to demonstrate\n\
                         5. Explain what each control finding means for our team",
                        args.org, goals
                    ),
                },
            }],
        })
    }
}
