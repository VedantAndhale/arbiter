use crate::service::{TaskStart, ThreadSpec, WS};
use crate::{AppState, WsMsg};
use arbiter_core::NoWindow;
use arbiter_core::{EventKind, PermissionMode, ProjectId, TaskId, ThreadId};
use arbiter_store::{NewTask, StoreError, TaskPatch};
use arbiter_supervisor::Worktree;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub fn router(state: AppState) -> Router {
    let authed = Router::new()
        .route("/v1/project-setup/inspect", post(project_inspect))
        .route("/v1/project-setup/overview", post(project_overview))
        .route("/v1/project-setup/baseline", post(project_baseline))
        .route("/v1/project-setup/docs", post(project_docs))
        .route("/v1/project-setup/{id}", get(project_adoption))
        .route("/v1/project-setup/{id}/apply", post(project_apply))
        .route("/v1/project-setup/{id}/rollback", post(project_rollback))
        .route("/v1/project-setup/{id}/initialize", post(project_initialize))
        .route("/v1/setup", get(setup_get).post(setup_save))
        .route("/v1/documentation/setup", post(documentation_setup))
        .route("/v1/local/capability", get(local_capability).post(test_local_capability))
        .route("/v1/documentation/query", post(documentation_query))
        .route("/v1/documentation/operations/{id}/cancel", post(documentation_cancel))
        .route("/v1/documentation/credential", get(credential_status).post(credential_save).delete(credential_remove))
        .route("/v1/threads/{id}/second-opinion", post(second_opinion))
        .route("/v1/threads/{id}/review-fixes", post(review_fixes))
        .route("/v1/threads/{id}/review", post(review_comments))
        .route("/v1/threads/{id}/landing", get(landing_status))
        .route("/v1/threads/{id}/commit", post(commit_reviewed))
        .route("/v1/threads/{id}/publish", get(publish_draft).post(publish_reviewed))
        .route("/v1/threads/{id}/cleanup", post(clean_up))
        .route("/v1/cleanup", post(sweep_published))
        .route("/v1/shutdown", post(shutdown))
        .route("/v1/memory", get(memory_status))
        .route("/v1/memory/sync", post(memory_sync))
        .route("/v1/memory/remote", post(memory_remote))
        .route("/v1/memory/github", post(memory_github))
        .route("/v1/setup/refresh", post(setup_refresh))
        .route("/v1/threads/{id}/vault", get(vault_state).post(vault_save))
        .route("/v1/threads/{id}/vault/propose", post(vault_propose))
        .route("/v1/threads/{id}/vault/resolve", post(vault_resolve))
        .route("/v1/threads/{id}/vault/tool", post(vault_tool))
        .route("/v1/threads/{id}/vault/rebuild", post(vault_rebuild))
        .route("/v1/threads/{id}/learning", get(learning_state).post(learning_preference))
        .route("/v1/threads/{id}/learning/rework", post(learning_rework))
        .route("/v1/threads/{id}/plan", get(get_plan).patch(edit_plan))
        .route("/v1/threads/{id}/plan/draft", post(refine_plan))
        .route("/v1/threads/{id}/plan/approve", post(approve_plan))
        .route("/v1/threads/{id}/plan/control", post(control_plan))
        .route("/v1/threads/{id}/plan/scope", post(approve_plan_scope))
        .route("/v1/threads/{id}/plan/budget", post(approve_plan_budget))
        .route("/v1/threads/{id}/plan/diff", get(plan_diff))
        .route("/v1/threads/{id}/handoff", post(report_handoff))
        .route("/v1/threads/{id}/scope-request", post(request_scope))
        .route("/v1/models", get(local_models))
        .route("/v1/models/{id}/install", post(install_model))
        .route("/v1/models/{id}/cancel", post(cancel_model))
        .route("/v1/models/{id}/benchmark", post(benchmark_model))
        .route("/v1/models/{id}/select", post(select_model))
        .route("/v1/projects/{id}/files", get(project_files))
        .route("/v1/threads/{id}/answers", post(answer_intake))
        .route("/v1/threads/{id}/intake", axum::routing::patch(correct_intake))
        .route("/v1/threads/{id}/approvals", post(answer_approval))
        .route("/v1/harnesses", get(harnesses))
        .route("/v1/projects", get(list_projects).post(create_project))
        .route("/v1/threads", get(list_threads).post(create_thread))
        .route("/v1/threads/{id}", get(get_thread).patch(patch_thread))
        .route("/v1/threads/{id}/events", get(thread_events))
        .route("/v1/threads/{id}/messages", post(post_message))
        .route("/v1/threads/{id}/interrupt", post(interrupt_thread))
        .route("/v1/threads/{id}/stop", post(stop_thread))
        .route("/v1/threads/{id}/escalate", post(escalate_thread))
        .route("/v1/threads/{id}/side", post(side_question))
        .route("/v1/research", post(research))
        .route("/v1/research/held", get(held_shares))
        .route("/v1/research/held/{id}", post(decide_share))
        .route("/v1/web/sites", get(web_sites).post(add_web_site))
        .route("/v1/web/sites/{host}", delete(remove_web_site))
        .route("/v1/threads/{id}/research", post(thread_research))
        .route("/v1/threads/{id}/terminal", get(terminal_socket))
        .route("/v1/threads/{id}/terminal/close", post(close_terminal))
        .route("/v1/threads/{id}/files", get(working_files))
        .route("/v1/threads/{id}/file", get(read_file).put(write_file))
        .route("/v1/folders", get(list_folders).post(create_folder))
        .route("/v1/prerequisites", get(prerequisites))
        .route("/v1/prerequisites/{id}/install", post(install_prerequisite))
        .route("/v1/prerequisites/{id}/sign-in", post(sign_in_tool))
        .route("/v1/git-identity", post(set_git_identity))
        .route("/v1/projects/add", post(quick_add_project))
        .route("/v1/comparisons", post(start_comparison))
        .route("/v1/comparisons/{id}", get(get_comparison))
        .route("/v1/comparisons/{id}/keep", post(keep_candidate))
        .route("/v1/threads/{id}/fork", post(fork_thread))
        .route("/v1/threads/{id}/diff", get(thread_diff))
        .route("/v1/threads/{id}/checkpoints", get(thread_checkpoints))
        .route("/v1/threads/{id}/revert", post(revert_thread))
        .route("/v1/threads/{id}/settle", post(settle_thread))
        .route("/v1/threads/{id}/preview", get(preview_status))
        .route("/v1/threads/{id}/preview/start", post(preview_start))
        .route("/v1/threads/{id}/preview/command", get(preview_command).put(set_preview_command))
        .route("/v1/threads/{id}/preview/stop", post(preview_stop))
        .route("/v1/threads/{id}/preview/frame", get(preview_frame))
        .route("/v1/threads/{id}/preview/viewport", post(preview_resize))
        .route("/v1/threads/{id}/preview/captures", get(preview_captures).post(preview_capture))
        .route("/v1/threads/{id}/browser", post(browser_tool))
        .route(
            "/v1/attachments",
            post(upload_attachment).layer(axum::extract::DefaultBodyLimit::max(crate::attach::MAX_UPLOAD)),
        )
        .route("/v1/attachments/{id}", get(get_attachment))
        .route("/v1/tasks", get(list_tasks).post(create_task))
        .route("/v1/tasks/{id}", get(get_task).patch(patch_task))
        .route("/v1/tasks/{id}/start", post(start_task))
        .route("/v1/ws", get(ws))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        // The version lets a newer app replace an older background service;
        // the pid lets a client reject a daemon.json left by a previous run.
        .route(
            "/v1/health",
            get(|| async {
                Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION"), "pid": std::process::id() }))
            }),
        )
        .merge(authed)
        .layer(cors())
        .with_state(state)
}

async fn setup_get(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.setup_state()?))
}
#[derive(Deserialize)]
struct DocumentationSetup {
    #[serde(default)]
    test: bool,
    operation_id: Option<String>,
}
async fn local_capability(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.capability_report()))
}

#[derive(Deserialize)]
struct CapabilityTest {
    model: String,
}

async fn test_local_capability(State(s): State<AppState>, Json(b): Json<CapabilityTest>) -> ApiResult {
    Ok(Json(s.run_capability_probe(b.model.trim()).await?))
}

async fn documentation_setup(State(s): State<AppState>, Json(b): Json<DocumentationSetup>) -> ApiResult {
    let id = b.operation_id.unwrap_or_else(|| arbiter_core::RunId::new().to_string());
    Ok(Json(s.documentation_operation(&id, s.documentation_setup(b.test)).await?))
}
#[derive(Deserialize)]
struct DocumentationQuery {
    library: String,
    topic: String,
    operation_id: Option<String>,
}
#[derive(Deserialize)]
struct CredentialInput {
    key: String,
    #[serde(default)]
    persist: bool,
}
async fn credential_status(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.credential_status()?))
}
async fn credential_save(State(s): State<AppState>, Json(b): Json<CredentialInput>) -> ApiResult {
    Ok(Json(s.set_context7_key(&b.key, b.persist)?))
}
async fn credential_remove(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.remove_context7_key()?))
}
async fn documentation_query(State(s): State<AppState>, Json(b): Json<DocumentationQuery>) -> ApiResult {
    let id = b.operation_id.unwrap_or_else(|| arbiter_core::RunId::new().to_string());
    Ok(Json(s.documentation_operation(&id, s.retrieve_documentation(&b.library, &b.topic)).await?))
}
async fn documentation_cancel(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.cancel_documentation_operation(&id)?;
    Ok(Json(json!({"cancelled":true})))
}
#[derive(Deserialize)]
struct ReviewStage {
    stage: String,
}
#[derive(Deserialize)]
struct ReviewFixes {
    revision: u32,
    comments: Vec<crate::landing::ReviewComment>,
    #[serde(default)]
    project: Option<String>,
}
#[derive(Deserialize)]
struct ReviewBatch {
    revision: Option<u32>,
    comments: Vec<crate::landing::ReviewComment>,
    #[serde(default)]
    project: Option<String>,
}
async fn review_comments(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<ReviewBatch>) -> ApiResult {
    Ok(Json(s.review(parse_id(&id)?, b.revision, b.comments, b.project).await?))
}
async fn review_fixes(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<ReviewFixes>) -> ApiResult {
    let id = parse_id(&id)?;
    s.request_review_fixes(id, b.revision, b.comments, b.project).await?;
    Ok(Json(json!(s.plan_state(id)?)))
}
/// `?project=<id>` picks one of a multi-project plan's other projects.
#[derive(Deserialize, Default)]
struct ForProject {
    #[serde(default)]
    project: Option<String>,
}
async fn landing_status(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<ForProject>) -> ApiResult {
    Ok(Json(s.landing_status(parse_id(&id)?, q.project.as_deref()).await?))
}
#[derive(Deserialize)]
struct CommitReviewed {
    fingerprint: String,
    message: String,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    project: Option<String>,
}
#[derive(Deserialize)]
struct PublishReviewed {
    fingerprint: String,
    #[serde(default = "pr_mode")]
    mode: String,
    base: String,
    branch: String,
    message: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    approved: bool,
    #[serde(default)]
    project: Option<String>,
}
fn pr_mode() -> String {
    "pr".into()
}
async fn publish_draft(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<ForProject>) -> ApiResult {
    Ok(Json(s.publish_draft(parse_id(&id)?, q.project.as_deref()).await?))
}
async fn clean_up(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id = parse_id(&id)?;
    Ok(Json(json!({"copies": s.clean_up_task(id).await?})))
}
#[derive(Deserialize, Default)]
struct Shutdown {
    #[serde(default)]
    force: bool,
}
/// Stop the background service, for example so an updated app can start
/// its own. Refused while an agent is working unless forced. Memory is
/// committed locally first.
async fn shutdown(State(s): State<AppState>, body: Option<Json<Shutdown>>) -> ApiResult {
    let force = body.is_some_and(|b| b.force);
    if !force && s.anything_working() {
        return Err(
            anyhow::Error::from(crate::runs::Busy("An agent is working. Try again when it finishes.".into())).into()
        );
    }
    let _ = s.sync_memory(false, false).await;
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        std::process::exit(0);
    });
    Ok(Json(json!({"stopping": true})))
}
async fn memory_status(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.memory_status().await?))
}
#[derive(Deserialize)]
struct MemorySync {
    #[serde(default)]
    push: bool,
    #[serde(default)]
    closing: bool,
}
async fn memory_sync(State(s): State<AppState>, Json(b): Json<MemorySync>) -> ApiResult {
    Ok(Json(s.sync_memory(b.push, b.closing).await?))
}
#[derive(Deserialize)]
struct MemoryRemote {
    url: String,
}
async fn memory_remote(State(s): State<AppState>, Json(b): Json<MemoryRemote>) -> ApiResult {
    Ok(Json(s.set_memory_remote(&b.url).await?))
}
#[derive(Deserialize)]
struct MemoryGithub {
    #[serde(default = "memory_repo_name")]
    name: String,
    approved: bool,
}
fn memory_repo_name() -> String {
    "arbiter-memory".into()
}
async fn memory_github(State(s): State<AppState>, Json(b): Json<MemoryGithub>) -> ApiResult {
    if !b.approved {
        return Err(bad("creating a GitHub repository needs your approval"));
    }
    Ok(Json(s.create_memory_repo(&b.name).await?))
}
async fn sweep_published(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!({"cleaned": s.sweep_published().await?})))
}
async fn publish_reviewed(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<PublishReviewed>,
) -> ApiResult {
    if !b.approved {
        return Err(bad("explicit publish approval required"));
    }
    Ok(Json(
        s.publish_squashed(
            parse_id(&id)?,
            b.project.as_deref(),
            &b.fingerprint,
            &b.mode,
            &b.base,
            &b.branch,
            &b.message,
            &b.title,
            &b.body,
        )
        .await?,
    ))
}
async fn commit_reviewed(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<CommitReviewed>,
) -> ApiResult {
    Ok(Json(s.commit_reviewed(parse_id(&id)?, &b.fingerprint, &b.message, &b.include, b.project.as_deref()).await?))
}
async fn second_opinion(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<ReviewStage>) -> ApiResult {
    Ok(Json(s.second_opinion(parse_id(&id)?, &b.stage).await?))
}

#[derive(Deserialize)]
struct ProjectSetupRequest {
    path: String,
    #[serde(default)]
    purpose: String,
    #[serde(default)]
    stack: String,
    #[serde(default)]
    fingerprint: String,
    #[serde(default)]
    index: usize,
    #[serde(default)]
    refresh: bool,
}
async fn project_inspect(State(s): State<AppState>, Json(b): Json<ProjectSetupRequest>) -> ApiResult {
    Ok(Json(s.inspect_project(&b.path, &b.purpose, &b.stack)?))
}
async fn project_overview(State(s): State<AppState>, Json(b): Json<ProjectSetupRequest>) -> ApiResult {
    Ok(Json(s.project_overview(&b.path)?))
}
async fn project_baseline(State(s): State<AppState>, Json(b): Json<ProjectSetupRequest>) -> ApiResult {
    Ok(Json(s.project_baseline(&b.path, &b.fingerprint).await?))
}
async fn project_docs(State(s): State<AppState>, Json(b): Json<ProjectSetupRequest>) -> ApiResult {
    Ok(Json(s.documentation(&b.path, b.index, b.refresh).await?))
}
async fn project_adoption(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!(s.adoption(&id)?)))
}
async fn project_apply(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(s.apply_adoption(&id, false)?))
}
async fn project_rollback(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(s.apply_adoption(&id, true)?))
}
#[derive(Deserialize)]
struct InitialCommit {
    files: Vec<String>,
}
async fn project_initialize(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<InitialCommit>,
) -> ApiResult {
    Ok(Json(s.initialize_project(&id, b.files).await?))
}
async fn setup_save(
    State(s): State<AppState>,
    Json(p): Json<arbiter_setup::Preferences>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.save_setup(p)?))
}
async fn setup_refresh(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.refresh_accounts().await?))
}

/// Only the desktop webview (and the Vite dev server) may call the API from a
/// browser context; arbitrary websites are refused even if they guess a port.
fn cors() -> CorsLayer {
    let origins = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost", "http://localhost:1420"]
        .map(HeaderValue::from_static);
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .expose_headers([
            header::HeaderName::from_static("x-preview-width"),
            header::HeaderName::from_static("x-preview-height"),
            header::HeaderName::from_static("x-preview-route"),
        ])
}

/// Bearer header, or `?token=` for WebSocket clients that cannot set headers.
async fn auth(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let header_token =
        req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    let query_token = req.uri().query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token=")));
    match header_token.or(query_token) {
        Some(t) if constant_time_eq(t.as_bytes(), s.inner.token.as_bytes()) => next.run(req).await,
        _ => ApiError(StatusCode::UNAUTHORIZED, "missing or invalid token".into()).into_response(),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        let code = match e {
            StoreError::NotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self(code, e.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        if matches!(e.downcast_ref::<arbiter_setup::Error>(), Some(arbiter_setup::Error::Conflict))
            || e.downcast_ref::<crate::runs::Busy>().is_some()
        {
            return Self(StatusCode::CONFLICT, e.to_string());
        }
        let code = match e.downcast_ref::<StoreError>() {
            Some(StoreError::NotFound(_)) => StatusCode::NOT_FOUND,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        Self(code, format!("{e:#}"))
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

#[derive(Deserialize)]
struct IntakeCorrection {
    task_type: String,
    size: String,
    ambiguous: bool,
}
async fn correct_intake(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<IntakeCorrection>,
) -> ApiResult {
    s.correct_intake(parse_id(&id)?, b.task_type, b.size, b.ambiguous).await?;
    Ok(Json(json!({"ok":true})))
}

async fn local_models(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.inner.intake.status()))
}
#[derive(Deserialize)]
struct InstallModel {
    #[serde(default)]
    accept_license: bool,
}
async fn install_model(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<InstallModel>) -> ApiResult {
    let _lock = s.inner.admission_lock.lock().unwrap();
    if !s.inner.setup.lock().unwrap().read().map_err(anyhow::Error::from)?.network {
        return Err(bad("Model downloads are disabled in Settings"));
    }
    if let Some(model) = arbiter_intake::models::catalog().into_iter().find(|m| m.id == id) {
        let needed = model.files.iter().map(|f| f.bytes).sum::<u64>();
        if arbiter_supervisor::hardware::capacity(&s.inner.home).1.is_some_and(|free| free < needed + 256_000_000) {
            return Err(bad(
                "Not enough free space for this model and verification. Free space or choose a smaller model.",
            ));
        }
    }
    s.inner.intake.install(&id, b.accept_license)?;
    Ok(Json(s.inner.intake.status()))
}
async fn cancel_model(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.inner.intake.cancel(&id)?;
    Ok(Json(s.inner.intake.status()))
}
async fn benchmark_model(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.inner.intake.start_benchmark(&id)?;
    Ok(Json(s.inner.intake.status()))
}
async fn select_model(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.inner.intake.select(&id)?;
    Ok(Json(s.inner.intake.status()))
}
#[derive(Deserialize)]
struct IntakeAnswers {
    id: String,
    answers: Vec<arbiter_core::Answer>,
    #[serde(default)]
    more: bool,
}
async fn answer_intake(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<IntakeAnswers>) -> ApiResult {
    let id = parse_id(&id)?;
    s.answer_intake(id, &b.id, b.answers, b.more).await?;
    Ok(Json(json!(s.store(|st| st.thread(id))?)))
}
#[derive(Deserialize)]
struct ApprovalAnswer {
    run_id: arbiter_core::RunId,
    request_id: String,
    allowed: bool,
}
async fn answer_approval(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<ApprovalAnswer>,
) -> ApiResult {
    s.approve_tool(parse_id(&id)?, b.run_id, &b.request_id, b.allowed)?;
    Ok(Json(json!({"ok":true})))
}
async fn project_files(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let p = s.store(|st| st.project(parse_id(&id).map_err(|_| StoreError::NotFound("project".into()))?))?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .no_window()
            .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
            .current_dir(p.path)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| bad("file listing timed out"))?
    .map_err(|e| bad(e.to_string()))?;
    if !output.status.success() {
        return Err(bad("could not list project files"));
    }
    let paths: Vec<_> = output
        .stdout
        .split(|b| *b == 0)
        .filter_map(|b| std::str::from_utf8(b).ok())
        .filter(|p| !p.is_empty() && p.len() <= 300 && !crate::intake::sensitive(p))
        .take(5000)
        .collect();
    Ok(Json(json!(paths)))
}

fn parse_id<T: std::str::FromStr>(s: &str) -> Result<T, ApiError> {
    s.parse().map_err(|_| ApiError(StatusCode::BAD_REQUEST, format!("invalid id {s}")))
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

async fn get_plan(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!(s.plan_state(parse_id(&id)?)?)))
}
async fn plan_diff(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ForProject>,
) -> Result<String, ApiError> {
    let state = s.plan_state(parse_id(&id)?)?;
    let (path, base) = match q.project.as_deref() {
        Some(p) => {
            let i = state.integrations.get(p).ok_or_else(|| bad("that project has no changes yet"))?;
            (i.path.clone(), i.base.clone())
        }
        None => (
            state.path.ok_or_else(|| bad("plan is not executing yet"))?,
            state.base.ok_or_else(|| bad("plan base missing"))?,
        ),
    };
    Ok(arbiter_supervisor::checkpoint::diff(std::path::Path::new(&path), &base, "HEAD").await?)
}
#[derive(Deserialize)]
struct PlanBudget {
    node: Option<String>,
    usd: Option<f64>,
    tokens: Option<u64>,
}
async fn approve_plan_budget(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<PlanBudget>,
) -> ApiResult {
    let id = parse_id(&id)?;
    s.approve_plan_budget(id, b.node, b.usd, b.tokens).await?;
    Ok(Json(json!(s.plan_state(id)?)))
}
async fn edit_plan(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(plan): Json<arbiter_core::plan::Plan>,
) -> ApiResult {
    let _lock = s.inner.plan_lock.lock().await;
    let id = parse_id(&id)?;
    let state = s.plan_state(id)?;
    if let Some(planner) = state.planner {
        s.stop(planner)?;
    }
    s.propose_plan(id, plan, "Edited by you")?;
    Ok(Json(json!(s.plan_state(id)?)))
}
async fn refine_plan(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let _lock = s.inner.plan_lock.lock().await;
    let id = parse_id(&id)?;
    s.start_planner(id).await?;
    Ok(Json(json!(s.plan_state(id)?)))
}
#[derive(Deserialize)]
struct PlanApproval {
    revision: u32,
}
async fn approve_plan(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<PlanApproval>) -> ApiResult {
    let id = parse_id(&id)?;
    s.approve_plan(id, b.revision).await?;
    Ok(Json(json!(s.plan_state(id)?)))
}
#[derive(Deserialize)]
struct PlanControl {
    action: String,
}
async fn control_plan(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<PlanControl>) -> ApiResult {
    let id = parse_id(&id)?;
    s.control_plan(id, &b.action).await?;
    Ok(Json(json!(s.plan_state(id)?)))
}
#[derive(Deserialize)]
struct PlanScope {
    node: String,
    paths: Vec<String>,
}
async fn approve_plan_scope(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<PlanScope>) -> ApiResult {
    let id = parse_id(&id)?;
    s.approve_scope(id, &b.node, b.paths).await?;
    Ok(Json(json!(s.plan_state(id)?)))
}
async fn report_handoff(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(h): Json<arbiter_core::plan::Handoff>,
) -> ApiResult {
    s.report_handoff(parse_id(&id)?, h)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct ScopeRequest {
    reason: String,
}
async fn request_scope(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<ScopeRequest>) -> ApiResult {
    s.request_plan_scope(parse_id(&id)?, &b.reason)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct Refresh {
    #[serde(default)]
    refresh: bool,
}

/// Installed harnesses and their models, for pickers.
async fn harnesses(State(s): State<AppState>, Query(q): Query<Refresh>) -> ApiResult {
    let mut list = s.catalog(q.refresh).await;
    let models = s.inner.intake.status()["models"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m["ready"] == true && m["model"]["id"] != "potion")
        .map(|m| arbiter_adapters::catalog::ModelInfo {
            id: m["model"]["id"].as_str().map(str::to_owned),
            name: m["model"]["name"].as_str().unwrap_or("Local model").into(),
            description: if s.capable_local_model().as_deref() == m["model"]["id"].as_str() {
                "Passed the local coding check".into()
            } else {
                "Run the local coding check in Settings before relying on it".into()
            },
            efforts: vec![],
            default_effort: None,
        })
        .collect::<Vec<_>>();
    list.push(arbiter_adapters::catalog::HarnessInfo {
        id: "local",
        name: "Local agent",
        installed: !models.is_empty(),
        models,
        note: Some(
            "Isolated worktree required. File tools and the project's checks, up to 24 actions per turn; no shell or network tools."
                .into(),
        ),
    });
    Ok(Json(json!(list)))
}

async fn list_projects(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(s.store(|st| st.projects(WS))?)))
}

#[derive(Deserialize)]
struct NewProject {
    name: Option<String>,
    path: String,
}

async fn create_project(State(s): State<AppState>, Json(b): Json<NewProject>) -> ApiResult {
    if b.name.is_none() {
        return Ok(Json(s.register_project(&b.path)?));
    }
    let path = std::fs::canonicalize(&b.path).map_err(|e| bad(format!("{}: {e}", b.path)))?;
    if !path.join(".git").exists() {
        return Err(bad(format!("{} is not a git repository root", path.display())));
    }
    let name = b.name.unwrap_or_else(|| {
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into())
    });
    let path = path.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
    Ok(Json(json!(s.store(|st| st.create_project(WS, &name, &path))?)))
}

async fn list_threads(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(s.store(|st| st.threads(WS))?)))
}

/// Composer-first: send `message` (and optionally a title) and the thread is
/// created, titled and started in one call.
async fn create_thread(State(s): State<AppState>, Json(spec): Json<ThreadSpec>) -> ApiResult {
    Ok(Json(json!(s.create_thread(spec).await?)))
}

async fn get_thread(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    match s.store(|st| st.thread(id))? {
        Some(t) => Ok(Json(json!(t))),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("thread {id}"))),
    }
}

/// Distinguishes `"model": null` (reset to default) from an absent field.
fn nullable<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct ThreadPatch {
    tool_profile: Option<arbiter_core::ToolProfile>,
    title: Option<String>,
    harness: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    model: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    effort: Option<Option<String>>,
    permission: Option<PermissionMode>,
    /// Spending cap in USD; `null` removes it.
    #[serde(default, deserialize_with = "nullable_f64")]
    budget_usd: Option<Option<f64>>,
}

fn nullable_f64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

async fn patch_thread(State(s): State<AppState>, Path(id): Path<String>, Json(p): Json<ThreadPatch>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    let t = s.store(|st| st.thread(id))?.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("thread {id}")))?;
    if let Some(title) = p.title.map(|x| x.trim().to_owned()).filter(|x| !x.is_empty())
        && title != t.title
    {
        s.append(id, EventKind::Renamed { title })?;
    }
    let harness = p.harness.unwrap_or_else(|| t.harness.clone());
    if !matches!(harness.as_str(), "auto" | "claude" | "codex" | "local") {
        return Err(bad(format!("unknown harness {harness:?}")));
    }
    if harness != t.harness && t.session_id.is_some() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "harness is fixed once the agent has started; fork the thread to try another".into(),
        ));
    }
    let clean = |v: Option<String>| v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let switched = harness != t.harness;
    // Models and efforts are harness-specific: switching harness resets them.
    let model = p.model.map(clean).unwrap_or(if switched { None } else { t.model.clone() });
    let effort = p.effort.map(clean).unwrap_or(if switched { None } else { t.effort.clone() });
    let permission = p.permission.unwrap_or(t.permission);
    if (harness.as_str(), &model, &effort, permission) != (t.harness.as_str(), &t.model, &t.effort, t.permission) {
        s.append(id, EventKind::ConfigChanged { harness, model, effort, permission, reason: None })?;
        s.restart_for_config(id);
    }
    if let Some(usd) = p.budget_usd
        && usd != t.budget_usd
    {
        if usd.is_some_and(|u| u.is_nan() || u <= 0.0) {
            return Err(bad("budget must be positive"));
        }
        s.append(id, EventKind::BudgetSet { usd })?;
    }
    if let Some(profile) = p.tool_profile
        && profile != t.tool_profile
    {
        s.append(id, EventKind::ToolProfileChanged { profile })?;
        s.restart_for_config(id);
    }
    Ok(Json(json!(s.store(|st| st.thread(id))?)))
}

#[derive(Deserialize)]
struct After {
    #[serde(default)]
    after: i64,
}

async fn thread_events(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<After>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    Ok(Json(json!(s.store(|st| st.events(id, q.after))?)))
}

#[derive(Deserialize)]
struct NewMessage {
    text: String,
    #[serde(default)]
    attachments: Vec<String>,
}

/// Records the user's message and hands it to the thread's harness: starts
/// (or resumes) a run, or steers the one already in progress.
async fn post_message(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<NewMessage>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    if s.plan_state(id).is_ok_and(|p| p.plan.is_some()) {
        return Err(ApiError(StatusCode::CONFLICT, "Use the plan controls or open an individual agent task".into()));
    }
    if s.has_pending_intake(id) {
        return Err(ApiError(StatusCode::CONFLICT, "answer the clarification cards first".into()));
    }
    if s.has_pending_approval(id) {
        return Err(ApiError(StatusCode::CONFLICT, "resolve the pending tool approval first".into()));
    }
    let text = b.text.trim().to_owned();
    if text.is_empty() {
        return Err(bad("empty message"));
    }
    if s.store(|st| st.thread(id))?.is_none() {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("thread {id}")));
    }
    let event = s.send_message(id, text, &b.attachments).await.map_err(|e| {
        let code = if e.downcast_ref::<crate::runs::Busy>().is_some() {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_GATEWAY
        };
        ApiError(code, format!("{e:#}"))
    })?;
    Ok(Json(json!(event)))
}

#[derive(Deserialize)]
struct FolderQuery {
    #[serde(default)]
    path: String,
}

/// Folder names for the project picker; empty `path` starts at home.
async fn list_folders(Query(q): Query<FolderQuery>) -> ApiResult {
    use arbiter_supervisor::folders;
    let places = folders::places();
    let start = if q.path.trim().is_empty() {
        places.first().map(|p| p.path.clone()).ok_or_else(|| bad("no home folder found"))?
    } else {
        q.path
    };
    let listing = folders::list(std::path::Path::new(&start)).map_err(|e| bad(e.to_string()))?;
    Ok(Json(json!({"listing":listing,"places":places})))
}

async fn prerequisites(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.prerequisites().await?))
}

#[derive(Deserialize)]
struct Approved {
    #[serde(default)]
    approved: bool,
}

async fn install_prerequisite(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<Approved>) -> ApiResult {
    if !b.approved {
        return Err(bad("installing software needs your explicit approval"));
    }
    s.install_prerequisite(&id).await?;
    Ok(Json(json!({ "started": true })))
}

async fn sign_in_tool(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.sign_in(&id)?;
    Ok(Json(json!({ "opened": true })))
}

#[derive(Deserialize)]
struct GitIdentity {
    name: String,
    email: String,
}

async fn set_git_identity(State(s): State<AppState>, Json(b): Json<GitIdentity>) -> ApiResult {
    Ok(Json(s.set_git_identity(&b.name, &b.email).await?))
}

#[derive(Deserialize)]
struct AddFolder {
    path: String,
}

/// Add a folder in one step when possible; otherwise return what needs review.
async fn quick_add_project(State(s): State<AppState>, Json(b): Json<AddFolder>) -> ApiResult {
    Ok(Json(s.quick_add(&b.path).await?))
}

#[derive(Deserialize)]
struct NewFolder {
    parent: String,
    name: String,
}

async fn create_folder(Json(b): Json<NewFolder>) -> ApiResult {
    let path = arbiter_supervisor::folders::create(std::path::Path::new(&b.parent), &b.name)
        .map_err(|e| bad(e.to_string()))?;
    Ok(Json(json!({"path":path})))
}

#[derive(Deserialize)]
struct NewComparison {
    project_id: String,
    message: String,
    candidates: Vec<crate::compare::Candidate>,
    #[serde(default)]
    approved: bool,
}

async fn start_comparison(State(s): State<AppState>, Json(b): Json<NewComparison>) -> ApiResult {
    if !b.approved {
        return Err(bad("comparing agents runs each one on its own allowance; approve it explicitly"));
    }
    Ok(Json(s.start_comparison(parse_id(&b.project_id)?, &b.message, b.candidates).await?))
}

async fn get_comparison(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(s.comparison(&id).await?))
}

#[derive(Deserialize)]
struct KeepCandidate {
    thread: String,
}

async fn keep_candidate(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<KeepCandidate>) -> ApiResult {
    Ok(Json(s.keep_candidate(&id, parse_id(&b.thread)?).await?))
}

#[derive(Deserialize)]
struct Research {
    question: String,
    #[serde(default)]
    url: Option<String>,
}

async fn research(State(s): State<AppState>, Json(b): Json<Research>) -> ApiResult {
    Ok(Json(s.user_research(&b.question, b.url.as_deref()).await?))
}

async fn thread_research(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<Research>) -> ApiResult {
    Ok(Json(s.thread_research(parse_id(&id)?, &b.question, b.url.as_deref()).await?))
}

#[derive(Deserialize)]
struct SiteAdd {
    url: String,
}

async fn web_sites(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.web_sites()))
}

async fn add_web_site(State(s): State<AppState>, Json(b): Json<SiteAdd>) -> ApiResult {
    Ok(Json(s.add_web_site(&b.url).await?))
}

async fn remove_web_site(State(s): State<AppState>, Path(host): Path<String>) -> ApiResult {
    Ok(Json(s.remove_web_site(&host).await?))
}

#[derive(Deserialize)]
struct ShareDecision {
    allow: bool,
}

async fn held_shares(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.held_shares()))
}

async fn decide_share(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<ShareDecision>) -> ApiResult {
    Ok(Json(s.decide_share(&id, b.allow)?))
}

#[derive(Deserialize)]
struct SideQuestion {
    question: String,
}

async fn side_question(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<SideQuestion>) -> ApiResult {
    Ok(Json(s.side_question(parse_id(&id)?, &b.question).await?))
}

#[derive(Deserialize)]
struct Escalate {
    harness: String,
}

async fn escalate_thread(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<Escalate>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    if !matches!(b.harness.as_str(), "claude" | "codex") {
        return Err(bad("escalate to claude or codex"));
    }
    let chars = s.escalate(id, &b.harness).await?;
    Ok(Json(json!({ "ok": true, "handoff_chars": chars })))
}

async fn interrupt_thread(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    if !s.interrupt(id) {
        return Err(ApiError(StatusCode::CONFLICT, "no active run".into()));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn stop_thread(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    if !s.stop(id)? {
        return Err(ApiError(StatusCode::CONFLICT, "no active run".into()));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ForkReq {
    at_seq: i64,
}

async fn fork_thread(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<ForkReq>) -> ApiResult {
    let original: ThreadId = parse_id(&id)?;
    if s.plan_state(original).is_ok_and(|p| p.plan.is_some()) || s.plan_child(original).is_some() {
        return Err(ApiError(StatusCode::CONFLICT, "Plan tasks cannot be forked into another executing plan".into()));
    }
    let id: ThreadId = parse_id(&id)?;
    let new = s.store(|st| st.fork(id, b.at_seq))?;
    for e in s.store(|st| st.events(new, 0))? {
        s.publish(&e);
    }
    s.expire_approvals(new, "Approval is not transferable to a fork")?;
    Ok(Json(json!(s.store(|st| st.thread(new))?)))
}

#[derive(Deserialize)]
struct DiffRange {
    /// Checkpoint numbers; 0 = the thread's base. Omit both for all changes.
    from: Option<u32>,
    to: Option<u32>,
}

/// Commit for checkpoint `n` (0 = base).
fn checkpoint_commit(s: &AppState, t: &arbiter_store::ThreadSummary, n: u32) -> Result<String, ApiError> {
    if n == 0 {
        return t.base.clone().ok_or_else(|| ApiError(StatusCode::CONFLICT, "thread has no recorded base".into()));
    }
    s.store(|st| st.events(t.id, 0))?
        .into_iter()
        .find_map(|e| match e.kind {
            EventKind::Checkpoint { n: m, commit } if m == n => Some(commit),
            _ => None,
        })
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("checkpoint {n}")))
}

async fn thread_diff(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(r): Query<DiffRange>,
) -> Result<String, ApiError> {
    let id: ThreadId = parse_id(&id)?;
    let t = s.store(|st| st.thread(id))?.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("thread {id}")))?;
    if let (Some(from), Some(to)) = (r.from, r.to) {
        let cwd = s.thread_cwd(&t)?;
        let (a, b) = (checkpoint_commit(&s, &t, from)?, checkpoint_commit(&s, &t, to)?);
        return Ok(arbiter_supervisor::checkpoint::diff(&cwd, &a, &b).await?);
    }
    let (Some(path), Some(branch)) = (t.worktree, t.branch) else {
        return Err(ApiError(StatusCode::CONFLICT, "thread has no worktree".into()));
    };
    let project = s.store(|st| st.project(t.project_id))?;
    let wt = Worktree { path: path.into(), branch };
    Ok(s.inner.worktrees.diff(std::path::Path::new(&project.path), &wt).await?)
}

/// Per-turn snapshots, newest last: `[{n, commit, seq, ts}]`.
async fn thread_checkpoints(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    let list: Vec<Value> = s
        .store(|st| st.events(id, 0))?
        .into_iter()
        .filter_map(|e| match e.kind {
            EventKind::Checkpoint { n, commit } => Some(json!({ "n": n, "commit": commit, "seq": e.seq, "ts": e.ts })),
            _ => None,
        })
        .collect();
    Ok(Json(json!(list)))
}

#[derive(Deserialize)]
struct RevertReq {
    /// Checkpoint to restore; 0 = the thread's starting point.
    n: u32,
}

/// Restore the worktree's files to a checkpoint. The current state is
/// checkpointed first, so a revert can itself be reverted.
async fn revert_thread(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<RevertReq>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    let t = s.store(|st| st.thread(id))?.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("thread {id}")))?;
    if matches!(t.status, arbiter_core::ThreadStatus::Running | arbiter_core::ThreadStatus::Healing) {
        return Err(ApiError(StatusCode::CONFLICT, "the agent is working; interrupt it before reverting".into()));
    }
    let target = checkpoint_commit(&s, &t, b.n)?;
    let cwd = s.thread_cwd(&t)?;
    s.checkpoint_now(id, &t, &cwd).await;
    let t = s.store(|st| st.thread(id))?.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("thread {id}")))?;
    let current = match &t.last_checkpoint {
        Some(c) => c.clone(),
        None => arbiter_supervisor::checkpoint::head(&cwd).await?,
    };
    arbiter_supervisor::checkpoint::restore(&cwd, &target, &current).await?;
    s.append(id, EventKind::Reverted { n: b.n })?;
    s.checkpoint_now(id, &t, &cwd).await;
    Ok(Json(json!(s.store(|st| st.thread(id))?)))
}

/// Inbox "done for now": hidden until the thread's status changes again.
async fn settle_thread(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    s.append(id, EventKind::Settled)?;
    Ok(Json(json!(s.store(|st| st.thread(id))?)))
}

async fn preview_status(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    Ok(Json(json!({ "url": s.dev_server_url(id).await, "viewport": s.preview_viewport(id)? })))
}

/// Start (or reuse) the thread's dev server on a free port.
async fn preview_start(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    let url = s.dev_server(id).await?;
    Ok(Json(json!({ "url": url, "viewport": s.preview_viewport(id)? })))
}

async fn preview_command(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(s.preview_command(parse_id(&id)?)?))
}

#[derive(Deserialize)]
struct PreviewCommand {
    dev: Option<String>,
}

/// Save (or clear, with null) the project's start command, then restart the
/// thread's server so the change takes effect.
async fn set_preview_command(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<PreviewCommand>,
) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    let t = s.store(|st| st.thread(id))?.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("thread {id}")))?;
    s.set_preview_command(t.project_id, b.dev.as_deref())?;
    s.stop_dev_server(id).await;
    Ok(Json(s.preview_command(id)?))
}

async fn preview_stop(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    Ok(Json(json!({ "stopped": s.stop_dev_server(id).await })))
}

async fn preview_frame(State(s): State<AppState>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let page = s.preview_page(parse_id(&id)?).await?;
    let page = page.lock().await;
    let jpeg = page.frame(page.viewport().width).await?;
    let location = page.location().await?;
    let route = location["route"].as_str().unwrap_or("/").split(['?', '#']).next().unwrap_or("/");
    Ok((
        [
            ("content-type", "image/jpeg".to_owned()),
            ("cache-control", "no-store".to_owned()),
            ("x-preview-width", page.viewport().width.to_string()),
            ("x-preview-height", page.viewport().height.to_string()),
            ("x-preview-route", json!(route).to_string()),
        ],
        jpeg,
    )
        .into_response())
}

async fn preview_resize(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<arbiter_browser::session::Viewport>,
) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    v.validate()?;
    let page = s.preview_page(id).await?;
    let mut page = page.lock().await;
    page.resize(v).await?;
    s.append(id, EventKind::PreviewViewportChanged { width: v.width, height: v.height })?;
    Ok(Json(json!(v)))
}
async fn preview_captures(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    s.store(|st| st.thread(id))?.ok_or_else(|| bad("task not found"))?;
    let captures: Vec<_> = s
        .store(|st| st.events(id, 0))?
        .into_iter()
        .rev()
        .filter_map(|e| match e.kind {
            EventKind::PreviewCaptured { id, route, width, height } => {
                Some(json!({"id":id,"route":route,"width":width,"height":height,"ts":e.ts.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()}))
            }
            _ => None,
        })
        .take(100)
        .collect();
    Ok(Json(json!(captures)))
}
async fn preview_capture(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: ThreadId = parse_id(&id)?;
    let page = s.preview_page(id).await?;
    let page = page.lock().await;
    let viewport = page.viewport();
    let location = page.location().await?;
    let jpeg = page.frame(viewport.width).await?;
    let a = crate::attach::ingest(&s.inner.home, "preview.jpg", &jpeg)?;
    let route = location["route"].as_str().unwrap_or("/").split(['?', '#']).next().unwrap_or("/").to_owned();
    let event = s.append(
        id,
        EventKind::PreviewCaptured {
            id: a.id.clone(),
            route: route.clone(),
            width: viewport.width,
            height: viewport.height,
        },
    )?;
    Ok(Json(
        json!({"id":a.id,"route":route,"width":viewport.width,"height":viewport.height,"ts":event.ts.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()}),
    ))
}

#[derive(Deserialize)]
struct BrowserReq {
    /// errors | query | a11y | screenshot
    op: String,
    route: Option<String>,
    selector: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    width: Option<u32>,
    x: Option<f64>,
    y: Option<f64>,
    text: Option<String>,
    dy: Option<f64>,
}

/// Page tools for agents (via arbiter-mcp) and the UI. Text in, text out;
/// screenshots are written into the worktree and returned as a path.
async fn browser_tool(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<BrowserReq>) -> ApiResult {
    use arbiter_browser::page;
    let id: ThreadId = parse_id(&id)?;
    let route = b.route.as_deref().unwrap_or("/");
    if !route.starts_with('/') || route.starts_with("//") || route.contains('\\') {
        return Err(bad("route must start with /"));
    }
    if matches!(
        b.op.as_str(),
        "goto" | "click" | "type" | "inspect" | "scroll" | "location" | "query" | "a11y" | "screenshot"
    ) {
        let page = s.preview_page(id).await?;
        let page = page.lock().await;
        if b.op != "goto" && b.route.is_some() {
            page.goto(route).await?;
        }
        let out = match b.op.as_str() {
            "goto" => page.goto(route).await?,
            "click" => page.click(b.selector.as_deref(), b.x.zip(b.y)).await?,
            "type" => {
                page.type_text(
                    b.selector.as_deref().ok_or_else(|| bad("selector is required"))?,
                    b.text.as_deref().ok_or_else(|| bad("text is required"))?,
                )
                .await?
            }
            "inspect" => page.inspect(b.selector.as_deref(), b.x.zip(b.y)).await?,
            "scroll" => page.scroll(b.dy.unwrap_or(500.0)).await?,
            "query" => {
                json!({"elements":page.query(b.selector.as_deref().ok_or_else(|| bad("selector is required"))?,b.limit.unwrap_or(5)).await?})
            }
            "a11y" => json!({"tree":page.a11y().await?}),
            "screenshot" => {
                let jpeg = page.screenshot(b.selector.as_deref(), b.width.unwrap_or(768)).await?;
                let a = crate::attach::ingest(&s.inner.home, "screenshot.jpg", &jpeg)?;
                let t = s.store(|st| st.thread(id))?.ok_or_else(|| bad("thread not found"))?;
                crate::attach::materialize(&s.inner.home, &s.thread_cwd(&t)?, std::slice::from_ref(&a.id)).await?;
                json!({"path":crate::attach::attachment_dir(&s.inner.home).join(&a.file),"note":a.note,"id":a.id})
            }
            _ => page.location().await?,
        };
        return Ok(Json(out));
    }
    let base = s.dev_server(id).await?;
    let url = format!("{base}{route}");
    let browser = s.browser().await?;
    let out = match b.op.as_str() {
        "errors" => {
            let r = page::check(&browser, &url, std::time::Duration::from_millis(800)).await?;
            let lines: Vec<String> = r
                .issues
                .iter()
                .take(20)
                .map(|i| {
                    let loc = match (i.source_path(&base), i.line) {
                        (Some(p), Some(l)) => format!(" at {p}:{l}"),
                        (Some(p), None) => format!(" ({p})"),
                        _ => String::new(),
                    };
                    format!("[{}] {}{loc}", i.kind, i.message).chars().take(280).collect()
                })
                .collect();
            json!({ "title": r.title, "load_ms": r.load_ms, "issues": lines })
        }
        other => return Err(bad(format!("unknown browser operation {other:?}"))),
    };
    Ok(Json(out))
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
}

/// Raw file body; returns the stored, processed attachment.
async fn upload_attachment(
    State(s): State<AppState>,
    Query(q): Query<UploadQuery>,
    body: axum::body::Bytes,
) -> ApiResult {
    let home = s.inner.home.clone();
    let a = tokio::task::spawn_blocking(move || crate::attach::ingest(&home, &q.name, &body))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(Json(json!(a)))
}

async fn get_attachment(State(s): State<AppState>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let (a, path) =
        crate::attach::get(&s.inner.home, &id).map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("{e:#}")))?;
    let bytes = tokio::fs::read(&path).await.map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    let ctype = match a.file.rsplit('.').next().unwrap_or("") {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "md" | "log" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    };
    Ok(([(header::CONTENT_TYPE, ctype)], bytes).into_response())
}

#[derive(Deserialize)]
struct TaskQuery {
    project_id: Option<String>,
}

async fn list_tasks(State(s): State<AppState>, Query(q): Query<TaskQuery>) -> ApiResult {
    let project: Option<ProjectId> = q.project_id.as_deref().map(parse_id).transpose()?;
    Ok(Json(json!(s.store(|st| st.tasks(WS, project))?)))
}

async fn create_task(State(s): State<AppState>, Json(n): Json<NewTask>) -> ApiResult {
    if n.title.trim().is_empty() {
        return Err(bad("task title is required"));
    }
    s.store(|st| st.project(n.project_id))?;
    let t = s.store(|st| st.create_task(WS, n))?;
    s.publish_tasks();
    Ok(Json(json!(t)))
}

async fn get_task(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let id: TaskId = parse_id(&id)?;
    Ok(Json(json!(s.store(|st| st.task(id))?)))
}

async fn patch_task(State(s): State<AppState>, Path(id): Path<String>, Json(p): Json<TaskPatch>) -> ApiResult {
    let id: TaskId = parse_id(&id)?;
    let t = s.store(|st| st.update_task(id, p))?;
    s.publish_tasks();
    Ok(Json(json!(t)))
}

async fn start_task(State(s): State<AppState>, Path(id): Path<String>, Json(cfg): Json<TaskStart>) -> ApiResult {
    let id: TaskId = parse_id(&id)?;
    let (task, thread) = s.start_task(id, cfg).await?;
    Ok(Json(json!({ "task": task, "thread": thread })))
}

/// Live stream: every appended event as a JSON text frame, plus
/// `{"type":"tasks_changed"}` nudges and `{"type":"lagged"}` resync hints.
#[derive(Deserialize)]
struct TermSize {
    #[serde(default = "default_cols")]
    cols: u16,
    #[serde(default = "default_rows")]
    rows: u16,
}
fn default_cols() -> u16 {
    100
}
fn default_rows() -> u16 {
    30
}

/// The task's terminal over a WebSocket: output as binary frames, input as
/// `{"type":"input","data"}` or `{"type":"resize","cols","rows"}` text frames.
async fn terminal_socket(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<TermSize>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let term = s.terminal(parse_id(&id)?, q.cols, q.rows)?;
    Ok(upgrade.on_upgrade(move |socket| pump_terminal(term, socket)))
}

async fn pump_terminal(term: std::sync::Arc<arbiter_supervisor::pty::Terminal>, mut socket: WebSocket) {
    let mut rx = term.output.subscribe();
    let replay = term.snapshot();
    if !replay.is_empty() && socket.send(Message::Binary(replay.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Ok(bytes) => if socket.send(Message::Binary(bytes.into())).await.is_err() { return },
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(t))) => {
                    let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
                    match v["type"].as_str() {
                        Some("input") => { let _ = term.write(v["data"].as_str().unwrap_or("").as_bytes()); }
                        Some("resize") => term.resize(v["cols"].as_u64().unwrap_or(100) as u16, v["rows"].as_u64().unwrap_or(30) as u16),
                        _ => {}
                    }
                }
                Some(Ok(Message::Binary(b))) => { let _ = term.write(&b); }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
        }
        if !term.is_alive() {
            let _ = socket.send(Message::Text("{\"type\":\"exit\"}".into())).await;
            return;
        }
    }
}

async fn close_terminal(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!({ "closed": s.close_terminal(parse_id(&id)?) })))
}

async fn working_files(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!(s.working_files(parse_id(&id)?).await?)))
}

#[derive(Deserialize)]
struct FilePath {
    path: String,
}

async fn read_file(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<FilePath>) -> ApiResult {
    Ok(Json(s.read_file(parse_id(&id)?, &q.path)?))
}

#[derive(Deserialize)]
struct FileWrite {
    path: String,
    content: String,
    hash: String,
}

async fn write_file(State(s): State<AppState>, Path(id): Path<String>, Json(b): Json<FileWrite>) -> ApiResult {
    Ok(Json(s.write_file(parse_id(&id)?, &b.path, &b.content, &b.hash)?))
}

async fn ws(State(s): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| stream_events(s, socket))
}

async fn stream_events(s: AppState, mut socket: WebSocket) {
    let mut rx = s.inner.events.subscribe();
    loop {
        tokio::select! {
            msg = rx.recv() => {
                let text = match msg {
                    Ok(WsMsg::Event(e)) => match serde_json::to_string(&e) {
                        Ok(t) => t,
                        Err(_) => continue,
                    },
                    Ok(WsMsg::TasksChanged) => json!({ "type": "tasks_changed" }).to_string(),
                    // Tell the client to resync via /events?after=<last seq>.
                    Err(RecvError::Lagged(n)) => json!({ "type": "lagged", "missed": n }).to_string(),
                    Err(RecvError::Closed) => return,
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
        }
    }
}

async fn vault_state(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let state = s.knowledge(s.knowledge_project(parse_id(&id)?)?)?;
    let links: Vec<_> = state
        .notes
        .values()
        .flat_map(|n| arbiter_vault::links(&n.body).into_iter().map(|to| json!({"from":n.id,"to":to})))
        .collect();
    Ok(Json(
        json!({"notes":state.notes.values().collect::<Vec<_>>(),"proposals":state.proposals.values().collect::<Vec<_>>(),"links":links}),
    ))
}
#[derive(Deserialize)]
struct NoteWrite {
    note: arbiter_core::vault::Note,
    base_revision: u32,
}
async fn vault_save(State(s): State<AppState>, Path(id): Path<String>, Json(body): Json<NoteWrite>) -> ApiResult {
    Ok(Json(json!(s.save_note(parse_id(&id)?, body.note, body.base_revision)?)))
}
async fn vault_propose(State(s): State<AppState>, Path(id): Path<String>, Json(body): Json<NoteWrite>) -> ApiResult {
    Ok(Json(json!({"id":s.propose_note(parse_id(&id)?,body.note,body.base_revision)?})))
}
#[derive(Deserialize)]
struct NoteResolve {
    id: String,
    accepted: bool,
}
async fn vault_resolve(State(s): State<AppState>, Path(id): Path<String>, Json(body): Json<NoteResolve>) -> ApiResult {
    s.resolve_note(parse_id(&id)?, &body.id, body.accepted)?;
    Ok(Json(json!({"resolved":true})))
}
async fn vault_rebuild(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!({"notes":s.rebuild_vault(s.knowledge_project(parse_id(&id)?)?)?})))
}
#[derive(Deserialize)]
struct VaultTool {
    op: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    budget: Option<usize>,
}
async fn vault_tool(State(s): State<AppState>, Path(id): Path<String>, Json(body): Json<VaultTool>) -> ApiResult {
    let state = s.knowledge(s.knowledge_project(parse_id(&id)?)?)?;
    if body.query.len() > 1000 {
        return Err(bad("query exceeds 1000 bytes"));
    }
    let budget = body.budget.unwrap_or(1000).clamp(100, 4000);
    let notes: Vec<_> = state.notes.into_values().collect();
    let text = match body.op.as_str() {
        "search" => arbiter_vault::catalog(&notes, &body.query, budget).0,
        "read" => {
            let n = notes.iter().find(|n| n.id == body.id).ok_or_else(|| bad("note not found"))?;
            arbiter_vault::validate(n).map_err(|_| bad("note failed memory guards"))?;
            let full = format!("[[{}]] revision {}: {}\n{}", n.id, n.revision, n.title, n.body);
            let mut end = full.len().min(budget.saturating_sub(30));
            while !full.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}{}", &full[..end], if end < full.len() { "\n[truncated to budget]" } else { "" })
        }
        _ => return Err(bad("expected search or read")),
    };
    Ok(Json(json!({"text":text,"bytes":text.len(),"budget":budget})))
}
async fn learning_state(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let state = s.knowledge(s.knowledge_project(parse_id(&id)?)?)?;
    Ok(Json(json!({"strengths":state.strengths(),"outcomes":state.outcomes,"preferences":state.preferences})))
}
#[derive(Deserialize)]
struct PreferenceWrite {
    task_type: String,
    harness: Option<String>,
    model: Option<String>,
    enabled: bool,
}
async fn learning_preference(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PreferenceWrite>,
) -> ApiResult {
    s.routing_preference(parse_id(&id)?, body.task_type, body.harness, body.model, body.enabled)?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize)]
struct ReworkWrite {
    rework: bool,
}
async fn learning_rework(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReworkWrite>,
) -> ApiResult {
    let id = parse_id(&id)?;
    let state = s.knowledge(s.knowledge_project(id)?)?;
    if !state.outcomes.contains_key(&id.to_string()) {
        return Err(bad("no measured outcome for this task"));
    }
    s.append(id, EventKind::OutcomeRework { rework: body.rework })?;
    Ok(Json(json!({"saved":true})))
}
