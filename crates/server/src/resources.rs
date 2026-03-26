use libverify_core::control::builtin;
use libverify_core::registry::ControlRegistry;
use rmcp::model::*;
use serde::Serialize;

const CONTROLS_URI: &str = "metsuke://controls";
const POLICIES_URI: &str = "metsuke://policies";

#[derive(Serialize)]
struct ControlDoc {
    id: String,
    description: &'static str,
    tsc_criteria: &'static [&'static str],
}

struct PolicyDoc {
    name: &'static str,
    description: &'static str,
}

const POLICY_PRESETS: &[PolicyDoc] = &[
    PolicyDoc {
        name: "default",
        description: "All controls strict — indeterminate/violated fail",
    },
    PolicyDoc {
        name: "oss",
        description: "Tolerates unsigned commits and self-reviewed merges",
    },
    PolicyDoc {
        name: "aiops",
        description: "Escalates all indeterminate to review instead of fail",
    },
    PolicyDoc {
        name: "soc1",
        description: "Strict on ICFR-relevant controls; advisory on dev quality",
    },
    PolicyDoc {
        name: "soc2",
        description: "Strict on CC6/CC7/CC8; review on build-track indeterminate",
    },
    PolicyDoc {
        name: "slsa-l1",
        description: "SLSA Level 1 enforcement (Source + Build + Dependencies)",
    },
    PolicyDoc {
        name: "slsa-l2",
        description: "SLSA Level 2 enforcement",
    },
    PolicyDoc {
        name: "slsa-l3",
        description: "SLSA Level 3 enforcement",
    },
    PolicyDoc {
        name: "slsa-l4",
        description: "SLSA Level 4 enforcement",
    },
];

fn make_resource(uri: &str, name: &str, description: &str) -> Resource {
    Annotated::new(
        RawResource {
            uri: uri.into(),
            name: name.into(),
            title: None,
            description: Some(description.into()),
            mime_type: Some("application/json".into()),
            size: None,
            icons: None,
        },
        None,
    )
}

fn make_template(uri_template: &str, name: &str, description: &str) -> ResourceTemplate {
    Annotated::new(
        RawResourceTemplate {
            uri_template: uri_template.into(),
            name: name.into(),
            title: None,
            description: Some(description.into()),
            mime_type: Some("application/json".into()),
        },
        None,
    )
}

pub fn list_resources() -> ListResourcesResult {
    ListResourcesResult {
        resources: vec![
            make_resource(
                CONTROLS_URI,
                "controls",
                "All 28 built-in SDLC controls with descriptions",
            ),
            make_resource(
                POLICIES_URI,
                "policies",
                "All 9 policy presets with descriptions",
            ),
        ],
        next_cursor: None,
    }
}

pub fn list_resource_templates() -> ListResourceTemplatesResult {
    ListResourceTemplatesResult {
        resource_templates: vec![
            make_template(
                "metsuke://controls/{control_id}",
                "control",
                "Detailed documentation for a specific control",
            ),
            make_template(
                "metsuke://policies/{preset_name}",
                "policy",
                "Detailed documentation for a specific policy preset",
            ),
        ],
        next_cursor: None,
    }
}

pub fn read_resource(uri: &str) -> Result<ReadResourceResult, ErrorData> {
    if uri == CONTROLS_URI {
        let registry = ControlRegistry::builtin();
        let controls: Vec<ControlDoc> = registry
            .controls()
            .iter()
            .map(|c| ControlDoc {
                id: c.id().to_string(),
                description: c.description(),
                tsc_criteria: c.tsc_criteria(),
            })
            .collect();
        let json = serde_json::to_string_pretty(&controls)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        return Ok(ReadResourceResult {
            contents: vec![ResourceContents::text(json, CONTROLS_URI)],
        });
    }

    if uri == POLICIES_URI {
        let list: Vec<serde_json::Value> = POLICY_PRESETS
            .iter()
            .map(|p| serde_json::json!({"name": p.name, "description": p.description}))
            .collect();
        let json = serde_json::to_string_pretty(&list)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        return Ok(ReadResourceResult {
            contents: vec![ResourceContents::text(json, POLICIES_URI)],
        });
    }

    if let Some(id) = uri.strip_prefix("metsuke://controls/") {
        let registry = ControlRegistry::builtin();
        let control = registry
            .controls()
            .iter()
            .find(|c| c.id().as_str() == id)
            .ok_or_else(|| {
                ErrorData::invalid_params(
                    format!("Unknown control: {id}. Available: {:?}", builtin::ALL),
                    None,
                )
            })?;
        let doc = ControlDoc {
            id: control.id().to_string(),
            description: control.description(),
            tsc_criteria: control.tsc_criteria(),
        };
        let json = serde_json::to_string_pretty(&doc)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        return Ok(ReadResourceResult {
            contents: vec![ResourceContents::text(json, uri)],
        });
    }

    if let Some(name) = uri.strip_prefix("metsuke://policies/") {
        let preset = POLICY_PRESETS
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| ErrorData::invalid_params(format!("Unknown policy: {name}"), None))?;
        let json = serde_json::to_string_pretty(
            &serde_json::json!({"name": preset.name, "description": preset.description}),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        return Ok(ReadResourceResult {
            contents: vec![ResourceContents::text(json, uri)],
        });
    }

    Err(ErrorData::invalid_params(
        format!("Unknown resource URI: {uri}"),
        None,
    ))
}
