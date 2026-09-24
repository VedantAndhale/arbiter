//! Durable setup preferences and conservative cloud admission. No provider I/O.
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Setup changed elsewhere. Reload before saving.")]
    Conflict,
    #[error("Invalid setup preferences")]
    Invalid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Local,
    #[default]
    Subscription,
    Paid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub revision: u64,
    pub step: u8,
    pub complete: bool,
    pub purpose: String,
    pub mode: Mode,
    pub network: bool,
    pub paid_consent: bool,
    pub max_cloud_runs: usize,
    pub reviewer_model: Option<String>,
    pub documentation_model: Option<String>,
    pub context7_enabled: bool,
    /// Local model searches and reads the web; agents get only its summary.
    pub web_research_enabled: bool,
    /// Only run cloud agents when remaining allowance is reported and paid
    /// extras are confirmed off. Off by default: most CLIs cannot report
    /// either, and the provider still enforces the plan's own limits.
    pub strict_allowance: bool,
    /// Download app updates in the background and install them when the app
    /// closes. On by default.
    #[serde(default = "on")]
    pub auto_update: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            revision: 0,
            step: 0,
            complete: false,
            purpose: "Build and maintain software".into(),
            mode: Mode::Subscription,
            network: true,
            paid_consent: false,
            max_cloud_runs: 1,
            reviewer_model: None,
            documentation_model: None,
            context7_enabled: false,
            web_research_enabled: false,
            strict_allowance: false,
            auto_update: true,
        }
    }
}

fn on() -> bool {
    true
}

pub struct Store(rusqlite::Connection);
impl Store {
    pub fn open(path: &std::path::Path) -> Result<Self, Error> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS setup (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);")?;
        conn.execute("INSERT OR IGNORE INTO setup VALUES (1, ?1)", [serde_json::to_string(&Preferences::default())?])?;
        Ok(Self(conn))
    }
    pub fn read(&self) -> Result<Preferences, Error> {
        let body: String = self.0.query_row("SELECT body FROM setup WHERE id=1", [], |r| r.get(0))?;
        Ok(serde_json::from_str(&body)?)
    }
    pub fn save(&mut self, mut p: Preferences) -> Result<Preferences, Error> {
        if p.step > 3
            || p.purpose.trim().is_empty()
            || p.purpose.len() > 500
            || !(1..=3).contains(&p.max_cloud_runs)
            || (p.mode == Mode::Paid && !p.paid_consent)
            || (p.complete && p.step != 3)
            || p.reviewer_model.as_ref().is_some_and(|s| s.len() > 80)
            || p.documentation_model.as_ref().is_some_and(|s| s.len() > 80)
        {
            return Err(Error::Invalid);
        }
        let tx = self.0.transaction()?;
        let body: String = tx.query_row("SELECT body FROM setup WHERE id=1", [], |r| r.get(0))?;
        let old: Preferences = serde_json::from_str(&body)?;
        if old.revision != p.revision {
            return Err(Error::Conflict);
        }
        p.revision += 1;
        tx.execute("UPDATE setup SET body=?1 WHERE id=1", [serde_json::to_string(&p)?])?;
        tx.commit()?;
        Ok(p)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Window {
    pub used_percent: f64,
    pub duration_mins: Option<u64>,
    pub resets_at: Option<i64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Account {
    pub harness: String,
    pub version: Option<String>,
    pub installed: bool,
    /// subscription, api, signed_out, unknown. Never contains identity/secrets.
    pub auth: String,
    pub plan: Option<String>,
    pub windows: Vec<Window>,
    pub credits_enabled: Option<bool>,
    pub observed_at: i64,
    pub source: String,
    pub conflict: bool,
    pub note: String,
}

pub fn admission(p: &Preferences, a: &Account, now: i64) -> Result<(), String> {
    let deny = |s: &str| Err(s.to_owned());
    if !p.complete {
        return deny("Finish setup before starting a cloud agent.");
    }
    if !p.network || p.mode == Mode::Local {
        return deny("Cloud agents are off in Settings. Local agents still work.");
    }
    if !a.installed {
        return deny("Install this harness, then refresh setup status.");
    }
    if a.conflict && p.mode != Mode::Paid {
        return deny(
            "Conflicting API credentials or provider configuration detected. Resolve them and refresh status.",
        );
    }
    if now < a.observed_at || now - a.observed_at > 120 {
        return deny("Account status is stale. Refresh usage in Settings before continuing.");
    }
    if !matches!(a.auth.as_str(), "subscription" | "api") {
        return deny("Sign in through the official harness, then refresh setup status.");
    }
    if p.mode == Mode::Paid && p.paid_consent {
        return Ok(());
    }
    if a.auth != "subscription" {
        return deny("Subscription-only mode blocks API billing. Sign in with your subscription.");
    }
    // Paid extras known to be on are never used without explicit consent.
    if a.credits_enabled == Some(true) {
        return deny(
            "Paid extra credits are turned on for this account. Turn them off with the provider, or allow paid usage in Settings.",
        );
    }
    if p.strict_allowance && a.credits_enabled != Some(false) {
        return deny("Strict protection: this account does not report whether paid extras are off.");
    }
    if p.strict_allowance && a.windows.is_empty() {
        return deny("Strict protection: this account does not report its remaining allowance.");
    }
    if a.windows.iter().any(|w| {
        !w.used_percent.is_finite()
            || !(0.0..90.0).contains(&w.used_percent)
            || w.resets_at.is_none_or(|reset| reset <= now)
    }) {
        return deny("This plan's allowance is nearly used up. Work waits for the reset or goes to another agent.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_and_rejects_stale_updates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("setup.db");
        let mut s = Store::open(&path).unwrap();
        let p = s.read().unwrap();
        let saved = s.save(p.clone()).unwrap();
        assert!(matches!(s.save(p), Err(Error::Conflict)));
        drop(s);
        assert_eq!(Store::open(&path).unwrap().read().unwrap().revision, saved.revision);
    }
    #[test]
    fn strict_admission_never_interprets_unknown_as_free() {
        let p = Preferences { complete: true, step: 3, ..Default::default() };
        let mut a = Account {
            installed: true,
            auth: "subscription".into(),
            credits_enabled: Some(false),
            observed_at: 100,
            windows: vec![Window { used_percent: 20.0, resets_at: Some(500), ..Default::default() }],
            ..Default::default()
        };
        assert!(admission(&p, &a, 101).is_ok());
        a.credits_enabled = Some(false);
        a.auth = "api".into();
        assert!(admission(&p, &a, 101).is_err());
        a.auth = "subscription".into();
        a.windows[0].used_percent = 95.0;
        assert!(admission(&p, &a, 101).is_err());
        a.windows[0].used_percent = 20.0;
        assert!(admission(&p, &a, 250).is_err());
        a.conflict = true;
        assert!(admission(&p, &a, 101).is_err());
        a.conflict = false;
        // Unknown allowance or credit status: allowed by default (the provider
        // enforces the plan), refused in strict mode; known paid extras never.
        a.windows.clear();
        assert!(admission(&p, &a, 101).is_ok());
        a.credits_enabled = None;
        assert!(admission(&p, &a, 101).is_ok());
        a.credits_enabled = Some(true);
        assert!(admission(&p, &a, 101).is_err());
        a.credits_enabled = None;
        let strict = Preferences { strict_allowance: true, ..p.clone() };
        assert!(admission(&strict, &a, 101).is_err());
        a.credits_enabled = Some(false);
        assert!(admission(&strict, &a, 101).is_err(), "strict needs a reported allowance too");
    }
}
