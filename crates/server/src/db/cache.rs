use anyhow::Result;

use super::Database;
use super::types::{CachedPullRow, CachedReleaseRow, RepoRow};

impl Database {
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
                    r.default_branch, r.pushed_at, r.synced_at
             FROM repositories r
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
}
