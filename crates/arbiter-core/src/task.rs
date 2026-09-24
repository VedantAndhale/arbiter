//! Linear-style tasks. Unlike threads (an append-only log of what an agent
//! did), a task is a mutable planning record: what should be done, its
//! status, and which threads worked on it.

use crate::ids::{ProjectId, TaskId, ThreadId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Backlog,
    #[default]
    Todo,
    InProgress,
    InReview,
    Done,
    Canceled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Urgent,
    High,
    Medium,
    Low,
    #[default]
    None,
}

macro_rules! str_enum {
    ($t:ty { $($v:ident => $s:literal),* $(,)? }) => {
        impl $t {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$v => $s),* }
            }
        }
        impl std::str::FromStr for $t {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s { $($s => Ok(Self::$v),)* _ => Err(format!("unknown {} {s:?}", stringify!($t))) }
            }
        }
    };
}

str_enum!(TaskStatus {
    Backlog => "backlog", Todo => "todo", InProgress => "in_progress",
    InReview => "in_review", Done => "done", Canceled => "canceled",
});
str_enum!(Priority { Urgent => "urgent", High => "high", Medium => "medium", Low => "low", None => "none" });

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub project_id: ProjectId,
    /// Human key like `ORC-12` (project prefix + per-project number).
    pub key: String,
    pub number: i64,
    pub title: String,
    /// Markdown.
    pub description: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub labels: Vec<String>,
    pub parent_id: Option<TaskId>,
    /// Manual ordering within a status column.
    pub position: f64,
    pub thread_ids: Vec<ThreadId>,
    pub created_at: String,
    pub updated_at: String,
}

/// Uppercase key prefix from a project name: "orchestrator" → "ORC",
/// "my-web app" → "MWA". Always 2–4 ASCII letters.
pub fn key_prefix(project_name: &str) -> String {
    let words: Vec<&str> = project_name.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let letters: String = if words.len() >= 2 {
        words.iter().filter_map(|w| w.chars().find(|c| c.is_ascii_alphabetic())).take(4).collect()
    } else {
        project_name.chars().filter(|c| c.is_ascii_alphabetic()).take(3).collect()
    };
    let p = letters.to_ascii_uppercase();
    if p.len() >= 2 { p } else { "TSK".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes() {
        assert_eq!(key_prefix("orchestrator"), "ORC");
        assert_eq!(key_prefix("my-web app"), "MWA");
        assert_eq!(key_prefix("x"), "TSK");
        assert_eq!(key_prefix("42"), "TSK");
    }

    #[test]
    fn enums_roundtrip() {
        for s in ["backlog", "todo", "in_progress", "in_review", "done", "canceled"] {
            let st: TaskStatus = s.parse().unwrap();
            assert_eq!(serde_json::to_value(st).unwrap(), s);
        }
        assert_eq!("urgent".parse::<Priority>().unwrap().as_str(), "urgent");
    }
}
