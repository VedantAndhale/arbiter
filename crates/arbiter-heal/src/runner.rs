//! Runs one check in a worktree with a timeout. Output from stdout and stderr
//! is merged and only the tail is kept: failures are almost always at the end.

use crate::detect::Check;
use arbiter_core::NoWindow;
use arbiter_supervisor::SupervisedChild;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

const KEEP_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    #[serde(skip)]
    pub output: String,
    pub duration_ms: u64,
    pub timed_out: bool,
}

fn shell(cmd: &str) -> tokio::process::Command {
    if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        #[cfg(windows)]
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.no_window();
        c.arg("-c").arg(cmd);
        c
    }
}

pub async fn run_check(check: &Check, cwd: &Path) -> CheckRun {
    let (ok, exit_code, output, duration_ms, timed_out) = exec(&check.cmd, cwd, check.timeout_secs).await;
    CheckRun { name: check.name.clone(), ok, exit_code, output, duration_ms, timed_out }
}

/// Environment setup (e.g. dependency install). Generous timeout.
pub async fn run_setup(cmd: &str, cwd: &Path) -> CheckRun {
    let (ok, exit_code, output, duration_ms, timed_out) = exec(cmd, cwd, 900).await;
    CheckRun { name: "setup".into(), ok, exit_code, output, duration_ms, timed_out }
}

async fn exec(cmd: &str, cwd: &Path, timeout_secs: u64) -> (bool, Option<i32>, String, u64, bool) {
    let start = Instant::now();
    let mut c = shell(cmd);
    c.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Non-interactive, colourless, no watch modes.
        .env("CI", "1")
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("CARGO_TERM_COLOR", "never");
    let mut child = match SupervisedChild::spawn(c) {
        Ok(ch) => ch,
        Err(e) => return (false, None, format!("failed to start `{cmd}`: {e}"), 0, false),
    };
    let mut stdout = child.child.stdout.take().expect("piped");
    let mut stderr = child.child.stderr.take().expect("piped");
    let collect = async {
        let (mut a, b) = tokio::join!(bounded_tail(&mut stdout), bounded_tail(&mut stderr));
        a.extend_from_slice(&b);
        a
    };
    let result = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        let bytes = collect.await;
        let status = child.child.wait().await;
        (bytes, status)
    })
    .await;
    let elapsed = start.elapsed().as_millis() as u64;
    match result {
        Ok((bytes, status)) => {
            let code = status.ok().and_then(|s| s.code());
            (code == Some(0), code, tail(&bytes), elapsed, false)
        }
        Err(_) => {
            let _ = child.kill_tree().await;
            (false, None, format!("timed out after {timeout_secs}s: `{cmd}`"), elapsed, true)
        }
    }
}

async fn bounded_tail(reader: &mut (impl tokio::io::AsyncRead + Unpin)) -> Vec<u8> {
    let mut tail = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(n) = reader.read(&mut chunk).await {
        if n == 0 {
            break;
        }
        if tail.len() + n > KEEP_BYTES {
            tail.drain(..tail.len() + n - KEEP_BYTES);
        }
        tail.extend_from_slice(&chunk[..n]);
    }
    tail
}

fn tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(KEEP_BYTES);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::CheckKind;

    fn check(cmd: &str, timeout_secs: u64) -> Check {
        Check { name: "t".into(), kind: CheckKind::Custom, cmd: cmd.into(), fix: None, timeout_secs }
    }

    #[tokio::test]
    async fn captures_output_and_exit_code() {
        let d = tempfile::tempdir().unwrap();
        let r = run_check(&check("echo hello && exit 3", 30), d.path()).await;
        assert!(!r.ok);
        assert_eq!(r.exit_code, Some(3));
        assert!(r.output.contains("hello"));
        let ok = run_check(&check("echo fine", 30), d.path()).await;
        assert!(ok.ok);
    }

    #[tokio::test]
    async fn times_out_and_kills() {
        let d = tempfile::tempdir().unwrap();
        let cmd = if cfg!(windows) { "ping -n 30 127.0.0.1 > nul" } else { "sleep 30" };
        let r = run_check(&check(cmd, 1), d.path()).await;
        assert!(r.timed_out && !r.ok);
        assert!(r.duration_ms < 10_000);
    }
}
