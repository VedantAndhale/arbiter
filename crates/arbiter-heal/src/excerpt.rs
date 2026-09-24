//! Failures → the smallest message that lets an agent fix them.
//! Every character here is re-sent on each heal attempt, so it is capped,
//! deduplicated, grouped by file, and shows only ±2 lines of code.

use crate::detect::{Check, CheckKind};
use crate::parse::{Failure, strip_ansi};
use crate::runner::CheckRun;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Excerpt {
    pub text: String,
    /// Stable across runs for the same set of failures (ignores line shifts).
    pub signature: String,
    /// Failures included after filtering.
    pub count: usize,
    /// Failures dropped as pre-existing (in files the agent didn't touch).
    pub ignored: usize,
}

/// A failed check with its parsed failures.
pub struct Failed<'a> {
    pub check: &'a Check,
    pub run: &'a CheckRun,
    pub failures: Vec<Failure>,
}

/// Build the heal message. `changed` = files the agent modified (relative,
/// `/`-separated). Typecheck/lint failures in untouched files are treated as
/// pre-existing and dropped; test failures always count (a change can break a
/// test elsewhere).
pub fn build_excerpt(
    failed: &[Failed],
    root: &Path,
    changed: &[String],
    budget: usize,
    attempt: u32,
    max: u32,
) -> Excerpt {
    let touched = |f: &Failure| {
        f.file.as_deref().is_some_and(|p| changed.iter().any(|c| p.ends_with(c.as_str()) || c.ends_with(p)))
    };
    let mut body = String::new();
    let mut sig_items: Vec<String> = Vec::new();
    let (mut count, mut ignored) = (0, 0);

    for f in failed {
        let keep: Vec<&Failure> =
            f.failures.iter().filter(|x| f.check.kind == CheckKind::Test || x.file.is_none() || touched(x)).collect();
        ignored += f.failures.len() - keep.len();
        if keep.is_empty() && !f.failures.is_empty() {
            continue; // only pre-existing noise
        }
        body.push_str(&format!("\n## {} failed (`{}`)\n", f.check.name, f.check.cmd));
        if keep.is_empty() {
            // Unparsed output: the tail usually holds the reason.
            let tail = tail_lines(&strip_ansi(&f.run.output), 25);
            body.push_str("```\n");
            body.push_str(&tail);
            body.push_str("\n```\n");
            sig_items.push(format!("{}|raw|{}", f.check.name, normalize(&tail)));
            count += 1;
            continue;
        }
        let mut by_file: BTreeMap<String, Vec<&Failure>> = BTreeMap::new();
        for x in &keep {
            by_file.entry(x.file.clone().unwrap_or_else(|| "(no file)".into())).or_default().push(x);
        }
        for (file, items) in by_file {
            for x in items {
                count += 1;
                sig_items.push(format!(
                    "{}|{}|{}|{}",
                    f.check.name,
                    file,
                    x.code.as_deref().unwrap_or(""),
                    normalize(&x.message)
                ));
                let loc = match (x.line, x.col) {
                    (Some(l), Some(c)) => format!("{file}:{l}:{c}"),
                    (Some(l), None) => format!("{file}:{l}"),
                    _ => file.clone(),
                };
                let code = x.code.as_deref().map(|c| format!(" [{c}]")).unwrap_or_default();
                body.push_str(&format!("- {loc}{code} {}\n", x.message));
                if let Some(l) = x.line
                    && let Some(snippet) = snippet(root, &file, l)
                {
                    body.push_str(&snippet);
                }
            }
        }
    }

    sig_items.sort();
    sig_items.dedup();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sig_items.hash(&mut h);
    let signature = format!("{:016x}", h.finish());

    let head = format!(
        "Arbiter ran the project's checks on your changes (attempt {attempt}/{max}). Fix these failures, keep the change minimal, and don't touch unrelated code.\n"
    );
    let mut text = head + &body;
    if ignored > 0 {
        text.push_str(&format!("\n({ignored} pre-existing issue(s) in files you didn't touch were ignored.)\n"));
    }
    if text.len() > budget {
        let mut cut = budget.saturating_sub(80);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n… (more failures omitted; fix the ones above first)\n");
    }
    Excerpt { text, signature, count, ignored }
}

/// Drop digits and quoted identifiers' positions so a failure that merely
/// moved lines keeps the same signature.
fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_ascii_digit()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Last `n` meaningful lines. Runtime-internal stack frames (node internals,
/// std/tokio, site-packages) never help the agent and cost tokens, so they go.
fn tail_lines(s: &str, n: usize) -> String {
    let noise = |l: &str| {
        let t = l.trim_start();
        t.is_empty()
            || (t.starts_with("at ") && (t.contains("node:") || t.contains("internal/") || t.contains("node_modules")))
            || t.contains("/rustc/")
            || t.contains("site-packages")
    };
    let lines: Vec<&str> = s.lines().filter(|l| !noise(l)).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn snippet(root: &Path, file: &str, line: u32) -> Option<String> {
    let path = if Path::new(file).is_absolute() { Path::new(file).to_path_buf() } else { root.join(file) };
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let idx = (line as usize).checked_sub(1)?;
    if idx >= lines.len() {
        return None;
    }
    let (from, to) = (idx.saturating_sub(2), (idx + 2).min(lines.len() - 1));
    let mut s = String::from("```\n");
    for (i, l) in lines.iter().enumerate().take(to + 1).skip(from) {
        let marker = if i == idx { ">" } else { " " };
        let l: String = l.chars().take(160).collect();
        s.push_str(&format!("{marker}{:>5} | {l}\n", i + 1));
    }
    s.push_str("```\n");
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::CheckKind;

    fn check(kind: CheckKind) -> Check {
        Check { name: format!("{kind:?}").to_lowercase(), kind, cmd: "x".into(), fix: None, timeout_secs: 1 }
    }

    fn run(output: &str) -> CheckRun {
        CheckRun {
            name: "x".into(),
            ok: false,
            exit_code: Some(1),
            output: output.into(),
            duration_ms: 1,
            timed_out: false,
        }
    }

    fn fail(file: &str, line: u32, msg: &str) -> Failure {
        Failure {
            file: Some(file.into()),
            line: Some(line),
            col: Some(1),
            code: Some("E1".into()),
            message: msg.into(),
        }
    }

    #[test]
    fn filters_preexisting_includes_snippet_and_is_stable() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src/a.ts"), "one\ntwo\nthree bad\nfour\nfive\n").unwrap();
        let tc = check(CheckKind::Typecheck);
        let r = run("");
        let failed = [Failed {
            check: &tc,
            run: &r,
            failures: vec![fail("src/a.ts", 3, "bad type 42"), fail("src/old.ts", 9, "legacy")],
        }];
        let e = build_excerpt(&failed, d.path(), &["src/a.ts".into()], 6000, 1, 3);
        assert_eq!((e.count, e.ignored), (1, 1));
        assert!(e.text.contains(">    3 | three bad"), "{}", e.text);
        assert!(!e.text.contains("legacy"));

        // Same failure on a different line/number → same signature.
        let moved = [Failed { check: &tc, run: &r, failures: vec![fail("src/a.ts", 4, "bad type 43")] }];
        let e2 = build_excerpt(&moved, d.path(), &["src/a.ts".into()], 6000, 2, 3);
        assert_eq!(e.signature, e2.signature);
    }

    #[test]
    fn test_failures_count_even_in_untouched_files_and_budget_is_enforced() {
        let t = check(CheckKind::Test);
        let r = run("");
        let many: Vec<Failure> = (0..200).map(|i| fail(&format!("tests/t{i}.py"), 1, "assert failed")).collect();
        let failed = [Failed { check: &t, run: &r, failures: many }];
        let e = build_excerpt(&failed, Path::new("."), &[], 1500, 1, 3);
        assert!(e.text.len() <= 1500 + 80, "{}", e.text.len());
        assert!(e.text.contains("more failures omitted"));
        assert_eq!(e.ignored, 0);
    }

    #[test]
    fn unparsed_output_uses_tail() {
        let b = check(CheckKind::Build);
        let r = run("step 1\nstep 2\nFATAL: out of disk\n");
        let e = build_excerpt(&[Failed { check: &b, run: &r, failures: vec![] }], Path::new("."), &[], 6000, 1, 3);
        assert!(e.text.contains("FATAL: out of disk"));
        assert_eq!(e.count, 1);
    }
}
