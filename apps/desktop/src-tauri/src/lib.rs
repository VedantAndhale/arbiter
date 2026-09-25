//! The desktop shell is deliberately thin: it makes sure `arbiterd` is running
//! (spawning it detached so agents outlive the window) and hands the UI the
//! daemon's URL and token. All real work goes through the daemon API.

use arbiter_core::{DaemonInfo, arbiter_home};
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize)]
struct Connection {
    base_url: String,
    token: String,
}

async fn healthy(info: &DaemonInfo) -> bool {
    health(info).await.is_some()
}

/// The running daemon's version, if it answers and is the daemon that wrote
/// `daemon.json`. A new daemon opens its port a moment before it rewrites the
/// file; in that window the file still holds the previous run's token, so a
/// pid mismatch means "not ready yet", not "healthy".
async fn health(info: &DaemonInfo) -> Option<String> {
    let client = reqwest::Client::builder().timeout(Duration::from_millis(800)).build().ok()?;
    let r = client.get(format!("{}/v1/health", info.base_url())).send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let v: serde_json::Value = r.json().await.ok()?;
    if v["pid"].as_u64().is_some_and(|pid| pid != u64::from(info.pid)) {
        return None;
    }
    Some(v["version"].as_str().unwrap_or("").to_owned())
}

/// After an update the old daemon may still run (it outlives the window).
/// Ask it to stop, unless an agent is working, so this version's starts.
async fn replace_if_outdated(home: &std::path::Path) {
    let Ok(info) = DaemonInfo::read(home) else { return };
    let Some(version) = health(&info).await else { return };
    if version == env!("CARGO_PKG_VERSION") {
        return;
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().expect("client");
    let stopping = client
        .post(format!("{}/v1/shutdown", info.base_url()))
        .bearer_auth(&info.token)
        .json(&serde_json::json!({}))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success());
    if stopping {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline && healthy(&info).await {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

/// `arbiterd` ships next to the app executable (Tauri sidecar in release; shared
/// cargo target dir in dev).
fn daemon_binary() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("executable has no parent dir")?;
    let name = if cfg!(windows) { "arbiterd.exe" } else { "arbiterd" };
    let path = dir.join(name);
    if path.exists() {
        Ok(path)
    } else {
        Err(format!("{} not found; build it with `cargo build -p arbiterd`", path.display()))
    }
}

fn spawn_detached(bin: &PathBuf) -> Result<(), String> {
    let mut cmd = Command::new(bin);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map(drop).map_err(|e| format!("failed to start arbiterd: {e}"))
}

/// Attach to a running daemon, or start one and wait for it to come up.
#[tauri::command]
async fn connect() -> Result<Connection, String> {
    let home = arbiter_home();
    replace_if_outdated(&home).await;
    if let Some(c) = attach(&home).await {
        return Ok(c);
    }
    spawn_detached(&daemon_binary()?)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(150)).await;
        if let Some(c) = attach(&home).await {
            return Ok(c);
        }
    }
    Err("arbiterd did not become healthy within 10s".into())
}

/// A stale `daemon.json` (daemon crashed) fails the health check and is ignored.
async fn attach(home: &std::path::Path) -> Option<Connection> {
    let info = DaemonInfo::read(home).ok()?;
    healthy(&info).await.then(|| Connection { base_url: info.base_url(), token: info.token })
}

/// Closing asks the UI first (it offers to save your memory; the daemon does
/// the work). A second close within a few seconds always closes, so a stuck
/// page can never trap the window.
fn on_close(window: &tauri::Window, event: &tauri::WindowEvent) {
    use std::sync::Mutex;
    use tauri::Emitter;
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let mut last = LAST.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return;
        }
        *last = Some(Instant::now());
        api.prevent_close();
        let _ = window.emit("arbiter://close-requested", ());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_window_event(on_close)
        .invoke_handler(tauri::generate_handler![connect])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
