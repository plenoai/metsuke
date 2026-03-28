mod auth;
mod blocking;
mod config;
mod db;
mod github_app;
mod oauth;
mod server;
mod swr_cache;
mod validation;
mod web;

use std::sync::Arc;

use auth::OAuthAuthLayer;
use config::AppConfig;
use db::Database;
use github_app::GitHubApp;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use server::MetsukeServer;
use tower::ServiceBuilder;
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

    let db = Arc::new(Database::open(&config.database_url)?);

    let github_app = Arc::new(GitHubApp::new(
        config.github_app_id,
        &config.github_app_private_key,
        config.github_app_client_id.clone(),
        config.github_app_client_secret.clone(),
    )?);

    let mcp_db = db.clone();
    let mcp_app = github_app.clone();
    let service = StreamableHttpService::new(
        move || Ok(MetsukeServer::new(mcp_db.clone(), mcp_app.clone())),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let authed_mcp = ServiceBuilder::new()
        .layer(OAuthAuthLayer::new(db.clone(), &config.base_url))
        .service(service);

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "crates/server/static".into());
    let app = axum::Router::new()
        .nest_service("/mcp", authed_mcp)
        .nest_service("/static", tower_http::services::ServeDir::new(&static_dir))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .merge(oauth::router(db.clone(), github_app.clone(), &config))
        .merge(web::router(db, github_app, &config));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("metsuke listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}
