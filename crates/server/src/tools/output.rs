use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::tool;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::server::MetsukeServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FormatSarifArgs {
    #[schemars(
        description = "JSON string of a VerificationResult (output from verify_pr/release/repo)"
    )]
    pub result_json: String,
}

impl MetsukeServer {
    #[tool(description = "Convert a verification result JSON to SARIF 2.1.0 format")]
    pub async fn format_sarif(
        &self,
        Parameters(args): Parameters<FormatSarifArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let result: libverify_core::assessment::VerificationResult =
            serde_json::from_str(&args.result_json).map_err(|e| {
                ErrorData::invalid_params(format!("Invalid VerificationResult JSON: {e}"), None)
            })?;

        let opts = libverify_output::OutputOptions {
            format: libverify_output::Format::Sarif,
            only_failures: false,
            tool_name: "metsuke".into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        };

        let sarif = libverify_output::render(&opts, &result)
            .map_err(|e| ErrorData::internal_error(format!("SARIF rendering failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(sarif)]))
    }
}
