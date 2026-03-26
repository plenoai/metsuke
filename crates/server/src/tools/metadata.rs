use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use libverify_core::control::builtin;
use libverify_core::registry::ControlRegistry;

use crate::server::MetsukeServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListControlsArgs {
    #[schemars(description = "Filter: slsa, compliance, or all (default: all)")]
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExplainControlArgs {
    #[schemars(description = "Control ID (e.g. review-independence)")]
    pub control_id: String,
}

#[derive(Serialize)]
struct ControlInfo {
    id: String,
    description: &'static str,
    tsc_criteria: &'static [&'static str],
}

impl MetsukeServer {
    #[tool(description = "List all built-in SDLC controls with descriptions and TSC criteria")]
    pub async fn list_controls(
        &self,
        Parameters(_args): Parameters<ListControlsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let registry = ControlRegistry::builtin();
        let controls: Vec<ControlInfo> = registry
            .controls()
            .iter()
            .map(|c| ControlInfo {
                id: c.id().to_string(),
                description: c.description(),
                tsc_criteria: c.tsc_criteria(),
            })
            .collect();

        let json = serde_json::to_string_pretty(&controls)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Get detailed explanation of a specific SDLC control")]
    pub async fn explain_control(
        &self,
        Parameters(args): Parameters<ExplainControlArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let registry = ControlRegistry::builtin();
        let control = registry
            .controls()
            .iter()
            .find(|c| c.id().as_str() == args.control_id)
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    format!(
                        "Unknown control: {}. Available: {:?}",
                        args.control_id,
                        builtin::ALL
                    ),
                    None,
                )
            })?;

        let info = ControlInfo {
            id: control.id().to_string(),
            description: control.description(),
            tsc_criteria: control.tsc_criteria(),
        };

        let json = serde_json::to_string_pretty(&info)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "List all available policy presets")]
    pub async fn list_policies(&self) -> Result<CallToolResult, ErrorData> {
        let presets = [
            (
                "default",
                "All controls strict — indeterminate/violated fail",
            ),
            ("oss", "Tolerates unsigned commits and self-reviewed merges"),
            (
                "aiops",
                "Escalates all indeterminate to review instead of fail",
            ),
            (
                "soc1",
                "Strict on ICFR-relevant controls; advisory on dev quality",
            ),
            (
                "soc2",
                "Strict on CC6/CC7/CC8; review on build-track indeterminate",
            ),
            (
                "slsa-l1",
                "SLSA Level 1 enforcement (Source + Build + Dependencies)",
            ),
            ("slsa-l2", "SLSA Level 2 enforcement"),
            ("slsa-l3", "SLSA Level 3 enforcement"),
            ("slsa-l4", "SLSA Level 4 enforcement"),
        ];

        let list: Vec<serde_json::Value> = presets
            .iter()
            .map(|(name, desc)| {
                serde_json::json!({
                    "name": name,
                    "description": desc,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&list)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
