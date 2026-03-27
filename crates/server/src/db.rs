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
            );",
        )?;
        Ok(())
    }

    pub fn upsert_user(
        &self,
        github_id: i64,
        login: &str,
        avatar_url: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (github_id, github_login, avatar_url)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(github_id) DO UPDATE SET
                github_login = excluded.github_login,
                avatar_url = excluded.avatar_url,
                updated_at = datetime('now')",
            rusqlite::params![github_id, login, avatar_url],
        )?;
        let user_id: i64 = conn.query_row(
            "SELECT id FROM users WHERE github_id = ?1",
            [github_id],
            |row| row.get(0),
        )?;
        Ok(user_id)
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

    pub fn get_installations_for_user(
        &self,
        user_id: i64,
    ) -> Result<Vec<(i64, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT installation_id, account_login, account_type
             FROM installations WHERE user_id = ?1",
        )?;
        let rows = stmt.query_map([user_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        Ok(())
    }
}
