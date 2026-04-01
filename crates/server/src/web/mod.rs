mod audit;
mod auth;
mod events;
mod helpers;
mod jobs;
mod pages;
mod repos;
mod templates;
mod verify;

use std::sync::Arc;

use axum::Router;
use serde::Serialize;

use crate::config::AppConfig;
use crate::db::Database;
use crate::github_app::GitHubApp;

// ---------------------------------------------------------------------------
// Job events (broadcast to SSE clients)
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(tag = "type")]
pub enum JobEvent {
    #[serde(rename = "repos_synced")]
    ReposSynced { user_id: i64 },
    #[serde(rename = "pulls_synced")]
    PullsSynced {
        user_id: i64,
        owner: String,
        repo: String,
    },
    #[serde(rename = "releases_synced")]
    ReleasesSynced {
        user_id: i64,
        owner: String,
        repo: String,
    },
    #[serde(rename = "verification_complete")]
    VerificationComplete {
        user_id: i64,
        owner: String,
        repo: String,
    },
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct WebState {
    db: Arc<Database>,
    github_app: Arc<GitHubApp>,
    base_url: String,
    events_tx: tokio::sync::broadcast::Sender<JobEvent>,
    github_web_base_url: String,
    github_api_host: String,
}

// ---------------------------------------------------------------------------
// Policy options (replaces old POLICY_OPTIONS constant)
// ---------------------------------------------------------------------------

fn policy_options() -> Vec<&'static str> {
    libverify_policy::available_presets()
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(db: Arc<Database>, github_app: Arc<GitHubApp>, config: &AppConfig) -> Router {
    let (events_tx, _) = tokio::sync::broadcast::channel::<JobEvent>(256);
    let state = WebState {
        db,
        github_app: github_app.clone(),
        base_url: config.base_url.clone(),
        events_tx,
        github_web_base_url: github_app.web_base_url().to_string(),
        github_api_host: config.github_api_host.clone(),
    };

    Router::new()
        .route("/", axum::routing::get(auth::index))
        .route("/settings", axum::routing::get(auth::settings))
        .route("/auth/login", axum::routing::get(auth::login))
        .route("/auth/callback", axum::routing::get(auth::auth_callback))
        .route("/auth/logout", axum::routing::get(auth::logout))
        .route(
            "/auth/install/callback",
            axum::routing::get(auth::install_callback),
        )
        .route("/repos", axum::routing::get(repos::repos_page))
        .route(
            "/repos/{owner}/{repo}",
            axum::routing::get(repos::repo_detail_page),
        )
        .route(
            "/repos/{owner}/{repo}/releases",
            axum::routing::get(pages::verify_release_page),
        )
        .route(
            "/repos/{owner}/{repo}/pulls",
            axum::routing::get(pages::verify_pr_page),
        )
        .route("/audit", axum::routing::get(audit::audit_page))
        .route("/api/repos", axum::routing::get(repos::api_repos))
        .route(
            "/api/repos/{owner}/{repo}/verify",
            axum::routing::get(verify::api_get_latest_verification).post(verify::api_verify_repo),
        )
        .route(
            "/api/repos/{owner}/{repo}/releases",
            axum::routing::get(verify::api_list_releases),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-release",
            axum::routing::post(verify::api_verify_release),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-release/latest",
            axum::routing::get(verify::api_get_latest_release_verifications),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-release/latest/{target_ref}",
            axum::routing::get(verify::api_get_latest_release_verification_by_ref),
        )
        .route(
            "/api/repos/{owner}/{repo}/pulls",
            axum::routing::get(verify::api_list_pulls),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-pr/{pr_number}",
            axum::routing::post(verify::api_verify_pr),
        )
        .route(
            "/api/repos/{owner}/{repo}/verify-pr/{pr_number}/latest",
            axum::routing::get(verify::api_get_latest_pr_verification),
        )
        .route(
            "/api/repos/{owner}/{repo}/readme",
            axum::routing::get(repos::api_readme),
        )
        .route("/api/events", axum::routing::get(events::api_events))
        .route(
            "/api/audit-history",
            axum::routing::get(audit::api_audit_history),
        )
        .route(
            "/api/audit-history/export",
            axum::routing::get(audit::api_audit_export_csv),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::helpers::session_cookie;
    use super::policy_options;
    use super::templates::*;
    use askama::Template;

    #[test]
    fn error_template_renders() {
        let t = ErrorTemplate {
            title: "テスト".into(),
            message: "エラーメッセージ".into(),
        };
        let html = t.render().unwrap();
        assert!(html.contains("テスト"));
        assert!(html.contains("エラーメッセージ"));
        assert!(html.contains("style.css"));
    }

    #[test]
    fn settings_template_renders() {
        let t = SettingsTemplate {
            login: "testuser".into(),
            active_page: "settings",
            installations: vec![(1, "myorg".into(), "Organization".into())],
            base_url: "https://example.com".into(),
        };
        let html = t.render().unwrap();
        assert!(html.contains("testuser"));
        assert!(html.contains("myorg"));
        assert!(html.contains("Organization"));
        assert!(html.contains("https://example.com/mcp"));
    }

    #[test]
    fn repos_template_renders() {
        let t = ReposTemplate {
            login: "testuser".into(),
            active_page: "repos",
        };
        let html = t.render().unwrap();
        assert!(html.contains("Repositories"));
        assert!(html.contains("testuser"));
    }

    #[test]
    fn repo_detail_template_renders() {
        let t = RepoDetailTemplate {
            login: "testuser".into(),
            active_page: "repos",
            owner: "myorg".into(),
            repo: "myrepo".into(),
            policy_options: policy_options(),
        };
        let html = t.render().unwrap();
        assert!(html.contains("myorg"));
        assert!(html.contains("myrepo"));
        assert!(html.contains("リリース検証"));
        assert!(html.contains("PR検証"));
    }

    #[test]
    fn verify_pr_template_renders() {
        let t = VerifyPrTemplate {
            login: "testuser".into(),
            active_page: "repos",
            owner: "myorg".into(),
            repo: "myrepo".into(),
            policy_options: policy_options(),
        };
        let html = t.render().unwrap();
        assert!(html.contains("PR検証"));
        assert!(html.contains("pr-policy"));
        assert!(html.contains("Pull Request"));
    }

    #[test]
    fn verify_release_template_renders() {
        let t = VerifyReleaseTemplate {
            login: "testuser".into(),
            active_page: "repos",
            owner: "myorg".into(),
            repo: "myrepo".into(),
            policy_options: policy_options(),
        };
        let html = t.render().unwrap();
        assert!(html.contains("Release 検証"));
        assert!(html.contains("base-tag"));
        assert!(html.contains("head-tag"));
        assert!(html.contains("リリース一覧"));
    }

    #[test]
    fn audit_template_renders() {
        let t = AuditTemplate {
            login: "testuser".into(),
            active_page: "audit",
        };
        let html = t.render().unwrap();
        assert!(html.contains("監査ログ"));
        assert!(html.contains("filter-type"));
        assert!(html.contains(r#"nav-link active" href="/audit"#));
    }

    #[test]
    fn nav_highlights_active_page() {
        let settings = SettingsTemplate {
            login: "u".into(),
            active_page: "settings",
            installations: vec![],
            base_url: "https://x.com".into(),
        };
        let html = settings.render().unwrap();
        assert!(html.contains(r#"nav-link active" href="/settings"#));

        let repos = ReposTemplate {
            login: "u".into(),
            active_page: "repos",
        };
        let html = repos.render().unwrap();
        assert!(html.contains(r#"nav-link active" href="/repos"#));

        let audit = AuditTemplate {
            login: "u".into(),
            active_page: "audit",
        };
        let html = audit.render().unwrap();
        assert!(html.contains(r#"nav-link active" href="/audit"#));
    }

    #[test]
    fn policy_options_constant() {
        let options = libverify_policy::available_presets();
        assert!(options.contains(&"default"));
        assert!(options.contains(&"slsa-l4"));
        assert_eq!(options.len(), libverify_policy::available_presets().len());
    }

    #[test]
    fn session_cookie_format() {
        let cookie = session_cookie("abc123", 3600);
        assert!(cookie.contains("session=abc123"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Lax"));
    }
}
