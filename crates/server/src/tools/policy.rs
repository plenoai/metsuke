use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use libverify_core::control::{ControlFinding, ControlStatus, builtin};
use libverify_core::registry::ControlRegistry;
use libverify_policy::OpaProfile;

use crate::server::MetsukeServer;
use crate::validation::validate_policy;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PolicyDiffArgs {
    #[schemars(description = "First policy preset name")]
    pub policy_a: String,
    #[schemars(description = "Second policy preset name")]
    pub policy_b: String,
}

#[derive(Serialize)]
struct PolicyDiffEntry {
    control_id: &'static str,
    violated_a: String,
    violated_b: String,
    indeterminate_a: String,
    indeterminate_b: String,
}

fn mcp_err(msg: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(msg.to_string(), None)
}

impl MetsukeServer {
    #[tool(
        description = "Compare two policy presets showing how each control is treated differently"
    )]
    pub async fn policy_diff(
        &self,
        Parameters(args): Parameters<PolicyDiffArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_policy(&args.policy_a)?;
        validate_policy(&args.policy_b)?;
        let profile_a = OpaProfile::from_preset_or_file(&args.policy_a).map_err(mcp_err)?;
        let profile_b = OpaProfile::from_preset_or_file(&args.policy_b).map_err(mcp_err)?;

        let registry = ControlRegistry::builtin();
        let mut diffs = Vec::new();

        for control in registry.controls() {
            let id = control.id();

            let violated_finding = ControlFinding {
                control_id: id.clone(),
                status: ControlStatus::Violated,
                rationale: "synthetic".into(),
                subjects: vec![],
                evidence_gaps: vec![],
            };
            let indeterminate_finding = ControlFinding {
                control_id: id.clone(),
                status: ControlStatus::Indeterminate,
                rationale: "synthetic".into(),
                subjects: vec![],
                evidence_gaps: vec![],
            };

            use libverify_core::profile::ControlProfile;
            let va = profile_a.map(&violated_finding);
            let vb = profile_b.map(&violated_finding);
            let ia = profile_a.map(&indeterminate_finding);
            let ib = profile_b.map(&indeterminate_finding);

            if va.decision != vb.decision || ia.decision != ib.decision {
                diffs.push(PolicyDiffEntry {
                    control_id: builtin::ALL
                        .iter()
                        .find(|&&s| s == id.as_str())
                        .copied()
                        .unwrap_or("unknown"),
                    violated_a: format!("{:?}", va.decision),
                    violated_b: format!("{:?}", vb.decision),
                    indeterminate_a: format!("{:?}", ia.decision),
                    indeterminate_b: format!("{:?}", ib.decision),
                });
            }
        }

        let json = serde_json::to_string_pretty(&diffs).map_err(mcp_err)?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
