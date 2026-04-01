use anyhow::Result;
use rusqlite::OptionalExtension;

use super::Database;
use super::types::{AuditEntry, VerificationSummary};

impl Database {
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

    /// Get the latest repo verification result_json for a specific repo.
    pub fn get_latest_repo_verification(
        &self,
        user_id: i64,
        owner: &str,
        repo: &str,
    ) -> Result<Option<String>> {
        let conn = self.reader();
        let mut stmt = conn.prepare_cached(
            "SELECT result_json FROM audit_log
             WHERE user_id = ?1 AND verification_type = 'repo' AND owner = ?2 AND repo = ?3
             ORDER BY id DESC LIMIT 1",
        )?;
        let result = stmt
            .query_row(rusqlite::params![user_id, owner, repo], |row| row.get(0))
            .optional()?;
        Ok(result)
    }

    /// Get latest verification results for all target_refs of a given type in a repo.
    pub fn get_latest_verifications_by_type(
        &self,
        user_id: i64,
        verification_type: &str,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<VerificationSummary>> {
        let conn = self.reader();
        let mut stmt = conn.prepare_cached(
            "SELECT target_ref, pass_count, fail_count, review_count, na_count
             FROM audit_log a
             WHERE a.user_id = ?1 AND a.verification_type = ?2 AND a.owner = ?3 AND a.repo = ?4
               AND a.id = (SELECT MAX(b.id) FROM audit_log b
                           WHERE b.user_id = a.user_id AND b.verification_type = a.verification_type
                             AND b.owner = a.owner AND b.repo = a.repo AND b.target_ref = a.target_ref)
             ORDER BY a.id DESC",
        )?;
        let rows = stmt
            .query_map(
                rusqlite::params![user_id, verification_type, owner, repo],
                |row| {
                    Ok(VerificationSummary {
                        target_ref: row.get(0)?,
                        pass_count: row.get(1)?,
                        fail_count: row.get(2)?,
                        review_count: row.get(3)?,
                        na_count: row.get(4)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_latest_verification_by_ref(
        &self,
        user_id: i64,
        owner: &str,
        repo: &str,
        target_ref: &str,
    ) -> Result<Option<String>> {
        let conn = self.reader();
        let mut stmt = conn.prepare_cached(
            "SELECT result_json FROM audit_log
             WHERE user_id = ?1 AND owner = ?2 AND repo = ?3 AND target_ref = ?4
             ORDER BY id DESC LIMIT 1",
        )?;
        let result = stmt
            .query_row(rusqlite::params![user_id, owner, repo, target_ref], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(result)
    }
}
