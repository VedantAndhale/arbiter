//! Child processes without console windows. Arbiter's background service has
//! no console, so on Windows every console program it starts (git, gh, the
//! coding CLIs, checks) would otherwise open its own window and flash on
//! screen. Windows that are meant to be seen, such as a sign-in terminal,
//! simply don't use this.

/// `CREATE_NO_WINDOW`: run a console program without creating a window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub trait NoWindow {
    /// Start this program without a console window (no-op outside Windows).
    fn no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(CREATE_NO_WINDOW);
        }
        self
    }
}

impl NoWindow for tokio::process::Command {
    fn no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        self.creation_flags(CREATE_NO_WINDOW);
        self
    }
}
