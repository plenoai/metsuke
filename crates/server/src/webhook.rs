use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::blocking::run_blocking;
use crate::config::AppConfig;
use crate::db::Database;
use crate::github_app::GitHubApp;

type HmacSha256 = Hmac<Sha256>;

/// Maximum number of delivery IDs to keep in the dedup cache.
const MAX_DELIVERY_IDS: usize = 1000;

#[derive(Clone)]
struct WebhookState {
    github_app: Arc<GitHubApp>,
    webhook_secret: Option<String>,
    delivery_ids: Arc<Mutex<HashSet<String>>>,
}

pub fn router(_db: Arc<Database>, github_app: Arc<GitHubApp>, config: &AppConfig) -> Router {
    let state = WebhookState {
        github_app,
        webhook_secret: config.github_webhook_secret.clone(),
        delivery_ids: Arc::new(Mutex::new(HashSet::new())),
    };

    Router::new()
        .route("/webhook", axum::routing::post(handle_webhook))
        .with_state(state)
}

fn verify_signature(secret: &str, body: &[u8], signature_header: &str) -> bool {
    let Some(hex_sig) = signature_header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_sig) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

async fn handle_webhook(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // Deduplicate by X-GitHub-Delivery header
    if let Some(delivery_id) = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
    {
        let mut ids = state.delivery_ids.lock().unwrap();
        if ids.contains(delivery_id) {
            tracing::debug!("webhook: duplicate delivery {delivery_id}, skipping");
            return StatusCode::OK;
        }
        // Cap the set size to avoid unbounded memory growth
        if ids.len() >= MAX_DELIVERY_IDS {
            ids.clear();
        }
        ids.insert(delivery_id.to_string());
    }

    // Verify webhook signature if secret is configured
    if let Some(ref secret) = state.webhook_secret {
        let sig = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !verify_signature(secret, &body, sig) {
            tracing::warn!("webhook signature verification failed");
            return StatusCode::UNAUTHORIZED;
        }
    }

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("webhook: invalid JSON payload: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    let delivery_id = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    match event {
        "pull_request" => {
            tracing::info!(delivery_id, event, "webhook: dispatching event");
            tokio::spawn(handle_pull_request(state, payload));
        }
        "release" => {
            tracing::info!(delivery_id, event, "webhook: dispatching event");
            tokio::spawn(handle_release(state, payload));
        }
        "ping" => {
            tracing::info!(delivery_id, "webhook: ping received");
        }
        _ => {
            tracing::debug!(delivery_id, event, "webhook: ignoring event");
        }
    }

    StatusCode::OK
}

async fn handle_pull_request(state: WebhookState, payload: serde_json::Value) {
    let action = payload["action"].as_str().unwrap_or("");
    if !matches!(action, "opened" | "synchronize" | "reopened") {
        tracing::debug!(action, "webhook: skipping pull_request action");
        return;
    }

    let owner = match payload["repository"]["owner"]["login"].as_str() {
        Some(s) => s.to_string(),
        None => return,
    };
    let repo = match payload["repository"]["name"].as_str() {
        Some(s) => s.to_string(),
        None => return,
    };
    let pr_number = match payload["pull_request"]["number"].as_u64() {
        Some(n) => n as u32,
        None => return,
    };
    let head_sha = match payload["pull_request"]["head"]["sha"].as_str() {
        Some(s) => s.to_string(),
        None => return,
    };
    let installation_id = match payload["installation"]["id"].as_i64() {
        Some(id) => id,
        None => {
            tracing::warn!("webhook: pull_request event missing installation.id");
            return;
        }
    };

    tracing::info!(
        %owner, %repo, pr_number, sha = &head_sha[..7.min(head_sha.len())],
        "webhook: processing PR"
    );

    let token = match state
        .github_app
        .create_installation_token(installation_id)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("webhook: failed to create installation token: {e:#}");
            return;
        }
    };

    let verify_owner = owner.clone();
    let verify_repo = repo.clone();
    let verify_token = token;
    let result = match run_blocking(move || {
        let config = libverify_github::GitHubConfig {
            token: verify_token,
            repo: format!("{verify_owner}/{verify_repo}"),
            host: "api.github.com".into(),
        };
        let client = libverify_github::GitHubClient::new(&config)?;
        libverify_github::verify_pr(&client, &verify_owner, &verify_repo, pr_number, None, false)
    })
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("webhook: verify_pr failed for {owner}/{repo}#{pr_number}: {e:#}");
            let _ = state
                .github_app
                .create_check_run(
                    installation_id,
                    &owner,
                    &repo,
                    &head_sha,
                    "metsuke / verify-pr",
                    "failure",
                    "Verification Error",
                    &format!("Failed to run verification: {e}"),
                )
                .await;
            return;
        }
    };

    let result_json = serde_json::to_string_pretty(&result).unwrap_or_default();
    let (conclusion, title, summary) = format_check_result(&result_json, "PR");

    match state
        .github_app
        .create_check_run(
            installation_id,
            &owner,
            &repo,
            &head_sha,
            "metsuke / verify-pr",
            &conclusion,
            &title,
            &summary,
        )
        .await
    {
        Ok(_) => {
            tracing::info!(%owner, %repo, pr_number, %conclusion, "webhook: check run created")
        }
        Err(e) => {
            tracing::error!(%owner, %repo, pr_number, "webhook: failed to create check run: {e:#}")
        }
    }
}

async fn handle_release(state: WebhookState, payload: serde_json::Value) {
    let action = payload["action"].as_str().unwrap_or("");
    if action != "published" {
        tracing::debug!(action, "webhook: skipping release action");
        return;
    }

    let owner = match payload["repository"]["owner"]["login"].as_str() {
        Some(s) => s.to_string(),
        None => return,
    };
    let repo = match payload["repository"]["name"].as_str() {
        Some(s) => s.to_string(),
        None => return,
    };
    let tag_name = match payload["release"]["tag_name"].as_str() {
        Some(s) => s.to_string(),
        None => return,
    };
    let installation_id = match payload["installation"]["id"].as_i64() {
        Some(id) => id,
        None => {
            tracing::warn!("webhook: release event missing installation.id");
            return;
        }
    };

    tracing::info!(%owner, %repo, %tag_name, "webhook: processing release");

    let token = match state
        .github_app
        .create_installation_token(installation_id)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("webhook: failed to create installation token: {e:#}");
            return;
        }
    };

    let verify_owner = owner.clone();
    let verify_repo = repo.clone();
    let verify_tag = tag_name.clone();
    let verify_token = token;
    let result = match run_blocking(move || {
        let config = libverify_github::GitHubConfig {
            token: verify_token,
            repo: format!("{verify_owner}/{verify_repo}"),
            host: "api.github.com".into(),
        };
        let client = libverify_github::GitHubClient::new(&config)?;
        libverify_github::verify_repo(
            &client,
            &verify_owner,
            &verify_repo,
            &verify_tag,
            None,
            false,
        )
    })
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("webhook: verify_repo failed for {owner}/{repo}@{tag_name}: {e:#}");
            return;
        }
    };

    let result_json = serde_json::to_string_pretty(&result).unwrap_or_default();
    let (_conclusion, _title, summary) = format_check_result(&result_json, "Release");

    tracing::info!(%owner, %repo, %tag_name, %summary, "webhook: release verification complete");
}

/// Parse verification result JSON and produce check run conclusion + summary.
fn format_check_result(result_json: &str, scope: &str) -> (String, String, String) {
    let parsed: serde_json::Value =
        serde_json::from_str(result_json).unwrap_or(serde_json::Value::Null);

    let pass = parsed["pass_count"].as_u64().unwrap_or(0);
    let fail = parsed["fail_count"].as_u64().unwrap_or(0);
    let review = parsed["review_count"].as_u64().unwrap_or(0);
    let na = parsed["na_count"].as_u64().unwrap_or(0);

    let conclusion = if fail > 0 { "failure" } else { "success" };
    let title = format!("{scope}: {pass} pass, {fail} fail, {review} review, {na} N/A");

    let mut summary = title.clone();

    // Append failed controls if present
    if let Some(controls) = parsed["controls"].as_array() {
        let failed: Vec<&str> = controls
            .iter()
            .filter(|c| c["result"].as_str() == Some("fail"))
            .filter_map(|c| c["id"].as_str())
            .collect();
        if !failed.is_empty() {
            summary.push_str("\n\n**Failed controls:**\n");
            for id in &failed {
                summary.push_str(&format!("- `{id}`\n"));
            }
        }
    }

    (conclusion.to_string(), title, summary)
}

/// Decode hex string to bytes (avoids adding `hex` crate).
mod hex {
    pub fn decode(s: &str) -> Result<Vec<u8>, ()> {
        if !s.len().is_multiple_of(2) {
            return Err(());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_signature_valid() {
        let secret = "test-secret";
        let body = b"hello world";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize().into_bytes();
        let sig = format!(
            "sha256={}",
            result
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        assert!(verify_signature(secret, body, &sig));
    }

    #[test]
    fn test_verify_signature_invalid() {
        assert!(!verify_signature("secret", b"body", "sha256=deadbeef"));
    }

    #[test]
    fn test_verify_signature_bad_prefix() {
        assert!(!verify_signature("secret", b"body", "sha1=abc"));
    }

    #[test]
    fn test_format_check_result_pass() {
        let json = r#"{"pass_count": 10, "fail_count": 0, "review_count": 2, "na_count": 1}"#;
        let (conclusion, title, _summary) = format_check_result(json, "PR");
        assert_eq!(conclusion, "success");
        assert!(title.contains("10 pass"));
    }

    #[test]
    fn test_format_check_result_fail() {
        let json = r#"{"pass_count": 8, "fail_count": 2, "review_count": 0, "na_count": 0, "controls": [{"id": "SLSA-SRC-001", "result": "fail"}, {"id": "SLSA-SRC-002", "result": "pass"}]}"#;
        let (conclusion, _title, summary) = format_check_result(json, "PR");
        assert_eq!(conclusion, "failure");
        assert!(summary.contains("SLSA-SRC-001"));
        assert!(!summary.contains("SLSA-SRC-002"));
    }

    #[test]
    fn test_hex_decode() {
        assert_eq!(
            hex::decode("deadbeef").unwrap(),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
        assert!(hex::decode("xyz").is_err());
    }

    #[test]
    fn test_hex_decode_empty() {
        assert_eq!(hex::decode("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn test_hex_decode_odd_length() {
        assert!(hex::decode("abc").is_err());
    }

    #[test]
    fn test_hex_decode_uppercase() {
        assert_eq!(
            hex::decode("DEADBEEF").unwrap(),
            vec![0xde, 0xad, 0xbe, 0xef]
        );
    }

    #[test]
    fn test_format_check_result_empty_json() {
        let (conclusion, title, _) = format_check_result("{}", "PR");
        assert_eq!(conclusion, "success"); // no failures → success
        assert!(title.contains("0 pass"));
        assert!(title.contains("0 fail"));
    }

    #[test]
    fn test_format_check_result_invalid_json() {
        let (conclusion, title, _) = format_check_result("not json", "PR");
        assert_eq!(conclusion, "success"); // defaults to 0 failures
        assert!(title.contains("0 pass"));
    }

    #[test]
    fn test_format_check_result_multiple_failed_controls() {
        let json = r#"{
            "pass_count": 1, "fail_count": 3, "review_count": 0, "na_count": 0,
            "controls": [
                {"id": "CTL-001", "result": "fail"},
                {"id": "CTL-002", "result": "fail"},
                {"id": "CTL-003", "result": "pass"},
                {"id": "CTL-004", "result": "fail"}
            ]
        }"#;
        let (conclusion, _, summary) = format_check_result(json, "Release");
        assert_eq!(conclusion, "failure");
        assert!(summary.contains("CTL-001"));
        assert!(summary.contains("CTL-002"));
        assert!(summary.contains("CTL-004"));
        assert!(!summary.contains("CTL-003"));
    }

    #[test]
    fn dedup_cache_rejects_duplicate_delivery_ids() {
        let ids: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

        // First insert succeeds
        {
            let mut set = ids.lock().unwrap();
            assert!(!set.contains("delivery-1"));
            set.insert("delivery-1".to_string());
        }

        // Second insert is a duplicate
        {
            let set = ids.lock().unwrap();
            assert!(set.contains("delivery-1"));
        }
    }

    #[test]
    fn dedup_cache_clears_at_max_capacity() {
        let ids: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

        // Fill to MAX_DELIVERY_IDS
        {
            let mut set = ids.lock().unwrap();
            for i in 0..MAX_DELIVERY_IDS {
                set.insert(format!("id-{i}"));
            }
            assert_eq!(set.len(), MAX_DELIVERY_IDS);
        }

        // Next insert should clear and then insert
        {
            let mut set = ids.lock().unwrap();
            if set.len() >= MAX_DELIVERY_IDS {
                set.clear();
            }
            set.insert("new-id".to_string());
            assert_eq!(set.len(), 1);
            assert!(set.contains("new-id"));
            // Old IDs are gone
            assert!(!set.contains("id-0"));
        }
    }

    #[test]
    fn verify_signature_with_empty_body() {
        let secret = "test-secret";
        let body = b"";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize().into_bytes();
        let sig = format!(
            "sha256={}",
            result
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        assert!(verify_signature(secret, body, &sig));
    }
}
