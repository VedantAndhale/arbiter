use crate::AppState;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

impl AppState {
    fn context7_target(&self) -> String {
        // One credential per daemon home, independent of project locations.
        let path = std::fs::canonicalize(&self.inner.home).unwrap_or(self.inner.home.clone());
        format!("Arbiter/Context7/{}", arbiter_project::hash(path.to_string_lossy().as_bytes()))
    }
    pub(crate) fn context7_key(&self) -> Result<Option<String>> {
        if let Some(key) = self.inner.context7_session_key.lock().unwrap().clone() {
            return Ok(Some(key));
        }
        if let Some(key) =
            self.inner.credential_store.read(&self.context7_target()).context("Cannot read the OS credential store")?
        {
            return Ok(Some(key));
        }
        Ok(std::env::var("CONTEXT7_API_KEY").ok().filter(|s| !s.is_empty()))
    }
    pub(crate) fn credential_status(&self) -> Result<Value> {
        let source = if self.inner.context7_session_key.lock().unwrap().is_some() {
            "session"
        } else if self
            .inner
            .credential_store
            .read(&self.context7_target())
            .context("Cannot read the OS credential store")?
            .is_some()
        {
            "os_store"
        } else if std::env::var("CONTEXT7_API_KEY").is_ok_and(|s| !s.is_empty()) {
            "environment"
        } else {
            "none"
        };
        Ok(
            json!({"configured":source!="none","source":source,"persistent_available":self.inner.credential_store.persistent()}),
        )
    }
    pub(crate) fn set_context7_key(&self, key: &str, persist: bool) -> Result<Value> {
        ensure!(
            key.starts_with("ctx7sk")
                && (12..=2500).contains(&key.len())
                && key.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
            "Enter a valid Context7 API key; it must start with ctx7sk"
        );
        if persist {
            ensure!(
                self.inner.credential_store.persistent(),
                "OS credential storage unavailable. Choose session-only storage."
            );
            self.inner
                .credential_store
                .write(&self.context7_target(), key)
                .context("Could not save the key in OS credential storage")?;
            *self.inner.context7_session_key.lock().unwrap() = None;
        } else {
            *self.inner.context7_session_key.lock().unwrap() = Some(key.into());
        }
        self.credential_status()
    }
    pub(crate) fn remove_context7_key(&self) -> Result<Value> {
        self.inner.credential_store.delete(&self.context7_target()).context("Could not remove the stored key")?;
        *self.inner.context7_session_key.lock().unwrap() = None;
        self.credential_status()
    }
}
