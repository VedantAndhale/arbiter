use super::*;
use arbiter_core::{AgentEvent, RunId, ThreadStatus, Usage};

fn created(project_id: ProjectId, harness: &str) -> EventKind {
    EventKind::ThreadCreated {
        project_id,
        title: "fix tests".into(),
        harness: harness.into(),
        worktree: None,
        branch: None,
        parent: None,
        permission: Default::default(),
        model: None,
        effort: None,
        base: None,
    }
}

fn setup() -> (Store, ThreadId) {
    let mut s = Store::open_in_memory().unwrap();
    let p = s.create_project(WorkspaceId::LOCAL, "demo", "/tmp/demo").unwrap();
    let t = ThreadId::new();
    s.append(WorkspaceId::LOCAL, t, created(p.id, "claude")).unwrap();
    (s, t)
}

fn usage(n: u64) -> EventKind {
    EventKind::Agent {
        run_id: RunId::new(),
        event: AgentEvent::Usage(Usage { input_tokens: n, output_tokens: n / 2, cache_read_tokens: 0, cost_usd: 0.01 }),
    }
}

#[test]
fn append_assigns_sequential_seq_and_projects() {
    let (mut s, t) = setup();
    let e2 = s.append(WorkspaceId::LOCAL, t, usage(100)).unwrap();
    let e3 = s.append(WorkspaceId::LOCAL, t, EventKind::StatusChanged { status: ThreadStatus::Running }).unwrap();
    assert_eq!((e2.seq, e3.seq), (2, 3));

    let summary = s.thread(t).unwrap().unwrap();
    assert_eq!(summary.status, ThreadStatus::Running);
    assert_eq!(summary.input_tokens, 100);
    assert_eq!(summary.last_seq, 3);
    assert_eq!(s.events(t, 1).unwrap().len(), 2);
}

#[test]
fn rebuild_matches_incremental_projection() {
    let (mut s, t) = setup();
    for n in [10, 20, 30] {
        s.append(WorkspaceId::LOCAL, t, usage(n)).unwrap();
    }
    let before = s.thread(t).unwrap().unwrap();
    s.rebuild_projections().unwrap();
    let after = s.thread(t).unwrap().unwrap();
    assert_eq!(after.input_tokens, 60);
    assert_eq!(before.input_tokens, after.input_tokens);
    assert_eq!(before.last_seq, after.last_seq);
}

#[test]
fn plan_child_links_survive_projection_rebuild() {
    let (mut s, t) = setup();
    let root = ThreadId::new();
    s.append(WorkspaceId::LOCAL, t, EventKind::PlanChild { root, node: Some("api".into()), repair: false }).unwrap();
    s.rebuild_projections().unwrap();
    let summary = s.thread(t).unwrap().unwrap();
    assert_eq!(summary.plan_root, Some(root));
    assert_eq!(summary.plan_node.as_deref(), Some("api"));
}

#[test]
fn fork_copies_prefix_and_records_parent() {
    let (mut s, t) = setup();
    s.append(WorkspaceId::LOCAL, t, usage(10)).unwrap();
    s.append(WorkspaceId::LOCAL, t, usage(20)).unwrap();

    let f = s.fork(t, 2).unwrap();
    let forked = s.thread(f).unwrap().unwrap();
    assert_eq!(forked.parent, Some((t, 2)));
    assert_eq!(forked.input_tokens, 10);
    assert_eq!(s.events(f, 0).unwrap().len(), 2);
    assert_eq!(s.events(t, 0).unwrap().len(), 3, "source log untouched");
}

#[test]
fn reopening_file_db_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("arbiter.db");
    let t = {
        let mut s = Store::open(&path).unwrap();
        let p = s.create_project(WorkspaceId::LOCAL, "demo", "/x").unwrap();
        let t = ThreadId::new();
        s.append(WorkspaceId::LOCAL, t, created(p.id, "codex")).unwrap();
        t
    };
    let s = Store::open(&path).unwrap();
    assert_eq!(s.threads(WorkspaceId::LOCAL).unwrap()[0].id, t);
    assert_eq!(s.projects(WorkspaceId::LOCAL).unwrap().len(), 1);
}
