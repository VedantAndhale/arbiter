//! Budgeted, reproducible per-step briefs. Never forwards agent transcripts.
use arbiter_core::plan::{Handoff, PlanNode};
#[derive(Debug, thiserror::Error)]
#[error("brief exceeds the 6000-byte budget; shorten the goal, scope or checks")]
pub struct TooLarge;
pub fn compile(conventions: &str, goal: &str, node: &PlanNode, dependencies: &[Handoff]) -> Result<String, TooLarge> {
    let mut prefix = String::new();
    for ch in conventions.chars() {
        if prefix.len() + ch.len_utf8() > 600 {
            break;
        }
        prefix.push(ch);
    }
    let mut out = format!(
        "Project conventions:\n{prefix}\n\nRole: implementation agent for one approved step.\nPlan goal: {}\nStep {}: {}\nGoal: {}\nWritable scope: {}\nMay read: {}\nNon-goals: {}\nAcceptance commands: {}\n",
        goal,
        node.id,
        node.title,
        node.goal,
        node.scope.join(", "),
        node.may_read.join(", "),
        node.non_goals.join("; "),
        node.checks.join("; ")
    );
    out.push_str("\n\nStay inside writable scope. If blocked or scope must expand, call request_scope and stop editing. Do not merge branches or publish. When finished, call report_handoff with {summary,files,decisions,interfaces,open_issues}; all lists contain short strings. If MCP is unavailable, end your final response with a JSON object containing that handoff under the key handoff. No raw logs in the handoff. The daemon runs acceptance checks and integrates your snapshot.");
    // Optional context yields space to the approved goal, scope and checks.
    // Keep JSON intact: truncated JSON is not a usable handoff contract.
    for h in dependencies {
        let note = format!("\nDependency handoff: {}", serde_json::to_string(h).unwrap_or_default());
        if out.len() + note.len() <= 6000 {
            out.push_str(&note);
        } else {
            let summary = format!("\nDependency summary: {}", h.summary.chars().take(180).collect::<String>());
            if out.len() + summary.len() <= 6000 {
                out.push_str(&summary);
            }
        }
    }
    if out.len() > 6000 { Err(TooLarge) } else { Ok(out) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn brief_preserves_scope_checks_and_rejects_overflow() {
        let mut node:PlanNode=serde_json::from_str(r#"{"id":"api","title":"API","goal":"Add endpoint","scope":["src/api/**"],"checks":["cargo test"],"non_goals":["No database changes"]}"#).unwrap();
        let brief = compile("Use Rust 2024", "Ship API", &node, &[]).unwrap();
        assert!(
            brief.contains("src/api/**")
                && brief.contains("cargo test")
                && brief.contains("No database changes")
                && brief.len() <= 6000
        );
        node.goal = "x".repeat(7000);
        assert!(compile("", "", &node, &[]).is_err());
    }
}
