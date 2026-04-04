use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod audit;
pub mod cache;
pub mod installation;
pub mod oauth;
pub mod session;
pub mod types;
pub mod user;

pub use types::*;

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
    pub(crate) fn reader(&self) -> std::sync::MutexGuard<'_, Connection> {
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
                verified_at TEXT NOT NULL DEFAULT (datetime('now')),
                trigger TEXT NOT NULL DEFAULT 'manual'
            );
            CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_id);
            CREATE INDEX IF NOT EXISTS idx_audit_log_repo ON audit_log(owner, repo);
            CREATE INDEX IF NOT EXISTS idx_audit_log_type ON audit_log(verification_type);
            CREATE INDEX IF NOT EXISTS idx_audit_log_verified_at ON audit_log(verified_at);
            CREATE INDEX IF NOT EXISTS idx_audit_log_user_type_repo ON audit_log(user_id, verification_type, owner, repo);",
        )?;
        // Add github_token column to users (idempotent migration)
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN github_token TEXT;");
        // Add trigger column to audit_log (idempotent migration)
        let _ = conn.execute_batch(
            "ALTER TABLE audit_log ADD COLUMN trigger TEXT NOT NULL DEFAULT 'manual';",
        );

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
        // Same github_id -> same internal id, updated login
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

        // Update without token -> old token preserved
        db.upsert_user(1, "bob", None, None).unwrap();
        assert_eq!(db.get_github_token(uid).unwrap(), Some("tok123".into()));

        // Update with new token -> overwritten
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
        // Second consume -> None (already used)
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
            "manual",
        )
        .unwrap();
        db.append_audit_entry(
            uid, "release", "owner", "repo", "v1.0", "oss", 3, 0, 0, 0, "{}", "webhook",
        )
        .unwrap();

        // No filters
        let all = db
            .get_audit_history(uid, None, None, None, None, None, 100, 0)
            .unwrap();
        assert_eq!(all.len(), 2);

        // Trigger field is preserved
        let triggers: Vec<&str> = all.iter().map(|e| e.trigger.as_str()).collect();
        assert!(triggers.contains(&"manual"));
        assert!(triggers.contains(&"webhook"));

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
                "manual",
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
