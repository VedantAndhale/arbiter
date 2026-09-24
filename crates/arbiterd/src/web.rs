//! Web dev support: one shared headless browser, one dev server per thread,
//! browser checks for the heal loop, and page tools for the UI and agents.
//! Everything returns compact text; screenshots are saved to a file and
//! referenced by path, never inlined.

use crate::AppState;
use anyhow::{Context, Result, anyhow};
use arbiter_browser::{Browser, DevServer, find_browser, page};
use arbiter_core::{CheckResult, ThreadId};
use arbiter_heal::{BrowserConfig, Failure, STATIC_SERVER, detect, run_setup};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Dev servers idle longer than this are stopped (restarted on demand).
const SERVER_IDLE: Duration = Duration::from_secs(20 * 60);

/// A project's own dev server, or arbiterd serving a plain HTML folder.
enum Server {
    Dev(Box<DevServer>),
    Static(StaticServer),
}

impl Server {
    fn url(&self) -> &str {
        match self {
            Self::Dev(s) => &s.url,
            Self::Static(s) => &s.url,
        }
    }
    fn is_running(&mut self) -> bool {
        match self {
            Self::Dev(s) => s.is_running(),
            Self::Static(s) => !s.task.is_finished(),
        }
    }
    async fn stop(self) {
        match self {
            Self::Dev(s) => s.stop().await,
            Self::Static(s) => s.task.abort(),
        }
    }
}

/// Serves files from a folder on 127.0.0.1, for sites with no toolchain.
struct StaticServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl StaticServer {
    async fn start(root: std::path::PathBuf) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let root = Arc::new(std::fs::canonicalize(root)?);
        let app = axum::Router::new().fallback(move |uri: axum::http::Uri| {
            let root = root.clone();
            async move { serve_static(&root, uri.path()).await }
        });
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self { url, task })
    }
}

/// Percent-decode a URL component.
pub(crate) fn decode_percent(path: &str) -> Option<String> {
    decode(path)
}

/// Percent-encode a search query for a URL.
pub(crate) fn encode_query(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

async fn serve_static(root: &Path, path: &str) -> axum::response::Response {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    let not_found = || (StatusCode::NOT_FOUND, "Not found").into_response();
    let Some(rel) = decode(path) else { return not_found() };
    // Only plain relative segments: no parent, drive or hidden components.
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| p.starts_with('.') || p.contains(['\\', ':'])) {
        return not_found();
    }
    let mut file = root.to_path_buf();
    file.extend(&parts);
    if file.is_dir() {
        file.push("index.html");
    }
    match tokio::fs::metadata(&file).await {
        Ok(m) if m.is_file() && m.len() <= 50 * 1024 * 1024 => {}
        _ => return not_found(),
    }
    let Ok(body) = tokio::fs::read(&file).await else { return not_found() };
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

#[derive(Default)]
pub(crate) struct Web {
    browser: tokio::sync::Mutex<Option<Arc<Browser>>>,
    servers: tokio::sync::Mutex<HashMap<ThreadId, (Server, Instant)>>,
    pages: tokio::sync::Mutex<HashMap<ThreadId, Arc<tokio::sync::Mutex<arbiter_browser::session::PageSession>>>>,
}

impl AppState {
    pub(crate) fn preview_viewport(&self, thread: ThreadId) -> Result<arbiter_browser::session::Viewport> {
        self.store(|st| st.thread(thread))?.context("thread not found")?;
        Ok(self
            .store(|st| st.events(thread, 0))?
            .into_iter()
            .rev()
            .find_map(|e| match e.kind {
                arbiter_core::EventKind::PreviewViewportChanged { width, height } => {
                    Some(arbiter_browser::session::Viewport { width, height })
                }
                _ => None,
            })
            .unwrap_or_default())
    }

    pub(crate) async fn preview_page(
        &self,
        thread: ThreadId,
    ) -> Result<Arc<tokio::sync::Mutex<arbiter_browser::session::PageSession>>> {
        let url = self.dev_server(thread).await?;
        let browser = self.browser().await?;
        let mut pages = self.inner.web.pages.lock().await;
        if let Some(p) = pages.get(&thread) {
            let old = p.lock().await;
            if old.belongs_to(&browser, &url) {
                return Ok(p.clone());
            }
            old.close().await;
        }
        let mut session = arbiter_browser::session::PageSession::open(browser, &url).await?;
        session.resize(self.preview_viewport(thread)?).await?;
        let page = Arc::new(tokio::sync::Mutex::new(session));
        pages.insert(thread, page.clone());
        Ok(page)
    }

    /// The shared headless browser, launched on first use and relaunched if it died.
    pub(crate) async fn browser(&self) -> Result<Arc<Browser>> {
        let mut slot = self.inner.web.browser.lock().await;
        if let Some(b) = slot.as_ref()
            && b.call(None, "Target.getTargets", serde_json::json!({})).await.is_ok()
        {
            return Ok(b.clone());
        }
        let exe = find_browser().ok_or_else(|| {
            anyhow!("no Chromium browser found (install Chrome or Edge, or set ARBITER_BROWSER to its path)")
        })?;
        let b = Arc::new(Browser::launch(&exe).await?);
        *slot = Some(b.clone());
        Ok(b)
    }

    fn preview_settings_path(&self, project: arbiter_core::ProjectId) -> std::path::PathBuf {
        self.inner.home.join("preview").join(format!("{project}.json"))
    }

    /// The start command the user saved for a project, if any. Kept in
    /// Arbiter's home, not in the repository.
    pub(crate) fn preview_command_override(&self, project: arbiter_core::ProjectId) -> Option<String> {
        let bytes = std::fs::read(self.preview_settings_path(project)).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        v["dev"].as_str().filter(|s| !s.trim().is_empty()).map(str::to_owned)
    }

    pub(crate) fn set_preview_command(&self, project: arbiter_core::ProjectId, dev: Option<&str>) -> Result<()> {
        let path = self.preview_settings_path(project);
        match dev.map(str::trim).filter(|d| !d.is_empty()) {
            Some(d) => {
                anyhow::ensure!(
                    d.len() <= 300 && !d.contains(['\n', '\r']),
                    "use a single-line command of up to 300 characters"
                );
                std::fs::create_dir_all(path.parent().context("preview settings path")?)?;
                std::fs::write(path, serde_json::to_vec(&serde_json::json!({ "dev": d }))?)?;
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    /// Detected and effective start commands for a thread's project.
    pub(crate) fn preview_command(&self, thread: ThreadId) -> Result<serde_json::Value> {
        let t = self.store(|st| st.thread(thread))?.context("thread not found")?;
        let cwd = self.thread_cwd(&t)?;
        let detected = self.heal_config(Some(thread), &cwd)?.browser.filter(|b| b.enabled).map(|b| b.dev);
        let custom = self.preview_command_override(t.project_id);
        Ok(
            serde_json::json!({ "detected": detected, "custom": custom, "effective": custom.clone().or(detected.clone()) }),
        )
    }

    fn browser_config_for(&self, thread: ThreadId, cwd: &Path) -> Result<BrowserConfig> {
        let project = self.store(|st| st.thread(thread))?.context("thread not found")?.project_id;
        let detected = self.heal_config(Some(thread), cwd)?.browser.filter(|b| b.enabled);
        if let Some(dev) = self.preview_command_override(project) {
            let mut b = detected.unwrap_or_else(|| BrowserConfig {
                dev: String::new(),
                routes: vec!["/".into()],
                enabled: true,
                ready_timeout_secs: 90,
            });
            b.dev = dev;
            return Ok(b);
        }
        detected.context("Arbiter could not tell how to start this app. Set the start command in Preview.")
    }

    /// URL of the thread's running dev server, starting it if needed.
    pub(crate) async fn dev_server(&self, thread: ThreadId) -> Result<String> {
        let t = self.store(|st| st.thread(thread))?.context("thread not found")?;
        let cwd = self.thread_cwd(&t)?;
        let mut servers = self.inner.web.servers.lock().await;
        if let Some((s, used)) = servers.get_mut(&thread)
            && s.is_running()
        {
            *used = Instant::now();
            return Ok(s.url().to_owned());
        }
        let bcfg = self.browser_config_for(thread, &cwd)?;
        let s = if bcfg.dev == STATIC_SERVER {
            Server::Static(StaticServer::start(cwd.clone()).await?)
        } else {
            if let Some(setup) = detect(&cwd)?.setup {
                let r = run_setup(&setup, &cwd).await;
                anyhow::ensure!(r.ok, "environment setup failed (`{setup}`)");
            }
            Server::Dev(Box::new(
                DevServer::start(&bcfg.dev, &cwd, Duration::from_secs(bcfg.ready_timeout_secs)).await?,
            ))
        };
        let url = s.url().to_owned();
        if let Some((old, _)) = servers.insert(thread, (s, Instant::now())) {
            old.stop().await;
        }
        Ok(url)
    }

    pub(crate) async fn stop_dev_server(&self, thread: ThreadId) -> bool {
        if let Some(page) = self.inner.web.pages.lock().await.remove(&thread) {
            page.lock().await.close().await;
        }
        match self.inner.web.servers.lock().await.remove(&thread) {
            Some((s, _)) => {
                s.stop().await;
                true
            }
            None => false,
        }
    }

    pub(crate) async fn dev_server_url(&self, thread: ThreadId) -> Option<String> {
        let mut servers = self.inner.web.servers.lock().await;
        let (s, _) = servers.get_mut(&thread)?;
        s.is_running().then(|| s.url().to_owned())
    }

    /// After an agent finishes on a web project, start the app so the user
    /// can see it without asking. Announced once per address.
    pub(crate) async fn auto_preview(&self, thread: ThreadId) {
        let Ok(Some(t)) = self.store(|st| st.thread(thread)) else { return };
        if t.status != arbiter_core::ThreadStatus::Review || t.worktree.is_none() {
            return;
        }
        let Ok(cwd) = self.thread_cwd(&t) else { return };
        if self.browser_config_for(thread, &cwd).is_err() {
            return;
        }
        match self.dev_server(thread).await {
            Ok(url) => {
                let known = self.store(|st| st.events(thread, 0)).is_ok_and(|events| {
                    events.iter().rev().find_map(|e| match &e.kind {
                        arbiter_core::EventKind::PreviewReady { url } => Some(url.clone()),
                        _ => None,
                    }) == Some(url.clone())
                });
                if !known {
                    self.append_logged(thread, arbiter_core::EventKind::PreviewReady { url });
                }
            }
            Err(e) => self.append_logged(
                thread,
                arbiter_core::EventKind::Notice {
                    text: format!(
                        "Your app could not be started: {}",
                        format!("{e:#}").chars().take(300).collect::<String>()
                    ),
                },
            ),
        }
    }

    /// Stop dev servers nobody has used for a while.
    pub(crate) fn spawn_web_sweeper(&self) {
        let s = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let stale: Vec<ThreadId> = {
                    let servers = s.inner.web.servers.lock().await;
                    servers.iter().filter(|(_, (_, used))| used.elapsed() > SERVER_IDLE).map(|(t, _)| *t).collect()
                };
                for t in stale {
                    s.stop_dev_server(t).await;
                }
            }
        });
    }

    /// Load each configured route; runtime errors become heal failures.
    /// Browser failures are not attributable to one file, so they always count.
    pub(crate) async fn browser_checks(
        &self,
        thread: ThreadId,
        cwd: &Path,
    ) -> Option<(CheckResult, Vec<Failure>, String)> {
        let bcfg = self.browser_config_for(thread, cwd).ok()?;
        let start = Instant::now();
        let summary = |ok, failures: &[Failure], timed_out| CheckResult {
            name: "browser".into(),
            ok,
            duration_ms: start.elapsed().as_millis() as u64,
            failures: failures.len() as u32,
            timed_out,
            fixed: false,
        };
        let url = match self.dev_server(thread).await {
            Ok(u) => u,
            Err(e) => {
                let f = vec![Failure {
                    file: None,
                    line: None,
                    col: None,
                    code: Some("dev-server".into()),
                    message: format!("{e:#}"),
                }];
                return Some((summary(false, &f, false), f, bcfg.dev));
            }
        };
        let browser = match self.browser().await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("browser checks skipped: {e:#}");
                return None;
            }
        };
        let mut failures = Vec::new();
        for route in &bcfg.routes {
            match page::check(&browser, &format!("{url}{route}"), Duration::from_millis(800)).await {
                Ok(report) => failures.extend(report.issues.into_iter().map(|i| Failure {
                    file: i.source_path(&url),
                    line: i.line,
                    col: i.col,
                    code: Some(format!("{} {route}", i.kind)),
                    message: match &i.source {
                        Some(src) if i.kind == "http" || i.kind == "request" => format!("{} ({src})", i.message),
                        _ => i.message,
                    },
                })),
                Err(e) => failures.push(Failure {
                    file: None,
                    line: None,
                    col: None,
                    code: Some(format!("load {route}")),
                    message: format!("{e:#}"),
                }),
            }
        }
        let label = format!("load {} in a headless browser", bcfg.routes.join(", "));
        Some((summary(failures.is_empty(), &failures, false), failures, label))
    }
}
