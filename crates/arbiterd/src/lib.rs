//! The Arbiter daemon. Owns the event log and all agent processes; every UI
//! (desktop, CLI, future web/mobile) is just a client of this API, so agents
//! survive UI crashes and remote access needs no architectural change.

mod api;
mod attach;
mod capability;
mod compare;
mod credentials;
mod discovery;
mod documentation;
mod documentation_handoff;
mod documentation_operations;
mod heal;
mod intake;
mod knowledge;
mod landing;
mod local;
mod memory;
mod office;
mod plans;
mod prerequisites;
mod projects;
mod publish;
mod quick_add;
mod review;
mod router;
mod runs;
mod service;
mod setup;
mod side;
mod terminal;
mod web;
mod web_research;
mod web_sites;

pub use arbiter_core::{DaemonInfo, arbiter_home};
pub use documentation::DocumentationTransport;
pub use local::LocalGenerator;
pub use memory::restore;
pub use runs::{Availability, IDLE_TIMEOUT, Launcher, real_availability, real_launcher};
pub use setup::fixture_accounts;
pub use web_research::WebTransport;

use anyhow::Result;
use arbiter_adapters::catalog::HarnessInfo;
use arbiter_core::Event;
use arbiter_store::Store;
use arbiter_supervisor::WorktreeManager;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

pub struct Config {
    /// Root for the database, worktrees and discovery file.
    pub home: PathBuf,
    /// 127.0.0.1 port; 0 picks a free one.
    pub port: u16,
    /// How harness processes are started ([`real_launcher`] outside tests).
    pub launcher: Launcher,
    /// Which harnesses are installed ([`real_availability`] outside tests).
    pub available: Availability,
    pub idle_timeout: Duration,
    /// Start harnesses in lean mode (default on; see `arbiter_adapters::StartOpts::lean`).
    pub lean: bool,
    /// Run project checks after each turn and feed failures back (default on).
    pub heal: bool,
    pub heal_policy: arbiter_heal::HealPolicy,
    /// Mid-turn silence before a "may be stuck" notice.
    pub stall_timeout: Duration,
    /// Scales rate-limit waits (tests use a tiny factor). 1s = real time.
    pub retry_scale: Duration,
    /// Sanitized evidence for fake-launcher tests. Production leaves this None.
    pub fixture_accounts: Option<Vec<arbiter_setup::Account>>,
    pub local_generator: Option<LocalGenerator>,
    pub documentation_transport: Option<DocumentationTransport>,
    pub web_transport: Option<WebTransport>,
    pub credential_store: Option<Arc<dyn arbiter_supervisor::credentials::CredentialStore>>,
}

impl Config {
    pub fn new(home: PathBuf, port: u16) -> Self {
        Self {
            home,
            port,
            launcher: real_launcher(),
            available: real_availability(),
            idle_timeout: IDLE_TIMEOUT,
            lean: true,
            heal: true,
            heal_policy: Default::default(),
            stall_timeout: Duration::from_secs(10 * 60),
            retry_scale: Duration::from_secs(1),
            fixture_accounts: None,
            local_generator: None,
            documentation_transport: None,
            web_transport: None,
            credential_store: None,
        }
    }
}

/// What the WebSocket carries: thread events, plus a nudge when tasks change.
#[derive(Clone, Debug)]
pub(crate) enum WsMsg {
    Event(Box<Event>),
    TasksChanged,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    /// Data directory (database, worktrees, attachments).
    home: PathBuf,
    store: Mutex<Store>,
    events: broadcast::Sender<WsMsg>,
    worktrees: WorktreeManager,
    token: String,
    runs: runs::Runs,
    launcher: Launcher,
    available: Availability,
    idle_timeout: Duration,
    lean: bool,
    heal_enabled: bool,
    heal_policy: arbiter_heal::HealPolicy,
    stall_timeout: Duration,
    retry_scale: Duration,
    heal: Mutex<std::collections::HashMap<arbiter_core::ThreadId, heal::ThreadHeal>>,
    web: web::Web,
    intake: arbiter_intake::models::Manager,
    intake_lock: tokio::sync::Mutex<()>,
    plan_lock: tokio::sync::Mutex<()>,
    knowledge_lock: Mutex<()>,
    /// `http://127.0.0.1:<port>`, known once the listener is bound.
    base_url: std::sync::OnceLock<String>,
    /// Model lists change rarely and listing Codex's spawns a process.
    catalog: tokio::sync::Mutex<Option<(Instant, Vec<HarnessInfo>)>>,
    setup: Mutex<arbiter_setup::Store>,
    accounts: Mutex<Vec<arbiter_setup::Account>>,
    account_refresh: tokio::sync::Mutex<Instant>,
    admission_lock: Mutex<()>,
    fixture_accounts: bool,
    project_setup: Mutex<arbiter_project::Store>,
    baseline_lock: tokio::sync::Mutex<()>,
    local_runs:
        Mutex<std::collections::HashMap<arbiter_core::ThreadId, (arbiter_core::RunId, tokio::task::AbortHandle)>>,
    local_generator: Option<LocalGenerator>,
    review_lock: tokio::sync::Mutex<()>,
    documentation_transport: Option<DocumentationTransport>,
    web_transport: Option<WebTransport>,
    credential_store: Arc<dyn arbiter_supervisor::credentials::CredentialStore>,
    context7_session_key: Mutex<Option<String>>,
    documentation_preparing:
        Mutex<std::collections::HashMap<arbiter_core::ThreadId, (arbiter_core::RunId, tokio::task::AbortHandle)>>,
    documentation_operations: Mutex<documentation_operations::Operations>,
    prereq_jobs: Mutex<prerequisites::Jobs>,
    terminals: Mutex<terminal::Terminals>,
    held_shares: Mutex<web_sites::Held>,
    memory_lock: tokio::sync::Mutex<()>,
    /// One browser at a time may use the signed-in profile.
    web_profile: tokio::sync::Mutex<()>,
}

impl AppState {
    /// Run `f` against the store. SQLite calls are short, so a sync mutex is fine.
    fn store<T>(&self, f: impl FnOnce(&mut Store) -> T) -> T {
        f(&mut self.inner.store.lock().expect("store mutex poisoned"))
    }

    fn publish(&self, e: &Event) {
        // No subscribers is fine; lagging subscribers resync via /events?after=.
        let _ = self.inner.events.send(WsMsg::Event(Box::new(e.clone())));
    }

    async fn catalog(&self, refresh: bool) -> Vec<HarnessInfo> {
        let mut cache = self.inner.catalog.lock().await;
        match &*cache {
            Some((at, c)) if !refresh && at.elapsed() < Duration::from_secs(600) => c.clone(),
            _ => {
                let c = arbiter_adapters::catalog::catalog().await;
                *cache = Some((Instant::now(), c.clone()));
                c
            }
        }
    }
}

/// A running daemon; dropping it does not stop the server task.
pub struct Running {
    pub addr: SocketAddr,
    pub token: String,
    pub handle: tokio::task::JoinHandle<std::io::Result<()>>,
}

pub async fn start(cfg: Config) -> Result<Running> {
    std::fs::create_dir_all(&cfg.home)?;
    let store = Store::open(cfg.home.join("arbiter.db"))?;
    let token = discovery::new_token();
    let (events, _) = broadcast::channel(1024);
    let mut setup = arbiter_setup::Store::open(&cfg.home.join("setup.db"))?;
    if cfg.fixture_accounts.is_some() && !setup.read()?.complete {
        let mut p = setup.read()?;
        p.complete = true;
        p.step = 3;
        p.max_cloud_runs = 3;
        setup.save(p)?;
    }
    let state = AppState {
        inner: Arc::new(Inner {
            home: cfg.home.clone(),
            store: Mutex::new(store),
            events,
            worktrees: WorktreeManager::new(cfg.home.join("wt")),
            token: token.clone(),
            runs: Default::default(),
            launcher: cfg.launcher,
            available: cfg.available,
            idle_timeout: cfg.idle_timeout,
            lean: cfg.lean,
            heal_enabled: cfg.heal,
            heal_policy: cfg.heal_policy,
            stall_timeout: cfg.stall_timeout,
            retry_scale: cfg.retry_scale,
            heal: Default::default(),
            web: Default::default(),
            intake: arbiter_intake::models::Manager::new(cfg.home.join("models")),
            intake_lock: Default::default(),
            plan_lock: Default::default(),
            knowledge_lock: Default::default(),
            base_url: Default::default(),
            catalog: Default::default(),
            setup: Mutex::new(setup),
            accounts: Mutex::new(cfg.fixture_accounts.clone().unwrap_or_default()),
            account_refresh: tokio::sync::Mutex::new(Instant::now() - Duration::from_secs(120)),
            admission_lock: Mutex::new(()),
            fixture_accounts: cfg.fixture_accounts.is_some(),
            project_setup: Mutex::new(arbiter_project::Store::open(&cfg.home.join("projects.db"))?),
            baseline_lock: Default::default(),
            local_runs: Default::default(),
            local_generator: cfg.local_generator,
            documentation_transport: cfg.documentation_transport,
            web_transport: cfg.web_transport,
            credential_store: cfg.credential_store.unwrap_or_else(|| {
                if cfg.fixture_accounts.is_some() {
                    Arc::new(arbiter_supervisor::credentials::MemoryCredentials::default())
                } else {
                    Arc::new(arbiter_supervisor::credentials::NativeCredentials)
                }
            }),
            context7_session_key: Default::default(),
            documentation_preparing: Default::default(),
            documentation_operations: Default::default(),
            prereq_jobs: Default::default(),
            held_shares: Default::default(),
            memory_lock: Default::default(),
            web_profile: Default::default(),
            terminals: Default::default(),
            review_lock: Default::default(),
        }),
    };
    state.recover_interrupted()?;
    state.spawn_web_sweeper();
    state.spawn_account_monitor();
    let model_manager = state.inner.intake.clone();
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(30));
        loop {
            timer.tick().await;
            model_manager.unload_idle();
        }
    });

    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], cfg.port))).await?;
    let addr = listener.local_addr()?;
    let _ = state.inner.base_url.set(format!("http://{addr}"));
    state.spawn_plan_engine();
    state.spawn_publish_sweeper();
    state.spawn_memory_keeper();
    DaemonInfo { port: addr.port(), token: token.clone(), pid: std::process::id() }.write(&cfg.home)?;

    let app = api::router(state);
    let handle = tokio::spawn(async move { axum::serve(listener, app).await });
    tracing::info!(%addr, "arbiterd listening");
    Ok(Running { addr, token, handle })
}
