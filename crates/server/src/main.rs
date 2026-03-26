mod blocking;
mod config;
mod server;
mod tools;

use config::AppConfig;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use server::MetsukeServer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
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

    let app = axum::Router::new()
        .nest_service("/mcp", service)
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
