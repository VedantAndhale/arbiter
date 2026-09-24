//! Owns a harness child process: feeds stdin from commands, parses stdout
//! through the codec, and guarantees the whole process tree dies on shutdown.

use crate::{Codec, Command, HarnessEvent};
use arbiter_supervisor::SupervisedChild;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

pub struct RunHandle {
    commands: mpsc::UnboundedSender<Command>,
    pub events: mpsc::UnboundedReceiver<HarnessEvent>,
}

impl RunHandle {
    /// Build a handle from raw channels, for in-process fakes in tests.
    pub fn from_channels(
        commands: mpsc::UnboundedSender<Command>,
        events: mpsc::UnboundedReceiver<HarnessEvent>,
    ) -> Self {
        Self { commands, events }
    }

    /// Returns false if the process has already exited.
    pub fn send(&self, cmd: Command) -> bool {
        self.commands.send(cmd).is_ok()
    }

    /// A cloneable sender, so callers can keep driving the run while another
    /// task consumes `events`.
    pub fn sender(&self) -> mpsc::UnboundedSender<Command> {
        self.commands.clone()
    }
}

pub(crate) fn spawn(mut cmd: tokio::process::Command, mut codec: impl Codec) -> anyhow::Result<RunHandle> {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = SupervisedChild::spawn(cmd)?;
    let mut stdin = child.child.stdin.take().expect("piped stdin");
    let stdout = child.child.stdout.take().expect("piped stdout");
    let mut stderr = child.child.stderr.take().expect("piped stderr");

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Command>();
    let (ev_tx, ev_rx) = mpsc::unbounded_channel();

    // Keep only the tail of stderr; it explains crashes without hoarding memory.
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 8192 {
                buf.drain(..buf.len() - 4096);
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut write_ok = write_all(&mut stdin, codec.on_start()).await;
        let mut killed = false;
        loop {
            tokio::select! {
                cmd = cmd_rx.recv(), if !killed => match cmd {
                    Some(Command::Shutdown) | None => {
                        let _ = child.kill_tree().await;
                        killed = true;
                    }
                    Some(c) => {
                        if write_ok {
                            write_ok = write_all(&mut stdin, codec.on_command(&c)).await;
                        }
                    }
                },
                line = lines.next_line() => match line {
                    Ok(Some(line)) if line.trim().is_empty() => {}
                    Ok(Some(line)) => {
                        let (events, replies) = codec.on_line(&line);
                        for e in events {
                            let _ = ev_tx.send(e);
                        }
                        if write_ok {
                            write_ok = write_all(&mut stdin, replies).await;
                        }
                    }
                    // EOF or broken pipe: the process is done.
                    Ok(None) | Err(_) => break,
                },
            }
        }
        let code = child.child.wait().await.ok().and_then(|s| s.code());
        let stderr_tail = stderr_task.await.unwrap_or_default();
        let _ = ev_tx.send(HarnessEvent::Exited { code, stderr_tail });
    });

    Ok(RunHandle { commands: cmd_tx, events: ev_rx })
}

async fn write_all(stdin: &mut tokio::process::ChildStdin, lines: Vec<String>) -> bool {
    for l in lines {
        tracing::trace!(line = %l, "-> harness");
        if stdin.write_all(l.as_bytes()).await.is_err() || stdin.write_all(b"\n").await.is_err() {
            return false;
        }
    }
    stdin.flush().await.is_ok()
}
