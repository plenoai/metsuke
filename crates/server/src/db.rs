use anyhow::Result;
use rusqlite::Connection;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY,
                github_id INTEGER UNIQUE NOT NULL,
                github_login TEXT NOT NULL,
                avatar_url TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS installations (
                id INTEGER PRIMARY KEY,
                installation_id INTEGER UNIQUE NOT NULL,
                user_id INTEGER NOT NULL REFERENCES users(id),
                account_login TEXT NOT NULL,
                account_type TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id),
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS oauth_clients (
                client_id TEXT PRIMARY KEY,
                client_secret TEXT,
                client_name TEXT,
                redirect_uris TEXT NOT NULL,
                grant_types TEXT NOT NULL DEFAULT 'authorization_code',
                token_endpoint_auth_method TEXT NOT NULL DEFAULT 'none',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS authorization_codes (
                code TEXT PRIMARY KEY,
                client_id TEXT NOT NULL REFERENCES oauth_clients(client_id),
                user_id INTEGER NOT NULL REFERENCES users(id),
                redirect_uri TEXT NOT NULL,
                code_challenge TEXT NOT NULL,
                code_challenge_method TEXT NOT NULL DEFAULT 'S256',
                scope TEXT NOT NULL DEFAULT 'mcp',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at TEXT NOT NULL,
                used INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS oauth_tokens (
                access_token TEXT PRIMARY KEY,
                refresh_token TEXT UNIQUE,
                client_id TEXT NOT NULL REFERENCES oauth_clients(client_id),
                user_id INTEGER NOT NULL REFERENCES users(id),
                scope TEXT NOT NULL DEFAULT 'mcp',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at TEXT NOT NULL,
                refresh_expires_at TEXT
            );
            CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id),
                verification_type TEXT NOT NULL,
                owner TEXT NOT NULL,
                repo TEXT NOT NULL,
                target_ref TEXT NOT NULL,
                policy TEXT NOT NULL DEFAULT 'default',
                pass_count INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0,
                review_count INTEGER NOT NULL DEFAULT 0,
                na_count INTEGER NOT NULL DEFAULT 0,
                result_json TEXT NOT NULL,
                verified_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_id);
            CREATE INDEX IF NOT EXISTS idx_audit_log_repo ON audit_log(owner, repo);
            CREATE INDEX IF NOT EXISTS idx_audit_log_type ON audit_log(verification_type);
            CREATE INDEX IF NOT EXISTS idx_audit_log_verified_at ON audit_log(verified_at);",
        )?;
        // Add github_token column to users (idempotent migration)
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN github_token TEXT;");
        Ok(())
    }

    pub fn upsert_user(
        &self,
        github_id: i64,
        login: &str,
        avatar_url: Option<&str>,
        github_token: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (github_id, github_login, avatar_url, github_token)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(github_id) DO UPDATE SET
                github_login = excluded.github_login,
                avatar_url = excluded.avatar_url,
                github_token = COALESCE(excluded.github_token, github_token),
                updated_at = datetime('now')",
            rusqlite::params![github_id, login, avatar_url, github_token],
        )?;
        let user_id: i64 = conn.query_row(
            "SELECT id FROM users WHERE github_id = ?1",
            [github_id],
            |row| row.get(0),
        )?;
        Ok(user_id)
    }

    pub fn get_github_token(&self, user_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT github_token FROM users WHERE id = ?1",
            [user_id],
            |row| row.get(0),
        );
        match result {
            Ok(v) => Ok(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn create_session(&self, user_id: i64) -> Result<String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, user_id, expires_at)
             VALUES (?1, ?2, datetime('now', '+30 days'))",
            rusqlite::params![session_id, user_id],
        )?;
        Ok(session_id)
    }

    pub fn get_user_by_session(&self, session_id: &str) -> Result<Option<(i64, String)>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT u.id, u.github_login
             FROM sessions s JOIN users u ON s.user_id = u.id
             WHERE s.id = ?1 AND s.expires_at > datetime('now')",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_installation(
        &self,
        installation_id: i64,
        user_id: i64,
        account_login: &str,
        account_type: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO installations (installation_id, user_id, account_login, account_type)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![installation_id, user_id, account_login, account_type],
        )?;
        Ok(())
    }

    pub fn get_installation_for_owner(&self, user_id: i64, owner: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT installation_id FROM installations
             WHERE user_id = ?1 AND account_login = ?2",
            rusqlite::params![user_id, owner],
            |row| row.get(0),
        );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_installations_for_user(&self, user_id: i64) -> Result<Vec<(i64, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT installation_id, account_login, account_type
             FROM installations WHERE user_id = ?1",
        )?;
        let rows = stmt.query_map([user_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        Ok(())
    }

    // --- OAuth Client Registration (RFC 7591) ---

    pub fn register_oauth_client(
        &self,
        client_id: &str,
        client_secret: Option<&str>,
        client_name: Option<&str>,
        redirect_uris: &[String],
        token_endpoint_auth_method: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let uris_json = serde_json::to_string(redirect_uris)?;
        conn.execute(
            "INSERT INTO oauth_clients (client_id, client_secret, client_name, redirect_uris, grant_types, token_endpoint_auth_method)
             VALUES (?1, ?2, ?3, ?4, 'authorization_code,refresh_token', ?5)",
            rusqlite::params![client_id, client_secret, client_name, uris_json, token_endpoint_auth_method],
        )?;
        Ok(())
    }

    pub fn get_oauth_client(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO authorization_codes (code, client_id, user_id, redirect_uri, code_challenge, scope, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now', '+10 minutes'))",
            rusqlite::params![code, client_id, user_id, redirect_uri, code_challenge, scope],
        )?;
        Ok(())
    }

    pub fn consume_authorization_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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

    /// Store OAuth state parameter → (client_id, redirect_uri, code_challenge, scope) mapping
    pub fn create_oauth_state(
        &self,
        state: &str,
        client_id: &str,
        redirect_uri: &str,
        code_challenge: &str,
        scope: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS oauth_states (
                state TEXT PRIMARY KEY,
                client_id TEXT NOT NULL,
                redirect_uri TEXT NOT NULL,
                code_challenge TEXT NOT NULL,
                scope TEXT NOT NULL DEFAULT 'mcp',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at TEXT NOT NULL
            );",
        )?;
        conn.execute(
            "INSERT INTO oauth_states (state, client_id, redirect_uri, code_challenge, scope, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now', '+10 minutes'))",
            rusqlite::params![state, client_id, redirect_uri, code_challenge, scope],
        )?;
        Ok(())
    }

    pub fn consume_oauth_state(&self, state: &str) -> Result<Option<OAuthState>> {
        let conn = self.conn.lock().unwrap();
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

    // --- Audit Log ---

    #[allow(clippy::too_many_arguments)]
    pub fn append_audit_entry(
        &self,
        user_id: i64,
        verification_type: &str,
        owner: &str,
        repo: &str,
        target_ref: &str,
        policy: &str,
        pass_count: i64,
        fail_count: i64,
        review_count: i64,
        na_count: i64,
        result_json: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO audit_log (user_id, verification_type, owner, repo, target_ref, policy, pass_count, fail_count, review_count, na_count, result_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![user_id, verification_type, owner, repo, target_ref, policy, pass_count, fail_count, review_count, na_count, result_json],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn get_audit_history(
        &self,
        user_id: i64,
        verification_type: Option<&str>,
        owner: Option<&str>,
        repo: Option<&str>,
        from_date: Option<&str>,
        to_date: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from(
            "SELECT id, verification_type, owner, repo, target_ref, policy, pass_count, fail_count, review_count, na_count, verified_at
             FROM audit_log WHERE user_id = ?1",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(user_id)];
        let mut idx = 2;

        if let Some(vt) = verification_type {
            sql.push_str(&format!(" AND verification_type = ?{idx}"));
            params.push(Box::new(vt.to_string()));
            idx += 1;
        }
        if let Some(o) = owner {
            sql.push_str(&format!(" AND owner = ?{idx}"));
            params.push(Box::new(o.to_string()));
            idx += 1;
        }
        if let Some(r) = repo {
            sql.push_str(&format!(" AND repo = ?{idx}"));
            params.push(Box::new(r.to_string()));
            idx += 1;
        }
        if let Some(fd) = from_date {
            sql.push_str(&format!(" AND verified_at >= ?{idx}"));
            params.push(Box::new(fd.to_string()));
            idx += 1;
        }
        if let Some(td) = to_date {
            sql.push_str(&format!(" AND verified_at < ?{idx}"));
            params.push(Box::new(td.to_string()));
            idx += 1;
        }
        sql.push_str(&format!(
            " ORDER BY verified_at DESC LIMIT ?{idx} OFFSET ?{}",
            idx + 1
        ));
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(AuditEntry {
                id: row.get(0)?,
                verification_type: row.get(1)?,
                owner: row.get(2)?,
                repo: row.get(3)?,
                target_ref: row.get(4)?,
                policy: row.get(5)?,
                pass_count: row.get(6)?,
                fail_count: row.get(7)?,
                review_count: row.get(8)?,
                na_count: row.get(9)?,
                verified_at: row.get(10)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Check database connectivity.
    pub fn ping(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("SELECT 1")?;
        Ok(())
    }

    /// Delete expired sessions, authorization codes, tokens, and OAuth states.
    pub fn cleanup_expired(&self) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let mut total = 0u64;
        total += conn.execute(
            "DELETE FROM sessions WHERE expires_at <= datetime('now')",
            [],
        )? as u64;
        total += conn.execute(
            "DELETE FROM authorization_codes WHERE expires_at <= datetime('now')",
            [],
        )? as u64;
        total += conn.execute(
            "DELETE FROM oauth_tokens WHERE expires_at <= datetime('now') AND refresh_expires_at <= datetime('now')",
            [],
        )? as u64;
        total += conn.execute(
            "DELETE FROM oauth_states WHERE expires_at <= datetime('now')",
            [],
        )? as u64;
        Ok(total)
    }

    /// Latest verification per (owner, repo, type) for cache display on repos list
    pub fn get_latest_verifications_for_user(&self, user_id: i64) -> Result<Vec<AuditEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT a.id, a.verification_type, a.owner, a.repo, a.target_ref, a.policy,
                    a.pass_count, a.fail_count, a.review_count, a.na_count, a.verified_at
             FROM audit_log a
             INNER JOIN (
                 SELECT MAX(id) as max_id FROM audit_log
                 WHERE user_id = ?1
                 GROUP BY owner, repo, verification_type
             ) latest ON a.id = latest.max_id
             ORDER BY a.verified_at DESC",
        )?;
        let rows = stmt.query_map([user_id], |row| {
            Ok(AuditEntry {
                id: row.get(0)?,
                verification_type: row.get(1)?,
                owner: row.get(2)?,
                repo: row.get(3)?,
                target_ref: row.get(4)?,
                policy: row.get(5)?,
                pass_count: row.get(6)?,
                fail_count: row.get(7)?,
                review_count: row.get(8)?,
                na_count: row.get(9)?,
                verified_at: row.get(10)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

pub struct AuditEntry {
    pub id: i64,
    pub verification_type: String,
    pub owner: String,
    pub repo: String,
    pub target_ref: String,
    pub policy: String,
    pub pass_count: i64,
    pub fail_count: i64,
    pub review_count: i64,
    pub na_count: i64,
    pub verified_at: String,
}

pub struct OAuthClient {
    pub client_secret: Option<String>,
    redirect_uris_json: String,
    pub token_endpoint_auth_method: String,
}

impl OAuthClient {
    pub fn redirect_uris(&self) -> Vec<String> {
        serde_json::from_str(&self.redirect_uris_json).unwrap_or_default()
    }
}

pub struct AuthorizationCode {
    pub client_id: String,
    pub user_id: i64,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub scope: String,
}

pub struct RefreshedToken {
    pub scope: String,
}

pub struct OAuthState {
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub scope: String,
}
