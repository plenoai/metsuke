use anyhow::Result;

use super::Database;
use super::types::{AuthorizationCode, OAuthClient, OAuthState, RefreshedToken};

impl Database {
    // --- OAuth Client Registration (RFC 7591) ---

    pub fn register_oauth_client(
        &self,
        client_id: &str,
        client_secret: Option<&str>,
        client_name: Option<&str>,
        redirect_uris: &[String],
        token_endpoint_auth_method: &str,
    ) -> Result<()> {
        let conn = self.writer.lock().unwrap();
        let uris_json = serde_json::to_string(redirect_uris)?;
        conn.execute(
            "INSERT INTO oauth_clients (client_id, client_secret, client_name, redirect_uris, grant_types, token_endpoint_auth_method)
             VALUES (?1, ?2, ?3, ?4, 'authorization_code,refresh_token', ?5)",
            rusqlite::params![client_id, client_secret, client_name, uris_json, token_endpoint_auth_method],
        )?;
        Ok(())
    }

    pub fn get_oauth_client(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        let conn = self.reader();
        let result = conn.query_row(
            "SELECT client_id, client_secret, client_name, redirect_uris, token_endpoint_auth_method FROM oauth_clients WHERE client_id = ?1",
            [client_id],
            |row| {
                Ok(OAuthClient {
                    client_secret: row.get(1)?,
                    redirect_uris_json: row.get(3)?,
                    token_endpoint_auth_method: row.get(4)?,
                })
            },
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // --- Authorization Codes ---

    pub fn create_authorization_code(
        &self,
        code: &str,
        client_id: &str,
        user_id: i64,
        redirect_uri: &str,
        code_challenge: &str,
        scope: &str,
    ) -> Result<()> {
        let conn = self.writer.lock().unwrap();
        conn.execute(
            "INSERT INTO authorization_codes (code, client_id, user_id, redirect_uri, code_challenge, scope, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now', '+10 minutes'))",
            rusqlite::params![code, client_id, user_id, redirect_uri, code_challenge, scope],
        )?;
        Ok(())
    }

    pub fn consume_authorization_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let conn = self.writer.lock().unwrap();
        let result = conn.query_row(
            "SELECT client_id, user_id, redirect_uri, code_challenge, scope
             FROM authorization_codes
             WHERE code = ?1 AND used = 0 AND expires_at > datetime('now')",
            [code],
            |row| {
                Ok(AuthorizationCode {
                    client_id: row.get(0)?,
                    user_id: row.get(1)?,
                    redirect_uri: row.get(2)?,
                    code_challenge: row.get(3)?,
                    scope: row.get(4)?,
                })
            },
        );
        match result {
            Ok(ac) => {
                conn.execute(
                    "UPDATE authorization_codes SET used = 1 WHERE code = ?1",
                    [code],
                )?;
                Ok(Some(ac))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // --- OAuth Tokens ---

    #[allow(clippy::too_many_arguments)]
    pub fn create_oauth_token(
        &self,
        access_token: &str,
        refresh_token: &str,
        client_id: &str,
        user_id: i64,
        scope: &str,
        access_token_ttl_secs: i64,
        refresh_token_ttl_secs: i64,
    ) -> Result<()> {
        let conn = self.writer.lock().unwrap();
        conn.execute(
            "INSERT INTO oauth_tokens (access_token, refresh_token, client_id, user_id, scope, expires_at, refresh_expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now', ?6), datetime('now', ?7))",
            rusqlite::params![
                access_token,
                refresh_token,
                client_id,
                user_id,
                scope,
                format!("+{access_token_ttl_secs} seconds"),
                format!("+{refresh_token_ttl_secs} seconds"),
            ],
        )?;
        Ok(())
    }

    pub fn validate_access_token(&self, access_token: &str) -> Result<Option<i64>> {
        let conn = self.reader();
        let result = conn.query_row(
            "SELECT user_id FROM oauth_tokens WHERE access_token = ?1 AND expires_at > datetime('now')",
            [access_token],
            |row| row.get(0),
        );
        match result {
            Ok(user_id) => Ok(Some(user_id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn refresh_oauth_token(
        &self,
        old_refresh_token: &str,
        new_access_token: &str,
        new_refresh_token: &str,
        access_token_ttl_secs: i64,
        refresh_token_ttl_secs: i64,
    ) -> Result<Option<RefreshedToken>> {
        let conn = self.writer.lock().unwrap();
        let existing = conn.query_row(
            "SELECT client_id, user_id, scope FROM oauth_tokens
             WHERE refresh_token = ?1 AND refresh_expires_at > datetime('now')",
            [old_refresh_token],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        );
        match existing {
            Ok((client_id, user_id, scope)) => {
                // Delete old token
                conn.execute(
                    "DELETE FROM oauth_tokens WHERE refresh_token = ?1",
                    [old_refresh_token],
                )?;
                // Create new token
                conn.execute(
                    "INSERT INTO oauth_tokens (access_token, refresh_token, client_id, user_id, scope, expires_at, refresh_expires_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, datetime('now', ?6), datetime('now', ?7))",
                    rusqlite::params![
                        new_access_token,
                        new_refresh_token,
                        client_id,
                        user_id,
                        scope,
                        format!("+{access_token_ttl_secs} seconds"),
                        format!("+{refresh_token_ttl_secs} seconds"),
                    ],
                )?;
                Ok(Some(RefreshedToken { scope }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Store OAuth state parameter -> (client_id, redirect_uri, code_challenge, scope) mapping
    pub fn create_oauth_state(
        &self,
        state: &str,
        client_id: &str,
        redirect_uri: &str,
        code_challenge: &str,
        scope: &str,
    ) -> Result<()> {
        let conn = self.writer.lock().unwrap();
        conn.execute(
            "INSERT INTO oauth_states (state, client_id, redirect_uri, code_challenge, scope, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now', '+10 minutes'))",
            rusqlite::params![state, client_id, redirect_uri, code_challenge, scope],
        )?;
        Ok(())
    }

    pub fn consume_oauth_state(&self, state: &str) -> Result<Option<OAuthState>> {
        let conn = self.writer.lock().unwrap();
        // Table might not exist yet if no authorize calls have been made
        let result = conn.query_row(
            "SELECT state, client_id, redirect_uri, code_challenge, scope
             FROM oauth_states WHERE state = ?1 AND expires_at > datetime('now')",
            [state],
            |row| {
                Ok(OAuthState {
                    client_id: row.get(1)?,
                    redirect_uri: row.get(2)?,
                    code_challenge: row.get(3)?,
                    scope: row.get(4)?,
                })
            },
        );
        match result {
            Ok(s) => {
                conn.execute("DELETE FROM oauth_states WHERE state = ?1", [state])?;
                Ok(Some(s))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
