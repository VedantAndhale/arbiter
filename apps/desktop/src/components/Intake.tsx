import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { ArbEvent, Assessment, EventKind, LocalCapability, Question } from "../api";
import { Button, Dialog, Input } from "./ui";

export function pendingCards(events: ArbEvent[]) {
  let questions: Extract<EventKind, { type: "questions_asked" }> | null = null;
  const approvals = new Map<string, Extract<EventKind, { type: "approval_requested" }>>();
  for (const { kind: k } of events) {
    if (k.type === "questions_asked") questions = k;
    if (k.type === "questions_answered" && questions?.id === k.id) questions = null;
    if (k.type === "intent_ready") questions = null;
    if (k.type === "approval_requested") approvals.set(`${k.run_id}:${k.request_id}`, k);
    if (k.type === "approval_resolved") approvals.delete(`${k.run_id}:${k.request_id}`);
  }
  return { questions, approvals: [...approvals.values()] };
}

export function IntakeCards({ threadId, events }: { threadId: string; events: ArbEvent[] }) {
  const { questions, approvals } = pendingCards(events);
  const assessment = [...events].reverse().find(e => e.kind.type === "intake_assessed");
  return <>
    {questions && assessment?.kind.type === "intake_assessed" && <ClassificationEditor key={assessment.seq} threadId={threadId} assessment={assessment.kind.assessment} />}
    {questions && <QuestionCard key={questions.id} threadId={threadId} card={questions} moreAvailable={events.filter(e => e.kind.type === "questions_answered").length === 0} />}
    {approvals.map(card => <ApprovalCard key={`${card.run_id}:${card.request_id}`} threadId={threadId} card={card} />)}
  </>;
}

function ClassificationEditor({ threadId, assessment }: { threadId: string; assessment: Assessment }) {
  const api = useApi(), qc = useQueryClient();
  const [type, setType] = useState(assessment.task_type), [size, setSize] = useState(assessment.size);
  const [ambiguous, setAmbiguous] = useState(assessment.ambiguity >= 0.6);
  const save = useMutation({ mutationFn: () => api.correctIntake(threadId, type, size, ambiguous), onSuccess: () => qc.invalidateQueries({ queryKey: ["events", threadId] }) });
  return <details className="rounded border border-line p-2 text-[12px]">
    <summary className="cursor-pointer text-dim">Adjust intake classification</summary>
    <p className="my-2 text-dim">Corrections train your local classifier. They do not remove flagged risks.</p>
    <div className="flex flex-wrap gap-2">
      <select aria-label="Task type" value={type} disabled={save.isPending} onChange={e => setType(e.target.value)} className="rounded border border-line bg-bg p-1">{["bugfix","feature","refactor","test","docs","research","ops"].map(t => <option key={t}>{t}</option>)}</select>
      <select aria-label="Task size" value={size} disabled={save.isPending} onChange={e => setSize(e.target.value)} className="rounded border border-line bg-bg p-1">{["S","M","L"].map(s => <option key={s}>{s}</option>)}</select>
      <label className="flex items-center gap-1"><input type="checkbox" disabled={save.isPending} checked={ambiguous} onChange={e => setAmbiguous(e.target.checked)} />Needs clarification</label>
      <Button disabled={save.isPending} onClick={() => save.mutate()}>Save correction</Button>
    </div>{save.error && <p role="alert" className="mt-2 text-bad">{save.error.message}</p>}
  </details>;
}

function QuestionCard({ threadId, card, moreAvailable }: { threadId: string; card: Extract<EventKind, { type: "questions_asked" }>; moreAvailable: boolean }) {
  const api = useApi(), qc = useQueryClient();
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const submit = useMutation({
    mutationFn: (more: boolean) => api.answerIntake(threadId, card.id, card.questions.map(q => ({ question_id: q.id, text: answers[q.id]?.trim() ?? "" })), more),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["events", threadId] }); qc.invalidateQueries({ queryKey: ["threads"] }); },
  });
  const ready = card.questions.every(q => answers[q.id]?.trim());
  return <section aria-label="Clarify this task" className="rounded-lg border border-accent/50 bg-panel p-4">
    <h2 className="font-semibold">A few details before starting</h2>
    <p className="mt-1 whitespace-pre-wrap text-[12px] text-dim">{card.request}</p>
    <form noValidate onSubmit={e => { e.preventDefault(); if (ready && !submit.isPending) submit.mutate(false); }} className="mt-4 space-y-4">
      {card.questions.map(q => <QuestionField key={q.id} question={q} value={answers[q.id] ?? ""} disabled={submit.isPending} onChange={value => setAnswers(a => ({ ...a, [q.id]: value }))} />)}
      {submit.error && <p role="alert" className="text-bad">{submit.error.message}</p>}
      <div className="flex flex-wrap gap-2">
        <Button type="submit" variant="primary" disabled={!ready || submit.isPending}>{submit.isPending ? "Saving answers…" : "Continue with these answers"}</Button>
        {moreAvailable && <Button type="button" disabled={!ready || submit.isPending} onClick={() => submit.mutate(true)}>Ask me more</Button>}
      </div>
    </form>
  </section>;
}

function QuestionField({ question: q, value, onChange, disabled }: { question: Question; value: string; onChange: (v: string) => void; disabled: boolean }) {
  return <fieldset disabled={disabled} className="space-y-2">
    <legend className="mb-2 text-[13px] font-medium">{q.question}</legend>
    {q.options.map((option, index) => <label key={index} className="flex items-start gap-2 rounded border border-line p-2 text-[12px]">
      <input type={q.kind === "multi" ? "checkbox" : "radio"} name={q.id} checked={q.kind === "multi" ? value.split("; ").includes(option.label) : value === option.label}
        onChange={e => onChange(q.kind === "multi" ? (e.target.checked ? [...value.split("; ").filter(Boolean), option.label] : value.split("; ").filter(v => v !== option.label)).join("; ") : option.label)} />
      <span>{option.label}{q.recommended === index && " (Recommended)"}<span className="block text-dim">{option.description}</span></span>
    </label>)}
    <textarea aria-label={q.question} maxLength={1500} rows={2} value={value} onChange={e => onChange(e.target.value)} className="w-full resize-none rounded border border-line bg-bg p-2 outline-none focus:border-accent" placeholder={q.kind === "short" ? "Your answer…" : "Or write your own answer…"} />
  </fieldset>;
}

function ApprovalCard({ threadId, card }: { threadId: string; card: Extract<EventKind, { type: "approval_requested" }> }) {
  const api = useApi(), qc = useQueryClient();
  const decide = useMutation({ mutationFn: (allowed: boolean) => api.approveTool(threadId, card.run_id, card.request_id, allowed),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ["events", threadId] }); qc.invalidateQueries({ queryKey: ["threads"] }); } });
  return <section aria-label="Tool approval" className="rounded-lg border border-warn/50 bg-panel p-4">
    <h2 className="font-semibold">Allow {card.tool}?</h2><p className="mt-1 text-[12px] text-dim">The agent is waiting. This decision applies to this request only.</p>
    <pre className="my-3 max-h-64 overflow-auto whitespace-pre-wrap break-all rounded bg-bg p-2 font-mono text-[12px]">{JSON.stringify(card.input, null, 2)}</pre>
    <div className="flex gap-2"><Button disabled={decide.isPending} onClick={() => decide.mutate(false)}>Deny</Button><Button variant="primary" disabled={decide.isPending} onClick={() => decide.mutate(true)}>Allow once</Button></div>
    {decide.error && <p role="alert" className="mt-2 text-bad">{decide.error.message}</p>}
  </section>;
}

export function LocalModelSettings() {
  const [open, setOpen] = useState(false);
  return <><Button onClick={() => setOpen(true)}>Local models</Button>{open && <ModelDialog onClose={() => setOpen(false)} />}</>;
}

export function ModelDialog({ onClose }: { onClose: () => void }) {
  const api = useApi(), qc = useQueryClient();
  const [license, setLicense] = useState(false);
  const cancel = useMutation({mutationFn:api.cancelModel,onSuccess:()=>qc.invalidateQueries({queryKey:["local-models"]})});
  const models = useQuery({ queryKey: ["local-models"], queryFn: () => api.localModels(), refetchInterval: 2000 });
  const capability = useQuery({ queryKey: ["local-capability"], queryFn: api.localCapability });
  const setup = useQuery({ queryKey: ["setup"], queryFn: api.setup });
  // The best fit for this whole machine: measured speed first, then CPU,
  // memory and disk. Quality order: Granite, then LFM.
  const hw = setup.data?.hardware;
  const gb = (b?: number | null) => (b ?? 0) / 2 ** 30;
  const sizeGb = (id: string) => gb(models.data?.models.find(m => m.model.id === id)?.model.files.reduce((n, f) => n + f.bytes, 0));
  const fits = (id: string) => gb(hw?.memory_bytes) >= sizeGb(id) * 3 + 4 && gb(hw?.free_disk_bytes) >= sizeGb(id) * 2;
  const order = ["granite", "lfm"];
  const measured = order.map(id => models.data?.models.find(m => m.model.id === id)).filter(m => m?.benchmark);
  const smooth = measured.find(m => m!.benchmark!.meets_target);
  const pick = smooth?.model.id
    ?? (measured.length ? [...measured].sort((a, b) => a!.benchmark!.complete_ms - b!.benchmark!.complete_ms)[0]!.model.id
    : order.find(id => fits(id) && (id !== "granite" || (hw?.logical_cpus ?? 0) >= 8)) ?? "lfm");
  const reason = smooth
    ? `measured smooth here (${Math.round(smooth.benchmark!.complete_ms)} ms)`
    : measured.length ? "fastest measured here; none met the speed target"
    : `likely smooth on ${hw?.logical_cpus ?? "?"} threads and ${Math.round(gb(hw?.memory_bytes))} GB · run the benchmark to confirm`;
  const why = (id: string) => id === "potion" ? "Needed: sorts tasks instantly" : id === pick ? `Recommended: ${reason}` : null;
  const check = useMutation({ mutationFn: api.checkLocalCapability, onSettled: () => qc.invalidateQueries({ queryKey: ["local-capability"] }) });
  const action = useMutation({ mutationFn: ({ id, op }: { id: string; op: "install" | "benchmark" | "select" }) => op === "install" ? api.installModel(id, license) : op === "benchmark" ? api.benchmarkModel(id) : api.selectLocalModel(id), onSuccess: () => qc.invalidateQueries({ queryKey: ["local-models"] }) });
  return <Dialog title="Local models" onClose={onClose}>
    <div className="mb-3 flex justify-end"><Button onClick={onClose}>Close model setup</Button></div>
    <p className="mb-3 text-[12px] text-dim">Models run on your CPU. Setup verifies each download and benchmarks it. Until ready, intake uses standard rules and clarification cards. The coding check runs {capability.data?.tasks.length ?? 3} small tasks in a scratch folder; only a model that passes all of them is used for coding.</p>
    {models.isPending && <p role="status">Loading model status…</p>}
    {models.error && <p role="alert" className="text-bad">{models.error.message} <Button onClick={() => models.refetch()}>Retry</Button></p>}
    <div className="space-y-3">{models.data?.models.map(({ model: m, ready, progress, benchmark }) => {
      const busy = progress?.phase === "downloading" || progress?.phase === "cancelling" || progress?.phase === "benchmarking";
      return <section key={m.id} className="rounded-lg border border-line p-3">
        <h3 className="flex flex-wrap items-center gap-2 font-medium">{m.name}{models.data?.selected === m.id && <span className="text-[12px] text-dim">Selected</span>}{why(m.id) && <span className={`rounded-full px-2 py-0.5 text-[11px] font-normal ${m.id === "potion" ? "bg-raised text-dim" : "bg-accent/15 text-accent"}`}>{m.id === "potion" ? "" : "★ "}{why(m.id)}</span>}</h3>
        <p className="text-[12px] text-dim">{Math.round(m.files.reduce((n,f) => n+f.bytes, 0)/1_000_000)} MB · {m.license}</p>
        {m.id === "lfm" && !ready && <label className="my-2 flex items-start gap-2 text-[12px]"><input type="checkbox" checked={license} onChange={e => setLicense(e.target.checked)} /><span>I have reviewed and can use the <a className="text-accent underline" href={`https://huggingface.co/${m.repository}/blob/main/LICENSE`} target="_blank" rel="noreferrer">LFM license</a>.</span></label>}
        {progress?.phase === "downloading" && <div className="my-2" role="status"><progress aria-label={`${m.name} download`} className="w-full" max={progress.total || 1} value={progress.downloaded} /><p className="text-[12px] text-dim">{Math.round(progress.downloaded/1_000_000)} / {Math.round(progress.total/1_000_000)} MB</p></div>}
        {progress?.phase === "benchmarking" && <p role="status" className="my-2 text-dim">Benchmarking on this computer…</p>}
        {progress?.phase === "downloading" && <Button disabled={cancel.isPending} onClick={()=>cancel.mutate(m.id)}>Cancel download</Button>}
        {progress?.phase === "cancelling" && <p role="status">Cancelling download…</p>}
        {benchmark && <p className={`my-2 text-[12px] ${benchmark.meets_target ? "text-ok" : "text-warn"}`}>Warm completion: {Math.round(benchmark.complete_ms)} ms · target {benchmark.target_ms} ms · {benchmark.meets_target ? "target met" : "target not met"}</p>}
        {progress?.error && <p role="alert" className="my-2 text-[12px] text-bad">{progress.error}</p>}
        <div className="mt-2 flex flex-wrap gap-2"><Button disabled={busy || action.isPending || (!ready && m.id === "lfm" && !license)} onClick={() => action.mutate({ id: m.id, op: ready ? "benchmark" : "install" })}>{busy ? "Working…" : ready ? "Run benchmark" : "Download and set up"}</Button>
          {ready && m.id !== "potion" && models.data?.selected !== m.id && <Button disabled={busy || action.isPending} onClick={() => action.mutate({ id: m.id, op: "select" })}>Use for questions</Button>}
          {ready && m.id !== "potion" && <Button disabled={busy || check.isPending} onClick={() => check.mutate(m.id)}>{check.isPending && check.variables === m.id ? "Checking coding…" : "Check coding ability"}</Button>}</div>
        {ready && m.id !== "potion" && <CodingResult result={capability.data?.results.find(r => r.model === m.id)} />}
        {check.error && check.variables === m.id && <p role="alert" className="mt-2 text-[12px] text-bad">{check.error.message}</p>}
      </section>;
    })}</div>
    {action.error && <p role="alert" className="mt-3 text-bad">{action.error.message}</p>}
    {cancel.error && <p role="alert" className="mt-3 text-bad">{cancel.error.message}</p>}
    {models.data?.last_error && <p className="mt-3 text-[12px] text-warn">{models.data.last_error}</p>}
  </Dialog>;
}

function CodingResult({ result }: { result?: LocalCapability }) {
  if (!result) return <p className="mt-2 text-[12px] text-dim">Coding not checked yet. Until it passes, this model is not used for coding.</p>;
  const stale = result.current === false;
  return <details className="mt-2 text-[12px]">
    <summary className={`cursor-pointer ${result.passed && !stale ? "text-ok" : "text-warn"}`}>
      {stale ? "Coding check is out of date; run it again" : result.passed ? `Passed the coding check · ${result.score}/${result.of} tasks` : `Not trusted for coding · ${result.score}/${result.of} tasks passed`} · {(result.duration_ms / 1000).toFixed(1)}s · {new Date(result.at * 1000).toLocaleString()}
    </summary>
    <ul className="mt-1 space-y-1">{result.tasks.map(t => <li key={t.name} className={t.passed ? "text-ok" : "text-warn"}>{t.passed ? "✓" : "✗"} {t.name}{t.steps !== undefined && ` · ${t.steps} steps`}{t.reason && <span className="text-dim">: {t.reason}</span>}</li>)}</ul>
  </details>;
}

export function ContextPicker({ projectId, paths, onChange }: { projectId: string; paths: string[]; onChange: (paths: string[]) => void }) {
  const api = useApi();
  const [open, setOpen] = useState(false), [search, setSearch] = useState("");
  const files = useQuery({ queryKey: ["project-files", projectId], queryFn: () => api.projectFiles(projectId), enabled: open && !!projectId });
  return <div className="px-3 pb-2">
    <div className="flex flex-wrap items-center gap-1"><Button onClick={() => setOpen(true)}>@ Add context</Button>{paths.map(p => <button type="button" key={p} onClick={() => onChange(paths.filter(x => x !== p))} title={`Remove ${p}`} className="max-w-full truncate rounded border border-line px-2 py-1 text-[12px] text-dim">@{p} ×</button>)}</div>
    {open && <Dialog title="Mention project files" onClose={() => setOpen(false)}>
      <Input autoFocus aria-label="Find project files" value={search} onChange={e => setSearch(e.target.value)} placeholder="Search by path…" />
      <p className="my-2 text-[12px] text-dim">Up to 8 paths. The agent reads file content only when needed.</p>
      {files.isPending && <p role="status">Loading files…</p>}
      {files.error && <p role="alert" className="text-bad">{files.error.message} <Button onClick={() => files.refetch()}>Retry</Button></p>}
      <div className="max-h-64 overflow-y-auto">{files.data?.filter(p => p.toLowerCase().includes(search.toLowerCase())).slice(0,50).map(p => <button type="button" disabled={paths.includes(p) || paths.length >= 8} key={p} className="block w-full truncate rounded px-2 py-1.5 text-left font-mono text-[12px] hover:bg-raised disabled:opacity-40" onClick={() => { onChange([...paths, p]); setOpen(false); }}>{p}</button>)}</div>
      {files.data && !files.data.some(p => p.toLowerCase().includes(search.toLowerCase())) && <p className="py-3 text-dim">No matching files.</p>}
    </Dialog>}
  </div>;
}
