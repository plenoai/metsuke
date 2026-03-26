use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, prompt_handler, prompt_router, tool_handler, tool_router};

use crate::config::AppConfig;

#[derive(Clone)]
pub struct MetsukeServer {
    pub(crate) config: AppConfig,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    #[allow(dead_code)]
    prompt_router: PromptRouter<Self>,
}

#[tool_router]
#[prompt_router]
impl MetsukeServer {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    pub fn github_token(&self) -> anyhow::Result<String> {
        self.config
            .github_token
            .clone()
            .ok_or_else(|| anyhow::anyhow!("GH_TOKEN not configured"))
    }
}

#[tool_handler]
#[prompt_handler]
impl ServerHandler for MetsukeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
            server_info: Implementation {
                name: "metsuke".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(
                "Metsuke (目付) — SDLC process inspector. \
                 Provides compliance assessment, gap analysis, and policy management \
                 for GitHub repositories powered by libverify."
                    .into(),
            ),
        }
    }
}
