//! Append-only event log backed by SQLite (WAL). Every write goes through
//! [`Store::append`], which assigns the per-thread `seq` and updates the
//! read-side projections in the same transaction.

mod migrations;
mod projection;
mod tasks;

pub use projection::{Project, ThreadSummary};
pub use tasks::{NewTask, TaskPatch};

use arbiter_core::{Event, EventKind, ProjectId, ThreadId, WorkspaceId};
use rusqlite::{Connection, params};
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("corrupt row: {0}")]
    Corrupt(String),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    conn: Connection,
}

/// One event row exactly as stored, for backups and restores.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BackupEvent {
    pub id: i64,
    pub workspace_id: String,
    pub thread_id: String,
    pub seq: i64,
    pub ts: String,
    pub kind: String,
    pub payload: String,
}

/// Mutable tables kept in backups; `threads` is a projection and is rebuilt.
const BACKUP_TABLES: &[(&str, &[&str])] = &[
    ("projects", &["id", "workspace_id", "name", "path", "created_at"]),
    (
        "tasks",
        &[
            "id",
            "workspace_id",
            "project_id",
            "number",
            "title",
            "description",
            "status",
            "priority",
            "labels",
            "parent_id",
            "position",
            "created_at",
            "updated_at",
        ],
    ),
    ("task_threads", &["task_id", "thread_id"]),
];

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        migrations::run(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn create_project(&self, ws: WorkspaceId, name: &str, path: &str) -> Result<Project> {
        let p = Project { id: ProjectId::new(), workspace_id: ws, name: name.to_owned(), path: path.to_owned() };
        self.conn.execute(
            "INSERT INTO projects (id, workspace_id, name, path, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![p.id.to_string(), ws.to_string(), p.name, p.path, now_str()],
        )?;
        Ok(p)
    }

    pub fn projects(&self, ws: WorkspaceId) -> Result<Vec<Project>> {
        let mut stmt =
            self.conn.prepare("SELECT id, name, path FROM projects WHERE workspace_id = ?1 ORDER BY created_at")?;
        let rows = stmt.query_map([ws.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        rows.map(|r| {
            let (id, name, path) = r?;
            Ok(Project { id: parse(&id)?, workspace_id: ws, name, path })
        })
        .collect()
    }

    pub fn project(&self, id: ProjectId) -> Result<Project> {
        self.conn
            .query_row("SELECT workspace_id, name, path FROM projects WHERE id = ?1", [id.to_string()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound(format!("project {id}")),
                e => e.into(),
            })
            .and_then(|(ws, name, path)| Ok(Project { id, workspace_id: parse(&ws)?, name, path }))
    }

    /// Append one event to a thread's log, returning the stored envelope.
    pub fn append(&mut self, ws: WorkspaceId, thread: ThreadId, kind: EventKind) -> Result<Event> {
        let tx = self.conn.transaction()?;
        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE thread_id = ?1",
            [thread.to_string()],
            |r| r.get(0),
        )?;
        let ts = OffsetDateTime::now_utc();
        let payload = serde_json::to_string(&kind)?;
        tx.execute(
            "INSERT INTO events (workspace_id, thread_id, seq, ts, kind, payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![ws.to_string(), thread.to_string(), seq, fmt_ts(ts), kind.tag(), payload],
        )?;
        let event = Event { id: tx.last_insert_rowid(), workspace_id: ws, thread_id: thread, seq, ts, kind };
        projection::apply(&tx, &event)?;
        tx.commit()?;
        Ok(event)
    }

    /// Events for a thread with `seq > after`, in order.
    pub fn events(&self, thread: ThreadId, after: i64) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace_id, thread_id, seq, ts, payload FROM events
             WHERE thread_id = ?1 AND seq > ?2 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![thread.to_string(), after], row_to_raw)?;
        rows.map(|r| decode(r?)).collect()
    }

    /// Project knowledge replay, ordered by global event id; fork copies are excluded at write time.
    pub fn knowledge_events(&self, project: ProjectId) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare("SELECT e.id,e.workspace_id,e.thread_id,e.seq,e.ts,e.payload FROM events e JOIN threads t ON t.id=e.thread_id WHERE t.project_id=?1 AND e.kind IN ('vault_saved','vault_proposed','vault_resolved','outcome_recorded','outcome_rework','routing_preference') ORDER BY e.id")?;
        let rows = stmt.query_map([project.to_string()], row_to_raw)?;
        rows.map(|r| decode(r?)).collect()
    }

    pub fn threads(&self, ws: WorkspaceId) -> Result<Vec<ThreadSummary>> {
        projection::list(&self.conn, ws)
    }

    pub fn thread(&self, id: ThreadId) -> Result<Option<ThreadSummary>> {
        projection::get(&self.conn, id)
    }

    /// Fork `source` at `at_seq`: copies events 1..=at_seq into a new thread
    /// whose `ThreadCreated` records the parent. The source log is never mutated.
    pub fn fork(&mut self, source: ThreadId, at_seq: i64) -> Result<ThreadId> {
        let events = self.events(source, 0)?;
        let ws =
            events.first().map(|e| e.workspace_id).ok_or_else(|| StoreError::NotFound(format!("thread {source}")))?;
        let new = ThreadId::new();
        for e in events.into_iter().take_while(|e| e.seq <= at_seq) {
            if matches!(
                e.kind,
                EventKind::VaultSaved { .. }
                    | EventKind::VaultProposed { .. }
                    | EventKind::VaultResolved { .. }
                    | EventKind::OutcomeRecorded { .. }
                    | EventKind::OutcomeRework { .. }
                    | EventKind::RoutingPreference { .. }
                    | EventKind::MemoryUsed { .. }
            ) {
                continue;
            }
            let kind = match e.kind {
                // Copied Session events keep the parent's harness session id; the
                // daemon sees `session_id == parent's` and forks the harness session.
                EventKind::ThreadCreated { project_id, title, harness, permission, model, effort, .. } => {
                    EventKind::ThreadCreated {
                        project_id,
                        title: format!("{title} (fork)"),
                        harness,
                        worktree: None,
                        branch: None,
                        parent: Some((source, at_seq)),
                        permission,
                        model,
                        effort,
                        base: None,
                    }
                }
                other => other,
            };
            self.append(ws, new, kind)?;
        }
        Ok(new)
    }

    /// Raw event rows with `id > after`, oldest first, for backups:
    /// `(id, workspace, thread, seq, ts, kind, payload)`. At most `limit`.
    pub fn backup_events(&self, after: i64, limit: usize) -> Result<Vec<BackupEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace_id, thread_id, seq, ts, kind, payload FROM events WHERE id > ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after, limit as i64], |r| {
            Ok(BackupEvent {
                id: r.get(0)?,
                workspace_id: r.get(1)?,
                thread_id: r.get(2)?,
                seq: r.get(3)?,
                ts: r.get(4)?,
                kind: r.get(5)?,
                payload: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The mutable tables (projects, tasks, task links) as JSON rows.
    pub fn backup_tables(&self) -> Result<serde_json::Value> {
        let mut out = serde_json::Map::new();
        for (table, columns) in BACKUP_TABLES {
            let mut stmt = self.conn.prepare(&format!("SELECT {} FROM {table}", columns.join(", ")))?;
            let rows = stmt.query_map([], |r| {
                let mut row = serde_json::Map::new();
                for (i, c) in columns.iter().enumerate() {
                    let v: rusqlite::types::Value = r.get(i)?;
                    row.insert(
                        (*c).into(),
                        match v {
                            rusqlite::types::Value::Null => serde_json::Value::Null,
                            rusqlite::types::Value::Integer(n) => n.into(),
                            rusqlite::types::Value::Real(f) => serde_json::json!(f),
                            rusqlite::types::Value::Text(s) => s.into(),
                            rusqlite::types::Value::Blob(_) => serde_json::Value::Null,
                        },
                    );
                }
                Ok(serde_json::Value::Object(row))
            })?;
            out.insert((*table).into(), serde_json::Value::Array(rows.collect::<rusqlite::Result<_>>()?));
        }
        Ok(serde_json::Value::Object(out))
    }

    /// Restore a backup into an empty store: tables, then events with their
    /// original ids, sequence numbers and times, then rebuilt projections.
    pub fn restore_backup(&mut self, tables: &serde_json::Value, events: &[BackupEvent]) -> Result<()> {
        let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
        if count > 0 {
            return Err(StoreError::Corrupt("restore needs an empty database".into()));
        }
        let tx = self.conn.transaction()?;
        for (table, columns) in BACKUP_TABLES {
            let sql = format!(
                "INSERT OR REPLACE INTO {table} ({}) VALUES ({})",
                columns.join(", "),
                (1..=columns.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ")
            );
            for row in tables[*table].as_array().into_iter().flatten() {
                let values: Vec<rusqlite::types::Value> = columns
                    .iter()
                    .map(|c| match &row[*c] {
                        serde_json::Value::Null => rusqlite::types::Value::Null,
                        serde_json::Value::Number(n) if n.is_i64() => {
                            rusqlite::types::Value::Integer(n.as_i64().unwrap())
                        }
                        serde_json::Value::Number(n) => rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0)),
                        serde_json::Value::String(s) => rusqlite::types::Value::Text(s.clone()),
                        other => rusqlite::types::Value::Text(other.to_string()),
                    })
                    .collect();
                tx.execute(&sql, rusqlite::params_from_iter(values))?;
            }
        }
        for e in events {
            // Refuse rows that would not decode later.
            serde_json::from_str::<EventKind>(&e.payload)?;
            tx.execute(
                "INSERT INTO events (id, workspace_id, thread_id, seq, ts, kind, payload) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![e.id, e.workspace_id, e.thread_id, e.seq, e.ts, e.kind, e.payload],
            )?;
        }
        tx.commit()?;
        self.rebuild_projections()
    }

    /// Drop and rebuild all projections from the log (crash recovery, schema changes).
    pub fn rebuild_projections(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM threads", [])?;
        {
            let mut stmt =
                tx.prepare("SELECT id, workspace_id, thread_id, seq, ts, payload FROM events ORDER BY id")?;
            let rows = stmt.query_map([], row_to_raw)?;
            for r in rows {
                projection::apply(&tx, &decode(r?)?)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod intake_replay_tests {
    use super::*;
    #[test]
    fn old_events_default_to_research_and_profile_changes_replay() {
        let mut store = Store::open_in_memory().unwrap();
        let p = store.create_project(WorkspaceId::LOCAL, "fixture", "/fixture").unwrap();
        let t = ThreadId::new();
        let old = serde_json::json!({"type":"thread_created","project_id":p.id,"title":"old","harness":"claude","worktree":null,"branch":null,"parent":null});
        store.append(WorkspaceId::LOCAL, t, serde_json::from_value(old).unwrap()).unwrap();
        assert_eq!(store.thread(t).unwrap().unwrap().tool_profile, arbiter_core::ToolProfile::Research);
        store
            .append(
                WorkspaceId::LOCAL,
                t,
                EventKind::ToolProfileChanged { profile: arbiter_core::ToolProfile::Implementation },
            )
            .unwrap();
        let before = store.thread(t).unwrap().unwrap().tool_profile;
        store.rebuild_projections().unwrap();
        assert_eq!(store.thread(t).unwrap().unwrap().tool_profile, before);
    }
}

type RawRow = (i64, String, String, i64, String, String);

fn row_to_raw(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
}

fn decode((id, ws, thread, seq, ts, payload): RawRow) -> Result<Event> {
    Ok(Event {
        id,
        workspace_id: parse(&ws)?,
        thread_id: parse(&thread)?,
        seq,
        ts: OffsetDateTime::parse(&ts, &Rfc3339).map_err(|e| StoreError::Corrupt(e.to_string()))?,
        kind: serde_json::from_str(&payload)?,
    })
}

pub(crate) fn parse<T: std::str::FromStr>(s: &str) -> Result<T> {
    s.parse().map_err(|_| StoreError::Corrupt(format!("bad id {s}")))
}

fn fmt_ts(ts: OffsetDateTime) -> String {
    ts.format(&Rfc3339).expect("rfc3339 formatting is infallible for UTC")
}

pub(crate) fn now_str() -> String {
    fmt_ts(OffsetDateTime::now_utc())
}

#[cfg(test)]
mod tests;
