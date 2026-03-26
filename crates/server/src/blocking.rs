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
