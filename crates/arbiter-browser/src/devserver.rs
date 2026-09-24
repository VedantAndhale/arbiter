//! Start a project's dev server on a free port and wait until it answers.

use anyhow::{Result, bail};
use arbiter_core::NoWindow;
use arbiter_supervisor::SupervisedChild;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct DevServer {
    pub port: u16,
    pub url: String,
    child: SupervisedChild,
    pub started: Instant,
}

/// A port nobody is listening on right now.
pub fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(l.local_addr()?.port())
}

impl DevServer {
    /// `cmd` may contain `{port}`. `PORT` is also set, and `BROWSER=none`
    /// stops tools like create-react-app from opening a window.
    pub async fn start(cmd: &str, cwd: &Path, ready_timeout: Duration) -> Result<Self> {
        let port = free_port()?;
        let cmd = cmd.replace("{port}", &port.to_string());
        let mut c = if cfg!(windows) {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C").arg(&cmd);
            #[cfg(windows)]
            c.creation_flags(0x0800_0000);
            c
        } else {
            let mut c = tokio::process::Command::new("sh");
            c.no_window();
            c.arg("-c").arg(&cmd);
            c
        };
        c.current_dir(cwd)
            .env("PORT", port.to_string())
            .env("BROWSER", "none")
            .env("NO_COLOR", "1")
            .env("FORCE_COLOR", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = SupervisedChild::spawn(c)?;
        // Keep a tail of output for error reports; drain so the server never blocks.
        let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        for pipe in [
            child.child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            child.child.stderr.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let log = log.clone();
            let mut pipe = pipe;
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let mut l = log.lock().unwrap();
                    l.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if l.len() > 16_384 {
                        let cut = l.len() - 8_192;
                        let cut = (cut..l.len()).find(|i| l.is_char_boundary(*i)).unwrap_or(l.len());
                        l.drain(..cut);
                    }
                }
            });
        }
        let url = format!("http://localhost:{port}");
        let deadline = Instant::now() + ready_timeout;
        loop {
            if let Some(status) = child.child.try_wait()? {
                bail!("dev server exited ({status}) before it was ready:\n{}", tail(&log.lock().unwrap(), 15));
            }
            if http_ok(port).await {
                return Ok(Self { port, url, child, started: Instant::now() });
            }
            if Instant::now() > deadline {
                let _ = child.kill_tree().await;
                bail!(
                    "dev server not answering on port {port} after {}s:\n{}",
                    ready_timeout.as_secs(),
                    tail(&log.lock().unwrap(), 15)
                );
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    pub async fn stop(mut self) {
        let _ = self.child.kill_tree().await;
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.child.try_wait(), Ok(None))
    }
}

fn tail(s: &str, n: usize) -> String {
    let l: Vec<&str> = s.lines().collect();
    l[l.len().saturating_sub(n)..].join("\n")
}

/// Any HTTP response on either loopback form counts as ready (dev servers
/// often bind only `localhost`, which may resolve to ::1).
async fn http_ok(port: u16) -> bool {
    for host in ["127.0.0.1", "[::1]"] {
        let Ok(Ok(mut s)) =
            tokio::time::timeout(Duration::from_millis(500), tokio::net::TcpStream::connect(format!("{host}:{port}")))
                .await
        else {
            continue;
        };
        let req = format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
        if s.write_all(req.as_bytes()).await.is_err() {
            continue;
        }
        let mut buf = [0u8; 12];
        if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_secs(2), s.read(&mut buf)).await
            && n >= 5
            && &buf[..5] == b"HTTP/"
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn starts_and_detects_readiness_and_failure() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("index.html"), "<h1>hi</h1>").unwrap();
        // node is a dev dependency of every web project we care about.
        let script = "require('http').createServer((q,s)=>s.end('ok')).listen(process.env.PORT)";
        std::fs::write(d.path().join("srv.js"), script).unwrap();
        let s = DevServer::start("node srv.js", d.path(), Duration::from_secs(15)).await.unwrap();
        assert!(s.url.contains(&s.port.to_string()));
        s.stop().await;

        let err =
            DevServer::start("node -e \"console.log('boom'); process.exit(3)\"", d.path(), Duration::from_secs(10))
                .await
                .err()
                .unwrap()
                .to_string();
        assert!(err.contains("exited") && err.contains("boom"), "{err}");
    }
}
