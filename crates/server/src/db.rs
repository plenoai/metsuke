use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const READ_POOL_SIZE: usize = 4;

pub struct Database {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    next_reader: AtomicUsize,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let writer = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        writer.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=-20000;
             PRAGMA busy_timeout=5000;",
        )?;

        let mut readers = Vec::with_capacity(READ_POOL_SIZE);
        for _ in 0..READ_POOL_SIZE {
            let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            conn.execute_batch(
                "PRAGMA foreign_keys=ON;
                 PRAGMA cache_size=-5000;
                 PRAGMA busy_timeout=5000;",
            )?;
            readers.push(Mutex::new(conn));
        }

        let db = Self {
            writer: Mutex::new(writer),
            readers,
            next_reader: AtomicUsize::new(0),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Round-robin reader connection from the pool.
    fn reader(&self) -> std::sync::MutexGuard<'_, Connection> {
        let idx = self.next_reader.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        self.readers[idx].lock().unwrap()
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.writer.lock().unwrap();
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
            CREATE INDEX IF NOT EXISTS idx_audit_log_verified_at ON audit_log(verified_at);
            CREATE INDEX IF NOT EXISTS idx_audit_log_user_type_repo ON audit_log(user_id, verification_type, owner, repo);",
        )?;
        // Add github_token column to users (idempotent migration)
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN github_token TEXT;");

        // Cache tables for GitHub data
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS repositories (
                id INTEGER PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id),
                owner TEXT NOT NULL,
                name TEXT NOT NULL,
                full_name TEXT NOT NULL,
                private INTEGER NOT NULL DEFAULT 0,
                description TEXT,
                language TEXT,
                default_branch TEXT,
                pushed_at TEXT,
                synced_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(user_id, full_name)
            );
            CREATE INDEX IF NOT EXISTS idx_repositories_user ON repositories(user_id);

            CREATE TABLE IF NOT EXISTS cached_pulls (
                id INTEGER PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id),
                owner TEXT NOT NULL,
                repo TEXT NOT NULL,
                pr_number INTEGER NOT NULL,
                title TEXT NOT NULL,
                state TEXT NOT NULL,
                author TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                merged_at TEXT,
                draft INTEGER NOT NULL DEFAULT 0,
                synced_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(user_id, owner, repo, pr_number)
            );
            CREATE INDEX IF NOT EXISTS idx_cached_pulls_repo ON cached_pulls(user_id, owner, repo);

            CREATE TABLE IF NOT EXISTS cached_releases (
                id INTEGER PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id),
                owner TEXT NOT NULL,
                repo TEXT NOT NULL,
                release_id INTEGER NOT NULL,
                tag_name TEXT NOT NULL,
                name TEXT,
                draft INTEGER NOT NULL DEFAULT 0,
                prerelease INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                published_at TEXT,
                author TEXT NOT NULL,
                html_url TEXT NOT NULL,
                body TEXT,
                synced_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(user_id, owner, repo, release_id)
            );
            CREATE INDEX IF NOT EXISTS idx_cached_releases_repo ON cached_releases(user_id, owner, repo);

            CREATE TABLE IF NOT EXISTS oauth_states (
                state TEXT PRIMARY KEY,
                client_id TEXT NOT NULL,
                redirect_uri TEXT NOT NULL,
                code_challenge TEXT NOT NULL,
                scope TEXT NOT NULL DEFAULT 'mcp',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at TEXT NOT NULL
            );",
        )?;

        Ok(())
    }

    pub fn upsert_user(
        &self,
        github_id: i64,
        login: &str,
        avatar_url: Option<&str>,
        github_token: Option<&str>,
    ) -> Result<i64> {
        let conn = self.writer.lock().unwrap();
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
        let conn = self.reader();
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

    #[cfg(test)]
    pub fn create_session(&self, user_id: i64) -> Result<String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let conn = self.writer.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, user_id, expires_at)
             VALUES (?1, ?2, datetime('now', '+30 days'))",
            rusqlite::params![session_id, user_id],
        )?;
        Ok(session_id)
    }

    pub fn get_user_by_session(&self, session_id: &str) -> Result<Option<(i64, String)>> {
        let conn = self.reader();
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
        let conn = self.writer.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO installations (installation_id, user_id, account_login, account_type)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![installation_id, user_id, account_login, account_type],
        )?;
        Ok(())
    }

    pub fn batch_save_installations(
        &self,
        user_id: i64,
        installations: &[(i64, String, String)],
    ) -> Result<()> {
        let conn = self.writer.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "INSERT OR REPLACE INTO installations (installation_id, user_id, account_login, account_type)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (inst_id, login, account_type) in installations {
            stmt.execute(rusqlite::params![inst_id, user_id, login, account_type])?;
        }
        Ok(())
    }

    pub fn get_installation_for_owner(&self, user_id: i64, owner: &str) -> Result<Option<i64>> {
        let conn = self.reader();
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
        let conn = self.reader();
        let mut stmt = conn.prepare_cached(
            "SELECT installation_id, account_login, account_type
             FROM installations WHERE user_id = ?1",
        )?;
        let rows = stmt.query_map([user_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.writer.lock().unwrap();
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

    /// Store OAuth state parameter → (client_id, redirect_uri, code_challenge, scope) mapping
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
        let conn = self.writer.lock().unwrap();
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
        let conn = self.reader();
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
        let conn = self.reader();
        conn.execute_batch("SELECT 1")?;
        Ok(())
    }

    /// Delete expired sessions, authorization codes, tokens, and OAuth states.
    pub fn cleanup_expired(&self) -> Result<u64> {
        let conn = self.writer.lock().unwrap();
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
        // oauth_states table is created lazily by OAuth module; skip if not present
        total += conn
            .execute(
                "DELETE FROM oauth_states WHERE expires_at <= datetime('now')",
                [],
            )
            .unwrap_or(0) as u64;
        Ok(total)
    }

    // --- Repository Cache ---

    pub fn upsert_repositories(&self, user_id: i64, repos: &[RepoRow]) -> Result<()> {
        let mut conn = self.writer.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO repositories (user_id, owner, name, full_name, private, description, language, default_branch, pushed_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))
                 ON CONFLICT(user_id, full_name) DO UPDATE SET
                    owner = excluded.owner,
                    name = excluded.name,
                    private = excluded.private,
                    description = excluded.description,
                    language = excluded.language,
                    default_branch = excluded.default_branch,
                    pushed_at = excluded.pushed_at,
                    synced_at = datetime('now')",
            )?;
            for r in repos {
                stmt.execute(rusqlite::params![
                    user_id,
                    r.owner,
                    r.name,
                    r.full_name,
                    r.private as i32,
                    r.description,
                    r.language,
                    r.default_branch,
                    r.pushed_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_cached_pulls(
        &self,
        user_id: i64,
        owner: &str,
        repo: &str,
        pulls: &[CachedPullRow],
    ) -> Result<()> {
        let mut conn = self.writer.lock().unwrap();
        let tx = conn.transaction()?;
        {
            tx.execute(
                "DELETE FROM cached_pulls WHERE user_id = ?1 AND owner = ?2 AND repo = ?3",
                rusqlite::params![user_id, owner, repo],
            )?;
            let mut stmt = tx.prepare_cached(
                "INSERT INTO cached_pulls (user_id, owner, repo, pr_number, title, state, author, created_at, updated_at, merged_at, draft, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'))",
            )?;
            for p in pulls {
                stmt.execute(rusqlite::params![
                    user_id,
                    owner,
                    repo,
                    p.pr_number,
                    p.title,
                    p.state,
                    p.author,
                    p.created_at,
                    p.updated_at,
                    p.merged_at,
                    p.draft as i32,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_cached_releases(
        &self,
        user_id: i64,
        owner: &str,
        repo: &str,
        releases: &[CachedReleaseRow],
    ) -> Result<()> {
        let mut conn = self.writer.lock().unwrap();
        let tx = conn.transaction()?;
        {
            tx.execute(
                "DELETE FROM cached_releases WHERE user_id = ?1 AND owner = ?2 AND repo = ?3",
                rusqlite::params![user_id, owner, repo],
            )?;
            let mut stmt = tx.prepare_cached(
                "INSERT INTO cached_releases (user_id, owner, repo, release_id, tag_name, name, draft, prerelease, created_at, published_at, author, html_url, body, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, datetime('now'))",
            )?;
            for r in releases {
                stmt.execute(rusqlite::params![
                    user_id,
                    owner,
                    repo,
                    r.release_id,
                    r.tag_name,
                    r.name,
                    r.draft as i32,
                    r.prerelease as i32,
                    r.created_at,
                    r.published_at,
                    r.author,
                    r.html_url,
                    r.body,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Combined: get repos + staleness check in a single reader lock acquisition.
    pub fn get_repos_with_staleness(&self, user_id: i64) -> Result<(Vec<RepoRow>, bool)> {
        let conn = self.reader();
        let stale: bool = conn.query_row(
            "SELECT CASE WHEN COUNT(*) = 0 THEN 1
                    ELSE MIN(synced_at) < datetime('now', '-5 minutes')
                    END
             FROM repositories WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?;
        let mut stmt = conn.prepare_cached(
            "SELECT r.owner, r.name, r.full_name, r.private, r.description, r.language,
                    r.default_branch, r.pushed_at, r.synced_at,
                    a.pass_count, a.fail_count, a.review_count, a.verified_at
             FROM repositories r
             LEFT JOIN (
                 SELECT owner, repo, pass_count, fail_count, review_count, verified_at
                 FROM audit_log WHERE user_id = ?1 AND verification_type = 'repo'
                 AND id IN (SELECT MAX(id) FROM audit_log WHERE user_id = ?1 AND verification_type = 'repo' GROUP BY owner, repo)
             ) a ON r.owner = a.owner AND r.name = a.repo
             WHERE r.user_id = ?1
             ORDER BY r.pushed_at DESC NULLS LAST",
        )?;
        let rows = stmt.query_map([user_id], |row| {
            Ok(RepoRow {
                owner: row.get(0)?,
                name: row.get(1)?,
                full_name: row.get(2)?,
                private: row.get::<_, i32>(3)? != 0,
                description: row.get(4)?,
                language: row.get(5)?,
                default_branch: row.get(6)?,
                pushed_at: row.get(7)?,
                synced_at: row.get(8)?,
                pass_count: row.get(9)?,
                fail_count: row.get(10)?,
                review_count: row.get(11)?,
                verified_at: row.get(12)?,
            })
        })?;
        let repos: Vec<RepoRow> = rows.collect::<Result<Vec<_>, _>>()?;
        Ok((repos, stale))
    }

    /// Combined: get cached pulls + staleness check in a single reader lock.
    pub fn get_pulls_with_staleness(
        &self,
        user_id: i64,
        owner: &str,
        repo: &str,
    ) -> Result<(Vec<CachedPullRow>, bool)> {
        let conn = self.reader();
        let stale: bool = conn.query_row(
            "SELECT CASE WHEN COUNT(*) = 0 THEN 1
                    ELSE MIN(synced_at) < datetime('now', '-5 minutes')
                    END
             FROM cached_pulls WHERE user_id = ?1 AND owner = ?2 AND repo = ?3",
            rusqlite::params![user_id, owner, repo],
            |row| row.get(0),
        )?;
        let mut stmt = conn.prepare_cached(
            "SELECT pr_number, title, state, author, created_at, updated_at, merged_at, draft
             FROM cached_pulls
             WHERE user_id = ?1 AND owner = ?2 AND repo = ?3
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![user_id, owner, repo], |row| {
            Ok(CachedPullRow {
                pr_number: row.get(0)?,
                title: row.get(1)?,
                state: row.get(2)?,
                author: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                merged_at: row.get(6)?,
                draft: row.get::<_, i32>(7)? != 0,
            })
        })?;
        let pulls: Vec<CachedPullRow> = rows.collect::<Result<Vec<_>, _>>()?;
        Ok((pulls, stale))
    }

    /// Combined: get cached releases + staleness check in a single reader lock.
    pub fn get_releases_with_staleness(
        &self,
        user_id: i64,
        owner: &str,
        repo: &str,
    ) -> Result<(Vec<CachedReleaseRow>, bool)> {
        let conn = self.reader();
        let stale: bool = conn.query_row(
            "SELECT CASE WHEN COUNT(*) = 0 THEN 1
                    ELSE MIN(synced_at) < datetime('now', '-5 minutes')
                    END
             FROM cached_releases WHERE user_id = ?1 AND owner = ?2 AND repo = ?3",
            rusqlite::params![user_id, owner, repo],
            |row| row.get(0),
        )?;
        let mut stmt = conn.prepare_cached(
            "SELECT release_id, tag_name, name, draft, prerelease, created_at, published_at, author, html_url, body
             FROM cached_releases
             WHERE user_id = ?1 AND owner = ?2 AND repo = ?3
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![user_id, owner, repo], |row| {
            Ok(CachedReleaseRow {
                release_id: row.get(0)?,
                tag_name: row.get(1)?,
                name: row.get(2)?,
                draft: row.get::<_, i32>(3)? != 0,
                prerelease: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
                published_at: row.get(6)?,
                author: row.get(7)?,
                html_url: row.get(8)?,
                body: row.get(9)?,
            })
        })?;
        let releases: Vec<CachedReleaseRow> = rows.collect::<Result<Vec<_>, _>>()?;
        Ok((releases, stale))
    }

    /// Combined: upsert user + create session in a single writer lock.
    pub fn upsert_user_and_create_session(
        &self,
        github_id: i64,
        login: &str,
        avatar_url: Option<&str>,
        github_token: Option<&str>,
    ) -> Result<(i64, String)> {
        let conn = self.writer.lock().unwrap();
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
        let session_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO sessions (id, user_id, expires_at)
             VALUES (?1, ?2, datetime('now', '+30 days'))",
            rusqlite::params![session_id, user_id],
        )?;
        Ok((user_id, session_id))
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

#[derive(Serialize)]
pub struct RepoRow {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub description: Option<String>,
    pub language: Option<String>,
    pub default_branch: Option<String>,
    pub pushed_at: Option<String>,
    pub synced_at: String,
    // From audit_log LEFT JOIN (latest repo verification)
    pub pass_count: Option<i64>,
    pub fail_count: Option<i64>,
    pub review_count: Option<i64>,
    pub verified_at: Option<String>,
}

#[derive(Serialize)]
pub struct CachedPullRow {
    pub pr_number: i64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub draft: bool,
}

#[derive(Serialize)]
pub struct CachedReleaseRow {
    pub release_id: i64,
    pub tag_name: String,
    pub name: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    pub published_at: Option<String>,
    pub author: String,
    pub html_url: String,
    pub body: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_db() -> Database {
        let path = std::env::temp_dir()
            .join(format!("metsuke-test-{}.db", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        Database::open(&path).unwrap()
    }

    #[test]
    fn cleanup_expired_deletes_old_sessions_preserves_valid() {
        let db = memory_db();
        let uid = db.upsert_user(1, "test", None, None).unwrap();

        // Create a valid session
        let valid_sid = db.create_session(uid).unwrap();

        // Insert an already-expired session
        let expired_sid = {
            let sid = uuid::Uuid::new_v4().to_string();
            let conn = db.writer.lock().unwrap();
            conn.execute(
                "INSERT INTO sessions (id, user_id, expires_at)
                 VALUES (?1, ?2, datetime('now', '-1 second'))",
                rusqlite::params![sid, uid],
            )
            .unwrap();
            sid
        };

        let deleted = db.cleanup_expired().unwrap();
        assert!(deleted >= 1, "should delete at least the expired session");

        // Valid session still reachable
        assert!(db.get_user_by_session(&valid_sid).unwrap().is_some());
        // Expired session gone
        assert!(db.get_user_by_session(&expired_sid).unwrap().is_none());
    }

    #[test]
    fn cleanup_expired_returns_zero_when_nothing_expired() {
        let db = memory_db();
        let uid = db.upsert_user(1, "test", None, None).unwrap();
        let _sid = db.create_session(uid).unwrap();

        let deleted = db.cleanup_expired().unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn ping_succeeds_on_valid_db() {
        let db = memory_db();
        assert!(db.ping().is_ok());
    }

    // --- User / Session tests ---

    #[test]
    fn upsert_user_creates_and_updates() {
        let db = memory_db();
        let id1 = db
            .upsert_user(42, "alice", Some("https://img/a"), None)
            .unwrap();
        // Same github_id → same internal id, updated login
        let id2 = db
            .upsert_user(42, "alice-renamed", Some("https://img/b"), None)
            .unwrap();
        assert_eq!(id1, id2);

        // Login was updated
        let sid = db.create_session(id1).unwrap();
        let (_, login) = db.get_user_by_session(&sid).unwrap().unwrap();
        assert_eq!(login, "alice-renamed");
    }

    #[test]
    fn upsert_user_preserves_token_when_new_is_none() {
        let db = memory_db();
        let uid = db.upsert_user(1, "bob", None, Some("tok123")).unwrap();
        assert_eq!(db.get_github_token(uid).unwrap(), Some("tok123".into()));

        // Update without token → old token preserved
        db.upsert_user(1, "bob", None, None).unwrap();
        assert_eq!(db.get_github_token(uid).unwrap(), Some("tok123".into()));

        // Update with new token → overwritten
        db.upsert_user(1, "bob", None, Some("tok456")).unwrap();
        assert_eq!(db.get_github_token(uid).unwrap(), Some("tok456".into()));
    }

    #[test]
    fn get_github_token_returns_none_for_missing_user() {
        let db = memory_db();
        assert_eq!(db.get_github_token(999).unwrap(), None);
    }

    #[test]
    fn create_and_get_session() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        let sid = db.create_session(uid).unwrap();
        let (found_uid, login) = db.get_user_by_session(&sid).unwrap().unwrap();
        assert_eq!(found_uid, uid);
        assert_eq!(login, "user");
    }

    #[test]
    fn get_user_by_session_returns_none_for_unknown() {
        let db = memory_db();
        assert!(db.get_user_by_session("no-such-session").unwrap().is_none());
    }

    #[test]
    fn delete_session_removes_it() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        let sid = db.create_session(uid).unwrap();
        db.delete_session(&sid).unwrap();
        assert!(db.get_user_by_session(&sid).unwrap().is_none());
    }

    // --- Installation tests ---

    #[test]
    fn save_and_get_installation() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        db.save_installation(100, uid, "my-org", "Organization")
            .unwrap();

        assert_eq!(
            db.get_installation_for_owner(uid, "my-org").unwrap(),
            Some(100)
        );
        assert_eq!(db.get_installation_for_owner(uid, "other").unwrap(), None);

        let all = db.get_installations_for_user(uid).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], (100, "my-org".into(), "Organization".into()));
    }

    #[test]
    fn save_installation_upserts_on_conflict() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        db.save_installation(100, uid, "org-old", "Organization")
            .unwrap();
        // Same installation_id, different account
        db.save_installation(100, uid, "org-new", "Organization")
            .unwrap();
        assert_eq!(
            db.get_installation_for_owner(uid, "org-new").unwrap(),
            Some(100)
        );
        assert_eq!(db.get_installations_for_user(uid).unwrap().len(), 1);
    }

    // --- OAuth Client tests ---

    #[test]
    fn register_and_get_oauth_client() {
        let db = memory_db();
        let uris = vec!["https://example.com/cb".to_string()];
        db.register_oauth_client(
            "cid",
            Some("secret"),
            Some("My App"),
            &uris,
            "client_secret_post",
        )
        .unwrap();

        let client = db.get_oauth_client("cid").unwrap().unwrap();
        assert_eq!(client.client_secret, Some("secret".into()));
        assert_eq!(client.token_endpoint_auth_method, "client_secret_post");
        assert_eq!(client.redirect_uris(), vec!["https://example.com/cb"]);
    }

    #[test]
    fn get_oauth_client_returns_none_for_unknown() {
        let db = memory_db();
        assert!(db.get_oauth_client("nope").unwrap().is_none());
    }

    // --- Authorization Code tests ---

    #[test]
    fn create_and_consume_authorization_code() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        let uris = vec!["https://cb".to_string()];
        db.register_oauth_client("cid", None, None, &uris, "none")
            .unwrap();

        db.create_authorization_code("code1", "cid", uid, "https://cb", "challenge", "mcp")
            .unwrap();

        let ac = db.consume_authorization_code("code1").unwrap().unwrap();
        assert_eq!(ac.client_id, "cid");
        assert_eq!(ac.user_id, uid);
        assert_eq!(ac.redirect_uri, "https://cb");
        assert_eq!(ac.code_challenge, "challenge");
        assert_eq!(ac.scope, "mcp");
    }

    #[test]
    fn consume_authorization_code_is_single_use() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        let uris = vec!["https://cb".to_string()];
        db.register_oauth_client("cid", None, None, &uris, "none")
            .unwrap();
        db.create_authorization_code("code1", "cid", uid, "https://cb", "ch", "mcp")
            .unwrap();

        assert!(db.consume_authorization_code("code1").unwrap().is_some());
        // Second consume → None (already used)
        assert!(db.consume_authorization_code("code1").unwrap().is_none());
    }

    #[test]
    fn consume_authorization_code_returns_none_for_unknown() {
        let db = memory_db();
        assert!(
            db.consume_authorization_code("nonexistent")
                .unwrap()
                .is_none()
        );
    }

    // --- OAuth Token tests ---

    #[test]
    fn create_and_validate_access_token() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        let uris = vec!["https://cb".to_string()];
        db.register_oauth_client("cid", None, None, &uris, "none")
            .unwrap();

        db.create_oauth_token("at", "rt", "cid", uid, "mcp", 3600, 86400)
            .unwrap();

        assert_eq!(db.validate_access_token("at").unwrap(), Some(uid));
        assert_eq!(db.validate_access_token("unknown").unwrap(), None);
    }

    #[test]
    fn refresh_oauth_token_rotates_tokens() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        let uris = vec!["https://cb".to_string()];
        db.register_oauth_client("cid", None, None, &uris, "none")
            .unwrap();
        db.create_oauth_token("at1", "rt1", "cid", uid, "mcp", 3600, 86400)
            .unwrap();

        let refreshed = db
            .refresh_oauth_token("rt1", "at2", "rt2", 3600, 86400)
            .unwrap()
            .unwrap();
        assert_eq!(refreshed.scope, "mcp");

        // Old token invalid, new token valid
        assert_eq!(db.validate_access_token("at1").unwrap(), None);
        assert_eq!(db.validate_access_token("at2").unwrap(), Some(uid));
    }

    #[test]
    fn refresh_oauth_token_returns_none_for_unknown() {
        let db = memory_db();
        assert!(
            db.refresh_oauth_token("nope", "a", "b", 3600, 86400)
                .unwrap()
                .is_none()
        );
    }

    // --- OAuth State tests ---

    #[test]
    fn create_and_consume_oauth_state() {
        let db = memory_db();
        db.create_oauth_state("state1", "cid", "https://cb", "ch", "mcp")
            .unwrap();

        let s = db.consume_oauth_state("state1").unwrap().unwrap();
        assert_eq!(s.client_id, "cid");
        assert_eq!(s.redirect_uri, "https://cb");
        assert_eq!(s.code_challenge, "ch");
        assert_eq!(s.scope, "mcp");
    }

    #[test]
    fn consume_oauth_state_is_single_use() {
        let db = memory_db();
        db.create_oauth_state("s1", "cid", "https://cb", "ch", "mcp")
            .unwrap();
        assert!(db.consume_oauth_state("s1").unwrap().is_some());
        assert!(db.consume_oauth_state("s1").unwrap().is_none());
    }

    // --- Audit Log tests ---

    #[test]
    fn append_and_get_audit_history() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();

        db.append_audit_entry(
            uid,
            "pr",
            "owner",
            "repo",
            "refs/pull/1",
            "default",
            5,
            1,
            0,
            2,
            "{}",
        )
        .unwrap();
        db.append_audit_entry(
            uid, "release", "owner", "repo", "v1.0", "oss", 3, 0, 0, 0, "{}",
        )
        .unwrap();

        // No filters
        let all = db
            .get_audit_history(uid, None, None, None, None, None, 100, 0)
            .unwrap();
        assert_eq!(all.len(), 2);

        // Filter by type
        let prs = db
            .get_audit_history(uid, Some("pr"), None, None, None, None, 100, 0)
            .unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].verification_type, "pr");

        // Filter by owner
        let by_owner = db
            .get_audit_history(uid, None, Some("owner"), None, None, None, 100, 0)
            .unwrap();
        assert_eq!(by_owner.len(), 2);
        let by_nobody = db
            .get_audit_history(uid, None, Some("nobody"), None, None, None, 100, 0)
            .unwrap();
        assert_eq!(by_nobody.len(), 0);
    }

    #[test]
    fn get_audit_history_respects_limit_and_offset() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        for i in 0..5 {
            db.append_audit_entry(
                uid,
                "pr",
                "o",
                "r",
                &format!("ref{i}"),
                "default",
                1,
                0,
                0,
                0,
                "{}",
            )
            .unwrap();
        }

        let page1 = db
            .get_audit_history(uid, None, None, None, None, None, 2, 0)
            .unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = db
            .get_audit_history(uid, None, None, None, None, None, 2, 2)
            .unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = db
            .get_audit_history(uid, None, None, None, None, None, 2, 4)
            .unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn cleanup_expired_deletes_expired_authorization_codes() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        let uris = vec!["https://cb".to_string()];
        db.register_oauth_client("cid", None, None, &uris, "none")
            .unwrap();

        // Insert an already-expired authorization code
        {
            let conn = db.writer.lock().unwrap();
            conn.execute(
                "INSERT INTO authorization_codes (code, client_id, user_id, redirect_uri, code_challenge, scope, expires_at)
                 VALUES ('expired-code', 'cid', ?1, 'https://cb', 'ch', 'mcp', datetime('now', '-1 second'))",
                [uid],
            ).unwrap();
        }
        // Also create a valid code
        db.create_authorization_code("valid-code", "cid", uid, "https://cb", "ch", "mcp")
            .unwrap();

        let deleted = db.cleanup_expired().unwrap();
        assert!(deleted >= 1);

        // Expired code was already expired so consume should return None regardless,
        // but the row should be gone from the table
        assert!(
            db.consume_authorization_code("expired-code")
                .unwrap()
                .is_none()
        );
        // Valid code still consumable
        assert!(
            db.consume_authorization_code("valid-code")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn cleanup_expired_deletes_fully_expired_tokens() {
        let db = memory_db();
        let uid = db.upsert_user(1, "user", None, None).unwrap();
        let uris = vec!["https://cb".to_string()];
        db.register_oauth_client("cid", None, None, &uris, "none")
            .unwrap();

        // Insert a token where both access and refresh are expired
        {
            let conn = db.writer.lock().unwrap();
            conn.execute(
                "INSERT INTO oauth_tokens (access_token, refresh_token, client_id, user_id, scope, expires_at, refresh_expires_at)
                 VALUES ('at-expired', 'rt-expired', 'cid', ?1, 'mcp', datetime('now', '-1 hour'), datetime('now', '-1 second'))",
                [uid],
            ).unwrap();
        }

        // Insert a token where access expired but refresh is still valid (should NOT be deleted)
        {
            let conn = db.writer.lock().unwrap();
            conn.execute(
                "INSERT INTO oauth_tokens (access_token, refresh_token, client_id, user_id, scope, expires_at, refresh_expires_at)
                 VALUES ('at-refreshable', 'rt-valid', 'cid', ?1, 'mcp', datetime('now', '-1 second'), datetime('now', '+1 hour'))",
                [uid],
            ).unwrap();
        }

        let deleted = db.cleanup_expired().unwrap();
        assert!(deleted >= 1, "should delete the fully expired token");

        // Fully expired token gone
        assert_eq!(db.validate_access_token("at-expired").unwrap(), None);
        // Token with valid refresh still exists (refresh_oauth_token should find it)
        let refreshed = db
            .refresh_oauth_token("rt-valid", "new-at", "new-rt", 3600, 86400)
            .unwrap();
        assert!(
            refreshed.is_some(),
            "token with valid refresh should survive cleanup"
        );
    }

    #[test]
    fn cleanup_expired_deletes_expired_oauth_states() {
        let db = memory_db();
        db.create_oauth_state("valid-state", "cid", "https://cb", "ch", "mcp")
            .unwrap();

        // Insert an expired state
        {
            let conn = db.writer.lock().unwrap();
            conn.execute(
                "INSERT INTO oauth_states (state, client_id, redirect_uri, code_challenge, scope, expires_at)
                 VALUES ('expired-state', 'cid', 'https://cb', 'ch', 'mcp', datetime('now', '-1 second'))",
                [],
            ).unwrap();
        }

        let deleted = db.cleanup_expired().unwrap();
        assert!(deleted >= 1);

        // Expired state gone
        assert!(db.consume_oauth_state("expired-state").unwrap().is_none());
        // Valid state still available
        assert!(db.consume_oauth_state("valid-state").unwrap().is_some());
    }

    #[test]
    fn oauth_client_redirect_uris_parses_json() {
        let db = memory_db();
        let uris = vec![
            "https://a.com/cb".to_string(),
            "https://b.com/cb".to_string(),
        ];
        db.register_oauth_client("cid", None, None, &uris, "none")
            .unwrap();

        let client = db.get_oauth_client("cid").unwrap().unwrap();
        assert_eq!(client.redirect_uris(), uris);
    }
}
