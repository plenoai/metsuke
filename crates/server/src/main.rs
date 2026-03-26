mod blocking;
mod config;
mod github_app;
mod prompts;
mod server;
mod tools;
mod validation;

use config::AppConfig;
use github_app::webhook::{WebhookState, handle_webhook};
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use server::MetsukeServer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = AppConfig::from_env()?;
    let addr = config.bind_address();

    let mcp_config = config.clone();
    let service = StreamableHttpService::new(
        move || Ok(MetsukeServer::new(mcp_config.clone())),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let webhook_state = WebhookState {
        secret: config.github_webhook_secret.clone().unwrap_or_default(),
    };

    let app = axum::Router::new()
        .nest_service("/mcp", service)
        .route(
            "/webhook",
            axum::routing::post(handle_webhook).with_state(webhook_state),
        )
        .route("/health", axum::routing::get(|| async { "ok" }));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("metsuke listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}
