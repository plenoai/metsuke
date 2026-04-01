use anyhow::Result;

use super::Database;

impl Database {
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
}
