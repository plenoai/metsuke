use anyhow::{Context, Result};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
struct Claims {
    iat: u64,
    exp: u64,
    iss: String,
}

/// Generate a GitHub App JWT from the App ID and PEM private key.
///
/// The JWT is valid for 10 minutes (GitHub maximum).
/// `iat` is set 60 seconds in the past to account for clock drift.
pub fn generate_jwt(app_id: u64, private_key_pem: &[u8]) -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX epoch")?
        .as_secs();

    let claims = Claims {
        iat: now.saturating_sub(60),
        exp: now + 540, // 9 minutes
        iss: app_id.to_string(),
    };

    let key = EncodingKey::from_rsa_pem(private_key_pem).context("invalid RSA PEM private key")?;

    jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)
        .context("failed to encode JWT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_pem() {
        let result = generate_jwt(12345, b"not-a-pem-key");
        assert!(result.is_err());
    }
}
