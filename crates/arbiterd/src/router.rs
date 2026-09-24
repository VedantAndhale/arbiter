//! Picks a harness for threads created with `harness: "auto"`. Deterministic
//! and free: availability first, then spread load across subscriptions so one
//! provider's rate limits don't stall everything. Learned routing (the decider)
//! plugs in here later.

use arbiter_adapters::Harness;

pub struct Candidate {
    pub harness: Harness,
    pub installed: bool,
    pub active_runs: usize,
}

pub fn route(candidates: &[Candidate]) -> Result<(Harness, String), String> {
    let available: Vec<&Candidate> = candidates.iter().filter(|c| c.installed).collect();
    match available.as_slice() {
        [] => Err("No agent harness found. Install Claude Code (`claude`) or Codex (`codex`) and log in.".into()),
        [only] => Ok((only.harness, format!("{} is the only harness installed", name(only.harness)))),
        many => {
            let least = many.iter().map(|c| c.active_runs).min().unwrap_or(0);
            let pick = many.iter().find(|c| c.active_runs == least).expect("non-empty");
            let reason = if many.iter().all(|c| c.active_runs == least) {
                format!("{} is the default when harnesses are equally free", name(pick.harness))
            } else {
                let busiest = many.iter().max_by_key(|c| c.active_runs).expect("non-empty");
                format!(
                    "{} is busy with {} run{}; {} has {}",
                    name(busiest.harness),
                    busiest.active_runs,
                    if busiest.active_runs == 1 { "" } else { "s" },
                    name(pick.harness),
                    pick.active_runs
                )
            };
            Ok((pick.harness, reason))
        }
    }
}

/// Where an auto-routed task should run, before any cloud harness is chosen.
#[derive(Debug, PartialEq)]
pub enum Placement {
    Local(String),
    Cloud,
    Blocked(String),
}

/// Local-first placement. Only a model that passed the local coding check is
/// trusted, and only for work it can plausibly finish: small, low-risk tasks
/// the classifier does not flag for a frontier model. Local-only mode never
/// falls back to the cloud; it blocks with the reason instead.
pub fn place(
    local_only: bool,
    capable_model: Option<&str>,
    has_worktree: bool,
    assessment: &arbiter_core::intake::Assessment,
    cloud_available: bool,
) -> Placement {
    let Some(model) = capable_model else {
        return if local_only {
            Placement::Blocked(
                "Local-only mode needs a local model that passed the coding check. Run it in Settings.".into(),
            )
        } else {
            Placement::Cloud
        };
    };
    if !has_worktree {
        return if local_only {
            Placement::Blocked("Local coding needs an isolated worktree. Start the task with a worktree.".into())
        } else {
            Placement::Cloud
        };
    }
    if local_only {
        return Placement::Local(format!("Local-only mode; {model} passed the local coding check"));
    }
    if !cloud_available {
        return Placement::Local(format!(
            "No cloud agent is available under your usage policy; {model} passed the local coding check"
        ));
    }
    let small = assessment.size == "S"
        && assessment.risks.is_empty()
        && !assessment.needs_frontier
        && assessment.task_type != "research";
    if small {
        Placement::Local(format!(
            "Small, low-risk {} task; {model} passed the local coding check",
            assessment.task_type
        ))
    } else {
        Placement::Cloud
    }
}

pub fn name(h: Harness) -> &'static str {
    match h {
        Harness::Claude => "Claude Code",
        Harness::Codex => "Codex",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(harness: Harness, installed: bool, active_runs: usize) -> Candidate {
        Candidate { harness, installed, active_runs }
    }

    #[test]
    fn local_first_placement() {
        let a = |size: &str, risks: &[&str], frontier: bool| arbiter_core::intake::Assessment {
            task_type: "bugfix".into(),
            size: size.into(),
            ambiguity: 0.0,
            risks: risks.iter().map(|s| s.to_string()).collect(),
            needs_frontier: frontier,
            engine: "rules".into(),
            elapsed_ms: 0.0,
        };
        let small = a("S", &[], false);
        assert!(matches!(place(false, Some("m"), true, &small, true), Placement::Local(_)));
        assert_eq!(place(false, Some("m"), true, &a("M", &[], false), true), Placement::Cloud);
        assert_eq!(place(false, Some("m"), true, &a("S", &["auth"], false), true), Placement::Cloud);
        assert_eq!(place(false, Some("m"), true, &a("S", &[], true), true), Placement::Cloud);
        // No trusted model: the cloud, or a block in local-only mode.
        assert_eq!(place(false, None, true, &small, true), Placement::Cloud);
        assert!(matches!(place(true, None, true, &small, false), Placement::Blocked(_)));
        assert!(matches!(place(true, Some("m"), false, &small, false), Placement::Blocked(_)));
        // Large work stays local when the cloud is unavailable or disallowed.
        assert!(matches!(place(true, Some("m"), true, &a("L", &["auth"], true), false), Placement::Local(_)));
        assert!(matches!(place(false, Some("m"), true, &a("L", &[], true), false), Placement::Local(_)));
    }

    #[test]
    fn routing_rules() {
        assert!(route(&[c(Harness::Claude, false, 0), c(Harness::Codex, false, 0)]).is_err());
        assert_eq!(route(&[c(Harness::Claude, false, 0), c(Harness::Codex, true, 3)]).unwrap().0, Harness::Codex);
        assert_eq!(route(&[c(Harness::Claude, true, 0), c(Harness::Codex, true, 0)]).unwrap().0, Harness::Claude);
        let (h, why) = route(&[c(Harness::Claude, true, 2), c(Harness::Codex, true, 0)]).unwrap();
        assert_eq!(h, Harness::Codex);
        assert!(why.contains("busy with 2 runs"), "{why}");
    }
}
