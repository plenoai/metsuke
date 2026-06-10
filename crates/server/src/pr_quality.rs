use std::fmt::Write;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Configuration for PR quality checks.
pub struct QualityConfig {
    pub max_changed_files: u32,
    pub max_changed_lines: u32,
    pub min_account_age_days: u32,
    pub min_global_merge_ratio: f64,
    pub require_description: bool,
    pub max_description_length: usize,
    pub blocked_paths: Vec<String>,
    pub max_failures_before_close: u32,
}

impl Default for QualityConfig {
    fn default() -> Self {
        Self {
            max_changed_files: 50,
            max_changed_lines: 10_000,
            min_account_age_days: 30,
            min_global_merge_ratio: 0.3,
            require_description: true,
            max_description_length: 2500,
            blocked_paths: vec![
                ".github/workflows/".into(),
                ".github/VOUCHED.td".into(),
                ".goreleaser.yaml".into(),
            ],
            max_failures_before_close: 4,
        }
    }
}

#[derive(Debug)]
pub struct QualityCheck {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug)]
pub struct QualityReport {
    pub checks: Vec<QualityCheck>,
}

impl QualityReport {
    pub fn failed_count(&self) -> usize {
        self.checks.iter().filter(|c| !c.passed).count()
    }

    pub fn format_check_summary(&self, config: &QualityConfig) -> (String, String, String) {
        let failed = self.failed_count();
        let total = self.checks.len();
        let passed = total - failed;

        let threshold_breached = failed as u32 >= config.max_failures_before_close;
        let conclusion = if threshold_breached {
            "failure"
        } else {
            "success"
        };

        let title = format!("PR Quality: {passed}/{total} passed, {failed} failed");

        let mut summary = String::from(
            "## PR Quality Checks\n\n| Check | Status | Detail |\n|-------|--------|--------|\n",
        );
        for c in &self.checks {
            let icon = if c.passed { "✅" } else { "❌" };
            let _ = writeln!(summary, "| {} | {} | {} |", c.name, icon, c.detail);
        }

        if threshold_breached {
            let _ = write!(
                summary,
                "\n> **{failed}** checks failed (threshold: {}). PR does not meet quality requirements.",
                config.max_failures_before_close
            );
        }

        summary.push_str(
            "\n\n> This check runs via the [metsuke](https://github.com/plenoai/metsuke) GitHub App.",
        );

        (conclusion.to_string(), title, summary)
    }
}

/// Information extracted from the PR webhook payload + API calls.
pub struct PrInfo {
    pub title: String,
    pub body: String,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub author_login: String,
    pub author_association: String,
    pub file_paths: Vec<String>,
    pub author_account_age_days: u32,
    pub author_global_merge_ratio: Option<f64>,
    pub author_profile_signals: u32,
}

pub fn run_quality_checks(pr: &PrInfo, config: &QualityConfig) -> QualityReport {
    let mut checks = Vec::new();

    // Description check
    if config.require_description {
        let body_trimmed = pr.body.trim();
        let passed = !body_trimmed.is_empty();
        checks.push(QualityCheck {
            name: "Description present",
            passed,
            detail: if passed {
                format!("{} chars", body_trimmed.len())
            } else {
                "PR has no description".into()
            },
        });
    }

    // Description length
    if config.max_description_length > 0 {
        let len = pr.body.len();
        let passed = len <= config.max_description_length;
        checks.push(QualityCheck {
            name: "Description length",
            passed,
            detail: format!("{len}/{} chars", config.max_description_length),
        });
    }

    // Changed files
    if config.max_changed_files > 0 {
        let passed = pr.changed_files <= config.max_changed_files;
        checks.push(QualityCheck {
            name: "Changed files count",
            passed,
            detail: format!("{}/{}", pr.changed_files, config.max_changed_files),
        });
    }

    // Changed lines
    if config.max_changed_lines > 0 {
        let total = pr.additions + pr.deletions;
        let passed = total <= config.max_changed_lines;
        checks.push(QualityCheck {
            name: "Changed lines count",
            passed,
            detail: format!(
                "+{}/-{} = {}/{}",
                pr.additions, pr.deletions, total, config.max_changed_lines
            ),
        });
    }

    // Account age
    if config.min_account_age_days > 0 {
        let passed = pr.author_account_age_days >= config.min_account_age_days;
        checks.push(QualityCheck {
            name: "Account age",
            passed,
            detail: format!(
                "{} days (min: {})",
                pr.author_account_age_days, config.min_account_age_days
            ),
        });
    }

    // Global merge ratio
    if config.min_global_merge_ratio > 0.0 {
        if let Some(ratio) = pr.author_global_merge_ratio {
            let passed = ratio >= config.min_global_merge_ratio;
            checks.push(QualityCheck {
                name: "Global merge ratio",
                passed,
                detail: format!(
                    "{:.0}% (min: {:.0}%)",
                    ratio * 100.0,
                    config.min_global_merge_ratio * 100.0,
                ),
            });
        } else {
            checks.push(QualityCheck {
                name: "Global merge ratio",
                passed: true,
                detail: "No PR history (new contributor)".into(),
            });
        }
    }

    // Blocked paths
    if !config.blocked_paths.is_empty() {
        let blocked: Vec<&str> = pr
            .file_paths
            .iter()
            .filter(|p| {
                config
                    .blocked_paths
                    .iter()
                    .any(|b| p.starts_with(b.as_str()))
            })
            .map(|p| p.as_str())
            .collect();
        let passed = blocked.is_empty();
        checks.push(QualityCheck {
            name: "Protected paths",
            passed,
            detail: if passed {
                "No protected files modified".into()
            } else {
                format!("Blocked: {}", blocked.join(", "))
            },
        });
    }

    QualityReport { checks }
}

// --- GitHub API helpers for fetching contributor info ---

#[derive(Deserialize)]
pub struct GitHubUser {
    pub login: String,
    pub created_at: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub bio: Option<String>,
    pub company: Option<String>,
    pub location: Option<String>,
    pub blog: Option<String>,
    pub twitter_username: Option<String>,
    pub public_repos: u32,
    pub followers: u32,
}

impl GitHubUser {
    pub fn profile_signal_count(&self) -> u32 {
        let mut count = 0u32;
        if self.name.as_ref().is_some_and(|s| !s.is_empty()) {
            count += 1;
        }
        if self.email.as_ref().is_some_and(|s| !s.is_empty()) {
            count += 1;
        }
        if self.bio.as_ref().is_some_and(|s| !s.is_empty()) {
            count += 1;
        }
        if self.company.as_ref().is_some_and(|s| !s.is_empty()) {
            count += 1;
        }
        if self.location.as_ref().is_some_and(|s| !s.is_empty()) {
            count += 1;
        }
        if self.blog.as_ref().is_some_and(|s| !s.is_empty()) {
            count += 1;
        }
        if self
            .twitter_username
            .as_ref()
            .is_some_and(|s| !s.is_empty())
        {
            count += 1;
        }
        count
    }

    pub fn account_age_days(&self) -> u32 {
        let Ok(created) = chrono::DateTime::parse_from_rfc3339(&self.created_at) else {
            return 0;
        };
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(created);
        duration.num_days().max(0) as u32
    }
}

#[derive(Deserialize)]
struct SearchResult {
    total_count: u32,
}

/// Fetch the user's global merged / total PR counts to compute merge ratio.
pub async fn fetch_global_merge_ratio(
    http: &reqwest::Client,
    token: &str,
    api_base: &str,
    username: &str,
) -> Result<Option<f64>> {
    let total: SearchResult = http
        .get(format!("{api_base}/search/issues"))
        .query(&[("q", &format!("author:{username} type:pr is:closed"))])
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .context("search closed PRs")?
        .json()
        .await?;

    if total.total_count == 0 {
        return Ok(None);
    }

    let merged: SearchResult = http
        .get(format!("{api_base}/search/issues"))
        .query(&[("q", &format!("author:{username} type:pr is:merged"))])
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .context("search merged PRs")?
        .json()
        .await?;

    Ok(Some(merged.total_count as f64 / total.total_count as f64))
}

#[derive(Deserialize)]
pub struct PrFile {
    pub filename: String,
}

pub async fn fetch_pr_files(
    http: &reqwest::Client,
    token: &str,
    api_base: &str,
    owner: &str,
    repo: &str,
    pr_number: u32,
) -> Result<Vec<PrFile>> {
    http.get(format!(
        "{api_base}/repos/{owner}/{repo}/pulls/{pr_number}/files"
    ))
    .query(&[("per_page", "100")])
    .header("Authorization", format!("Bearer {token}"))
    .header("Accept", "application/vnd.github+json")
    .send()
    .await?
    .error_for_status()
    .context("fetch PR files")?
    .json()
    .await
    .context("parse PR files")
}

pub async fn fetch_github_user(
    http: &reqwest::Client,
    token: &str,
    api_base: &str,
    username: &str,
) -> Result<GitHubUser> {
    http.get(format!("{api_base}/users/{username}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .context("fetch user")?
        .json()
        .await
        .context("parse user")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_pr() -> PrInfo {
        PrInfo {
            title: "fix: something".into(),
            body: "Fixes a bug.".into(),
            changed_files: 3,
            additions: 50,
            deletions: 10,
            author_login: "contributor".into(),
            author_association: "NONE".into(),
            file_paths: vec!["src/main.rs".into(), "src/lib.rs".into()],
            author_account_age_days: 365,
            author_global_merge_ratio: Some(0.8),
            author_profile_signals: 5,
        }
    }

    #[test]
    fn all_pass() {
        let report = run_quality_checks(&default_pr(), &QualityConfig::default());
        assert_eq!(report.failed_count(), 0);
        let (conclusion, _, _) = report.format_check_summary(&QualityConfig::default());
        assert_eq!(conclusion, "success");
    }

    #[test]
    fn empty_description_fails() {
        let mut pr = default_pr();
        pr.body = String::new();
        let report = run_quality_checks(&pr, &QualityConfig::default());
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.name == "Description present" && !c.passed)
        );
    }

    #[test]
    fn too_many_files_fails() {
        let mut pr = default_pr();
        pr.changed_files = 100;
        let report = run_quality_checks(&pr, &QualityConfig::default());
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.name == "Changed files count" && !c.passed)
        );
    }

    #[test]
    fn too_many_lines_fails() {
        let mut pr = default_pr();
        pr.additions = 20000;
        let report = run_quality_checks(&pr, &QualityConfig::default());
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.name == "Changed lines count" && !c.passed)
        );
    }

    #[test]
    fn young_account_fails() {
        let mut pr = default_pr();
        pr.author_account_age_days = 5;
        let report = run_quality_checks(&pr, &QualityConfig::default());
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.name == "Account age" && !c.passed)
        );
    }

    #[test]
    fn low_merge_ratio_fails() {
        let mut pr = default_pr();
        pr.author_global_merge_ratio = Some(0.1);
        let report = run_quality_checks(&pr, &QualityConfig::default());
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.name == "Global merge ratio" && !c.passed)
        );
    }

    #[test]
    fn blocked_path_fails() {
        let mut pr = default_pr();
        pr.file_paths = vec![".github/workflows/ci.yml".into()];
        let report = run_quality_checks(&pr, &QualityConfig::default());
        assert!(
            report
                .checks
                .iter()
                .any(|c| c.name == "Protected paths" && !c.passed)
        );
    }

    #[test]
    fn threshold_triggers_failure_conclusion() {
        let mut pr = default_pr();
        pr.body = String::new();
        pr.changed_files = 100;
        pr.additions = 20000;
        pr.author_account_age_days = 5;
        let config = QualityConfig {
            max_failures_before_close: 4,
            ..QualityConfig::default()
        };
        let report = run_quality_checks(&pr, &config);
        assert!(report.failed_count() >= 4);
        let (conclusion, _, _) = report.format_check_summary(&config);
        assert_eq!(conclusion, "failure");
    }

    #[test]
    fn new_contributor_no_merge_history_passes() {
        let mut pr = default_pr();
        pr.author_global_merge_ratio = None;
        let report = run_quality_checks(&pr, &QualityConfig::default());
        assert!(report.checks.iter().all(|c| c.passed));
    }
}
