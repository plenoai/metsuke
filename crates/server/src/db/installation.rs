use anyhow::Result;

use super::Database;

impl Database {
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
}
