//! Just enough CDP: launch headless Chromium, talk JSON over one WebSocket,
//! correlate command ids, fan out events per session.

use anyhow::{Context, Result, anyhow, bail};
use arbiter_supervisor::SupervisedChild;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

/// A headless browser process plus its DevTools connection. Dropping it kills
/// the whole process tree.
pub struct Browser {
    out: mpsc::UnboundedSender<String>,
    pending: Pending,
    events: broadcast::Sender<Value>,
    next_id: AtomicU64,
    closed: Arc<AtomicBool>,
    _child: tokio::sync::Mutex<SupervisedChild>,
    _profile: Option<tempfile::TempDir>,
}

impl Browser {
    pub async fn launch(exe: &Path) -> Result<Self> {
        Self::launch_in(exe, None).await
    }

    /// Headless, using `profile` as the browser's data folder when given (for
    /// sites the user signed into), else a throwaway folder.
    pub async fn launch_in(exe: &Path, profile_dir: Option<&Path>) -> Result<Self> {
        let temp = match profile_dir {
            Some(_) => None,
            None => Some(tempfile::Builder::new().prefix("arbiter-browser-").tempdir()?),
        };
        let data_dir =
            profile_dir.map(Path::to_path_buf).unwrap_or_else(|| temp.as_ref().unwrap().path().to_path_buf());
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args([
            "--headless=new",
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-gpu",
            "--mute-audio",
            "--disable-background-networking",
            "--window-size=1280,800",
        ])
        .arg(format!("--user-data-dir={}", data_dir.display()))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = SupervisedChild::spawn(cmd).with_context(|| format!("failed to start {}", exe.display()))?;

        // Chrome announces "DevTools listening on ws://…" on stderr.
        let stderr = child.child.stderr.take().expect("piped");
        let mut lines = BufReader::new(stderr).lines();
        // A cold first start (new profile, slow disk, antivirus) can be slow.
        let ws_url = tokio::time::timeout(Duration::from_secs(60), async {
            while let Some(l) = lines.next_line().await? {
                if let Some(u) = l.split("DevTools listening on ").nth(1) {
                    return Ok::<_, anyhow::Error>(u.trim().to_owned());
                }
            }
            bail!("browser exited before opening DevTools")
        })
        .await
        .context("browser did not start within 60s")??;
        // Keep draining stderr so the browser never blocks on a full pipe.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

        let (ws, _) = tokio_tungstenite::connect_async(&ws_url).await.context("connect to DevTools")?;
        let (mut sink, mut stream) = ws.split();
        let (out, mut out_rx) = mpsc::unbounded_channel::<String>();
        let pending: Pending = Default::default();
        let (events, _) = broadcast::channel(4096);

        tokio::spawn(async move {
            while let Some(m) = out_rx.recv().await {
                if sink.send(Message::Text(m.into())).await.is_err() {
                    break;
                }
            }
        });
        let closed = Arc::new(AtomicBool::new(false));
        let (p, ev, gone) = (pending.clone(), events.clone(), closed.clone());
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                if let Some(id) = v["id"].as_u64() {
                    if let Some(tx) = p.lock().unwrap().remove(&id) {
                        let r = match v.get("error") {
                            Some(e) => Err(anyhow!("CDP error: {}", e["message"].as_str().unwrap_or("unknown"))),
                            None => Ok(v["result"].clone()),
                        };
                        let _ = tx.send(r);
                    }
                } else {
                    let _ = ev.send(v);
                }
            }
            // Connection gone: fail everything still waiting.
            gone.store(true, Ordering::Release);
            for (_, tx) in p.lock().unwrap().drain() {
                let _ = tx.send(Err(anyhow!("browser connection closed")));
            }
        });

        Ok(Self {
            out,
            pending,
            events,
            next_id: AtomicU64::new(1),
            closed,
            _child: tokio::sync::Mutex::new(child),
            _profile: temp,
        })
    }

    /// Send a command (to a page when `session` is set) and await its result.
    pub async fn call(&self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        anyhow::ensure!(!self.closed.load(Ordering::Acquire), "browser connection closed");
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        if self.out.send(msg.to_string()).is_err() {
            self.pending.lock().unwrap().remove(&id);
            bail!("browser connection closed");
        }
        let result = tokio::time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow!("CDP {method} timed out"))
            .and_then(|r| r.map_err(|_| anyhow!("browser connection closed")))
            .and_then(|r| r);
        self.pending.lock().unwrap().remove(&id);
        result
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }

    /// New tab with its own flattened session. Returns (target id, session id).
    pub async fn new_tab(&self) -> Result<(String, String)> {
        let t = self.call(None, "Target.createTarget", json!({ "url": "about:blank" })).await?;
        let target = t["targetId"].as_str().context("no targetId")?.to_owned();
        let a = self.call(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true })).await?;
        let session = a["sessionId"].as_str().context("no sessionId")?.to_owned();
        Ok((target, session))
    }

    pub async fn close_tab(&self, target: &str) {
        let _ = self.call(None, "Target.closeTarget", json!({ "targetId": target })).await;
    }
}

/// Open a visible browser window on `profile` at `url`, for the user to sign
/// in by themselves. Arbiter never sees the credentials.
pub fn open_visible(exe: &Path, profile: &Path, url: &str) -> Result<()> {
    std::fs::create_dir_all(profile)?;
    std::process::Command::new(exe)
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--no-first-run", "--no-default-browser-check", "--new-window", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to open {}", exe.display()))?;
    Ok(())
}
