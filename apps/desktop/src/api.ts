// Client for the arbiterd HTTP/WS API. The desktop shell only tells us where

// the daemon is; everything else is plain fetch + WebSocket so the same code

// can later run in a browser or on mobile.

import { invoke } from "@tauri-apps/api/core";

export type ThreadStatus = "idle" | "running" | "healing" | "needs_approval" | "review" | "failed" | "merged";

export interface Viewport { width: number; height: number }
export interface PreviewCapture extends Viewport { id: string; route: string; ts: string }
export interface PreviewStatus { url: string | null; viewport: Viewport }
export interface VaultNote { id:string; title:string; body:string; revision:number }
export interface VaultProposal { id:string; note:VaultNote; base_revision:number; thread:string }
export interface VaultState { notes:VaultNote[]; proposals:VaultProposal[]; links:{from:string;to:string}[] }
export interface Strength { task_type:string; harness:string; model:string|null; runs:number; first_try:number; passed:number; rework:number; tokens:number; cost_usd:number }
export interface LearningState { strengths:Strength[]; outcomes:Record<string,{task_type:string;harness:string;model:string|null;passed:boolean;checks_run:number;first_try:boolean;heal_attempts:number;input_tokens:number;output_tokens:number;cost_usd:number;user_rework:boolean}>; preferences:Record<string,{harness:string|null;model:string|null;enabled:boolean}> }
export interface PreviewImage extends Viewport { blob: Blob; route: string }
export interface Project {

  id: string;

  name: string;

  path: string;

}

export interface SetupPreferences { revision:number;step:number;complete:boolean;purpose:string;mode:"local"|"subscription"|"paid";network:boolean;paid_consent:boolean;max_cloud_runs:number;reviewer_model?:string|null;documentation_model?:string|null;context7_enabled?:boolean;web_research_enabled?:boolean;strict_allowance?:boolean;auto_update?:boolean }
export interface SetupState { preferences:SetupPreferences;accounts:{harness:string;version:string|null;installed:boolean;auth:string;plan:string|null;windows:{used_percent:number;duration_mins:number|null;resets_at:number|null}[];credits_enabled:boolean|null;observed_at:number;source:string;conflict:boolean;note:string}[];readiness:{harness:string;blocked:string|null}[];hardware:{logical_cpus:number;os:string;arch:string;memory_bytes:number|null;free_disk_bytes:number|null};local_coding_available:boolean }

export type PermissionMode = "safe" | "auto" | "plan";
export interface DocProfile { name:string; version:string; manifest:string; source:string; exact:boolean; note:string }
export interface ProjectInventory { root:string;git:boolean;empty:boolean;instructions:string[];profiles:DocProfile[];warnings:string[];fingerprint:string;initial_files:string[] }
export interface Adoption { id:string;inventory:ProjectInventory;changes:{path:string;content:string}[];state:string }
export interface ProjectCheck { name:string;cmd:string }
export interface Baseline { status:string;at:number;fingerprint?:string;checks_run:number;results:{name:string;ok:boolean;timed_out:boolean;duration_ms:number;cmd?:string;evidence?:string|null}[] }
export interface ProjectOverview { inventory:ProjectInventory;latest:Adoption|null;baseline:Baseline|null;checks:ProjectCheck[] }
export interface DocEvidence { profile:DocProfile;network:boolean;cached:{source:string;at:number;hash:string;text:string;applicability:string}|null;note?:string }
export interface CredentialStatus { configured:boolean;source:"none"|"session"|"os_store"|"environment";persistent_available:boolean }

export type HarnessChoice = "auto" | "claude" | "codex" | "local";

export type ToolProfile = "auto" | "implementation" | "research";

export interface PlanNode { id: string; title: string; goal: string; scope: string[]; may_read: string[]; non_goals: string[]; dependencies: string[]; checks: string[]; harness: HarnessChoice; model: string | null; model_reason: string; tool_profile: ToolProfile; budget_usd: number | null; token_budget: number | null; project?: string | null }

export interface Plan { title: string; goal: string; nodes: PlanNode[]; concurrency: number; budget_usd: number | null; projects?: string[] }

export interface Handoff { summary: string; files: string[]; decisions: string[]; interfaces: string[]; open_issues: string[] }

export interface PlanNodeState { status: "queued" | "running" | "checking" | "blocked" | "merging" | "merged"; reason: string; thread: string | null; task: string | null; base: string | null; brief: string | null; handoff: Handoff | null; commit: string | null; extra_scope: string[]; repair: string | null }

export interface PlanState { plan: Plan | null; revision: number; source: string; approved: boolean; path: string | null; branch: string | null; task: string | null; nodes: Record<string,PlanNodeState>; paused: string | null; completed: string | null; planner: string | null; integrations?: Record<string, { path: string; branch: string; base: string }> }

export interface Assessment { task_type: string; size: string; ambiguity: number; risks: string[]; needs_frontier: boolean; engine: string; elapsed_ms: number }

export interface Question { id: string; header: string; question: string; kind: "single" | "multi" | "short"; options: { label: string; description: string }[]; recommended: number | null }

export interface Answer { question_id: string; text: string }

export interface IntentSpec { request: string; answers: Answer[]; context_paths: string[]; task_type: string; size: string; risks: string[]; needs_frontier: boolean }

export interface LocalModels { selected: string | null; last_error: string | null; models: { model: { id: string; name: string; license: string; repository: string; files: { bytes: number }[] }; ready: boolean; progress: { phase: string; downloaded: number; total: number; error: string | null } | null; benchmark: { first_token_ms: number; complete_ms: number; target_ms: number; meets_target: boolean; samples: number } | null }[] }

/** What an agent runs with. `null` model/effort = the harness default. */

export interface RunConfig {

  tool_profile?: ToolProfile;

  harness: HarnessChoice;

  model: string | null;

  effort: string | null;

  permission: PermissionMode;

}

export interface ModelInfo {

  id: string | null;

  name: string;

  description: string;

  efforts: string[];

  default_effort: string | null;

}

export interface HarnessInfo {

  id: "claude" | "codex" | "local";

  name: string;

  installed: boolean;

  models: ModelInfo[];

  note: string | null;

}

export type TaskStatus = "backlog" | "todo" | "in_progress" | "in_review" | "done" | "canceled";

export type Priority = "urgent" | "high" | "medium" | "low" | "none";

export interface Task {

  id: string;

  project_id: string;

  key: string;

  number: number;

  title: string;

  description: string;

  status: TaskStatus;

  priority: Priority;

  labels: string[];

  parent_id: string | null;

  position: number;

  thread_ids: string[];

  created_at: string;

  updated_at: string;

}

export interface Thread {

  plan_root?: string | null;

  plan_node?: string | null;

  tool_profile: ToolProfile;

  id: string;

  project_id: string;

  title: string;

  /** "auto" until the router picks a harness on the first message. */

  harness: HarnessChoice;

  effort: string | null;

  status: ThreadStatus;

  worktree: string | null;

  branch: string | null;

  session_id: string | null;

  parent: [string, number] | null;

  permission: PermissionMode;

  model: string | null;

  input_tokens: number;

  output_tokens: number;

  cost_usd: number;

  heal_attempts: number;

  budget_usd: number | null;

  /** Hidden from the Inbox until the status changes again. */

  settled: boolean;

  checkpoints: number;

  last_seq: number;

  updated_at: string;

}

export type AgentEvent =

  | { type: "message"; text: string }

  | { type: "tool_call"; id: string; name: string; input: unknown }

  | { type: "tool_result"; id: string; output: string; is_error: boolean }

  | { type: "usage"; input_tokens: number; output_tokens: number; cost_usd: number }

  | { type: "error"; message: string }

  | { type: "done" };

export type EventKind =
  | {type:"vault_saved";note:VaultNote}
  | {type:"vault_proposed";id:string;note:VaultNote;base_revision:number}
  | {type:"vault_resolved";id:string;accepted:boolean;note:VaultNote|null}
  | {type:"memory_used";notes:string[];bytes:number}
  | {type:"outcome_recorded";outcome:LearningState["outcomes"][string]}
  | {type:"outcome_rework";rework:boolean}
  | {type:"routing_preference";task_type:string;harness:string|null;model:string|null;enabled:boolean}
  | {type: "preview_viewport_changed"; width: number; height: number}
  | {type: "preview_captured"; id: string; route: string; width: number; height: number}

  | { type: "workflow_requested" }

  | { type: "plan"; event: { type: string; [key: string]: unknown } }

  | { type: "plan_child"; root: string; node: string | null; repair: boolean }

  | { type: "tool_profile_changed"; profile: ToolProfile }

  | { type: "intake_assessed"; assessment: Assessment }

  | { type: "questions_asked"; id: string; questions: Question[]; request: string; context_paths: string[]; attachments: string[] }

  | { type: "questions_answered"; id: string; answers: Answer[] }

  | { type: "intent_ready"; spec: IntentSpec }

  | { type: "approval_requested"; run_id: string; request_id: string; tool: string; input: unknown }

  | { type: "approval_resolved"; run_id: string; request_id: string; allowed: boolean; reason: string }

  | { type: "thread_created"; title: string; harness: string; branch: string | null; parent: [string, number] | null }

  | { type: "user_message"; text: string; attachments?: AttachmentRef[] }

  | { type: "run_started"; run_id: string; session_id: string | null }

  | { type: "session"; run_id: string; session_id: string }

  | { type: "notice"; text: string }
  | { type: "comparison_joined"; comparison: string; label: string }
  | { type: "comparison_decided"; comparison: string; kept: string }
  | { type: "preview_ready"; url: string }
  | { type: "share_requested"; id: string; host: string; question: string; reasons: string[] }
  | { type: "share_decided"; id: string; allowed: boolean }
  | { type: "approach_asked" }
  | { type: "side_chat"; question: string; answer: string; model: string }
  | { type: "second_opinion"; stage:string;model:string;evidence_hash:string;report: {summary:string;recommendation:string;limitations:string;issues:{finding:string;source:string;quote:string}[]} }

  | { type: "config_changed"; harness: string; model: string | null; effort: string | null; permission: PermissionMode; reason: string | null }

  | { type: "renamed"; title: string }

  | { type: "agent"; run_id: string; event: AgentEvent }

  | { type: "run_ended"; run_id: string; exit_code: number | null }

  | { type: "status_changed"; status: ThreadStatus }

  | { type: "heal"; attempt: number; signature: string; excerpt: string }

  | { type: "checks_ran"; attempt: number; results: CheckResult[] }

  | { type: "checkpoint"; n: number; commit: string }

  | { type: "reverted"; n: number }

  | { type: "rate_limited"; resets_at: number | null; message: string }

  | { type: "budget_set"; usd: number | null }

  | { type: "settled" };

export interface CheckResult {

  name: string;

  ok: boolean;

  duration_ms: number;

  failures: number;

  timed_out: boolean;

  fixed: boolean;

}

export interface CheckpointInfo {

  n: number;

  commit: string;

  seq: number;

  ts: string;

}

export interface ArbEvent {

  id: number;

  thread_id: string;

  seq: number;

  ts: string;

  kind: EventKind;

}

export interface Connection {

  base_url: string;

  token: string;

}

export interface AttachmentRef { id: string; name: string; kind: string; note: string }

export interface Attachment extends AttachmentRef { file: string; bytes: number }

export interface ElementDescriptor {

  selector: string; component: string | null; source: string | null; text: string;

  box: { x: number; y: number; width: number; height: number };

  descriptor: string;

}

const inTauri = "__TAURI_INTERNALS__" in window;

/** In Tauri, ask the shell (which starts the daemon if needed). In a plain

 * browser (dev), take `?port=&token=` from the URL. */

export async function connect(): Promise<Connection> {

  if (inTauri) return invoke<Connection>("connect");

  // An explicit ?port=&token= wins, then moves out of the address bar so the
  // URL stays readable and the token is not kept in history.
  const url = new URL(location.href);
  const token = url.searchParams.get("token");
  if (token) {
    const conn = { base_url: `http://127.0.0.1:${url.searchParams.get("port") ?? "7433"}`, token };
    try { sessionStorage.setItem("arbiter.connection", JSON.stringify(conn)); } catch { /* storage unavailable */ }
    url.searchParams.delete("token");
    url.searchParams.delete("port");
    history.replaceState(null, "", url);
    return conn;
  }
  try {
    const saved = sessionStorage.getItem("arbiter.connection");
    if (saved) {
      const conn = JSON.parse(saved) as Connection;
      // A restarted daemon has a new token; drop a stale connection.
      const ok = await fetch(`${conn.base_url}/v1/setup`, { headers: { Authorization: `Bearer ${conn.token}` } }).then(r => r.ok).catch(() => false);
      if (ok) return conn;
      sessionStorage.removeItem("arbiter.connection");
    }
  } catch { /* storage unavailable */ }
  // Dev server: find the daemon from its discovery file, like the desktop app.
  const res = await fetch("/__arbiter/connect").catch(() => null);
  if (res?.ok) {
    const conn = (await res.json()) as Partial<Connection>;
    if (conn.base_url && conn.token) return conn as Connection;
  }
  throw new Error("No running Arbiter found. Start arbiterd, or open with ?port=<port>&token=<token> from daemon.json.");

}

export class Api {
  setup = () => this.req<SetupState>("/v1/setup");
  cancelModel = (id:string) => this.req<LocalModels>(`/v1/models/${id}/cancel`, {method:"POST"});
  saveSetup = (p:SetupPreferences) => this.req<SetupState>("/v1/setup", {method:"POST",body:JSON.stringify(p)});
  refreshSetup = () => this.req<SetupState>("/v1/setup/refresh", {method:"POST"});

  /** Finds the daemon again (for example after it restarted with a new token). */
  reconnect?: () => Promise<Connection>;

  constructor(private conn: Connection) {}

  /** WebSocket address of a task's terminal (browsers cannot set headers). */
  terminalUrl = (id: string, cols: number, rows: number) =>
    `${this.conn.base_url.replace(/^http/, "ws")}/v1/threads/${id}/terminal?cols=${cols}&rows=${rows}&token=${this.conn.token}`;
  closeTerminal = (id: string) => this.req<{ closed: boolean }>(`/v1/threads/${id}/terminal/close`, { method: "POST", body: "{}" });
  workingFiles = (id: string) => this.req<string[]>(`/v1/threads/${id}/files`);
  readFile = (id: string, path: string) => this.req<{ path: string; content: string; hash: string }>(`/v1/threads/${id}/file?path=${encodeURIComponent(path)}`);
  writeFile = (id: string, path: string, content: string, hash: string) => this.req<{ path: string; hash: string }>(`/v1/threads/${id}/file`, { method: "PUT", body: JSON.stringify({ path, content, hash }) });

  /** `slow` is for work done by a local model, which can take minutes on
   *  an ordinary computer; everything else answers within two minutes. */
  private async req<T>(path: string, init?: RequestInit & { text?: boolean; slow?: boolean; retried?: boolean }): Promise<T> {

    const limit = init?.slow ? 20 * 60_000 : 120_000;
    let res: Response;
    try {
      res = await fetch(this.conn.base_url + path, {
        signal: AbortSignal.timeout(limit),
        ...init,
        headers: { Authorization: `Bearer ${this.conn.token}`, "Content-Type": "application/json", ...init?.headers },
      });
    } catch (e) {
      if (e instanceof DOMException && (e.name === "TimeoutError" || e.name === "AbortError")) {
        throw new Error(init?.slow
          ? "This is taking unusually long. Arbiter keeps working on it in the background; check again in a few minutes."
          : "Arbiter's background service did not answer in time. It may be busy; try again in a moment.");
      }
      throw new Error("Can't reach Arbiter's background service. If you closed it, start Arbiter again.");
    }

    // A restarted background service has a new token: pick it up and retry once.
    if (res.status === 401 && this.reconnect && !init?.retried) {
      try {
        this.conn = await this.reconnect();
        return this.req<T>(path, { ...init, retried: true });
      } catch { /* fall through to the error below */ }
    }
    if (!res.ok) {

      const body = await res.json().catch(() => ({}));

      // An unknown route answers 404 with no message: the background service
      // is older than this window.
      if (res.status === 404 && !body.error) throw new Error("Arbiter's background service is out of date. Quit Arbiter completely and start it again to finish updating.");
      throw new Error(body.error ?? `${res.status} ${res.statusText}`);

    }

    return (init?.text ? res.text() : res.json()) as Promise<T>;

  }

  projects = () => this.req<Project[]>("/v1/projects");

  addProject = (path: string) => this.req<Project>("/v1/projects", { method: "POST", body: JSON.stringify({ path }) });
  inspectProject = (path:string,purpose:string,stack:string) => this.req<{proposal:Adoption;checks:ProjectCheck[]}>("/v1/project-setup/inspect",{method:"POST",body:JSON.stringify({path,purpose,stack})});
  projectOverview = (path:string) => this.req<ProjectOverview>("/v1/project-setup/overview",{method:"POST",body:JSON.stringify({path})});
  applyAdoption = (id:string,rollback=false) => this.req<Adoption>(`/v1/project-setup/${id}/${rollback?'rollback':'apply'}`,{method:"POST"});
  getAdoption = (id:string) => this.req<Adoption>(`/v1/project-setup/${encodeURIComponent(id)}`);
  initializeProject = (id:string,files:string[]) => this.req<Adoption>(`/v1/project-setup/${id}/initialize`,{method:"POST",body:JSON.stringify({files})});
  projectBaseline = (path:string,fingerprint:string) => this.req<Baseline>("/v1/project-setup/baseline",{method:"POST",body:JSON.stringify({path,fingerprint})});
  documentationSetup = (test:boolean,signal?:AbortSignal,operation_id?:string) => this.req<{ready?:boolean;message:string}>("/v1/documentation/setup",{method:"POST",body:JSON.stringify({test,operation_id}),signal});
  cancelDocumentation = (id:string) => this.req<{cancelled:boolean}>(`/v1/documentation/operations/${encodeURIComponent(id)}/cancel`,{method:"POST"});
  documentationCredential = (signal?:AbortSignal) => this.req<CredentialStatus>("/v1/documentation/credential",{signal});
  saveDocumentationCredential = (key:string,persist:boolean) => this.req<CredentialStatus>("/v1/documentation/credential",{method:"POST",body:JSON.stringify({key,persist})});
  removeDocumentationCredential = () => this.req<CredentialStatus>("/v1/documentation/credential",{method:"DELETE"});
  documentationQuery = (library:string,topic:string,signal?:AbortSignal,operation_id?:string) => this.req<{summary:string;limitations:string;sources:string[];library_id:string}>("/v1/documentation/query",{method:"POST",body:JSON.stringify({library,topic,operation_id}),signal});
  projectDocs = (path:string,index:number,refresh:boolean) => this.req<DocEvidence>("/v1/project-setup/docs",{method:"POST",body:JSON.stringify({path,index,refresh})});

  threads = () => this.req<Thread[]>("/v1/threads");

  harnesses = (refresh = false) => this.req<HarnessInfo[]>(`/v1/harnesses${refresh ? "?refresh=true" : ""}`);

  /** Composer-first: creates, titles and starts the thread in one call. */

  createThread = (b: RunConfig & { project_id: string; message: string; worktree: boolean; attachments?: string[]; intake?: boolean; context_paths?: string[]; workflow?: boolean; auto?: boolean; projects?: string[] }) =>

    this.req<Thread>("/v1/threads", { method: "POST", body: JSON.stringify(b) });

  patchThread = (id: string, b: Partial<RunConfig> & { title?: string; budget_usd?: number | null }) =>

    this.req<Thread>(`/v1/threads/${id}`, { method: "PATCH", body: JSON.stringify(b) });

  tasks = (projectId?: string) => this.req<Task[]>(`/v1/tasks${projectId ? `?project_id=${projectId}` : ""}`);

  createTask = (b: { project_id: string; title: string; description?: string; status?: TaskStatus; priority?: Priority }) =>

    this.req<Task>("/v1/tasks", { method: "POST", body: JSON.stringify(b) });

  patchTask = (id: string, b: Partial<Pick<Task, "title" | "description" | "status" | "priority" | "labels" | "position">>) =>

    this.req<Task>(`/v1/tasks/${id}`, { method: "PATCH", body: JSON.stringify(b) });

  startTask = (id: string, cfg: RunConfig & { worktree: boolean }) =>

    this.req<{ task: Task; thread: Thread }>(`/v1/tasks/${id}/start`, { method: "POST", body: JSON.stringify(cfg) });

  interrupt = (id: string) => this.req<unknown>(`/v1/threads/${id}/interrupt`, { method: "POST", body: "{}" });

  localModels = () => this.req<LocalModels>("/v1/models");
  localCapability = () => this.req<LocalCapabilityReport>("/v1/local/capability");
  checkLocalCapability = (model: string) => this.req<LocalCapability>("/v1/local/capability", { method: "POST", body: JSON.stringify({ model }), slow: true });
  review = (id: string, comments: ReviewComment[], revision?: number) => this.req<{ kind: "plan" | "message" }>(`/v1/threads/${id}/review`, { method: "POST", body: JSON.stringify({ comments, revision }) });
  /** `project` picks one of a multi-project plan's other projects. */
  landing = (id: string, project?: string) => this.req<LandingStatus>(`/v1/threads/${id}/landing${project ? `?project=${encodeURIComponent(project)}` : ""}`);
  commit = (id: string, fingerprint: string, message: string, include: string[], project?: string) => this.req<LandingStatus>(`/v1/threads/${id}/commit`, { method: "POST", body: JSON.stringify({ fingerprint, message, include, project }) });
  publishDraft = (id: string, project?: string) => this.req<PublishDraft>(`/v1/threads/${id}/publish${project ? `?project=${encodeURIComponent(project)}` : ""}`, { slow: true });
  publish = (id: string, p: { fingerprint: string; mode: PublishMode; base: string; branch: string; message: string; title: string; body: string; project?: string }) => this.req<{ url: string | null; branch: string; commit: string; mode: PublishMode }>(`/v1/threads/${id}/publish`, { method: "POST", body: JSON.stringify({ ...p, approved: true }) });
  startComparison = (project_id: string, message: string, candidates: CompareCandidate[]) => this.req<{ id: string; threads: string[] }>("/v1/comparisons", { method: "POST", body: JSON.stringify({ project_id, message, candidates, approved: true }) });
  comparison = (id: string) => this.req<Comparison>(`/v1/comparisons/${id}`);
  keepCandidate = (id: string, thread: string) => this.req<Comparison>(`/v1/comparisons/${id}/keep`, { method: "POST", body: JSON.stringify({ thread }) });
  sideQuestion = (id: string, question: string) => this.req<{ answer: string; model: string }>(`/v1/threads/${id}/side`, { method: "POST", body: JSON.stringify({ question }), slow: true });
  escalate = (id: string, harness: "claude" | "codex") => this.req<{ ok: boolean; handoff_chars: number }>(`/v1/threads/${id}/escalate`, { method: "POST", body: JSON.stringify({ harness }), slow: true });

  planDiff = (id: string, project?: string) => this.req<string>(`/v1/threads/${id}/plan/diff${project ? `?project=${encodeURIComponent(project)}` : ""}`, {text:true});

  approvePlanBudget = (id: string, node: string | null, usd: number | null, tokens: number | null) => this.req<PlanState>(`/v1/threads/${id}/plan/budget`, {method:"POST",body:JSON.stringify({node,usd,tokens})});

  plan = (id: string) => this.req<PlanState>(`/v1/threads/${id}/plan`);

  savePlan = (id: string, plan: Plan) => this.req<PlanState>(`/v1/threads/${id}/plan`, {method:"PATCH",body:JSON.stringify(plan)});

  refinePlan = (id: string) => this.req<PlanState>(`/v1/threads/${id}/plan/draft`, {method:"POST",body:"{}"});

  approvePlan = (id: string, revision: number) => this.req<PlanState>(`/v1/threads/${id}/plan/approve`, {method:"POST",body:JSON.stringify({revision})});

  controlPlan = (id: string, action: "pause" | "resume") => this.req<PlanState>(`/v1/threads/${id}/plan/control`, {method:"POST",body:JSON.stringify({action})});

  approveScope = (id: string, node: string, paths: string[]) => this.req<PlanState>(`/v1/threads/${id}/plan/scope`, {method:"POST",body:JSON.stringify({node,paths})});

  installModel = (id: string, accept_license: boolean) => this.req<LocalModels>(`/v1/models/${id}/install`, { method: "POST", body: JSON.stringify({ accept_license }) });

  benchmarkModel = (id: string) => this.req<LocalModels>(`/v1/models/${id}/benchmark`, { method: "POST", body: "{}", slow: true });

  selectLocalModel = (id: string) => this.req<LocalModels>(`/v1/models/${id}/select`, { method: "POST", body: "{}" });

  prerequisites = () => this.req<{ tools: { id: string; name: string; why: string; required: boolean; installed: boolean; version: string | null; can_install: boolean; manual_url: string; sign_in: boolean; signed_in: boolean; account: { checked: boolean; plan: string | null; auth: string; allowance: string[]; blocked: string | null; note: string } | null; job: { running: boolean; ok: boolean | null; message: string } | null }[]; git_identity: { name: string | null; email: string | null } }>("/v1/prerequisites");
  installPrerequisite = (id: string) => this.req<{ started: boolean }>(`/v1/prerequisites/${id}/install`, { method: "POST", body: JSON.stringify({ approved: true }) });
  memory = () => this.req<MemoryStatus>("/v1/memory");
  /** Stop the background service (for updates). Refused while an agent works unless forced. */
  shutdown = (force: boolean) => this.req<{ stopping: boolean }>("/v1/shutdown", { method: "POST", body: JSON.stringify({ force }) });
  memorySync = (push: boolean, closing: boolean) => this.req<MemoryStatus>("/v1/memory/sync", { method: "POST", body: JSON.stringify({ push, closing }) });
  memoryRemote = (url: string) => this.req<MemoryStatus>("/v1/memory/remote", { method: "POST", body: JSON.stringify({ url }) });
  memoryGithub = (name: string) => this.req<MemoryStatus>("/v1/memory/github", { method: "POST", body: JSON.stringify({ name, approved: true }), slow: true });
  webSites = () => this.req<{ sites: string[] }>("/v1/web/sites");
  addWebSite = (url: string) => this.req<{ host: string; sites: string[] }>("/v1/web/sites", { method: "POST", body: JSON.stringify({ url }) });
  removeWebSite = (host: string) => this.req<{ sites: string[]; signed_out: boolean; note: string | null }>(`/v1/web/sites/${encodeURIComponent(host)}`, { method: "DELETE" });
  decideShare = (id: string, allow: boolean) => this.req<unknown>(`/v1/research/held/${encodeURIComponent(id)}`, { method: "POST", body: JSON.stringify({ allow }) });
  signIn = (id: string) => this.req<{ opened: boolean }>(`/v1/prerequisites/${id}/sign-in`, { method: "POST", body: "{}" });
  setGitIdentity = (name: string, email: string) => this.req<{ name: string; email: string }>("/v1/git-identity", { method: "POST", body: JSON.stringify({ name, email }) });
  quickAdd = (path: string) => this.req<{ status: "added"; project: Project } | { status: "review"; proposal: Adoption; checks: ProjectCheck[] }>("/v1/projects/add", { method: "POST", body: JSON.stringify({ path }) });
  folders = (path: string) => this.req<{ listing: { path: string; parent: string | null; entries: { name: string; path: string }[]; truncated: boolean }; places: { name: string; path: string }[] }>(`/v1/folders?path=${encodeURIComponent(path)}`);
  createFolder = (parent: string, name: string) => this.req<{ path: string }>("/v1/folders", { method: "POST", body: JSON.stringify({ parent, name }) });
  projectFiles = (id: string) => this.req<string[]>(`/v1/projects/${id}/files`);

  answerIntake = (thread: string, id: string, answers: Answer[], more: boolean) => this.req<Thread>(`/v1/threads/${thread}/answers`, { method: "POST", body: JSON.stringify({ id, answers, more }) });

  correctIntake = (thread: string, task_type: string, size: string, ambiguous: boolean) => this.req<unknown>(`/v1/threads/${thread}/intake`, { method: "PATCH", body: JSON.stringify({ task_type, size, ambiguous }) });

  approveTool = (thread: string, run_id: string, request_id: string, allowed: boolean) => this.req<unknown>(`/v1/threads/${thread}/approvals`, { method: "POST", body: JSON.stringify({ run_id, request_id, allowed }) });

  stop = (id: string) => this.req<unknown>(`/v1/threads/${id}/stop`, { method: "POST", body: "{}" });

  events = (id: string, after = 0) => this.req<ArbEvent[]>(`/v1/threads/${id}/events?after=${after}`);

  send = (id: string, text: string, attachments: string[] = []) =>

    this.req<ArbEvent>(`/v1/threads/${id}/messages`, { method: "POST", body: JSON.stringify({ text, attachments }) });

  upload = (file: File, signal: AbortSignal) => this.req<Attachment>(`/v1/attachments?name=${encodeURIComponent(file.name)}`, {

    method: "POST", headers: { "Content-Type": "application/octet-stream" }, body: file, signal,

  });

  private async blob(path: string, signal?: AbortSignal): Promise<Blob> {

    const res = await fetch(this.conn.base_url + path, { headers: { Authorization: `Bearer ${this.conn.token}` }, signal: signal ?? AbortSignal.timeout(30_000) });

    if (!res.ok) { const b = await res.json().catch(() => ({})); throw new Error(b.error ?? `Request failed (${res.status})`); }

    return res.blob();

  }

  attachment = (id: string) => this.blob(`/v1/attachments/${id}`);

  preview = (id: string) => this.req<PreviewStatus>(`/v1/threads/${id}/preview`);

  startPreview = (id: string) => this.req<PreviewStatus>(`/v1/threads/${id}/preview/start`, { method: "POST", body: "{}" });

  previewCommand = (id: string) => this.req<{ detected: string | null; custom: string | null; effective: string | null }>(`/v1/threads/${id}/preview/command`);
  setPreviewCommand = (id: string, dev: string | null) => this.req<{ detected: string | null; custom: string | null; effective: string | null }>(`/v1/threads/${id}/preview/command`, { method: "PUT", body: JSON.stringify({ dev }) });
  stopPreview = (id: string) => this.req<{ stopped: boolean }>(`/v1/threads/${id}/preview/stop`, { method: "POST", body: "{}" });

  vault = (id:string) => this.req<VaultState>(`/v1/threads/${id}/vault`);
  saveNote = (id:string,note:VaultNote,base_revision:number) => this.req<VaultNote>(`/v1/threads/${id}/vault`,{method:"POST",body:JSON.stringify({note,base_revision})});
  resolveNote = (thread:string,id:string,accepted:boolean) => this.req(`/v1/threads/${thread}/vault/resolve`,{method:"POST",body:JSON.stringify({id,accepted})});
  rebuildVault = (id:string) => this.req(`/v1/threads/${id}/vault/rebuild`,{method:"POST",body:"{}"});
  learning = (id:string) => this.req<LearningState>(`/v1/threads/${id}/learning`);
  routingPreference = (id:string,task_type:string,harness:string|null,model:string|null,enabled:boolean) => this.req(`/v1/threads/${id}/learning`,{method:"POST",body:JSON.stringify({task_type,harness,model,enabled})});
  markRework = (id:string,rework:boolean) => this.req(`/v1/threads/${id}/learning/rework`,{method:"POST",body:JSON.stringify({rework})});
  resizePreview = (id: string, viewport: Viewport) => this.req<Viewport>(`/v1/threads/${id}/preview/viewport`, {method:"POST", body:JSON.stringify(viewport)});
  captures = (id: string) => this.req<PreviewCapture[]>(`/v1/threads/${id}/preview/captures`);
  capturePreview = (id: string) => this.req<PreviewCapture>(`/v1/threads/${id}/preview/captures`, {method:"POST",body:"{}"});
  previewImage = async (id: string, signal: AbortSignal): Promise<PreviewImage> => {
    const res = await fetch(`${this.conn.base_url}/v1/threads/${id}/preview/frame`, {headers:{Authorization:`Bearer ${this.conn.token}`},signal});
    if (!res.ok) { const error = await res.json().catch(()=>({})); throw new Error(error.error??`Preview failed (${res.status})`); }
    return {blob:await res.blob(),route:JSON.parse(res.headers.get("x-preview-route") || '"/"'),width:Number(res.headers.get("x-preview-width"))||1280,height:Number(res.headers.get("x-preview-height"))||800};
  };
  previewFrame = (id: string, signal: AbortSignal) => this.blob(`/v1/threads/${id}/preview/frame`, signal);

  browser = <T,>(id: string, body: Record<string, unknown>) => this.req<T>(`/v1/threads/${id}/browser`, { method: "POST", body: JSON.stringify(body) });

  fork = (id: string, at_seq: number) =>

    this.req<Thread>(`/v1/threads/${id}/fork`, { method: "POST", body: JSON.stringify({ at_seq }) });

  /** All changes, or between two checkpoints (0 = the thread's base). */

  diff = (id: string, range?: { from: number; to: number }) =>

    this.req<string>(`/v1/threads/${id}/diff${range ? `?from=${range.from}&to=${range.to}` : ""}`, { text: true });

  checkpoints = (id: string) => this.req<CheckpointInfo[]>(`/v1/threads/${id}/checkpoints`);

  revert = (id: string, n: number) =>

    this.req<Thread>(`/v1/threads/${id}/revert`, { method: "POST", body: JSON.stringify({ n }) });

  settle = (id: string) => this.req<Thread>(`/v1/threads/${id}/settle`, { method: "POST", body: "{}" });

  /** Live event stream; reconnects with backoff. Returns an unsubscribe fn. */

  subscribe(onEvent: (e: ArbEvent | { type: "lagged" | "tasks_changed" }) => void, onState?: (live: boolean) => void): () => void {

    let ws: WebSocket | undefined;

    let closed = false;

    let delay = 250;

    const open = () => {

      const url = this.conn.base_url.replace(/^http/, "ws") + `/v1/ws?token=${this.conn.token}`;

      ws = new WebSocket(url);

      ws.onopen = () => {

        delay = 250;

        onState?.(true);

      };

      ws.onmessage = (m) => onEvent(JSON.parse(m.data));

      ws.onclose = () => {

        onState?.(false);

        if (!closed) setTimeout(open, (delay = Math.min(delay * 2, 5000)));

      };

    };

    open();

    return () => {

      closed = true;

      ws?.close();

    };

  }

}

export type LocalCapabilityTask = { name: string; passed: boolean; steps?: number; duration_ms: number; reason?: string };
export type LocalCapability = { model: string; probe_version: number; passed: boolean; score: number; of: number; duration_ms: number; at: number; tasks: LocalCapabilityTask[]; current?: boolean };
export type LocalCapabilityReport = { probe_version: number; tasks: string[]; results: LocalCapability[] };

export type ReviewComment = { path: string; line: number; text: string };
export type LandingStatus = { branch: string; head: string; status: string; fingerprint: string; clean: boolean; committed?: boolean; ahead?: number; untracked?: string[]; draft?: { message: string; title: string; body: string } };
export type CompareCandidate = { harness: "claude" | "codex" | "local"; model?: string | null };
export type Comparison = { id: string; candidates: { thread: string; label: string; kept: boolean; harness: string; model: string | null; status: string; input_tokens: number; output_tokens: number; cost_usd: number; checks: { passed: number; total: number } | null; changes: string; working: boolean }[] };

/** `pr`: push one commit and open a draft PR; `push`: push only; `local`: a local branch. */
export type PublishMode = "pr" | "push" | "local";
export type PublishDraft = { mode: PublishMode; base: string; prefix: string; branch: string; message: string; written_by_local_model: boolean; title: string; body: string; remote: boolean; gh: boolean; previous: { mode: PublishMode; branch: string; url: string | null } | null; fingerprint: string; committed: boolean };

export type MemoryStatus = { path: string; remote: string | null; unpushed: number | null; dirty: boolean; last_commit: string | null; last_push: number | null; gh: boolean; guidance: boolean; committed_in_projects: { project: string; files: string[] }[]; pushed?: boolean };
