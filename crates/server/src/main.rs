mod auth;
mod blocking;
mod config;
mod db;
mod github_app;
mod oauth;
mod server;
mod validation;
mod web;
mod webhook;

use std::sync::Arc;

use auth::OAuthAuthLayer;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use config::AppConfig;
use db::Database;
use github_app::GitHubApp;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use server::MetsukeServer;
use tower::ServiceBuilder;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
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

    let github_app = Arc::new(GitHubApp::with_hosts(
        config.github_app_id,
        &config.github_app_private_key,
        config.github_app_client_id.clone(),
        config.github_app_client_secret.clone(),
        &config.github_api_host,
        &config.github_web_host,
    )?);

    let mcp_db = db.clone();
    let mcp_app = github_app.clone();
    let mcp_api_host = config.github_api_host.clone();
    let service = StreamableHttpService::new(
        move || {
            Ok(MetsukeServer::with_api_host(
                mcp_db.clone(),
                mcp_app.clone(),
                &mcp_api_host,
            ))
        },
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let authed_mcp = ServiceBuilder::new()
        .layer(OAuthAuthLayer::new(db.clone(), &config.base_url))
        .service(service);

    // Periodic expired session cleanup (hourly)
    {
        let db = db.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                let db = db.clone();
                match crate::blocking::run_blocking(move || db.cleanup_expired()).await {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!("Cleaned up {count} expired records");
                        }
                    }
                    Err(e) => tracing::warn!("Cleanup failed: {e:#}"),
                }
            }
        });
    }

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "crates/server/static".into());
    let app = axum::Router::new()
        .nest_service("/mcp", authed_mcp)
        .nest_service(
            "/static",
            ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=86400"),
                ))
                .service(tower_http::services::ServeDir::new(&static_dir)),
        )
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/healthz", axum::routing::get(healthz))
        .with_state(db.clone())
        .merge(oauth::router(db.clone(), github_app.clone(), &config))
        .merge(webhook::router(db.clone(), github_app.clone(), &config))
        .merge(web::router(db, github_app, &config))
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            axum::http::HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::REFERRER_POLICY,
            axum::http::HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderValue::from_static(
                "default-src 'self'; script-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; style-src 'self' https://fonts.googleapis.com https://cdn.jsdelivr.net; font-src https://fonts.gstatic.com; connect-src 'self' https://cdn.jsdelivr.net; img-src 'self' data: https://raw.githubusercontent.com https://github.com https://user-images.githubusercontent.com https://private-user-images.githubusercontent.com https://avatars.githubusercontent.com https://img.shields.io https://github.githubassets.com https://camo.githubusercontent.com; frame-ancestors 'none'"
            ),
        ));

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("metsuke listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;

    Ok(())
}

async fn healthz(State(db): State<Arc<Database>>) -> impl IntoResponse {
    match db.ping() {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
            })),
        ),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "degraded",
                "error": format!("{e}"),
                "version": env!("CARGO_PKG_VERSION"),
            })),
        ),
    }
}
