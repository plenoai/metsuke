use std::collections::HashMap;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// A single verification target within a bulk request.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BulkTarget {
    Repo {
        #[schemars(description = "GitHub repository owner")]
        owner: String,
        #[schemars(description = "GitHub repository name")]
        repo: String,
    },
    Pr {
        #[schemars(description = "GitHub repository owner")]
        owner: String,
        #[schemars(description = "GitHub repository name")]
        repo: String,
        #[schemars(description = "Pull request number")]
        pr_number: u32,
    },
    Release {
        #[schemars(description = "GitHub repository owner")]
        owner: String,
        #[schemars(description = "GitHub repository name")]
        repo: String,
        #[schemars(description = "Base tag (older release)")]
        base_tag: String,
        #[schemars(description = "Head tag (newer release)")]
        head_tag: String,
    },
}

impl BulkTarget {
    pub fn owner(&self) -> &str {
        match self {
            Self::Repo { owner, .. } | Self::Pr { owner, .. } | Self::Release { owner, .. } => {
                owner
            }
        }
    }

    pub fn repo(&self) -> &str {
        match self {
            Self::Repo { repo, .. } | Self::Pr { repo, .. } | Self::Release { repo, .. } => repo,
        }
    }

    pub fn verification_type(&self) -> &str {
        match self {
            Self::Repo { .. } => "repo",
            Self::Pr { .. } => "pr",
            Self::Release { .. } => "release",
        }
    }

    pub fn target_ref(&self) -> String {
        match self {
            Self::Repo { .. } => "HEAD".into(),
            Self::Pr { pr_number, .. } => format!("#{pr_number}"),
            Self::Release {
                base_tag, head_tag, ..
            } => format!("{base_tag}..{head_tag}"),
        }
    }
}

/// Result of verifying a single target.
#[derive(Clone, Debug, Serialize)]
pub struct BulkTargetResult {
    pub target: BulkTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Status of a bulk verification job.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BulkJobStatus {
    Running,
    Completed,
}

/// A bulk verification job with progress tracking.
#[derive(Clone, Debug, Serialize)]
pub struct BulkJob {
    pub id: String,
    pub status: BulkJobStatus,
    pub total: usize,
    pub completed: usize,
    pub results: Vec<BulkTargetResult>,
}

pub type BulkJobStore = Arc<RwLock<HashMap<String, BulkJob>>>;

pub fn new_job_store() -> BulkJobStore {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Maximum number of concurrent verifications per bulk job.
pub const MAX_CONCURRENCY: usize = 4;

/// Maximum number of targets in a single bulk request.
pub const MAX_TARGETS: usize = 50;
