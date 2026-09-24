//! Tools Arbiter needs on this computer: detection with versions, and the
//! official install command per platform. Installing is always a separate,
//! explicitly approved step run by the daemon.
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Git,
    Node,
    Python,
    ClaudeCode,
    Codex,
}

pub const ALL: [Tool; 5] = [Tool::Git, Tool::Node, Tool::Python, Tool::ClaudeCode, Tool::Codex];

impl Tool {
    pub fn id(self) -> &'static str {
        match self {
            Tool::Git => "git",
            Tool::Node => "node",
            Tool::Python => "python",
            Tool::ClaudeCode => "claude",
            Tool::Codex => "codex",
        }
    }
    pub fn parse(id: &str) -> Option<Tool> {
        ALL.into_iter().find(|t| t.id() == id)
    }
    pub fn name(self) -> &'static str {
        match self {
            Tool::Git => "Git",
            Tool::Node => "Node.js",
            Tool::Python => "Python",
            Tool::ClaudeCode => "Claude Code",
            Tool::Codex => "Codex",
        }
    }
    /// Why a builder needs it, in plain words.
    pub fn why(self) -> &'static str {
        match self {
            Tool::Git => "Keeps every version of your project so nothing is lost. Required.",
            Tool::Node => "Runs most web apps and installs Claude Code and Codex.",
            Tool::Python => "Runs Python apps and APIs. Only needed for Python projects.",
            Tool::ClaudeCode => "An AI coding agent from Anthropic. Needs a Claude plan.",
            Tool::Codex => "An AI coding agent from OpenAI. Needs a ChatGPT plan.",
        }
    }
    pub fn required(self) -> bool {
        self == Tool::Git
    }
    fn probe(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Tool::Git => ("git", &["--version"]),
            Tool::Node => ("node", &["--version"]),
            Tool::Python => (if cfg!(windows) { "python" } else { "python3" }, &["--version"]),
            Tool::ClaudeCode => ("claude", &["--version"]),
            Tool::Codex => ("codex", &["--version"]),
        }
    }
    /// The official install command for this platform, when one exists.
    pub fn install_command(self) -> Option<Vec<String>> {
        let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
        match self {
            Tool::ClaudeCode => v(&["npm", "install", "-g", "@anthropic-ai/claude-code"]),
            Tool::Codex => v(&["npm", "install", "-g", "@openai/codex"]),
            _ if cfg!(windows) => {
                let id = match self {
                    Tool::Git => "Git.Git",
                    Tool::Node => "OpenJS.NodeJS.LTS",
                    _ => "Python.Python.3.12",
                };
                v(&[
                    "winget",
                    "install",
                    "--id",
                    id,
                    "--exact",
                    "--silent",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                ])
            }
            _ if cfg!(target_os = "macos") => v(&[
                "brew",
                "install",
                match self {
                    Tool::Git => "git",
                    Tool::Node => "node",
                    _ => "python",
                },
            ]),
            _ => None,
        }
    }
    pub fn manual_url(self) -> &'static str {
        match self {
            Tool::Git => "https://git-scm.com/downloads",
            Tool::Node => "https://nodejs.org/en/download",
            Tool::Python => "https://www.python.org/downloads/",
            Tool::ClaudeCode => "https://code.claude.com/docs/en/setup",
            Tool::Codex => "https://developers.openai.com/codex/cli",
        }
    }
    /// Interactive sign-in, for the agents.
    pub fn sign_in(self) -> Option<&'static str> {
        match self {
            Tool::ClaudeCode => Some("claude auth login"),
            Tool::Codex => Some("codex login"),
            _ => None,
        }
    }
}

fn command(program: &str) -> tokio::process::Command {
    // On Windows, npm-installed CLIs are .cmd shims; run through cmd.
    let mut c = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(program);
        #[cfg(windows)]
        c.creation_flags(0x0800_0000);
        c
    } else {
        tokio::process::Command::new(program)
    };
    c.stdin(std::process::Stdio::null()).kill_on_drop(true);
    c
}

/// The installed version, or None when the tool is missing.
pub async fn version(tool: Tool) -> Option<String> {
    let (program, args) = tool.probe();
    let out = tokio::time::timeout(Duration::from_secs(10), command(program).args(args).output()).await.ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("").trim();
    (!line.is_empty()).then(|| line.chars().take(80).collect())
}

/// Run an approved install. Returns the tail of its output.
pub async fn install(tool: Tool) -> Result<String, String> {
    let argv = tool.install_command().ok_or_else(|| format!("Install {} from {}", tool.name(), tool.manual_url()))?;
    let out = tokio::time::timeout(Duration::from_secs(15 * 60), command(&argv[0]).args(&argv[1..]).output())
        .await
        .map_err(|_| "The install took longer than 15 minutes and was stopped.".to_string())?
        .map_err(|e| format!("Could not start the installer: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = tail[tail.len().saturating_sub(6)..].join("\n");
    if out.status.success() {
        Ok(tail)
    } else {
        Err(if tail.is_empty() { "The installer reported an error.".into() } else { tail })
    }
}

/// Open a visible terminal window running an interactive command, for
/// sign-ins that need the user.
pub fn open_terminal(cmdline: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd").args(["/C", "start", "Arbiter sign-in", "cmd", "/K", cmdline]).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!("tell application \"Terminal\" to do script \"{}\"", cmdline.replace('"', "\\\""));
        std::process::Command::new("osascript").args(["-e", &script]).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("x-terminal-emulator").args(["-e", cmdline]).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tools_are_described_and_parse() {
        for t in ALL {
            assert_eq!(Tool::parse(t.id()), Some(t));
            assert!(!t.why().is_empty() && t.manual_url().starts_with("https://"));
        }
        assert!(Tool::Git.required() && !Tool::Codex.required());
        assert!(Tool::ClaudeCode.install_command().unwrap().contains(&"@anthropic-ai/claude-code".to_string()));
        assert_eq!(Tool::Codex.sign_in(), Some("codex login"));
    }
    #[tokio::test]
    async fn git_is_detected_here() {
        // The test machine runs git for every other test.
        assert!(version(Tool::Git).await.is_some_and(|v| v.contains("git")));
    }
}
