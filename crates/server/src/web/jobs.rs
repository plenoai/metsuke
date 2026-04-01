use crate::blocking::run_blocking;
use crate::db::{CachedPullRow, CachedReleaseRow, RepoRow};

use super::JobEvent;
use super::WebState;
use super::repos::sync_installations;

pub(super) fn spawn_sync_repos_job(state: &WebState, user_id: i64) {
    let state = state.clone();
    tokio::spawn(async move {
        sync_installations(&state, user_id).await;

        let db = state.db.clone();
        let installations = run_blocking(move || db.get_installations_for_user(user_id))
            .await
            .unwrap_or_default();

        let mut join_set = tokio::task::JoinSet::new();
        for (installation_id, account_login, _account_type) in installations {
            let github_app = state.github_app.clone();
            join_set.spawn(async move {
                match github_app.list_installation_repos(installation_id).await {
                    Ok(repos) => repos
                        .into_iter()
                        .map(|r| RepoRow {
                            owner: account_login.clone(),
                            name: r.name,
                            full_name: r.full_name,
                            private: r.private,
                            description: r.description,
                            language: r.language,
                            default_branch: r.default_branch,
                            pushed_at: r.pushed_at,
                            synced_at: String::new(), // set by DB
                        })
                        .collect::<Vec<_>>(),
                    Err(e) => {
                        tracing::warn!("Failed to list repos for {account_login}: {e:#}");
                        Vec::new()
                    }
                }
            });
        }

        let mut all_repos = Vec::new();
        while let Some(result) = join_set.join_next().await {
            if let Ok(batch) = result {
                all_repos.extend(batch);
            }
        }

        let db = state.db.clone();
        if let Err(e) = run_blocking(move || db.upsert_repositories(user_id, &all_repos)).await {
            tracing::warn!("Failed to save repos to DB: {e:#}");
            return;
        }

        let _ = state.events_tx.send(JobEvent::ReposSynced { user_id });
    });
}

pub(super) fn spawn_sync_pulls_job(state: &WebState, user_id: i64, owner: String, repo: String) {
    let state = state.clone();
    tokio::spawn(async move {
        let db = state.db.clone();
        let owner_q = owner.clone();
        let installation_id =
            match run_blocking(move || db.get_installation_for_owner(user_id, &owner_q)).await {
                Ok(Some(id)) => id,
                _ => return,
            };

        let pulls = match state
            .github_app
            .list_pull_requests(installation_id, &owner, &repo)
            .await
        {
            Ok(prs) => prs
                .into_iter()
                .map(|p| CachedPullRow {
                    pr_number: p.number as i64,
                    title: p.title,
                    state: p.state,
                    author: p.user.login,
                    created_at: p.created_at,
                    updated_at: p.updated_at,
                    merged_at: p.merged_at,
                    draft: p.draft.unwrap_or(false),
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!("Failed to list pulls for {owner}/{repo}: {e:#}");
                return;
            }
        };

        let db = state.db.clone();
        let o = owner.clone();
        let r = repo.clone();
        if let Err(e) = run_blocking(move || db.upsert_cached_pulls(user_id, &o, &r, &pulls)).await
        {
            tracing::warn!("Failed to save pulls to DB: {e:#}");
            return;
        }

        let _ = state.events_tx.send(JobEvent::PullsSynced {
            user_id,
            owner,
            repo,
        });
    });
}

pub(super) fn spawn_sync_releases_job(state: &WebState, user_id: i64, owner: String, repo: String) {
    let state = state.clone();
    tokio::spawn(async move {
        let db = state.db.clone();
        let owner_q = owner.clone();
        let installation_id =
            match run_blocking(move || db.get_installation_for_owner(user_id, &owner_q)).await {
                Ok(Some(id)) => id,
                _ => return,
            };

        let releases = match state
            .github_app
            .list_releases(installation_id, &owner, &repo)
            .await
        {
            Ok(rels) => rels
                .into_iter()
                .map(|r| CachedReleaseRow {
                    release_id: r.id,
                    tag_name: r.tag_name,
                    name: r.name,
                    draft: r.draft,
                    prerelease: r.prerelease,
                    created_at: r.created_at,
                    published_at: r.published_at,
                    author: r.author.login,
                    html_url: r.html_url,
                    body: r.body,
                })
                .collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!("Failed to list releases for {owner}/{repo}: {e:#}");
                return;
            }
        };

        let db = state.db.clone();
        let o = owner.clone();
        let r = repo.clone();
        if let Err(e) =
            run_blocking(move || db.upsert_cached_releases(user_id, &o, &r, &releases)).await
        {
            tracing::warn!("Failed to save releases to DB: {e:#}");
            return;
        }

        let _ = state.events_tx.send(JobEvent::ReleasesSynced {
            user_id,
            owner,
            repo,
        });
    });
}
