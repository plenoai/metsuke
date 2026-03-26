use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct WebhookState {
    pub secret: String,
}

/// Verify the GitHub webhook signature and dispatch events.
pub async fn handle_webhook(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // Reject if no webhook secret is configured
    if state.secret.is_empty() {
        tracing::warn!("webhook received but GITHUB_WEBHOOK_SECRET is not configured");
        return StatusCode::SERVICE_UNAVAILABLE;
    }

    // Verify signature
    let signature = match headers.get("x-hub-signature-256") {
        Some(sig) => sig.to_str().unwrap_or(""),
        None => return StatusCode::UNAUTHORIZED,
    };

    if !verify_signature(&state.secret, &body, signature) {
        return StatusCode::UNAUTHORIZED;
    }

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    tracing::info!(event, "received webhook");

    match event {
        "installation" => {
            tracing::info!("GitHub App installation event");
        }
        "pull_request" => {
            tracing::info!("Pull request event — auto-verification placeholder");
        }
        "release" => {
            tracing::info!("Release event — auto-verification placeholder");
        }
        "ping" => {
            tracing::info!("Ping event — GitHub App connected");
        }
        _ => {
            tracing::debug!(event, "unhandled event type");
        }
    }

    StatusCode::OK
}

fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let Some(hex_sig) = signature.strip_prefix("sha256=") else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_verification() {
        let secret = "test-secret";
        let body = b"test body";

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig = hex::encode(mac.finalize().into_bytes());
        let header = format!("sha256={sig}");

        assert!(verify_signature(secret, body, &header));
        assert!(!verify_signature(secret, body, "sha256=invalid"));
        assert!(!verify_signature(secret, body, "invalid"));
    }
}
