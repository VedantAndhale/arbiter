use crate::{Result, now_str, parse};
use arbiter_core::{AgentEvent, Event, EventKind, PermissionMode, ProjectId, ThreadId, ThreadStatus, WorkspaceId};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: ThreadId,
    pub project_id: ProjectId,
    pub title: String,
    pub harness: String,
    pub status: ThreadStatus,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub session_id: Option<String>,
    pub parent: Option<(ThreadId, i64)>,
    pub permission: PermissionMode,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub tool_profile: arbiter_core::ToolProfile,
    pub plan_root: Option<ThreadId>,
    pub plan_node: Option<String>,
    pub base: Option<String>,
    pub budget_usd: Option<f64>,
    /// Hidden from the Inbox until the next status change.
    pub settled: bool,
    pub checkpoints: u32,
    pub last_checkpoint: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub heal_attempts: u32,
    pub last_seq: i64,
    pub updated_at: String,
}

pub(crate) fn apply(conn: &Connection, e: &Event) -> Result<()> {
    let id = e.thread_id.to_string();
    let now = now_str();
    match &e.kind {
        EventKind::PlanChild { root, node, .. } => {
            conn.execute(
                "UPDATE threads SET plan_root=?2,plan_node=?3 WHERE id=?1",
                params![id, root.to_string(), node],
            )?;
        }
        EventKind::ToolProfileChanged { profile } => {
            let value = serde_json::to_value(profile)?;
            conn.execute("UPDATE threads SET tool_profile = ?2 WHERE id = ?1", params![id, value.as_str()])?;
        }
        EventKind::ThreadCreated {
            project_id,
            title,
            harness,
            worktree,
            branch,
            parent,
            permission,
            model,
            effort,
            base,
        } => {
            conn.execute(
                "INSERT INTO threads (id, workspace_id, project_id, title, harness, status, worktree, branch,
                    parent_thread_id, fork_seq, last_seq, permission, model, effort, base, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?13, ?14, ?15, ?16, ?12, ?12)",
                params![
                    id,
                    e.workspace_id.to_string(),
                    project_id.to_string(),
                    title,
                    harness,
                    ThreadStatus::Idle.as_str(),
                    worktree,
                    branch,
                    parent.map(|p| p.0.to_string()),
                    parent.map(|p| p.1),
                    e.seq,
                    now,
                    permission.as_str(),
                    model,
                    effort,
                    base,
                ],
            )?;
            return Ok(());
        }
        EventKind::ConfigChanged { harness, model, effort, permission, .. } => {
            conn.execute(
                "UPDATE threads SET harness = ?2, model = ?3, effort = ?4, permission = ?5 WHERE id = ?1",
                params![id, harness, model, effort, permission.as_str()],
            )?;
        }
        EventKind::Renamed { title } => {
            conn.execute("UPDATE threads SET title = ?2 WHERE id = ?1", params![id, title])?;
        }
        EventKind::RunStarted { session_id: Some(s), .. } | EventKind::Session { session_id: s, .. } => {
            conn.execute("UPDATE threads SET session_id = ?2 WHERE id = ?1", params![id, s])?;
        }
        EventKind::Agent { event: AgentEvent::Usage(u), .. } => {
            conn.execute(
                "UPDATE threads SET input_tokens = input_tokens + ?2, output_tokens = output_tokens + ?3,
                    cost_usd = cost_usd + ?4 WHERE id = ?1",
                params![id, u.input_tokens as i64, u.output_tokens as i64, u.cost_usd],
            )?;
        }
        EventKind::StatusChanged { status } => {
            conn.execute("UPDATE threads SET status = ?2, settled = 0 WHERE id = ?1", params![id, status.as_str()])?;
        }
        EventKind::Checkpoint { n, commit } => {
            conn.execute(
                "UPDATE threads SET checkpoints = ?2, last_checkpoint = ?3 WHERE id = ?1",
                params![id, n, commit],
            )?;
        }
        EventKind::BudgetSet { usd } => {
            conn.execute("UPDATE threads SET budget_usd = ?2 WHERE id = ?1", params![id, usd])?;
        }
        EventKind::Settled => {
            conn.execute("UPDATE threads SET settled = 1 WHERE id = ?1", params![id])?;
        }
        EventKind::Heal { attempt, .. } => {
            conn.execute("UPDATE threads SET heal_attempts = ?2 WHERE id = ?1", params![id, attempt])?;
        }
        _ => {}
    }
    conn.execute("UPDATE threads SET last_seq = ?2, updated_at = ?3 WHERE id = ?1", params![id, e.seq, now])?;
    Ok(())
}

const COLUMNS: &str = "id, project_id, title, harness, status, worktree, branch, session_id, parent_thread_id,
    fork_seq, input_tokens, output_tokens, cost_usd, heal_attempts, last_seq, updated_at, permission, model, effort,
    base, budget_usd, settled, checkpoints, last_checkpoint, tool_profile, plan_root, plan_node";

pub(crate) fn list(conn: &Connection, ws: WorkspaceId) -> Result<Vec<ThreadSummary>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {COLUMNS} FROM threads WHERE workspace_id = ?1 ORDER BY updated_at DESC"))?;
    let rows = stmt.query_map([ws.to_string()], decode_row)?;
    rows.map(|r| r?).collect()
}

pub(crate) fn get(conn: &Connection, id: ThreadId) -> Result<Option<ThreadSummary>> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM threads WHERE id = ?1"), [id.to_string()], decode_row)
        .optional()?
        .transpose()
}

/// Reads columns eagerly; id parsing happens outside rusqlite's error type.
fn decode_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<ThreadSummary>> {
    let id: String = r.get(0)?;
    let project_id: String = r.get(1)?;
    let status: String = r.get(4)?;
    let parent_id: Option<String> = r.get(8)?;
    let fork_seq: Option<i64> = r.get(9)?;
    let (title, harness, worktree, branch, session_id) = (r.get(2)?, r.get(3)?, r.get(5)?, r.get(6)?, r.get(7)?);
    let (input, output, cost, heals, last_seq, updated_at): (i64, i64, f64, i64, i64, String) =
        (r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?, r.get(15)?);
    let (permission, model, effort): (String, Option<String>, Option<String>) = (r.get(16)?, r.get(17)?, r.get(18)?);
    let (base, budget_usd, settled, checkpoints, last_checkpoint): (
        Option<String>,
        Option<f64>,
        i64,
        i64,
        Option<String>,
    ) = (r.get(19)?, r.get(20)?, r.get(21)?, r.get(22)?, r.get(23)?);
    Ok((|| {
        let parent = match (parent_id, fork_seq) {
            (Some(p), Some(seq)) => Some((parse(&p)?, seq)),
            _ => None,
        };
        Ok(ThreadSummary {
            id: parse(&id)?,
            project_id: parse(&project_id)?,
            title,
            harness,
            status: serde_json::from_value(serde_json::Value::String(status))?,
            worktree,
            branch,
            session_id,
            parent,
            permission: permission.parse().map_err(crate::StoreError::Corrupt)?,
            model,
            effort,
            tool_profile: serde_json::from_value(serde_json::Value::String(r.get(24)?))?,
            plan_root: r.get::<_, Option<String>>(25)?.map(|id| parse(&id)).transpose()?,
            plan_node: r.get(26)?,
            base,
            budget_usd,
            settled: settled != 0,
            checkpoints: checkpoints as u32,
            last_checkpoint,
            input_tokens: input as u64,
            output_tokens: output as u64,
            cost_usd: cost,
            heal_attempts: heals as u32,
            last_seq,
            updated_at,
        })
    })())
}
