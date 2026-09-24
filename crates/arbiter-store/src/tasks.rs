//! Task CRUD. Tasks are mutable planning records (see `arbiter_core::task`);
//! the append-only guarantee applies to thread events, not to tasks.

use crate::{Result, Store, StoreError, now_str, parse};
use arbiter_core::task::key_prefix;
use arbiter_core::{Priority, ProjectId, Task, TaskId, TaskStatus, ThreadId, WorkspaceId};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NewTask {
    pub project_id: ProjectId,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub parent_id: Option<TaskId>,
}

/// Partial update; absent fields are left unchanged.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub priority: Option<Priority>,
    pub labels: Option<Vec<String>>,
    /// `Some(None)` clears the parent.
    #[serde(default, with = "double_option")]
    pub parent_id: Option<Option<TaskId>>,
    pub position: Option<f64>,
}

mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

const COLS: &str = "t.id, t.project_id, t.number, t.title, t.description, t.status, t.priority, t.labels,
    t.parent_id, t.position, t.created_at, t.updated_at, p.name";

impl Store {
    pub fn create_task(&mut self, ws: WorkspaceId, n: NewTask) -> Result<Task> {
        let tx = self.conn.transaction()?;
        let number: i64 = tx.query_row(
            "SELECT COALESCE(MAX(number), 0) + 1 FROM tasks WHERE project_id = ?1",
            [n.project_id.to_string()],
            |r| r.get(0),
        )?;
        // New tasks go to the top of their column.
        let position: f64 = tx.query_row(
            "SELECT COALESCE(MIN(position), 1000.0) - 1.0 FROM tasks WHERE project_id = ?1 AND status = ?2",
            params![n.project_id.to_string(), n.status.as_str()],
            |r| r.get(0),
        )?;
        let id = TaskId::new();
        let now = now_str();
        tx.execute(
            "INSERT INTO tasks (id, workspace_id, project_id, number, title, description, status, priority, labels,
                parent_id, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![
                id.to_string(),
                ws.to_string(),
                n.project_id.to_string(),
                number,
                n.title.trim(),
                n.description,
                n.status.as_str(),
                n.priority.as_str(),
                serde_json::to_string(&n.labels)?,
                n.parent_id.map(|p| p.to_string()),
                position,
                now,
            ],
        )?;
        tx.commit()?;
        self.task(id)
    }

    pub fn task(&self, id: TaskId) -> Result<Task> {
        let sql = format!("SELECT {COLS} FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id = ?1");
        let task = self
            .conn
            .query_row(&sql, [id.to_string()], decode)
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("task {id}")))??;
        self.with_threads(vec![task]).map(|mut v| v.remove(0))
    }

    pub fn tasks(&self, ws: WorkspaceId, project: Option<ProjectId>) -> Result<Vec<Task>> {
        let sql = format!(
            "SELECT {COLS} FROM tasks t JOIN projects p ON p.id = t.project_id
             WHERE t.workspace_id = ?1 AND (?2 IS NULL OR t.project_id = ?2)
             ORDER BY t.position, t.number DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![ws.to_string(), project.map(|p| p.to_string())], decode)?;
        let tasks = rows.map(|r| r?).collect::<Result<Vec<_>>>()?;
        self.with_threads(tasks)
    }

    pub fn update_task(&mut self, id: TaskId, p: TaskPatch) -> Result<Task> {
        let cur = self.task(id)?;
        let labels = p.labels.unwrap_or(cur.labels);
        let parent = p.parent_id.unwrap_or(cur.parent_id);
        if parent == Some(id) {
            return Err(StoreError::Corrupt("a task cannot be its own parent".into()));
        }
        self.conn.execute(
            "UPDATE tasks SET title = ?2, description = ?3, status = ?4, priority = ?5, labels = ?6,
                parent_id = ?7, position = ?8, updated_at = ?9 WHERE id = ?1",
            params![
                id.to_string(),
                p.title.map(|t| t.trim().to_owned()).unwrap_or(cur.title),
                p.description.unwrap_or(cur.description),
                p.status.unwrap_or(cur.status).as_str(),
                p.priority.unwrap_or(cur.priority).as_str(),
                serde_json::to_string(&labels)?,
                parent.map(|p| p.to_string()),
                p.position.unwrap_or(cur.position),
                now_str(),
            ],
        )?;
        self.task(id)
    }

    pub fn link_task_thread(&self, task: TaskId, thread: ThreadId) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO task_threads (task_id, thread_id) VALUES (?1, ?2)",
            params![task.to_string(), thread.to_string()],
        )?;
        Ok(())
    }

    /// Tasks a thread works on (usually zero or one).
    pub fn tasks_for_thread(&self, thread: ThreadId) -> Result<Vec<TaskId>> {
        let mut stmt = self.conn.prepare("SELECT task_id FROM task_threads WHERE thread_id = ?1")?;
        let rows = stmt.query_map([thread.to_string()], |r| r.get::<_, String>(0))?;
        rows.map(|r| parse(&r?)).collect()
    }

    fn with_threads(&self, mut tasks: Vec<Task>) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare("SELECT thread_id FROM task_threads WHERE task_id = ?1 ORDER BY thread_id")?;
        for t in &mut tasks {
            let rows = stmt.query_map([t.id.to_string()], |r| r.get::<_, String>(0))?;
            t.thread_ids = rows.map(|r| parse(&r?)).collect::<Result<_>>()?;
        }
        Ok(tasks)
    }
}

fn decode(r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Task>> {
    let (id, project_id, number, title, description): (String, String, i64, String, String) =
        (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?);
    let (status, priority, labels, parent_id, position): (String, String, String, Option<String>, f64) =
        (r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?);
    let (created_at, updated_at, project_name): (String, String, String) = (r.get(10)?, r.get(11)?, r.get(12)?);
    Ok((|| {
        Ok(Task {
            id: parse(&id)?,
            project_id: parse(&project_id)?,
            key: format!("{}-{number}", key_prefix(&project_name)),
            number,
            title,
            description,
            status: status.parse().map_err(StoreError::Corrupt)?,
            priority: priority.parse().map_err(StoreError::Corrupt)?,
            labels: serde_json::from_str(&labels)?,
            parent_id: parent_id.map(|p| parse(&p)).transpose()?,
            position,
            thread_ids: Vec::new(),
            created_at,
            updated_at,
        })
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_update_link() {
        let mut s = Store::open_in_memory().unwrap();
        let p = s.create_project(WorkspaceId::LOCAL, "orchestrator", "/x").unwrap();
        let new = |title: &str| NewTask { project_id: p.id, title: title.into(), ..Default::default() };
        let a = s.create_task(WorkspaceId::LOCAL, new("first")).unwrap();
        let b = s.create_task(WorkspaceId::LOCAL, new("second")).unwrap();
        assert_eq!((a.key.as_str(), b.key.as_str()), ("ORC-1", "ORC-2"));
        assert!(b.position < a.position, "newest on top");

        let b = s
            .update_task(
                b.id,
                TaskPatch {
                    status: Some(TaskStatus::InProgress),
                    labels: Some(vec!["ui".into()]),
                    parent_id: Some(Some(a.id)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            (b.status, b.labels.as_slice(), b.parent_id),
            (TaskStatus::InProgress, &["ui".to_owned()][..], Some(a.id))
        );
        assert_eq!(b.title, "second", "untouched fields keep their value");
        assert!(s.update_task(a.id, TaskPatch { parent_id: Some(Some(a.id)), ..Default::default() }).is_err());

        let th = ThreadId::new();
        s.link_task_thread(b.id, th).unwrap();
        s.link_task_thread(b.id, th).unwrap();
        assert_eq!(s.task(b.id).unwrap().thread_ids, vec![th]);
        assert_eq!(s.tasks_for_thread(th).unwrap(), vec![b.id]);
        assert_eq!(s.tasks(WorkspaceId::LOCAL, Some(p.id)).unwrap().len(), 2);
    }
}
