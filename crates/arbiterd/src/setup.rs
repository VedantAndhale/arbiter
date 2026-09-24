use crate::AppState;
use anyhow::Result;
use arbiter_adapters::{Harness, account};
use arbiter_setup::{Account, Preferences};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

impl AppState {
    pub(crate) fn setup_state(&self) -> Result<Value> {
        let p = self.inner.setup.lock().unwrap().read()?;
        let accounts = self.inner.accounts.lock().unwrap().clone();
        let (memory_bytes, free_disk_bytes) = arbiter_supervisor::hardware::capacity(&self.inner.home);
        let readiness: Vec<_> = accounts
            .iter()
            .map(|a| {
                json!({"harness":a.harness,
            "blocked":arbiter_setup::admission(&p,a,account::now()).err()})
            })
            .collect();
        Ok(json!({"preferences":p,"accounts":accounts,"readiness":readiness,
            "hardware":{"logical_cpus":std::thread::available_parallelism().map(|n|n.get()).unwrap_or(1),
                "os":std::env::consts::OS,"arch":std::env::consts::ARCH,"memory_bytes":memory_bytes,"free_disk_bytes":free_disk_bytes},
            "local_coding_available":self.inner.intake.status()["models"].as_array().is_some_and(|ms|ms.iter().any(|m|m["ready"]==true&&m["model"]["id"]!="potion"))}))
    }
    pub(crate) fn save_setup(&self, p: Preferences) -> Result<Value> {
        let _lock = self.inner.admission_lock.lock().unwrap();
        let network = p.network;
        self.inner.setup.lock().unwrap().save(p)?;
        if !network {
            self.inner.intake.cancel_downloads();
        }
        self.stop_all_for_setup()?;
        self.setup_state()
    }
    pub(crate) async fn refresh_accounts(&self) -> Result<Value> {
        let mut last = self.inner.account_refresh.lock().await;
        let p = self.inner.setup.lock().unwrap().read()?;
        anyhow::ensure!(p.network, "Network access is disabled in Settings");
        if !self.inner.fixture_accounts && last.elapsed() >= Duration::from_secs(30) {
            let (claude, codex) = tokio::join!(account::probe(Harness::Claude), account::probe(Harness::Codex));
            *self.inner.accounts.lock().unwrap() = vec![claude, codex];
            *last = Instant::now();
        }
        self.setup_state()
    }
    pub(crate) fn spawn_account_monitor(&self) {
        if self.inner.fixture_accounts {
            return;
        }
        let state = self.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(60));
            loop {
                timer.tick().await;
                let enabled = state
                    .inner
                    .setup
                    .lock()
                    .unwrap()
                    .read()
                    .is_ok_and(|p| p.complete && p.network && p.mode != arbiter_setup::Mode::Local);
                if enabled {
                    let _ = state.refresh_accounts().await;
                }
            }
        });
    }
    pub(crate) fn check_cloud(&self, h: Harness) -> Result<()> {
        let p = self.inner.setup.lock().unwrap().read()?;
        let name = match h {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
        };
        let accounts = self.inner.accounts.lock().unwrap();
        let a = accounts
            .iter()
            .find(|a| a.harness == name)
            .ok_or_else(|| anyhow::anyhow!("Refresh account status in Settings before starting an agent."))?;
        if !self.inner.fixture_accounts && p.mode != arbiter_setup::Mode::Paid {
            anyhow::ensure!(
                !account::environment_conflict(h),
                "Conflicting API/provider environment. Resolve it and refresh Setup."
            );
        }
        arbiter_setup::admission(&p, a, account::now()).map_err(anyhow::Error::msg)
    }
    pub(crate) fn max_cloud_runs(&self) -> Result<usize> {
        Ok(self.inner.setup.lock().unwrap().read()?.max_cloud_runs)
    }
}

/// Deterministic, no-token evidence for examples/tests using fake launchers.
pub fn fixture_accounts() -> Vec<Account> {
    ["claude", "codex"]
        .into_iter()
        .map(|h| Account {
            harness: h.into(),
            installed: true,
            auth: "subscription".into(),
            observed_at: account::now(),
            credits_enabled: Some(false),
            source: "Simulated account (fake launcher)".into(),
            windows: vec![arbiter_setup::Window {
                used_percent: 10.0,
                resets_at: Some(account::now() + 86400),
                duration_mins: Some(300),
            }],
            ..Default::default()
        })
        .collect()
}
