use anyhow::Result;

use super::Database;

impl Database {
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

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.writer.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        Ok(())
    }
}
