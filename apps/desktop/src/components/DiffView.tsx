import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { LandingStatus, PublishMode, ReviewComment, Thread } from "../api";
import { Button, Input } from "./ui";

interface DiffLine {
  text: string;
  /** Line number in the new file, for commenting; null for removed lines and hunk headers. */
  line: number | null;
}

interface FileDiff {
  path: string;
  lines: DiffLine[];
  added: number;
  removed: number;
}

const MAX_COMMENTS = 8;
const COMMENT_CHARS = 500;

function parse(diff: string): FileDiff[] {
  const files: FileDiff[] = [];
  let next = 0;
  for (const text of diff.split("\n")) {
    if (text.startsWith("diff --git ")) {
      files.push({ path: text.split(" b/").pop() ?? text, lines: [], added: 0, removed: 0 });
      continue;
    }
    const f = files.at(-1);
    // Context lines always carry a leading space; an empty string is only the
    // split artifact after the final newline.
    if (!f || text === "" || /^(index |--- |\+\+\+ |new file|deleted file|similarity|rename )/.test(text)) continue;
    const hunk = /^@@ -\d+(?:,\d+)? \+(\d+)/.exec(text);
    if (hunk) {
      next = Number(hunk[1]);
      f.lines.push({ text, line: null });
    } else if (text.startsWith("-")) {
      f.removed++;
      f.lines.push({ text, line: null });
    } else if (text.startsWith("\\")) {
      f.lines.push({ text, line: null });
    } else {
      if (text.startsWith("+")) f.added++;
      f.lines.push({ text, line: next++ });
    }
  }
  return files;
}

/** Draft comments survive switching tabs; storage may be unavailable. */
function useDraft(threadId: string) {
  const key = `arbiter.review.${threadId}`;
  const [draft, setDraft] = useState<ReviewComment[]>(() => {
    try { return JSON.parse(sessionStorage.getItem(key) ?? "[]"); } catch { return []; }
  });
  useEffect(() => {
    try { sessionStorage.setItem(key, JSON.stringify(draft)); } catch { /* storage unavailable */ }
  }, [key, draft]);
  return [draft, setDraft] as const;
}

/** `turn` = checkpoint number to show on its own (changes made in that turn); null = everything. */
export function DiffView({ thread, turn, onTurn, onEdit }: { thread: Thread; turn: number | null; onTurn: (n: number | null) => void; onEdit?: (path: string) => void }) {
  const api = useApi(), qc = useQueryClient();
  const checkpoints = useQuery({ queryKey: ["checkpoints", thread.id, thread.checkpoints], queryFn: () => api.checkpoints(thread.id) });
  const range = turn != null ? { from: turn - 1, to: turn } : undefined;
  const diff = useQuery({
    queryKey: ["diff", thread.id, turn],
    queryFn: () => api.diff(thread.id, range),
    staleTime: 0,
  });
  const files = diff.data ? parse(diff.data) : [];
  const [draft, setDraft] = useDraft(thread.id);
  const [editing, setEditing] = useState<{ path: string; line: number } | null>(null);
  const [text, setText] = useState("");
  const busy = thread.status === "running" || thread.status === "healing";
  const send = useMutation({
    mutationFn: () => api.review(thread.id, draft),
    onSuccess: () => { setDraft([]); qc.invalidateQueries({ queryKey: ["threads"] }); qc.invalidateQueries({ queryKey: ["plan", thread.id] }); },
  });
  function save() {
    if (!editing || !text.trim()) return;
    setDraft(d => [...d.filter(c => !(c.path === editing.path && c.line === editing.line)), { ...editing, text: text.trim() }]);
    setEditing(null);
    setText("");
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <select
          className="h-7 rounded-md border border-line bg-bg px-1.5 text-[12px]"
          value={turn ?? ""}
          onChange={(e) => onTurn(e.target.value === "" ? null : Number(e.target.value))}
          title="Show all changes, or only what one agent turn changed"
        >
          <option value="">All changes</option>
          {(checkpoints.data ?? []).map((c) => (
            <option key={c.n} value={c.n}>
              Turn {c.n} · {new Date(c.ts).toLocaleTimeString()}
            </option>
          ))}
        </select>
        <span className="text-dim">
          {files.length} file{files.length === 1 ? "" : "s"} changed
        </span>
        <div className="flex-1" />
        <Button onClick={() => diff.refetch()} disabled={diff.isFetching}>
          Refresh
        </Button>
      </div>
      {draft.length > 0 && (
        <section aria-label="Review comments" className="mb-3 rounded-lg border border-line bg-panel p-3">
          <p className="font-medium">{draft.length} review comment{draft.length === 1 ? "" : "s"} ready</p>
          <ul className="mt-2 space-y-1 text-[12px]">
            {draft.map(c => (
              <li key={`${c.path}:${c.line}`} className="flex items-start gap-2">
                <span className="min-w-0 flex-1 break-words"><span className="font-mono text-dim">{c.path}:{c.line}</span> {c.text}</span>
                <button type="button" className="text-dim hover:text-fg" aria-label={`Remove comment on ${c.path} line ${c.line}`} onClick={() => setDraft(d => d.filter(x => x !== c))}>Remove</button>
              </li>
            ))}
          </ul>
          <p className="mt-2 text-[12px] text-dim">Sent together. A finished plan gets one fix step to approve; a single agent gets them as one message.</p>
          <div className="mt-2 flex flex-wrap gap-2">
            <Button variant="primary" disabled={busy || send.isPending || draft.length > MAX_COMMENTS} onClick={() => send.mutate()}>{send.isPending ? "Sending…" : "Send to agent"}</Button>
            <Button disabled={send.isPending} onClick={() => setDraft([])}>Discard all</Button>
          </div>
          {draft.length > MAX_COMMENTS && <p className="mt-2 text-[12px] text-warn">Send at most {MAX_COMMENTS} comments at a time.</p>}
          {busy && <p className="mt-2 text-[12px] text-dim">The agent is working; send when it finishes.</p>}
          {send.error && <p role="alert" className="mt-2 text-[12px] text-bad">{send.error.message}</p>}
        </section>
      )}
      {send.isSuccess && draft.length === 0 && <p role="status" className="mb-3 text-[12px] text-ok">{send.data.kind === "plan" ? "Review fix step added to the plan. Approve it in the plan to run it." : "Comments sent to the agent."}</p>}
      {diff.error && <div className="text-bad">{diff.error.message}</div>}
      {diff.isSuccess && files.length === 0 && <div className="text-faint">No changes in this worktree yet.</div>}
      <div className="space-y-3">
        {files.map((f) => (
          <div key={f.path} className="overflow-hidden rounded-lg border border-line">
            <div className="flex items-center gap-3 border-b border-line bg-panel px-3 py-1.5 font-mono text-[12px]">
              <span className="min-w-0 flex-1 truncate">{f.path}</span>
              {onEdit && <button type="button" className="font-sans text-dim hover:text-fg" onClick={() => onEdit(f.path)}>Edit</button>}
              <span className="text-ok">+{f.added}</span>
              <span className="text-bad">−{f.removed}</span>
            </div>
            <pre className="overflow-x-auto font-mono text-[12px] leading-5">
              {f.lines.map((l, i) => {
                const comment = l.line !== null ? draft.find(c => c.path === f.path && c.line === l.line) : undefined;
                const open = editing && l.line !== null && editing.path === f.path && editing.line === l.line;
                return (
                  <div key={i}>
                    <div className={`group flex ${l.text.startsWith("+") ? "bg-ok/10 text-ok" : l.text.startsWith("-") ? "bg-bad/10 text-bad" : l.text.startsWith("@@") ? "bg-accent/5 text-accent" : "text-dim"}`}>
                      <span className="w-12 shrink-0 select-none pr-2 text-right text-faint">{l.line ?? ""}</span>
                      {l.line !== null ? (
                        <button type="button" aria-label={`Comment on ${f.path} line ${l.line}`} title="Add a review comment" onClick={() => { setEditing({ path: f.path, line: l.line! }); setText(comment?.text ?? ""); }} className={`w-5 shrink-0 rounded text-accent hover:bg-raised ${comment ? "" : "opacity-0 focus:opacity-100 group-hover:opacity-100"}`}>{comment ? "●" : "+"}</button>
                      ) : <span className="w-5 shrink-0" />}
                      <span className="pr-3">{l.text || " "}</span>
                    </div>
                    {open && (
                      <form className="flex flex-wrap items-center gap-2 border-y border-line bg-panel px-3 py-2 font-sans" onSubmit={e => { e.preventDefault(); save(); }}>
                        <Input autoFocus aria-label={`Comment on ${f.path} line ${l.line}`} className="min-w-0 flex-1" value={text} maxLength={COMMENT_CHARS} onChange={e => setText(e.target.value)} onKeyDown={e => { if (e.key === "Escape") setEditing(null); }} placeholder="What should change here?" />
                        <Button type="submit" disabled={!text.trim()}>{comment ? "Update" : "Add comment"}</Button>
                        <Button type="button" onClick={() => setEditing(null)}>Cancel</Button>
                      </form>
                    )}
                  </div>
                );
              })}
            </pre>
          </div>
        ))}
      </div>
      {thread.worktree && <Landing thread={thread} busy={busy} />}
    </div>
  );
}

/** Commit the reviewed changes locally, then optionally push and open a draft PR.
 *  Each step is explicit; the text is drafted from recorded agent work. */
export function Landing({ thread, busy, project, startOpen = false }: { thread: Thread; busy: boolean; project?: string; startOpen?: boolean }) {
  const api = useApi(), qc = useQueryClient();
  const [open, setOpen] = useState(startOpen);
  const status = useQuery({ queryKey: ["landing", thread.id, project ?? ""], queryFn: () => api.landing(thread.id, project), enabled: open && !busy, staleTime: 0 });
  const [message, setMessage] = useState(""), [include, setInclude] = useState<string[]>([]);
  const s = status.data;
  const untracked = s?.untracked ?? [];
  useEffect(() => {
    if (!s) return;
    setMessage(m => m || (s.draft?.message ?? ""));
    setInclude(list => list.filter(p => (s.untracked ?? []).includes(p)));
  }, [s]);
  const refresh = (next: LandingStatus) => { qc.setQueryData(["landing", thread.id, project ?? ""], next); qc.invalidateQueries({ queryKey: ["diff", thread.id] }); qc.invalidateQueries({ queryKey: ["plan-diff", thread.id] }); };
  const commit = useMutation({ mutationFn: () => api.commit(thread.id, s!.fingerprint, message.trim(), include, project), onSuccess: refresh });
  const sensitive = (p: string) => /(^|\/)(\.env[^/]*|credentials|secrets)(\/|$)|\.(pem|key|p12|pfx)$/i.test(p);
  return (
    <section aria-label="Commit and publish" className="mt-4 rounded-lg border border-line bg-panel p-3">
      <button type="button" className="font-medium" aria-expanded={open} onClick={() => setOpen(o => !o)}>{open ? "▾" : "▸"} Commit and publish</button>
      {open && busy && <p className="mt-2 text-[12px] text-dim">Wait for the agent to finish before committing.</p>}
      {open && !busy && <div className="mt-2 space-y-3">
        {status.isPending && <p role="status" className="text-dim">Checking the branch…</p>}
        {status.error && <p role="alert" className="text-[12px] text-bad">{status.error.message} <Button onClick={() => status.refetch()}>Retry</Button></p>}
        {s && <>
          <p className="text-[12px] text-dim">Branch <span className="font-mono">{s.branch}</span> · {s.committed ? "everything is committed on Arbiter's local branch" : "uncommitted changes"}</p>
          {!s.committed && <form className="space-y-2" onSubmit={e => { e.preventDefault(); if (message.trim()) commit.mutate(); }}>
            <label className="block space-y-1"><span>Commit message</span><Input value={message} maxLength={200} onChange={e => setMessage(e.target.value)} /></label>
            {untracked.length > 0 && <fieldset className="space-y-1"><legend className="text-[12px] text-dim">New files are left out unless you tick them. Review them for secrets first.</legend>
              {untracked.map(p => <label key={p} className="flex items-start gap-2"><input type="checkbox" disabled={sensitive(p)} checked={include.includes(p)} onChange={e => setInclude(l => e.target.checked ? [...l, p] : l.filter(x => x !== p))} /><span className="break-all font-mono text-[12px]">{p}{sensitive(p) && <span className="font-sans text-warn"> · looks like a credential; not committable here</span>}</span></label>)}
            </fieldset>}
            <Button type="submit" disabled={!message.trim() || commit.isPending}>{commit.isPending ? "Committing…" : "Commit locally"}</Button>
            {commit.error && <p role="alert" className="text-[12px] text-bad">{commit.error.message}</p>}
          </form>}
          {s.committed && <Publish thread={thread} project={project} fingerprint={s.fingerprint} />}
        </>}
      </div>}
    </section>
  );
}

const MODE_LABEL: Record<PublishMode, string> = { pr: "Push and open a draft pull request", push: "Push the branch only", local: "Keep it on this computer (you push)" };

/** One squashed commit on a normal branch. Arbiter's own branches never leave
 *  this computer; the choice here is remembered for the project. */
function Publish({ thread, project, fingerprint }: { thread: Thread; project?: string; fingerprint: string }) {
  const api = useApi(), qc = useQueryClient();
  const draft = useQuery({ queryKey: ["publish", thread.id, project ?? "", fingerprint], queryFn: () => api.publishDraft(thread.id, project), staleTime: Infinity, retry: false });
  const [mode, setMode] = useState<PublishMode>("pr"), [base, setBase] = useState(""), [branch, setBranch] = useState("");
  const [message, setMessage] = useState(""), [title, setTitle] = useState(""), [body, setBody] = useState(""), [approved, setApproved] = useState(false);
  const d = draft.data;
  useEffect(() => {
    if (!d) return;
    setMode(d.mode); setBase(d.base); setBranch(d.branch); setMessage(d.message); setTitle(d.title); setBody(d.body);
  }, [d]);
  const publish = useMutation({
    mutationFn: () => api.publish(thread.id, { fingerprint, mode, base: base.trim(), branch: branch.trim(), message, title: title.trim(), body, project }),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["publish", thread.id] }); qc.invalidateQueries({ queryKey: ["events", thread.id] }); },
  });
  if (draft.isPending) return <p role="status" className="text-[12px] text-dim">Writing the commit message…</p>;
  if (draft.error || !d) return <p role="alert" className="text-[12px] text-bad">{draft.error?.message} <Button onClick={() => draft.refetch()}>Retry</Button></p>;
  const modes: PublishMode[] = d.remote ? (d.gh ? ["pr", "push", "local"] : ["push", "local"]) : ["local"];
  const remote = mode !== "local";
  const again = d.previous && d.previous.branch === branch && d.previous.mode === mode;
  return <form className="space-y-2" onSubmit={e => { e.preventDefault(); if (!remote || approved) publish.mutate(); }}>
    <p className="text-[12px] text-dim">{again ? `Adds one follow-up commit to ${branch}.` : "All of this task's work becomes one commit on a normal branch. Arbiter's own branches stay on this computer."}</p>
    <fieldset className="space-y-1"><legend className="sr-only">How to publish</legend>
      {modes.map(m => <label key={m} className="flex items-center gap-2"><input type="radio" name={`mode-${project ?? "main"}`} checked={mode === m} onChange={() => setMode(m)} /><span>{MODE_LABEL[m]}</span></label>)}
    </fieldset>
    <div className="flex flex-wrap gap-2">
      <label className="min-w-40 flex-1 space-y-1"><span className="text-[12px] text-dim">Branch</span><Input value={branch} maxLength={120} onChange={e => setBranch(e.target.value)} /></label>
      <label className="w-32 space-y-1"><span className="text-[12px] text-dim">{remote ? "Onto" : "From"}</span><Input value={base} maxLength={120} onChange={e => setBase(e.target.value)} /></label>
    </div>
    <label className="block space-y-1"><span className="text-[12px] text-dim">Commit message{d.written_by_local_model ? " · written by your local model" : ""}</span><textarea className="min-h-24 w-full rounded-md border border-line bg-bg p-2 font-mono text-[12px]" value={message} maxLength={2000} onChange={e => setMessage(e.target.value)} /></label>
    {mode === "pr" && <>
      <label className="block space-y-1"><span className="text-[12px] text-dim">Pull request title</span><Input value={title} maxLength={160} onChange={e => setTitle(e.target.value)} /></label>
      <label className="block space-y-1"><span className="text-[12px] text-dim">Description</span><textarea className="min-h-24 w-full rounded-md border border-line bg-bg p-2 font-mono text-[12px]" value={body} maxLength={5000} onChange={e => setBody(e.target.value)} /></label>
    </>}
    {remote && <label className="flex items-start gap-2"><input type="checkbox" checked={approved} onChange={e => setApproved(e.target.checked)} /><span>I approve pushing one commit to <span className="font-mono">{branch}</span>{mode === "pr" ? " and opening a draft pull request" : ""}.</span></label>}
    <Button type="submit" variant="primary" disabled={(remote && !approved) || !branch.trim() || !base.trim() || !message.trim() || publish.isPending}>{publish.isPending ? "Publishing…" : mode === "local" ? "Save as one commit" : mode === "pr" ? "Push and open draft PR" : "Push one commit"}</Button>
    {publish.data && <p role="status" className="text-[12px] text-ok">{publish.data.url ? <>Draft pull request: <a className="underline" href={publish.data.url} target="_blank" rel="noreferrer">{publish.data.url}</a></> : publish.data.mode === "local" ? `Saved on ${publish.data.branch}. Push it when you are ready.` : `Pushed to ${publish.data.branch}.`} Arbiter cleans up its local copies once this is merged.</p>}
    {publish.error && <p role="alert" className="text-[12px] text-bad">{publish.error.message}</p>}
  </form>;
}
