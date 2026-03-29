use tokio::task::spawn_blocking;

pub async fn run_blocking<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    spawn_blocking(f)
        .await
        .map_err(|e| anyhow::anyhow!("blocking task panicked: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_blocking_returns_ok() {
        let result = run_blocking(|| Ok(42)).await.unwrap();
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn run_blocking_propagates_error() {
        let result = run_blocking(|| Err::<(), _>(anyhow::anyhow!("oops"))).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("oops"));
    }

    #[tokio::test]
    async fn run_blocking_catches_panic() {
        let result = run_blocking(|| -> anyhow::Result<()> { panic!("boom") }).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("panicked"));
    }
}
