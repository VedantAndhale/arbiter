//! How clients find a running daemon: `<home>/daemon.json` holds the port and
//! a per-start bearer token. The token keeps other local users' processes and
//! web pages (via DNS rebinding) from driving agents on this machine.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

impl DaemonInfo {
    pub fn path(home: &Path) -> PathBuf {
        home.join("daemon.json")
    }

    /// Atomic write so clients never read a half-written file.
    pub fn write(&self, home: &Path) -> Result<()> {
        let tmp = home.join("daemon.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, Self::path(home))?;
        Ok(())
    }

    pub fn read(home: &Path) -> Result<Self> {
        let path = Self::path(home);
        let bytes = std::fs::read(&path).with_context(|| format!("no daemon running ({} missing)", path.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// `$ARBITER_HOME`, else `~/.arbiter`.
pub fn arbiter_home() -> PathBuf {
    std::env::var_os("ARBITER_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".arbiter")))
        .expect("cannot determine home directory; set ARBITER_HOME")
}
