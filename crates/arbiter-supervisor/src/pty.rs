//! An interactive shell on a pseudo-terminal (ConPTY on Windows), for the
//! user's terminal drawer. Output is fanned out to viewers and a bounded
//! replay buffer; it is never sent to an agent.
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Output kept for a viewer that (re)opens the terminal.
pub const SCROLLBACK: usize = 64 * 1024;

pub struct Terminal {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    scrollback: Arc<Mutex<VecDeque<u8>>>,
    alive: Arc<AtomicBool>,
    pub output: tokio::sync::broadcast::Sender<Vec<u8>>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows: rows.clamp(4, 500), cols: cols.clamp(10, 1000), pixel_width: 0, pixel_height: 0 }
}

fn shell() -> CommandBuilder {
    if cfg!(windows) {
        let mut c = CommandBuilder::new("powershell.exe");
        c.arg("-NoLogo");
        c
    } else {
        CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()))
    }
}

impl Terminal {
    /// Start the user's shell in `cwd`. Variables in `strip` are removed from
    /// the child's environment (daemon-held credentials).
    pub fn spawn(cwd: &Path, cols: u16, rows: u16, strip: &[&str]) -> anyhow::Result<Arc<Self>> {
        let pair = native_pty_system().openpty(size(cols, rows))?;
        let mut cmd = shell();
        cmd.cwd(cwd);
        for key in strip {
            cmd.env_remove(key);
        }
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (output, _) = tokio::sync::broadcast::channel(256);
        let term = Arc::new(Self {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            scrollback: Arc::new(Mutex::new(VecDeque::with_capacity(SCROLLBACK))),
            alive: Arc::new(AtomicBool::new(true)),
            output,
        });
        let (tx, scrollback, alive) = (term.output.clone(), term.scrollback.clone(), term.alive.clone());
        // The pty read is blocking; give it its own thread.
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        {
                            let mut sb = scrollback.lock().unwrap();
                            sb.extend(&chunk);
                            let over = sb.len().saturating_sub(SCROLLBACK);
                            sb.drain(..over);
                        }
                        let _ = tx.send(chunk);
                    }
                }
            }
            alive.store(false, Ordering::SeqCst);
        });
        Ok(term)
    }

    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(bytes)?;
        w.flush()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.lock().unwrap().resize(size(cols, rows));
    }

    /// Recent output, for a viewer that just connected.
    pub fn snapshot(&self) -> Vec<u8> {
        self.scrollback.lock().unwrap().iter().copied().collect()
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst) && self.child.lock().unwrap().try_wait().ok().flatten().is_none()
    }

    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
        self.alive.store(false, Ordering::SeqCst);
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.get_mut().map(|c| c.kill());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn runs_a_command_and_keeps_scrollback() {
        let d = tempfile::tempdir().unwrap();
        let t = Terminal::spawn(d.path(), 80, 24, &["SECRET_FOR_TEST"]).unwrap();
        let mut rx = t.output.subscribe();
        t.write(b"echo arbiter-pty-ok\r\n").unwrap();
        let mut seen = String::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        // The shell echoes the typed line, then prints the output: two copies.
        while seen.matches("arbiter-pty-ok").count() < 2 {
            let chunk = tokio::time::timeout_at(deadline, rx.recv())
                .await
                .unwrap_or_else(|_| panic!("no output in time; saw {seen:?}"))
                .unwrap();
            let text = String::from_utf8_lossy(&chunk);
            // ConPTY asks for the cursor position before it starts; a real
            // terminal (xterm.js) answers this, so the test does too.
            if text.contains("\u{1b}[6n") {
                t.write(b"\x1b[1;1R").unwrap();
            }
            seen.push_str(&text);
        }
        assert!(String::from_utf8_lossy(&t.snapshot()).contains("arbiter-pty-ok"));
        t.resize(120, 40);
        assert!(t.is_alive());
        t.kill();
        assert!(!t.is_alive());
    }
}
