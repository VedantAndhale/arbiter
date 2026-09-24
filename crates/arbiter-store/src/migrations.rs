use rusqlite::Connection;

/// Ordered, append-only list of migrations; index + 1 == schema version.
const MIGRATIONS: &[&str] = &[
    "
CREATE TABLE projects (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    name         TEXT NOT NULL,
    path         TEXT NOT NULL,
    created_at   TEXT NOT NULL
);

CREATE TABLE events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL,
    thread_id    TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    ts           TEXT NOT NULL,
    kind         TEXT NOT NULL,
    payload      TEXT NOT NULL,
    UNIQUE (thread_id, seq)
);
CREATE INDEX events_ws ON events (workspace_id, id);

CREATE TABLE threads (
    id               TEXT PRIMARY KEY,
    workspace_id     TEXT NOT NULL,
    project_id       TEXT NOT NULL,
    title            TEXT NOT NULL,
    harness          TEXT NOT NULL,
    status           TEXT NOT NULL,
    worktree         TEXT,
    branch           TEXT,
    session_id       TEXT,
    parent_thread_id TEXT,
    fork_seq         INTEGER,
    input_tokens     INTEGER NOT NULL DEFAULT 0,
    output_tokens    INTEGER NOT NULL DEFAULT 0,
    cost_usd         REAL NOT NULL DEFAULT 0,
    heal_attempts    INTEGER NOT NULL DEFAULT 0,
    last_seq         INTEGER NOT NULL,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
CREATE INDEX threads_ws ON threads (workspace_id, updated_at);
",
    "
ALTER TABLE threads ADD COLUMN permission TEXT NOT NULL DEFAULT 'safe';
ALTER TABLE threads ADD COLUMN model TEXT;
",
    "
ALTER TABLE threads ADD COLUMN effort TEXT;

CREATE TABLE tasks (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    project_id   TEXT NOT NULL,
    number       INTEGER NOT NULL,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL,
    priority     TEXT NOT NULL DEFAULT 'none',
    labels       TEXT NOT NULL DEFAULT '[]',
    parent_id    TEXT,
    position     REAL NOT NULL,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    UNIQUE (project_id, number)
);
CREATE INDEX tasks_project ON tasks (project_id, status, position);

CREATE TABLE task_threads (
    task_id   TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    PRIMARY KEY (task_id, thread_id)
);
CREATE INDEX task_threads_thread ON task_threads (thread_id);
",
    "
ALTER TABLE threads ADD COLUMN base TEXT;
ALTER TABLE threads ADD COLUMN budget_usd REAL;
ALTER TABLE threads ADD COLUMN settled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN checkpoints INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN last_checkpoint TEXT;
",
    "ALTER TABLE threads ADD COLUMN tool_profile TEXT NOT NULL DEFAULT 'research';",
    "ALTER TABLE threads ADD COLUMN plan_root TEXT; ALTER TABLE threads ADD COLUMN plan_node TEXT;",
];

pub fn run(conn: &mut Connection) -> rusqlite::Result<()> {
    let current = conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? as usize;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}
