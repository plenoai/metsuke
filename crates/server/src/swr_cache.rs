use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Server-side SWR (Stale-While-Revalidate) cache.
///
/// - **fresh** (within `max_age`): return cached value immediately.
/// - **stale** (between `max_age` and `max_age + stale_ttl`): return cached
///   value immediately AND the caller should spawn a background revalidation.
/// - **expired** (beyond `max_age + stale_ttl`): treat as cache miss.
#[derive(Clone)]
pub struct SwrCache {
    inner: Arc<RwLock<HashMap<String, CacheEntry>>>,
    max_age: Duration,
    stale_ttl: Duration,
}

struct CacheEntry {
    value: Arc<Value>,
    inserted_at: Instant,
    revalidating: bool,
}

/// Result of a cache lookup.
pub enum CacheStatus {
    /// Data is fresh — return as-is, no revalidation needed.
    Fresh(Arc<Value>),
    /// Data is stale — return it but caller should revalidate in background.
    Stale(Arc<Value>),
    /// No usable cache entry.
    Miss,
}

impl SwrCache {
    pub fn new(max_age: Duration, stale_ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            max_age,
            stale_ttl,
        }
    }

    pub async fn get(&self, key: &str) -> CacheStatus {
        let now = Instant::now();
        let map = self.inner.read().await;
        let Some(entry) = map.get(key) else {
            return CacheStatus::Miss;
        };

        let age = now.duration_since(entry.inserted_at);

        if age <= self.max_age {
            CacheStatus::Fresh(Arc::clone(&entry.value))
        } else if age <= self.max_age + self.stale_ttl && !entry.revalidating {
            CacheStatus::Stale(Arc::clone(&entry.value))
        } else if age <= self.max_age + self.stale_ttl {
            // Stale but already revalidating — treat as fresh to avoid duplicate work
            CacheStatus::Fresh(Arc::clone(&entry.value))
        } else {
            CacheStatus::Miss
        }
    }

    /// Mark a key as currently revalidating to avoid duplicate background tasks.
    pub async fn mark_revalidating(&self, key: &str) {
        let mut map = self.inner.write().await;
        if let Some(entry) = map.get_mut(key) {
            entry.revalidating = true;
        }
    }

    /// Insert or update a cache entry. Also evicts expired entries.
    pub async fn set(&self, key: String, value: impl Serialize) {
        let value = serde_json::to_value(value).unwrap_or_default();
        let now = Instant::now();
        let expiry = self.max_age + self.stale_ttl;
        let mut map = self.inner.write().await;
        map.retain(|_, e| now.duration_since(e.inserted_at) <= expiry);
        map.insert(
            key,
            CacheEntry {
                value: Arc::new(value),
                inserted_at: now,
                revalidating: false,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short_cache() -> SwrCache {
        SwrCache::new(Duration::from_millis(100), Duration::from_millis(200))
    }

    #[tokio::test]
    async fn fresh_within_max_age() {
        let cache = short_cache();
        cache.set("k".into(), serde_json::json!(42)).await;
        assert!(matches!(cache.get("k").await, CacheStatus::Fresh(_)));
    }

    #[tokio::test]
    async fn stale_after_max_age() {
        let cache = short_cache();
        cache.set("k".into(), serde_json::json!(1)).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(matches!(cache.get("k").await, CacheStatus::Stale(_)));
    }

    #[tokio::test]
    async fn miss_after_total_ttl() {
        let cache = short_cache();
        cache.set("k".into(), serde_json::json!(1)).await;
        tokio::time::sleep(Duration::from_millis(350)).await;
        assert!(matches!(cache.get("k").await, CacheStatus::Miss));
    }

    #[tokio::test]
    async fn revalidating_suppresses_stale() {
        let cache = short_cache();
        cache.set("k".into(), serde_json::json!(1)).await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        assert!(matches!(cache.get("k").await, CacheStatus::Stale(_)));
        cache.mark_revalidating("k").await;
        // Now returns Fresh to avoid duplicate background work
        assert!(matches!(cache.get("k").await, CacheStatus::Fresh(_)));
    }

    #[tokio::test]
    async fn set_evicts_expired_entries() {
        let cache = short_cache();
        cache.set("old".into(), serde_json::json!(1)).await;
        tokio::time::sleep(Duration::from_millis(350)).await;

        cache.set("new".into(), serde_json::json!(2)).await;
        assert!(matches!(cache.get("old").await, CacheStatus::Miss));
        assert!(matches!(cache.get("new").await, CacheStatus::Fresh(_)));
    }

    #[tokio::test]
    async fn miss_on_unknown_key() {
        let cache = short_cache();
        assert!(matches!(cache.get("unknown").await, CacheStatus::Miss));
    }
}
