//! Tool output → structured failures. One pass of cheap regexes; unknown
//! formats fall back to a generic `file:line: …error…` scan, and if nothing
//! matches at all the caller uses the output tail.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Failure {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub col: Option<u32>,
    /// Tool-specific code (TS2322, E0308, no-unused-vars) or the failing test name.
    pub code: Option<String>,
    pub message: String,
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect("valid regex"));
    };
}

re!(ANSI, r"\x1b\[[0-9;?]*[ -/]*[@-~]");
// src/a.ts(12,5): error TS2322: Type 'x' is not assignable…
re!(TSC, r"^(?P<file>[^\s(][^(]*?)\((?P<line>\d+),(?P<col>\d+)\): error (?P<code>TS\d+): (?P<msg>.+)$");
// src/a.ts:12:5 - error TS2322: … (tsc --pretty)
re!(TSC_PRETTY, r"^(?P<file>[^\s:][^:]*):(?P<line>\d+):(?P<col>\d+) - error (?P<code>TS\d+): (?P<msg>.+)$");
// cargo --message-format short: src/lib.rs:3:5: error[E0308]: mismatched types
re!(CARGO_SHORT, r"^(?P<file>[^\s:][^:]*\.rs):(?P<line>\d+):(?P<col>\d+): error(\[(?P<code>\w+)\])?: (?P<msg>.+)$");
// cargo human format: error[E0308]: msg  /  --> src/lib.rs:3:5
re!(CARGO_HEAD, r"^error(\[(?P<code>\w+)\])?: (?P<msg>.+)$");
re!(CARGO_ARROW, r"^\s*--> (?P<file>[^:]+):(?P<line>\d+):(?P<col>\d+)$");
// thread 'tests::adds' panicked at src/lib.rs:10:5:
re!(
    RUST_PANIC,
    r"^thread '(?P<test>[^']+)' (\(\d+\) )?panicked at (?P<file>[^:]+):(?P<line>\d+):(?P<col>\d+):?(?P<msg>.*)$"
);
// eslint stylish: "  12:5  error  'x' is defined but never used  no-unused-vars"
re!(ESLINT_ROW, r"^\s+(?P<line>\d+):(?P<col>\d+)\s+error\s+(?P<msg>.+?)\s{2,}(?P<code>[\w@/-]+)\s*$");
re!(ESLINT_FILE, r"^(?P<file>(?:[A-Za-z]:)?[\\/]?[^\s:][^:]*\.[cm]?[jt]sx?)$");
// pytest -rf: FAILED tests/test_a.py::test_add - assert 1 == 2
re!(PYTEST_FAILED, r"^(FAILED|ERROR) (?P<file>[^:\s]+\.py)(::(?P<test>\S+))?( - (?P<msg>.+))?$");
re!(PY_LOC, r"^(?P<file>[^:\s]+\.py):(?P<line>\d+): (?P<msg>\w*(Error|Exception).*)$");
// vitest/jest: " FAIL  src/a.test.ts > math > adds"  /  "FAIL src/a.test.ts"
re!(JS_FAIL, r"^\s*(FAIL|×|✕)\s+(?P<file>\S+\.(test|spec)\.[cm]?[jt]sx?)(\s+>\s+(?P<test>.+))?\s*$");
re!(JS_LOC, r"(❯|at .*\()\s*(?P<file>[^\s()]+\.[cm]?[jt]sx?):(?P<line>\d+):(?P<col>\d+)\)?\s*$");
re!(JS_ASSERT, r"^\s*(AssertionError|Error|TypeError|ReferenceError|expect\().*");
re!(NODE_TEST_AT, r"^\s*test at (?P<file>[^\s:][^:]*\.[cm]?[jt]sx?):(?P<line>\d+):(?P<col>\d+)\s*$");
re!(NODE_TEST_NAME, r"^\s*✖ (?P<test>.+?) \(\d+(\.\d+)?ms\)\s*$");
// go: ./a.go:3:5: undefined: x   /  --- FAIL: TestAdd (0.00s)  /  a_test.go:12: got 1 want 2
re!(GO_LOC, r"^\s*(?P<file>[^:\s]+\.go):(?P<line>\d+)(:(?P<col>\d+))?: (?P<msg>.+)$");
re!(GO_FAIL, r"^--- FAIL: (?P<test>\S+)");
re!(
    GENERIC,
    r"^(?P<file>(?:[A-Za-z]:)?[\w./\\-]+\.\w{1,5}):(?P<line>\d+)(:(?P<col>\d+))?:?\s+(?P<msg>.*(?i:error|failed|panic).*)$"
);

pub fn strip_ansi(s: &str) -> String {
    ANSI.replace_all(s, "").into_owned()
}

fn num(c: &regex::Captures, k: &str) -> Option<u32> {
    c.name(k).and_then(|m| m.as_str().parse().ok())
}

fn s(c: &regex::Captures, k: &str) -> Option<String> {
    c.name(k).map(|m| m.as_str().trim().to_owned()).filter(|v| !v.is_empty())
}

fn norm_path(p: String) -> String {
    p.trim_start_matches("./").replace('\\', "/")
}

/// Parse any supported tool's output. Order matters: specific formats first.
pub fn parse(output: &str) -> Vec<Failure> {
    let text = strip_ansi(output);
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<Failure> = Vec::new();
    let mut eslint_file: Option<String> = None;
    let mut cargo_pending: Option<(Option<String>, String)> = None;
    let mut js_pending: Option<Failure> = None;
    let mut go_test: Option<String> = None;

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim_end();
        if let Some(c) = TSC.captures(line).or_else(|| TSC_PRETTY.captures(line)) {
            out.push(Failure {
                file: s(&c, "file").map(norm_path),
                line: num(&c, "line"),
                col: num(&c, "col"),
                code: s(&c, "code"),
                message: s(&c, "msg").unwrap_or_default(),
            });
            continue;
        }
        if let Some(c) = CARGO_SHORT.captures(line) {
            out.push(Failure {
                file: s(&c, "file").map(norm_path),
                line: num(&c, "line"),
                col: num(&c, "col"),
                code: s(&c, "code"),
                message: s(&c, "msg").unwrap_or_default(),
            });
            continue;
        }
        if let Some(c) = CARGO_HEAD.captures(line) {
            let msg = s(&c, "msg").unwrap_or_default();
            // "error: could not compile" is a summary, not a failure.
            if !msg.starts_with("could not compile")
                && !msg.starts_with("aborting due to")
                && !msg.contains("test failed")
            {
                cargo_pending = Some((s(&c, "code"), msg));
            }
            continue;
        }
        if let Some(c) = CARGO_ARROW.captures(line)
            && let Some((code, msg)) = cargo_pending.take()
        {
            out.push(Failure {
                file: s(&c, "file").map(norm_path),
                line: num(&c, "line"),
                col: num(&c, "col"),
                code,
                message: msg,
            });
            continue;
        }
        if let Some(c) = RUST_PANIC.captures(line) {
            let mut msg = s(&c, "msg").unwrap_or_default();
            if msg.is_empty() {
                msg = lines.get(i + 1).map(|l| l.trim().to_owned()).unwrap_or_default();
            }
            out.push(Failure {
                file: s(&c, "file").map(norm_path),
                line: num(&c, "line"),
                col: num(&c, "col"),
                code: s(&c, "test"),
                message: msg,
            });
            continue;
        }
        if let Some(c) = ESLINT_FILE.captures(line) {
            eslint_file = s(&c, "file").map(norm_path);
            continue;
        }
        if let Some(c) = ESLINT_ROW.captures(line)
            && eslint_file.is_some()
        {
            out.push(Failure {
                file: eslint_file.clone(),
                line: num(&c, "line"),
                col: num(&c, "col"),
                code: s(&c, "code"),
                message: s(&c, "msg").unwrap_or_default(),
            });
            continue;
        }
        if let Some(c) = PYTEST_FAILED.captures(line) {
            out.push(Failure {
                file: s(&c, "file").map(norm_path),
                line: None,
                col: None,
                code: s(&c, "test"),
                message: s(&c, "msg").unwrap_or_else(|| "failed".into()),
            });
            continue;
        }
        if let Some(c) = PY_LOC.captures(line) {
            out.push(Failure {
                file: s(&c, "file").map(norm_path),
                line: num(&c, "line"),
                col: None,
                code: None,
                message: s(&c, "msg").unwrap_or_default(),
            });
            continue;
        }
        if let Some(c) = JS_FAIL.captures(line) {
            if let Some(f) = js_pending.take() {
                out.push(f);
            }
            js_pending = Some(Failure {
                file: s(&c, "file").map(norm_path),
                line: None,
                col: None,
                code: s(&c, "test"),
                message: "test failed".into(),
            });
            continue;
        }
        // node --test: "test at calc.test.js:4:1" then "✖ add (2.7ms)" then the error.
        if let Some(c) = NODE_TEST_AT.captures(line) {
            if let Some(f) = js_pending.take() {
                out.push(f);
            }
            js_pending = Some(Failure {
                file: s(&c, "file").map(norm_path),
                line: num(&c, "line"),
                col: num(&c, "col"),
                code: None,
                message: "test failed".into(),
            });
            continue;
        }
        if let Some(f) = js_pending.as_mut() {
            if f.code.is_none()
                && let Some(c) = NODE_TEST_NAME.captures(line)
            {
                f.code = s(&c, "test");
                continue;
            }
            if f.message == "test failed" && JS_ASSERT.is_match(line) {
                f.message = line.trim().to_owned();
                // "Expected values to be strictly equal:" — the values are on the next line.
                if f.message.ends_with(':')
                    && let Some(next) = lines.get(i + 1).map(|l| l.trim()).filter(|l| !l.is_empty() && l.len() < 200)
                {
                    f.message = format!("{} {next}", f.message);
                }
                continue;
            }
            if let Some(c) = JS_LOC.captures(line)
                && f.line.is_none()
            {
                f.file = s(&c, "file").map(norm_path).or(f.file.take());
                f.line = num(&c, "line");
                f.col = num(&c, "col");
                continue;
            }
        }
        if let Some(c) = GO_FAIL.captures(line) {
            go_test = s(&c, "test");
            continue;
        }
        if let Some(c) = GO_LOC.captures(line) {
            out.push(Failure {
                file: s(&c, "file").map(norm_path),
                line: num(&c, "line"),
                col: num(&c, "col"),
                code: go_test.clone(),
                message: s(&c, "msg").unwrap_or_default(),
            });
            continue;
        }
    }
    if let Some(f) = js_pending {
        out.push(f);
    }
    if out.is_empty() {
        for line in &lines {
            if let Some(c) = GENERIC.captures(line.trim_end()) {
                out.push(Failure {
                    file: s(&c, "file").map(norm_path),
                    line: num(&c, "line"),
                    col: num(&c, "col"),
                    code: None,
                    message: s(&c, "msg").unwrap_or_default(),
                });
            }
        }
    }
    // Same failure reported twice (e.g. by two passes) counts once.
    let mut seen = std::collections::HashSet::new();
    out.retain(|f| seen.insert((f.file.clone(), f.line, f.code.clone(), f.message.clone())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(output: &str) -> Failure {
        let f = parse(output);
        assert_eq!(f.len(), 1, "{f:#?}");
        f.into_iter().next().unwrap()
    }

    #[test]
    fn tsc() {
        let f = one("src/auth.ts(42,7): error TS2322: Type 'string' is not assignable to type 'number'.\n");
        assert_eq!(
            (f.file.as_deref(), f.line, f.col, f.code.as_deref()),
            (Some("src/auth.ts"), Some(42), Some(7), Some("TS2322"))
        );
        let p = one("\x1b[96msrc/a.ts\x1b[0m:3:1 - \x1b[91merror\x1b[0m TS1005: ';' expected.\n");
        assert_eq!((p.file.as_deref(), p.code.as_deref()), (Some("src/a.ts"), Some("TS1005")));
    }

    #[test]
    fn cargo_short_and_human() {
        let f = one(
            "src/lib.rs:3:5: error[E0308]: mismatched types\nerror: could not compile `x` (lib) due to 1 previous error\n",
        );
        assert_eq!((f.code.as_deref(), f.line), (Some("E0308"), Some(3)));
        let h = parse(
            "error[E0425]: cannot find value `y` in this scope\n --> src\\main.rs:4:13\n  |\n4 |     let x = y;\n\nerror: aborting due to 1 previous error\n",
        );
        assert_eq!(h.len(), 1);
        assert_eq!((h[0].file.as_deref(), h[0].line), (Some("src/main.rs"), Some(4)));
    }

    #[test]
    fn rust_test_panic() {
        let f = one(
            "---- tests::adds stdout ----\n\nthread 'tests::adds' panicked at src/lib.rs:10:9:\nassertion `left == right` failed\n  left: 3\n right: 4\n",
        );
        assert_eq!((f.code.as_deref(), f.line), (Some("tests::adds"), Some(10)));
        assert!(f.message.contains("assertion"));
    }

    #[test]
    fn eslint_stylish() {
        let f = parse(
            "\nC:\\work\\repo\\src\\app.tsx\n  12:5  error  'x' is assigned a value but never used  no-unused-vars\n  14:1  warning  Unexpected console statement  no-console\n\n✖ 2 problems (1 error, 1 warning)\n",
        );
        assert_eq!(f.len(), 1, "warnings are not failures: {f:#?}");
        assert_eq!((f[0].code.as_deref(), f[0].line), (Some("no-unused-vars"), Some(12)));
        assert!(f[0].file.as_deref().unwrap().ends_with("src/app.tsx"));
    }

    #[test]
    fn pytest() {
        let f = parse(
            "tests/test_calc.py:5: AssertionError\n=========== short test summary info ===========\nFAILED tests/test_calc.py::test_add - assert -1 == 3\n",
        );
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].code.as_deref(), Some("test_add"));
        assert_eq!(f[1].message, "assert -1 == 3");
    }

    #[test]
    fn vitest() {
        let f = one(
            " FAIL  src/calc.test.ts > calc > adds\nAssertionError: expected -1 to be 3 // Object.is equality\n ❯ src/calc.test.ts:5:23\n",
        );
        assert_eq!(
            (f.file.as_deref(), f.line, f.code.as_deref()),
            (Some("src/calc.test.ts"), Some(5), Some("calc > adds"))
        );
        assert!(f.message.starts_with("AssertionError"));
    }

    #[test]
    fn node_test_runner() {
        let out = "ℹ tests 1\nℹ fail 1\n✖ failing tests:\n\ntest at calc.test.js:4:1\n✖ add (2.7246ms)\n  AssertionError [ERR_ASSERTION]: Expected values to be strictly equal:\n  -1 !== 5\n      at TestContext.<anonymous> (C:\\x\\calc.test.js:4:26)\n      at Test.runInAsyncScope (node:async_hooks:227:14)\n";
        let f = one(out);
        assert_eq!((f.file.as_deref(), f.line, f.code.as_deref()), (Some("calc.test.js"), Some(4), Some("add")));
        assert!(f.message.ends_with("strictly equal: -1 !== 5"), "{}", f.message);
    }

    #[test]
    fn go() {
        let f = parse("--- FAIL: TestAdd (0.00s)\n    calc_test.go:9: got -1, want 3\nFAIL\n");
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].code.as_deref(), f[0].line), (Some("TestAdd"), Some(9)));
    }

    #[test]
    fn generic_fallback_and_nothing() {
        let f = one("build.sh ok\nlib/x.zig:7:3: error: expected ';'\n");
        assert_eq!(f.line, Some(7));
        assert!(parse("all good\n").is_empty());
    }
}
