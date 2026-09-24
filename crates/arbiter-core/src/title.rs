//! Thread titles from the first message, without spending tokens.

const MAX: usize = 50;

/// "please can you fix the flaky login test in auth.rs, it fails on CI"
/// → "Fix the flaky login test in auth.rs, it fails on…"
pub fn derive_title(message: &str) -> String {
    let line = message
        .lines()
        .map(|l| l.trim().trim_start_matches(['#', '>', '-', '*', ' ']).trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut s = line.split_whitespace().collect::<Vec<_>>().join(" ");
    // Strip conversational openers; they carry no meaning in a title.
    loop {
        let lower = s.to_ascii_lowercase();
        let Some(p) = ["please ", "pls ", "can you ", "could you ", "would you ", "hey ", "hi ", "ok ", "so "]
            .iter()
            .find(|p| lower.starts_with(**p))
        else {
            break;
        };
        s = s[p.len()..].trim_start().to_owned();
    }
    let s = s.trim_end_matches(['?', '.', '!', ' ']);
    if s.is_empty() {
        return "New thread".into();
    }
    let mut out: String = s.chars().take(MAX).collect();
    if s.chars().count() > MAX {
        if let Some(cut) = out.rfind(' ').filter(|&i| i > MAX / 2) {
            out.truncate(cut);
        }
        out = out.trim_end_matches([',', ';', ':', ' ']).to_owned() + "…";
    }
    let mut c = out.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => out,
    }
}

#[cfg(test)]
mod tests {
    use super::derive_title;

    #[test]
    fn titles() {
        assert_eq!(derive_title("fix the login bug"), "Fix the login bug");
        assert_eq!(derive_title("Please can you add dark mode?"), "Add dark mode");
        assert_eq!(derive_title("\n\n## Refactor the store\nmore details"), "Refactor the store");
        assert_eq!(derive_title("   "), "New thread");
        let long = derive_title("investigate why the websocket reconnect loop spins forever when the daemon restarts");
        assert!(long.ends_with('…') && long.chars().count() <= 51, "{long}");
        assert!(!long.contains("  "));
    }
}
