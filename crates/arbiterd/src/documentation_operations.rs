//! Bounded cancellation registry; stores signals only, never documentation results.
use crate::AppState;
use anyhow::{Result, ensure};
use std::time::{Duration, Instant};
use tokio::sync::watch;
pub(crate) type Operations = std::collections::HashMap<String, (Instant, watch::Sender<bool>)>;
fn valid_id(id: &str) -> Result<()> {
    ensure!(
        (8..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'),
        "Invalid documentation operation ID"
    );
    Ok(())
}
impl AppState {
    pub(crate) async fn documentation_operation<T>(
        &self,
        id: &str,
        work: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        valid_id(id)?;
        let mut cancellation = {
            let mut operations = self.inner.documentation_operations.lock().unwrap();
            operations.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(300));
            if let Some((_, tx)) = operations.get(id) {
                ensure!(!*tx.borrow(), "Documentation operation cancelled");
                anyhow::bail!("Documentation operation already running");
            }
            ensure!(operations.len() < 128, "Too many documentation operations; retry later");
            let (tx, rx) = watch::channel(false);
            operations.insert(id.into(), (Instant::now(), tx));
            rx
        };
        let result = tokio::select! {
            _=cancellation.changed()=>Err(anyhow::anyhow!("Documentation operation cancelled")),
            result=tokio::time::timeout(Duration::from_secs(240),work)=>result.map_err(|_|anyhow::anyhow!("Documentation operation timed out")).and_then(|r|r),
        };
        self.inner.documentation_operations.lock().unwrap().remove(id);
        result
    }
    pub(crate) fn cancel_documentation_operation(&self, id: &str) -> Result<()> {
        valid_id(id)?;
        let mut operations = self.inner.documentation_operations.lock().unwrap();
        operations.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(300));
        if let Some((_, tx)) = operations.get(id) {
            tx.send_replace(true);
        } else {
            // The cancel can arrive before the original POST. Keep a short-lived tombstone.
            ensure!(operations.len() < 128, "Too many documentation operations; retry later");
            let (tx, _) = watch::channel(true);
            operations.insert(id.into(), (Instant::now(), tx));
        }
        Ok(())
    }
}
