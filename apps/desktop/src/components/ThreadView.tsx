import { type ReactNode, Suspense, lazy, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { AgentEvent, ArbEvent, CheckResult, RunConfig, Thread } from "../api";
import { AccessControls, ModelPicker, ToolProfileControl, handleConfigKeys, useHarnesses } from "./RunControls";
import { IntakeCards, LocalModelSettings, pendingCards } from "./Intake";
import { PlanView } from "./PlanView";
import { Button, Dialog, Input, StatusChip, fmtTokens } from "./ui";
import { DiffView } from "./DiffView";
import { VaultButton } from "./Vault";
import { Preview } from "./Preview";
import { AttachButton, AttachmentChips, AttachmentLinks, useAttachments, useDropFiles } from "./Attachments";
import { useMentions } from "./Mentions";
import { useMediaQuery } from "../useMediaQuery";
// Loaded on first use: the terminal and editor are large and optional.
const TerminalDrawer = lazy(() => import("./TerminalDrawer").then(m => ({ default: m.TerminalDrawer })));
const FileEditor = lazy(() => import("./FileEditor").then(m => ({ default: m.FileEditor })));

export function ThreadView({ thread, onSelect }: { thread: Thread; onSelect: (id: string) => void }) {
  const api = useApi();
  const plan = useQuery({queryKey:["plan",thread.id],queryFn:()=>api.plan(thread.id),refetchInterval:2000,enabled:!thread.plan_root});
  const events = useQuery({ queryKey: ["events", thread.id], queryFn: () => api.events(thread.id), staleTime: Infinity });
  // The conversation stays in view; App and Changes open beside it.
  const [panel, setPanel] = useState<null | "app" | "changes">(null);
  const [changesView, setChangesView] = useState<"diff" | "files">("diff");
  const [editPath, setEditPath] = useState<string | null>(null);
  const [terminal, setTerminal] = useState(false);
  // Ctrl+J shows or hides the terminal, as in t3code and VS Code.
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "j") { e.preventDefault(); setTerminal(t => !t); } };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  const [picked, setPicked] = useState<string | null>(null);
  const [turn, setTurn] = useState<number | null>(null);
  const [advanced,setAdvanced]=useState(false);
  const wide = useMediaQuery("(min-width: 1100px)");
  const appReady = (events.data ?? []).some(e => e.kind.type === "preview_ready");
  const seen = useRef(appReady);
  // Open the app beside the conversation the first time it starts.
  useEffect(() => { if (appReady && !seen.current) { seen.current = true; setPanel(p => p ?? "app"); } }, [appReady]);
  const showTurn = (n: number) => { setTurn(n); setPanel("changes"); };
  const toggle = (p: "app" | "changes") => setPanel(cur => (cur === p ? null : p));
  if (plan.data?.plan) return <PlanView thread={thread} state={plan.data} onOpen={onSelect}/>;
  const side = panel && <div className={wide ? "flex min-h-0 w-[48%] shrink-0 flex-col border-l border-line" : "absolute inset-0 z-20 flex min-h-0 flex-col bg-bg"}>
    <div className="flex h-9 shrink-0 items-center gap-2 border-b border-line px-3 text-[12px]">
      {panel === "app" ? <span className="font-medium">Your app</span> : <div role="radiogroup" aria-label="Changes view" className="inline-flex rounded-md border border-line p-0.5">
        {(["diff", "files"] as const).map(v => <button key={v} type="button" role="radio" aria-checked={changesView === v} onClick={() => setChangesView(v)} className={`rounded px-2 py-0.5 ${changesView === v ? "bg-raised text-fg" : "text-dim hover:text-fg"}`}>{v === "diff" ? "What changed" : "All files"}</button>)}
      </div>}
      <span className="flex-1" />
      <Button variant="ghost" aria-label="Close panel" onClick={() => setPanel(null)}>✕</Button>
    </div>
    {panel === "changes" ? (changesView === "diff" ? <DiffView thread={thread} turn={turn} onTurn={setTurn} onEdit={p => { setEditPath(p); setChangesView("files"); }} /> : <Suspense fallback={<p role="status" className="p-3 text-dim">Loading editor…</p>}><FileEditor threadId={thread.id} initialPath={editPath} /></Suspense>) : <Preview threadId={thread.id} onAttach={text => { setPicked(text); if (!wide) setPanel(null); }} />}
  </div>;
  return (
    <div className="flex h-full flex-col">
      <header className="flex min-h-11 shrink-0 flex-wrap items-center gap-2 border-b border-line px-4 py-2">
        {thread.plan_root && <Button onClick={()=>onSelect(thread.plan_root!)}>← Plan</Button>}
        <div className="min-w-0 flex-1 truncate font-medium" title={thread.branch ? "Works in its own copy of the project" : "Works in your project folder"}>{thread.title}</div>
        <StatusChip status={thread.status} />
        <Button variant={panel === "app" ? "primary" : "default"} aria-pressed={panel === "app"} onClick={() => toggle("app")}>App</Button>
        <Button variant={panel === "changes" ? "primary" : "default"} aria-pressed={panel === "changes"} onClick={() => { setTurn(null); toggle("changes"); }}>Changes</Button>
        {thread.worktree && <Button variant={terminal ? "primary" : "default"} aria-pressed={terminal} title="Terminal (Ctrl+J)" onClick={() => setTerminal(t => !t)}>Terminal</Button>}
        <Button variant="ghost" onClick={()=>setAdvanced(true)}>Options</Button>
        {advanced && <TaskOptions thread={thread} onClose={()=>setAdvanced(false)} onMention={text=>{setPicked(text);setAdvanced(false);}} />}
      </header>
      <div className="relative flex min-h-0 flex-1">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          <Activity thread={thread} onSelect={onSelect} onShowTurn={showTurn} onOpenChanges={() => { setTurn(null); setChangesView("diff"); setPanel("changes"); }} onOpenPreview={() => setPanel("app")} picked={picked} onPicked={() => setPicked(null)} />
          {terminal && thread.worktree && <Suspense fallback={<p role="status" className="border-t border-line p-3 text-dim">Loading terminal…</p>}><TerminalDrawer threadId={thread.id} onClose={() => setTerminal(false)} /></Suspense>}
        </div>
        {side}
      </div>
    </div>
  );
}

/** Side-by-side view of an opt-in comparison this task belongs to. */
/** A summary from a signed-in site looked private: the agent waits for this answer. */
function ShareCards({ events }: { events: ArbEvent[] }) {
  const api = useApi(), qc = useQueryClient();
  const decide = useMutation({ mutationFn: ({ id, allow }: { id: string; allow: boolean }) => api.decideShare(id, allow), onSettled: () => qc.invalidateQueries({ queryKey: ["events"] }) });
  const decided = new Set(events.flatMap(e => (e.kind.type === "share_decided" ? [e.kind.id] : [])));
  const open = events.flatMap(e => (e.kind.type === "share_requested" && !decided.has(e.kind.id) ? [e.kind] : []));
  if (!open.length) return null;
  return <>{open.map(k => <section key={k.id} aria-label="Share permission" className="rounded-lg border border-warn/40 bg-warn/5 p-3 text-[13px]">
    <p className="font-medium">Share what was found on {k.host}?</p>
    <p className="mt-1 text-dim">The agent asked: {k.question}</p>
    <p className="mt-1 text-[12px] text-dim">You are signed in there, and a local check found it may be private: {k.reasons.join("; ")}. Nothing has been sent to the agent yet.</p>
    {decide.error && <p role="alert" className="mt-1 text-[12px] text-bad">{decide.error.message}</p>}
    <div className="mt-2 flex gap-2">
      <Button variant="primary" disabled={decide.isPending} onClick={() => decide.mutate({ id: k.id, allow: true })}>Allow sharing</Button>
      <Button disabled={decide.isPending} onClick={() => decide.mutate({ id: k.id, allow: false })}>Keep private</Button>
    </div>
  </section>)}</>;
}

function ComparisonCard({ events, onOpen, current }: { events: ArbEvent[]; onOpen: (id: string) => void; current: string }) {
  const api = useApi(), qc = useQueryClient();
  const id = events.map(e => e.kind).find((k): k is Extract<typeof k, { type: "comparison_joined" }> => k.type === "comparison_joined")?.comparison;
  const view = useQuery({ queryKey: ["comparison", id], queryFn: () => api.comparison(id!), enabled: !!id, refetchInterval: q => q.state.data?.candidates.some(c => c.working || c.status === "running") ? 3000 : false });
  const keep = useMutation({ mutationFn: (thread: string) => api.keepCandidate(id!, thread), onSuccess: (v) => { qc.setQueryData(["comparison", id], v); qc.invalidateQueries({ queryKey: ["threads"] }); } });
  if (!id) return null;
  const decided = view.data?.candidates.some(c => c.kept);
  return <section aria-label="Comparison" className="rounded-lg border border-line bg-panel p-3 text-[12px]">
    <p className="font-medium">Comparison</p>
    {view.error && <p role="alert" className="text-bad">{view.error.message}</p>}
    <div className="mt-2 overflow-x-auto"><table className="w-full text-left">
      <thead className="text-dim"><tr><th className="pr-3">Agent</th><th className="pr-3">Status</th><th className="pr-3">Checks</th><th className="pr-3">Changes</th><th className="pr-3">Tokens</th><th /></tr></thead>
      <tbody>{view.data?.candidates.map(c => <tr key={c.thread} className={c.thread === current ? "text-fg" : "text-dim"}>
        <td className="pr-3">{c.label} · {c.harness}{c.model ? ` · ${c.model}` : ""}{c.kept && <span className="text-ok"> · kept</span>}</td>
        <td className="pr-3">{c.status.replace("_", " ")}</td>
        <td className="pr-3">{c.checks ? `${c.checks.passed}/${c.checks.total} passed` : "not run"}</td>
        <td className="pr-3">{c.changes || "none yet"}</td>
        <td className="pr-3">{c.harness === "local" ? "local" : `${fmtTokens(c.input_tokens)} / ${fmtTokens(c.output_tokens)}`}</td>
        <td className="whitespace-nowrap">{c.thread !== current && <Button onClick={() => onOpen(c.thread)}>Open</Button>}{!decided && <Button disabled={c.working || keep.isPending} onClick={() => keep.mutate(c.thread)}>Keep</Button>}</td>
      </tr>)}</tbody>
    </table></div>
    {!decided && <p className="mt-2 text-dim">Keeping one stops the others and marks them done; their worktrees stay on disk.</p>}
    {keep.error && <p role="alert" className="mt-2 text-bad">{keep.error.message}</p>}
  </section>;
}

/** Everything that is not needed to follow the task: agent choice, access,
 *  budget, knowledge notes, local models and side questions. */
function TaskOptions({ thread, onClose, onMention }: { thread: Thread; onClose: () => void; onMention: (text: string) => void }) {
  const api = useApi(), qc = useQueryClient();
  const config = useMutation({ mutationFn: (c: RunConfig) => api.patchThread(thread.id, c), onSuccess: () => qc.invalidateQueries({ queryKey: ["threads"] }) });
  const end = useMutation({ mutationFn: () => api.stop(thread.id) });
  const cfg: RunConfig = { harness: thread.harness, model: thread.model, effort: thread.effort, permission: thread.permission, tool_profile: thread.tool_profile };
  const locked = thread.session_id && thread.harness !== "auto" ? thread.harness : undefined;
  return <Dialog title="Task options" onClose={onClose}>
    <div className="space-y-4">
      <section className="space-y-2"><h3 className="font-medium">Agent</h3>
        <div className="flex flex-wrap items-center gap-1"><ModelPicker value={cfg} onChange={c => config.mutate(c)} lockedHarness={locked} /><AccessControls value={cfg} onChange={c => config.mutate(c)} /><ToolProfileControl value={cfg} onChange={c => config.mutate(c)} /></div>
        {config.error && <p role="alert" className="text-[12px] text-bad">{config.error.message}</p>}
      </section>
      <section className="space-y-2"><h3 className="font-medium">Usage</h3>
        <p className="text-[12px] text-dim">{fmtTokens(thread.input_tokens)} in / {fmtTokens(thread.output_tokens)} out tokens</p>
        <BudgetButton thread={thread} />
      </section>
      <section className="flex flex-wrap gap-2"><SideQuestion thread={thread} /><VaultButton threadId={thread.id} onMention={onMention} /><LocalModelSettings />
        {thread.status !== "running" && <Button disabled={end.isPending} onClick={() => end.mutate()} title="Ends the agent process; the session can be resumed by sending a message">End agent process</Button>}
      </section>
      {end.error && <p role="alert" className="text-[12px] text-bad">{end.error.message}</p>}
      <div className="flex justify-end"><Button onClick={onClose}>Done</Button></div>
    </div>
  </Dialog>;
}

/** Ask about the task without interrupting or steering its agent. */
function SideQuestion({ thread }: { thread: Thread }) {
  const api = useApi();
  const [open, setOpen] = useState(false), [question, setQuestion] = useState("");
  const ask = useMutation({ mutationFn: () => api.sideQuestion(thread.id, question.trim()), onSuccess: () => setQuestion("") });
  return <>
    <Button onClick={() => setOpen(true)}>Ask aside</Button>
    {open && <Dialog title="Ask aside" onClose={() => !ask.isPending && setOpen(false)}>
      <p className="mb-3 text-[12px] text-dim">A local model answers from a short summary of this task. The agent is not interrupted and never sees the question.</p>
      <form onSubmit={e => { e.preventDefault(); if (question.trim()) ask.mutate(); }} className="space-y-2">
        <Input autoFocus aria-label="Side question" value={question} maxLength={500} onChange={e => setQuestion(e.target.value)} placeholder="For example, what has changed so far?" disabled={ask.isPending} />
        <div className="flex justify-end gap-2"><Button type="button" disabled={ask.isPending} onClick={() => setOpen(false)}>Close</Button><Button type="submit" variant="primary" disabled={!question.trim() || ask.isPending}>{ask.isPending ? "Thinking locally…" : "Ask"}</Button></div>
      </form>
      {ask.data && <p role="status" className="mt-3 whitespace-pre-wrap rounded border border-line p-2">{ask.data.answer}</p>}
      {ask.error && <p role="alert" className="mt-3 text-[12px] text-bad">{ask.error.message}</p>}
    </Dialog>}
  </>;
}

/** Spend so far, and the cap that pauses the agent for approval. */
function BudgetButton({ thread }: { thread: Thread }) {
  const api = useApi();
  const qc = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const [invalid, setInvalid] = useState("");
  const set = useMutation({
    mutationFn: (usd: number | null) => api.patchThread(thread.id, { budget_usd: usd }),
    onSuccess: () => { setEditing(false); qc.invalidateQueries({ queryKey: ["threads"] }); },
  });
  const over = thread.budget_usd != null && thread.cost_usd >= thread.budget_usd;
  return (
    <><button
      className={`rounded px-1.5 py-0.5 font-mono text-[11px] hover:bg-raised ${over ? "text-warn" : "text-faint"}`}
      title="Reported-cost diagnostics, not remaining subscription allowance. Thresholds can overshoot; Codex price is unavailable."
      onClick={() => {
        setValue(thread.budget_usd?.toString() ?? ""); setInvalid(""); setEditing(true);
      }}
    >
      {thread.harness === "codex" ? "Cost unavailable" : `Reported $${thread.cost_usd.toFixed(2)}`}
      {thread.budget_usd != null && ` / $${thread.budget_usd.toFixed(2)}`}
    </button>
    {editing && <Dialog title="Thread budget" onClose={() => !set.isPending && setEditing(false)}>
      <p className="mb-3 text-dim">This threshold uses reported cost, not subscription allowance. Missing prices are not free usage. In-flight work can overshoot.</p><label htmlFor="budget-usd" className="mb-2 block text-dim">Budget in USD (empty means no cap)</label>
      <Input id="budget-usd" inputMode="decimal" value={value} onChange={e => setValue(e.target.value)} aria-invalid={!!invalid} aria-describedby="budget-error" />
      <p id="budget-error" role="alert" className="min-h-6 text-[12px] text-bad">{invalid || set.error?.message}</p>
      <div className="flex justify-end gap-2"><Button autoFocus disabled={set.isPending} onClick={() => setEditing(false)}>Cancel</Button><Button variant="primary" disabled={set.isPending} onClick={() => { const n = value.trim() ? Number(value) : null; if(n !== null && (!Number.isFinite(n) || n <= 0)) setInvalid("Enter a positive amount, or leave it empty."); else set.mutate(n); }}>Save budget</Button></div>
    </Dialog>}</>
  );
}

type Row =
  | { kind: "event"; e: ArbEvent }
  | { kind: "tool"; e: ArbEvent; call: Extract<AgentEvent, { type: "tool_call" }>; result?: Extract<AgentEvent, { type: "tool_result" }> };

/** Pair each tool call with its result so the feed reads as one row per action. */
function toRows(events: ArbEvent[]): Row[] {
  const rows: Row[] = [];
  const byCall = new Map<string, Extract<Row, { kind: "tool" }>>();
  for (const e of events) {
    const k = e.kind;
    const silent =
      k.type === "session" ||
      k.type === "renamed" ||
      k.type === "budget_set" ||
      k.type === "settled" ||
      (k.type === "status_changed" && k.status === "healing") ||
      (k.type === "status_changed" && k.status === "running") ||
      (k.type === "agent" && (k.event.type === "usage" || k.event.type === "done"));
    if (silent) continue;
    if (k.type === "agent" && k.event.type === "tool_call") {
      const row = { kind: "tool" as const, e, call: k.event };
      byCall.set(k.event.id, row);
      rows.push(row);
    } else if (k.type === "agent" && k.event.type === "tool_result" && byCall.has(k.event.id)) {
      byCall.get(k.event.id)!.result = k.event;
    } else {
      rows.push({ kind: "event", e });
    }
  }
  return rows;
}

type Group = { kind: "steps"; first: number; rows: Extract<Row, { kind: "tool" }>[]; failed: number } | { kind: "row"; row: Row };

/** Consecutive tool steps fold into one "N steps" line unless shown. */
function groupSteps(rows: Row[]): Group[] {
  const out: Group[] = [];
  for (const r of rows) {
    const last = out.at(-1);
    if (r.kind === "tool") {
      if (last?.kind === "steps") { last.rows.push(r); if (r.result?.is_error) last.failed++; }
      else out.push({ kind: "steps", first: r.e.seq, rows: [r], failed: r.result?.is_error ? 1 : 0 });
    } else out.push({ kind: "row", row: r });
  }
  return out;
}

/** What is happening, in plain words, from the latest events. */
function describeStep(name: string, input: unknown): string {
  const path = typeof input === "object" && input && "path" in input ? String((input as { path?: unknown }).path ?? "") : typeof input === "object" && input && "file_path" in input ? String((input as { file_path?: unknown }).file_path ?? "") : "";
  const file = path ? path.split(/[\\/]/).pop() : "";
  const n = name.toLowerCase();
  if (["read", "view"].includes(n)) return file ? `Reading ${file}` : "Reading the code";
  if (["edit", "write", "multiedit", "apply_patch", "patch"].includes(n)) return file ? `Editing ${file}` : "Making changes";
  if (["grep", "glob", "search", "list", "ls"].includes(n)) return "Looking through the code";
  if (["bash", "shell", "exec", "command"].includes(n)) return "Running a command";
  if (n === "check") return "Running checks";
  if (n.startsWith("web")) return "Looking things up online";
  if (n.startsWith("browser")) return "Checking the app in a browser";
  if (n === "documentation") return "Reading documentation";
  return "Working on it";
}

function ProgressLine({ thread, events, showSteps, onToggleSteps }: { thread: Thread; events: ArbEvent[]; showSteps: boolean; onToggleSteps: () => void }) {
  const kinds = events.map(e => e.kind);
  const lastTool = [...kinds].reverse().find((k): k is Extract<typeof k, { type: "agent" }> => k.type === "agent" && k.event.type === "tool_call");
  const checks = [...kinds].reverse().find((k): k is Extract<typeof k, { type: "checks_ran" }> => k.type === "checks_ran");
  const heals = kinds.filter(k => k.type === "heal").length;
  const preview = kinds.some(k => k.type === "preview_ready");
  const notice = [...kinds].reverse().find((k): k is Extract<typeof k, { type: "notice" }> => k.type === "notice");
  const passed = checks ? checks.results.filter(r => r.ok).length : 0;
  const text = thread.status === "running"
    ? lastTool && lastTool.event.type === "tool_call" ? `${describeStep(lastTool.event.name, lastTool.event.input)}…` : "Working on it…"
    : thread.status === "healing" ? `The checks found problems. Fixing them${heals ? ` (attempt ${heals})` : ""}…`
    : thread.status === "needs_approval" ? "Waiting for you. Answer below to continue."
    : thread.status === "review" ? ["Done.", checks ? (passed === checks.results.length ? `All ${checks.results.length} checks passed.` : `${checks.results.length - passed} of ${checks.results.length} checks still fail.`) : "", preview ? "Your app is running." : ""].filter(Boolean).join(" ")
    : thread.status === "failed" ? `Stopped${notice ? `: ${notice.text.slice(0, 140)}${notice.text.length > 140 ? "…" : ""}` : "."}`
    : "";
  const hasSteps = kinds.some(k => k.type === "agent" && k.event.type === "tool_call");
  if (!text && !hasSteps) return null;
  return <div className="sticky top-0 z-10 -mx-1 flex items-center gap-3 rounded-lg bg-bg/90 px-1 py-1.5 backdrop-blur">
    <span role="status" className="min-w-0 flex-1 text-[13px]">{text}</span>
    {hasSteps && <button type="button" className="shrink-0 text-[12px] text-dim hover:text-fg" onClick={onToggleSteps}>{showSteps ? "Hide steps" : "Show steps"}</button>}
  </div>;
}

function Activity({
  thread,
  onSelect,
  onShowTurn,
  onOpenChanges,
  onOpenPreview,
  picked,
  onPicked,
}: {
  thread: Thread;
  onSelect: (id: string) => void;
  onShowTurn: (n: number) => void;
  onOpenChanges: () => void;
  onOpenPreview: () => void;
  picked: string | null;
  onPicked: () => void;
}) {
  const api = useApi();
  const qc = useQueryClient();
  const events = useQuery({ queryKey: ["events", thread.id], queryFn: () => api.events(thread.id), staleTime: Infinity });
  const [revertTo, setRevertTo] = useState<number | null>(null);
  const rows = useMemo(() => toRows(events.data ?? []), [events.data]);
  const [showSteps, setShowSteps] = useState(() => { try { return localStorage.getItem("arbiter.showSteps") === "1"; } catch { return false; } });
  const [openGroups, setOpenGroups] = useState<Set<number>>(new Set());
  const toggleSteps = () => setShowSteps(v => { try { localStorage.setItem("arbiter.showSteps", v ? "0" : "1"); } catch { /* storage unavailable */ } return !v; });
  const grouped = useMemo(() => groupSteps(rows), [rows]);
  const pending = pendingCards(events.data ?? []);
  const blocked = !!pending.questions || pending.approvals.length > 0;
  const fork = useMutation({
    mutationFn: (seq: number) => api.fork(thread.id, seq),
    onSuccess: (t) => {
      qc.invalidateQueries({ queryKey: ["threads"] });
      onSelect(t.id);
    },
  });
  const revert = useMutation({
    mutationFn: (n: number) => api.revert(thread.id, n),
    onSuccess: () => { setRevertTo(null); qc.invalidateQueries({ queryKey: ["diff", thread.id] }); },
  });
  const busy = thread.status === "running" || thread.status === "healing";
  const actions: RowActions = {
    showTurn: onShowTurn,
    revert: setRevertTo,
    canRevert: !busy,
  };
  const bottom = useRef<HTMLDivElement>(null);
  // Braces matter: newer Chromium returns a Promise from scrollIntoView, which
  // React would treat as a (broken) cleanup function.
  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [events.data?.length]);

  return (
    <>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <div className="mx-auto max-w-3xl space-y-2">
          <ProgressLine thread={thread} events={events.data ?? []} showSteps={showSteps} onToggleSteps={toggleSteps} />
          {grouped.map((g) => g.kind === "steps" && !showSteps && !openGroups.has(g.first) ? (
            <button key={`steps-${g.first}`} type="button" onClick={() => setOpenGroups(s => new Set(s).add(g.first))}
              className="flex items-center gap-2 px-1 text-[12px] text-faint hover:text-dim">
              <span aria-hidden>▸</span>{g.rows.length} {g.rows.length === 1 ? "step" : "steps"}{g.failed ? ` · ${g.failed} had a problem` : ""}
            </button>
          ) : (g.kind === "steps" ? g.rows : [g.row]).map((r) => (
            <Forkable key={r.e.seq} seq={r.e.seq} onFork={() => fork.mutate(r.e.seq)}>
              {r.kind === "tool" ? <ToolRow call={r.call} result={r.result} /> : <EventRow e={r.e} actions={actions} />}
            </Forkable>
          )))}
          {revert.error && <div className="text-[12px] text-bad">{revert.error.message}</div>}
          {thread.status === "running" && <Thinking />}
          {thread.status === "healing" && <Thinking label="Running checks…" />}
          <IntakeCards threadId={thread.id} events={events.data ?? []} />
          <ComparisonCard events={events.data ?? []} onOpen={onSelect} current={thread.id} />
          <ShareCards events={events.data ?? []} />
          {!busy && !blocked && <NextStep thread={thread} events={events.data ?? []} onOpenChanges={onOpenChanges} onOpenPreview={onOpenPreview} />}
          <div ref={bottom} />
        </div>
      </div>
      <Composer thread={thread} picked={picked} onPicked={onPicked} blocked={blocked} awaitingTool={pending.approvals.length > 0} />
      {revertTo !== null && <Dialog title={`Restore checkpoint ${revertTo}?`} onClose={() => !revert.isPending && setRevertTo(null)}>
        <p className="mb-3 text-dim">Restore this thread’s worktree files. The current state is checkpointed first so this can be undone.</p>
        {revert.error && <p role="alert" className="mb-2 text-bad">{revert.error.message}</p>}
        <div className="flex justify-end gap-2"><Button autoFocus disabled={revert.isPending} onClick={() => setRevertTo(null)}>Cancel</Button><Button disabled={revert.isPending || busy} onClick={() => revert.mutate(revertTo)}>Restore files</Button></div>
      </Dialog>}
    </>
  );
}

/** After an agent finishes, propose the one obvious next action instead of
 *  leaving the user to find it: commit, fix, hand off or publish. Consequential
 *  steps still need the click; publishing keeps its own approval. */
function NextStep({ thread, events, onOpenChanges, onOpenPreview }: { thread: Thread; events: ArbEvent[]; onOpenChanges: () => void; onOpenPreview: () => void }) {
  const api = useApi(), qc = useQueryClient();
  const harnesses = useHarnesses();
  const ran = events.some(e => e.kind.type === "run_started");
  const done = ran && ["review", "failed", "idle"].includes(thread.status);
  const checks = [...events].reverse().map(e => e.kind).find((k): k is Extract<typeof k, { type: "checks_ran" }> => k.type === "checks_ran");
  const failing = thread.status === "failed" || !!checks?.results.some(r => !r.ok);
  const landing = useQuery({ queryKey: ["landing", thread.id], queryFn: () => api.landing(thread.id), enabled: done && !failing && !!thread.worktree, staleTime: 0, retry: false });
  const refresh = () => { qc.invalidateQueries({ queryKey: ["landing", thread.id] }); qc.invalidateQueries({ queryKey: ["diff", thread.id] }); };
  const commit = useMutation({ mutationFn: () => api.commit(thread.id, landing.data!.fingerprint, landing.data!.draft?.message || thread.title, []), onSuccess: refresh });
  const retry = useMutation({ mutationFn: () => api.send(thread.id, "Checks still fail. Fix the remaining failures, then run the checks again.", []) });
  const escalate = useMutation({ mutationFn: (h: "claude" | "codex") => api.escalate(thread.id, h), onSuccess: () => qc.invalidateQueries({ queryKey: ["threads"] }) });
  if (!done) return null;
  const cloud = thread.harness === "local" ? (harnesses.data ?? []).filter(h => (h.id === "claude" || h.id === "codex") && h.installed) : [];
  const handOff = cloud.length > 0 && <div className="mt-2 flex flex-wrap items-center gap-2 text-[12px] text-dim"><span>Or hand it to a cloud agent (uses your allowance):</span>{cloud.map(h => <Button key={h.id} disabled={escalate.isPending} onClick={() => escalate.mutate(h.id as "claude" | "codex")}>{escalate.isPending && escalate.variables === h.id ? "Handing off…" : h.name}</Button>)}</div>;
  const error = commit.error ?? retry.error ?? escalate.error;
  const running = events.some(e => e.kind.type === "preview_ready");
  const seeApp = running && <Button onClick={onOpenPreview}>See your app</Button>;
  const card = (title: string, detail: string, actions: ReactNode) => <section aria-label="Next step" className="rounded-lg border border-accent/40 bg-accent/5 p-3">
    <p className="font-medium">{title}</p>
    {detail && <p className="mt-1 text-[12px] text-dim">{detail}</p>}
    <div className="mt-2 flex flex-wrap gap-2">{actions}</div>
    {handOff}
    {error && <p role="alert" className="mt-2 text-[12px] text-bad">{error.message}</p>}
  </section>;
  if (failing) {
    return card(thread.status === "failed" ? "The agent stopped with a problem" : "Checks still fail", "Arbiter already tried automatic fixes. Ask the agent to try again, or look at the changes yourself.",
      <><Button variant="primary" disabled={retry.isPending} onClick={() => retry.mutate()}>{retry.isPending ? "Asking…" : "Try again"}</Button><Button onClick={onOpenChanges}>See changes</Button></>);
  }
  if (!thread.worktree || landing.isPending || landing.error || !landing.data) return null;
  const l = landing.data, fresh = (l.untracked ?? []).length;
  if (!l.clean && fresh > 0) {
    return card("Changes are ready", `${fresh} new file${fresh === 1 ? "" : "s"} need a quick look before committing, so nothing sensitive slips in.`,
      <><Button variant="primary" onClick={onOpenChanges}>Review changes</Button>{seeApp}</>);
  }
  if (!l.committed) {
    return card(checks ? "Checks passed. Commit these changes?" : "Changes are ready. Commit them?", `Commit message: "${l.draft?.message || thread.title}". Nothing is pushed.`,
      <><Button variant="primary" disabled={commit.isPending} onClick={() => commit.mutate()}>{commit.isPending ? "Committing…" : "Commit"}</Button>{seeApp}<Button onClick={onOpenChanges}>Review first</Button></>);
  }
  if ((l.ahead ?? 0) > 0) {
    return card("Ready to publish", "Publishing turns this task into one commit on a normal branch, with a message written on your computer. You approve any push.",
      <><Button onClick={onOpenChanges}>Open Changes</Button>{seeApp}</>);
  }
  return null;
}

function Forkable({ seq, onFork, children }: { seq: number; onFork: () => void; children: ReactNode }) {
  if (!children) return null;
  return (
    <div className="group relative">
      {children}
      <button
        onClick={onFork}
        title="Fork a new thread from this point"
        className="absolute top-0 -right-14 hidden rounded px-1.5 text-[10px] text-faint group-hover:block hover:bg-raised hover:text-fg"
      >
        fork #{seq}
      </button>
    </div>
  );
}

function ToolRow({
  call,
  result,
}: {
  call: Extract<AgentEvent, { type: "tool_call" }>;
  result?: Extract<AgentEvent, { type: "tool_result" }>;
}) {
  const [open, setOpen] = useState(false);
  const summary = toolSummary(call.input);
  const state = !result ? "text-accent animate-pulse" : result.is_error ? "text-bad" : "text-ok";
  return (
    <div className="rounded-md border border-line bg-panel/50">
      <button onClick={() => setOpen((o) => !o)} className="flex w-full items-center gap-2 px-2 py-1 text-left font-mono text-[11px]">
        <span className={state}>{!result ? "●" : result.is_error ? "✗" : "✓"}</span>
        <span className="text-fg">{call.name}</span>
        <span className="min-w-0 flex-1 truncate text-faint">{summary}</span>
        <span className="text-faint">{open ? "▾" : "▸"}</span>
      </button>
      {open && (
        <div className="space-y-1 border-t border-line px-2 py-1.5">
          <pre className="max-h-40 overflow-auto font-mono text-[11px] whitespace-pre-wrap text-dim">
            {JSON.stringify(call.input, null, 2)}
          </pre>
          {result && (
            <pre className={`max-h-64 overflow-auto rounded bg-bg p-1.5 font-mono text-[11px] whitespace-pre-wrap ${result.is_error ? "text-bad" : "text-faint"}`}>
              {result.output || "(no output)"}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

/** One-line human summary of the most common tool inputs. */
function toolSummary(input: unknown): string {
  const i = (input ?? {}) as Record<string, unknown>;
  const pick = i.command ?? i.file_path ?? i.path ?? i.pattern ?? i.query ?? i.url ?? i.description;
  if (typeof pick === "string") return pick;
  if (Array.isArray(i.files)) return (i.files as string[]).join(", ");
  const s = JSON.stringify(input);
  return s === "{}" ? "" : s;
}

interface RowActions {
  showTurn: (n: number) => void;
  revert: (n: number) => void;
  canRevert: boolean;
}

function EventRow({ e, actions }: { e: ArbEvent; actions: RowActions }) {
  const k = e.kind;
  switch (k.type) {
    case "preview_ready":
      return <div className="flex items-center gap-2 rounded-lg border border-ok/30 bg-ok/5 px-3 py-2 text-[13px]"><span className="text-ok">●</span><span className="flex-1">Your app is running.</span><span className="font-mono text-[11px] text-faint">{k.url}</span></div>;
    case "share_requested":
      return null;
    case "share_decided":
      return <p className="text-[12px] text-dim">{k.allowed ? "You allowed the agent to see a summary from a signed-in site." : "You kept a summary from a signed-in site private."}</p>;
    case "side_chat":
      return <div className="rounded-lg border border-dashed border-line px-3 py-2 text-[12px]" aria-label="Side question">
        <p className="text-dim">Side question · answered locally by {k.model} · not sent to the agent</p>
        <p className="mt-1 font-medium">{k.question}</p>
        <p className="mt-1 whitespace-pre-wrap">{k.answer}</p>
      </div>;
    case "second_opinion":
      return <section className="space-y-2 rounded-lg border border-line bg-panel p-3"><h3 className="font-medium">Independent local review · {k.stage}</h3><p>{k.report.summary}</p><p className="text-[12px] text-dim">{k.model} · {k.report.recommendation}</p>{k.report.issues.map((issue,i)=><div key={i} className="text-[12px]"><p>{issue.finding}</p>{issue.source && <a className="break-all text-accent underline" href={issue.source} target="_blank" rel="noreferrer">Source excerpt: {issue.quote}</a>}</div>)}<p className="text-[12px] text-dim">{k.report.limitations}</p></section>;
    case "memory_used":
      return <Meta>Project memory: {k.notes.length} notes · {k.bytes} bytes <span className="break-all">{k.notes.join(", ")}</span></Meta>;
    case "vault_proposed":
      return <Meta>Decision awaiting review: {k.note.title}. Open Project memory → Proposals.</Meta>;
    case "vault_resolved":
      return <Meta>Decision proposal {k.accepted?"accepted":"rejected"}.</Meta>;
    case "checks_ran":
      return <ChecksRow results={k.results} attempt={k.attempt} />;
    case "checkpoint":
      return (
        <div className="flex items-center gap-2 px-1 text-[11px] text-faint">
          <span title={k.commit}>⎌ Checkpoint {k.n}</span>
          <button className="rounded px-1 hover:bg-raised hover:text-fg" onClick={() => actions.showTurn(k.n)}>
            view changes
          </button>
          <button
            className="rounded px-1 hover:bg-raised hover:text-fg disabled:opacity-40"
            disabled={!actions.canRevert}
            title={actions.canRevert ? "Restore the files to this point" : "Interrupt the agent first"}
            onClick={() => actions.revert(k.n)}
          >
            revert to here
          </button>
        </div>
      );
    case "reverted":
      return <Meta>Files restored to {k.n === 0 ? "the starting point" : `checkpoint ${k.n}`}</Meta>;
    case "rate_limited":
      return (
        <div className="rounded border border-warn/40 bg-warn/5 px-2 py-1 text-[12px] text-warn">
          Rate limited{k.resets_at ? `; resuming automatically at ${new Date(k.resets_at * 1000).toLocaleTimeString()}` : ""}.
          <span className="ml-1 text-faint">{k.message}</span>
        </div>
      );
    case "budget_set":
    case "settled":
    case "questions_asked":
    case "approval_requested":
      return null;
    case "tool_profile_changed":
      return k.profile === "auto" ? null : <Meta>Tools → {k.profile}</Meta>;
    case "intake_assessed":
      return <Meta>Intake: {k.assessment.task_type} · {k.assessment.size} · {k.assessment.elapsed_ms.toFixed(1)} ms <span title={k.assessment.engine}>({k.assessment.engine.startsWith("rules") ? "standard rules" : "local model"})</span>{k.assessment.risks.length > 0 && ` · Review: ${k.assessment.risks.join(", ")}`}</Meta>;
    case "questions_answered":
      return <details className="rounded border border-line p-2 text-[12px]"><summary className="cursor-pointer text-dim">Clarification answers saved</summary>{k.answers.map((a,i) => <p className="mt-1 whitespace-pre-wrap" key={i}>{a.question_id}: {a.text}</p>)}</details>;
    case "intent_ready":
      return <details className="rounded border border-line p-2 text-[12px]"><summary className="cursor-pointer text-dim">Intent spec · {k.spec.task_type}{k.spec.needs_frontier && " · planning recommended"}</summary><p className="mt-2 whitespace-pre-wrap">{k.spec.request}</p>{k.spec.context_paths.map(p => <p key={p} className="mt-1 font-mono text-dim">@{p}</p>)}</details>;
    case "approval_resolved":
      return <Meta>Tool {k.allowed ? "allowed once" : "denied or expired"} · {k.reason}</Meta>;
    case "user_message":
      return <div className="rounded-lg bg-raised px-3 py-2 whitespace-pre-wrap">{k.text}{!!k.attachments?.length && <AttachmentLinks items={k.attachments} />}</div>;
    case "agent":
      switch (k.event.type) {
        case "message":
          return <div className="px-1 leading-relaxed whitespace-pre-wrap">{k.event.text}</div>;
        case "tool_result": // orphan result (call not in view)
          return (
            <pre className={`max-h-40 overflow-auto rounded bg-bg px-2 py-1 font-mono text-[11px] ${k.event.is_error ? "text-bad" : "text-faint"}`}>
              {k.event.output}
            </pre>
          );
        case "error":
          return <div className="rounded border border-bad/40 bg-bad/5 px-2 py-1 text-[12px] text-bad">{k.event.message}</div>;
        default:
          return null;
      }
    case "notice":
      return <div className="rounded border border-warn/40 bg-warn/5 px-2 py-1 text-[12px] text-warn">{k.text}</div>;
    case "heal":
      return <HealRow attempt={k.attempt} excerpt={k.excerpt} />;
    case "thread_created":
      return <Meta>Thread created on {k.branch ?? "main checkout"}{k.parent && ` · forked at #${k.parent[1]}`}</Meta>;
    case "status_changed":
      return k.status === "running" ? null : <Meta>Status → {k.status.replace("_", " ")}</Meta>;
    case "run_started":
      return <Meta>Agent started</Meta>;
    case "run_ended":
      return <Meta>Agent process exited{k.exit_code != null && ` (code ${k.exit_code})`}</Meta>;
    case "config_changed":
      return (
        <Meta>
          {k.reason ?? `Settings → ${[k.harness, k.model ?? "default model", k.effort, k.permission].filter(Boolean).join(" · ")}`}
        </Meta>
      );
    case "session":
    case "renamed":
      return null;
  }
}

function Meta({ children }: { children: ReactNode }) {
  return <div className="px-1 text-[11px] text-faint">{children}</div>;
}

function ChecksRow({ results, attempt }: { results: CheckResult[]; attempt: number }) {
  const ok = results.every((r) => r.ok);
  return (
    <div className={`flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md border px-2 py-1 text-[11px] ${ok ? "border-ok/30 bg-ok/5" : "border-bad/30 bg-bad/5"}`}>
      <span className={ok ? "text-ok" : "text-bad"}>{ok ? "✓ Checks passed" : attempt === 0 ? "✗ Checks failed" : `✗ Still failing (after fix ${attempt})`}</span>
      {results.map((r) => (
        <span key={r.name} className="font-mono text-faint" title={r.timed_out ? "timed out" : undefined}>
          <span className={r.ok ? "text-ok" : "text-bad"}>{r.ok ? "✓" : "✗"}</span> {r.name}
          {!r.ok && r.failures > 0 && ` (${r.failures})`}
          {r.fixed && " · auto-fixed"}
          {r.timed_out && " · timeout"} · {(r.duration_ms / 1000).toFixed(1)}s
        </span>
      ))}
    </div>
  );
}

/** What the agent was told to fix: collapsed by default, it can be long. */
function HealRow({ attempt, excerpt }: { attempt: number; excerpt: string }) {
  const [open, setOpen] = useState(false);
  const tokens = Math.round(excerpt.length / 4);
  return (
    <div className="rounded-md border border-warn/40 bg-warn/5 text-[12px]">
      <button onClick={() => setOpen((o) => !o)} className="flex w-full items-center gap-2 px-2 py-1 text-left">
        <span className="text-warn">⟲ Fixing problems (attempt {attempt})</span>
        <span className="flex-1 text-faint">sent what failed back to the agent (~{tokens} tokens)</span>
        <span className="text-faint">{open ? "▾" : "▸"}</span>
      </button>
      {open && (
        <pre className="max-h-72 overflow-auto border-t border-warn/20 px-2 py-1.5 font-mono text-[11px] whitespace-pre-wrap text-dim">
          {excerpt}
        </pre>
      )}
    </div>
  );
}

function Thinking({ label = "Working…" }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 px-1 text-[12px] text-faint">
      <span className="size-1.5 animate-pulse rounded-full bg-accent" />
      {label}
    </div>
  );
}

function Composer({ thread, picked, onPicked, blocked, awaitingTool }: { thread: Thread; picked: string | null; onPicked: () => void; blocked: boolean; awaitingTool: boolean }) {
  const api = useApi();
  const qc = useQueryClient();
  const [text, setText] = useState("");
  const uploads = useAttachments();
  const drop = useDropFiles(uploads);
  const input = useRef<HTMLTextAreaElement>(null);
  const mentions = useMentions({ text, setText, input, projectId: thread.project_id, threadId: thread.id });
  useEffect(() => {
    if (picked) { setText(t => `${t}${t ? "\n\n" : ""}${picked}`); onPicked(); input.current?.focus(); }
  }, [picked, onPicked]);
  const send = useMutation({ mutationFn: (t: string) => api.send(thread.id, t, uploads.ids), onSuccess: () => { setText(""); uploads.clear(); } });
  const interrupt = useMutation({ mutationFn: () => api.interrupt(thread.id) });
  const config = useMutation({
    mutationFn: (c: RunConfig) => api.patchThread(thread.id, c),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["threads"] }),
  });
  const cfg: RunConfig = { harness: thread.harness, model: thread.model, effort: thread.effort, permission: thread.permission, tool_profile: thread.tool_profile };
  const running = thread.status === "running";
  const submit = () => text.trim() && !blocked && !send.isPending && !uploads.blocked && send.mutate(text.trim());
  const error = send.error ?? interrupt.error ?? config.error;

  return (
    <div className="shrink-0 border-t border-line p-3">
      <div {...drop.handlers} className={`relative mx-auto max-w-3xl rounded-xl border bg-panel transition-[border-color,box-shadow] focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/25 ${drop.drag ? "border-accent" : "border-line"}`}>
        {mentions.menu}
        <textarea
          ref={input}
          aria-label="Message the agent"
          disabled={send.isPending || blocked}
          rows={Math.min(8, text.split("\n").length)}
          className="block min-h-6 w-full resize-none bg-transparent px-3.5 pt-2.5 pb-1 outline-none placeholder:text-faint focus-visible:outline-none"
          placeholder={running ? "Steer the agent…  (Enter to send, Esc to stop)" : "Message the agent…  (@ to mention a file, Enter to send)"}
          value={text}
          onChange={(e) => { setText(e.target.value); mentions.track(); }}
          onPaste={(e) => { if (e.clipboardData.files.length) { e.preventDefault(); uploads.add(e.clipboardData.files); } }}
          onSelect={mentions.track}
          onClick={mentions.track}
          onKeyUp={mentions.track}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (mentions.onKeyDown(e)) return;
            if (handleConfigKeys(e, cfg, (c) => config.mutate(c))) return;
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            } else if (e.key === "Escape" && running) {
              interrupt.mutate();
            }
          }}
        />
        <AttachmentChips uploads={uploads} disabled={send.isPending} />
        <div className="flex flex-wrap items-center gap-1 px-2 pb-2">
          <AttachButton uploads={uploads} disabled={send.isPending} />
          <div className="flex-1" />
          {drop.drag && <span className="text-[12px] text-accent">Drop to attach</span>}
          {(running || awaitingTool) && (
            <Button variant="ghost" onClick={() => interrupt.mutate()} title="Stop the current turn (Esc). Send a message to continue.">
              Stop
            </Button>
          )}
          <Button variant="primary" onClick={submit} disabled={!text.trim() || blocked || send.isPending || uploads.blocked}>
            {running ? "Steer" : "Send"}
          </Button>
        </div>
      </div>
      {error && <div className="mx-auto mt-1 max-w-3xl text-[11px] text-bad">{error.message}</div>}
      {blocked && <p className="mx-auto mt-1 max-w-3xl text-[11px] text-dim">Resolve the card above to continue.</p>}
    </div>
  );
}
