//! Browser side of self-healing and agent tooling, built for low token cost:
//! pages are summarized as compact text (errors, element descriptors,
//! trimmed accessibility trees) and screenshots are opt-in.
//!
//! Uses the Chrome DevTools Protocol directly against an installed Chromium
//! (Chrome, Edge, Brave, or Playwright's headless shell); nothing is downloaded.

mod cdp;
pub mod devserver;
pub mod discover;
pub mod page;
pub mod session;

pub use cdp::{Browser, open_visible};
pub use devserver::DevServer;
pub use discover::find_browser;
pub use page::{PageIssue, PageReport};
