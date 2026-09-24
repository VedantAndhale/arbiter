import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { CompareCandidate, Project } from "../api";
import { Button } from "./ui";
import { useHarnesses, useRememberedConfig } from "./RunControls";
import { ModelDialog } from "./Intake";
import { AttachmentChips, useAttachments, useDropFiles } from "./Attachments";
import { type Command, useCommands, useMentions } from "./Mentions";

type Approach = "auto" | "plan" | "single";

/**
 * Composer-first new task: describe the outcome and press Start. Arbiter
 * decides the approach and asks, as question cards, only when a choice really
 * matters. Power users steer with `/` commands; nothing else is on screen.
 */
export function Draft({ projects, initialProject, onAddProject, onCreated }: { projects: Project[]; initialProject?: string; onAddProject: () => void; onCreated: (threadId: string) => void }) {
  const api = useApi();
  const qc = useQueryClient();
  // Chosen explicitly (sidebar +, @ or the + menu). The first is the main
  // project; more than one plans the work across them.
  const [picked, setPicked] = useState<string[]>(() => (initialProject && projects.some(p => p.id === initialProject) ? [initialProject] : []));
  const projectId = picked[0] ?? "";
  const addProject = (id: string) => setPicked(list => (list.includes(id) ? list : [...list, id].slice(0, 5)));
  const [askProject, setAskProject] = useState(false);
  const [cfg, setCfg] = useRememberedConfig(projectId);
  const [text, setText] = useState("");
  const [approach, setApproach] = useState<Approach>("auto");
  const [compare, setCompare] = useState(false), [candidates, setCandidates] = useState<CompareCandidate[]>([{ harness: "claude" }, { harness: "codex" }]), [compareConsent, setCompareConsent] = useState(false);
  const [models, setModels] = useState(false);
  const uploads = useAttachments();
  const drop = useDropFiles(uploads);
  const input = useRef<HTMLTextAreaElement>(null);
  const mentions = useMentions({ text, setText, input, projectId: projectId || undefined, projects: projects.filter(p => !picked.includes(p.id)), onProject: id => { addProject(id); setAskProject(false); } });
  const harnesses = useHarnesses();
  const installed = (id: string) => (harnesses.data ?? []).some(h => h.id === id && h.installed);
  const commands: Command[] = [
    { name: "plan", label: "Plan it first", hint: "approve a plan before agents start", run: () => { setApproach("plan"); setCompare(false); } },
    { name: "now", label: "Start one agent now", hint: "skip planning", run: () => setApproach("single") },
    ...(installed("claude") ? [{ name: "claude", label: "Use Claude Code", hint: "instead of automatic choice", run: () => setCfg({ ...cfg, harness: "claude", model: null, effort: null }) }] : []),
    ...(installed("codex") ? [{ name: "codex", label: "Use Codex", hint: "instead of automatic choice", run: () => setCfg({ ...cfg, harness: "codex", model: null, effort: null }) }] : []),
    { name: "compare", label: "Compare agents", hint: "run 2–3 agents, keep the best", run: () => { setCompare(true); setApproach("single"); } },
    { name: "models", label: "Local models", hint: "download and check local models", run: () => setModels(true) },
  ];
  const slash = useCommands({ text, setText, input, commands });
  const paths = mentions.mentioned;
  const comparison = useMutation({
    mutationFn: () => api.startComparison(projectId, text.trim(), candidates),
    onSuccess: (r) => { qc.invalidateQueries({ queryKey: ["threads"] }); if (r.threads[0]) onCreated(r.threads[0]); },
  });
  const create = useMutation({
    mutationFn: () => api.createThread({ project_id: projectId, projects: picked.slice(1), message: text.trim(), worktree: true, attachments: uploads.ids, intake: true, context_paths: paths, workflow: approach === "plan" || picked.length > 1, auto: approach === "auto" && picked.length < 2, ...cfg }),
    onSuccess: (t) => {
      qc.invalidateQueries({ queryKey: ["threads"] });
      onCreated(t.id);
    },
  });
  const busy = create.isPending || comparison.isPending;
  const submit = () => {
    if (!text.trim() || busy || uploads.blocked) return;
    if (!projectId) { setAskProject(true); return; }
    if (compare) { if (compareConsent) comparison.mutate(); } else create.mutate();
  };
  const track = () => { mentions.track(); slash.track(); };
  const chips: { label: string; clear: () => void }[] = [
    ...(approach === "plan" ? [{ label: "Plan first", clear: () => setApproach("auto") }] : []),
    ...(approach === "single" && !compare ? [{ label: "One agent now", clear: () => setApproach("auto") }] : []),
    ...(cfg.harness === "claude" || cfg.harness === "codex" ? [{ label: cfg.harness === "claude" ? "Claude Code" : "Codex", clear: () => setCfg({ ...cfg, harness: "auto", model: null, effort: null }) }] : []),
    ...(compare ? [{ label: "Compare agents", clear: () => { setCompare(false); setApproach("auto"); } }] : []),
  ];

  return (
    <div className="flex h-full flex-col items-center justify-center overflow-y-auto px-6 py-6">
      <div className="w-full max-w-2xl">
        <h1 className="mb-1 text-lg font-semibold">What do you want to build?</h1>
        <p className="mb-4 text-dim">Describe the outcome. Arbiter works out the approach and asks only when a choice is yours to make.</p>
        <div {...drop.handlers} data-tour="composer" className={`relative rounded-xl border bg-panel transition-[border-color,box-shadow] focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/25 ${drop.drag ? "border-accent" : "border-line"}`}>
          {slash.menu ?? mentions.menu}
          <textarea
            ref={input}
            aria-label="Task description"
            disabled={busy}
            autoFocus
            rows={Math.max(3, Math.min(12, text.split("\n").length))}
            className="block w-full resize-none bg-transparent px-3.5 pt-3 pb-1 outline-none placeholder:text-faint focus-visible:outline-none"
            placeholder="What should change?  @ for a project or file · / for commands"
            value={text}
            onChange={(e) => { setText(e.target.value); track(); }}
            onPaste={(e) => { if (e.clipboardData.files.length) { e.preventDefault(); uploads.add(e.clipboardData.files); } }}
            onSelect={track}
            onClick={track}
            onKeyUp={track}
            onKeyDown={(e) => {
              if (e.nativeEvent.isComposing) return;
              if (slash.onKeyDown(e) || mentions.onKeyDown(e)) return;
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submit();
              }
            }}
          />
          <AttachmentChips uploads={uploads} disabled={busy} />
          <div className="flex flex-wrap items-center gap-1 px-2 pb-2">
            <PlusMenu uploads={uploads} disabled={busy} projects={projects.filter(p => !picked.includes(p.id))} onProject={id => { addProject(id); setAskProject(false); }} onAddProject={onAddProject} />
            {picked.map((id, i) => { const p = projects.find(p => p.id === id); return <span key={id} className="flex h-7 items-center gap-1.5 rounded-md border border-accent/40 bg-accent/5 px-2 text-[12px]" title={`${p?.path ?? ""}${picked.length > 1 && i === 0 ? " · main project" : ""}`}><span aria-hidden className="text-faint">▭</span>{p?.name}<button type="button" aria-label={`Remove ${p?.name ?? "project"}`} className="text-dim hover:text-fg" onClick={() => setPicked(list => list.filter(x => x !== id))}>×</button></span>; })}
            {picked.length > 1 && <span className="text-[11px] text-faint">planned across {picked.length} projects</span>}
            {chips.map(c => <span key={c.label} className="flex items-center gap-1 rounded-md bg-raised px-2 py-0.5 text-[12px]">{c.label}<button type="button" aria-label={`Remove ${c.label}`} className="text-dim hover:text-fg" onClick={c.clear}>×</button></span>)}
            <div className="flex-1" />
            {drop.drag && <span className="text-[12px] text-accent">Drop to attach</span>}
            <Button variant="primary" onClick={submit} title={compare && picked.length > 1 ? "Comparing agents works in one project" : undefined} disabled={!text.trim() || busy || uploads.blocked || (compare && (!compareConsent || picked.length > 1))}>
              {busy ? "Starting…" : compare ? `Compare ${candidates.length} agents` : "Start"}
            </Button>
          </div>
        </div>
        {askProject && !projectId && <div role="alert" className="mt-2 flex flex-wrap items-center gap-2 text-[13px]"><span className="text-dim">Which project is this for?</span>{projects.map(p => <Button key={p.id} onClick={() => { addProject(p.id); setAskProject(false); }}>{p.name}</Button>)}<Button variant="ghost" onClick={onAddProject}>+ Add a project</Button></div>}
        {create.error && <div role="alert" className="mt-2 text-[12px] text-bad">{create.error.message}</div>}
        {comparison.error && <div role="alert" className="mt-2 text-[12px] text-bad">{comparison.error.message}</div>}
        {compare && <CompareSetup candidates={candidates} onChange={setCandidates} consent={compareConsent} onConsent={setCompareConsent} extras={uploads.ids.length + paths.length} />}
        {models && <ModelDialog onClose={() => setModels(false)} />}
      </div>
    </div>
  );
}

/** Pick two or three agents to run the same request side by side. */
function CompareSetup({ candidates, onChange, consent, onConsent, extras }: { candidates: CompareCandidate[]; onChange: (c: CompareCandidate[]) => void; consent: boolean; onConsent: (v: boolean) => void; extras: number }) {
  const harnesses = useHarnesses();
  const available = (harnesses.data ?? []).filter(h => h.installed && ["claude", "codex", "local"].includes(h.id));
  const set = (i: number, c: CompareCandidate) => onChange(candidates.map((x, j) => (j === i ? c : x)));
  return <section aria-label="Compare agents" className="mt-3 space-y-2 rounded-lg border border-line bg-panel p-3 text-[12px]">
    <p className="text-dim">Each agent gets the same request in its own worktree. You compare the results and keep one; the others stay on disk. Every cloud candidate uses its own allowance, and each needs a parallel cloud run in Settings.</p>
    {candidates.map((c, i) => {
      const info = available.find(h => h.id === c.harness);
      return <div key={i} className="flex flex-wrap items-center gap-2">
        <span className="w-4 font-medium">{"ABC"[i]}</span>
        <select aria-label={`Agent ${"ABC"[i]}`} className="h-7 rounded-md border border-line bg-bg px-1.5" value={c.harness} onChange={e => set(i, { harness: e.target.value as CompareCandidate["harness"], model: null })}>
          {available.map(h => <option key={h.id} value={h.id}>{h.name}</option>)}
        </select>
        {info && info.models.length > 0 && <select aria-label={`Model for ${"ABC"[i]}`} className="h-7 rounded-md border border-line bg-bg px-1.5" value={c.model ?? ""} onChange={e => set(i, { ...c, model: e.target.value || null })}>
          {c.harness !== "local" && <option value="">Default model</option>}
          {c.harness === "local" && <option value="">Choose a local model</option>}
          {info.models.filter(m => m.id).map(m => <option key={m.id!} value={m.id!}>{m.name}</option>)}
        </select>}
        {candidates.length > 2 && <Button onClick={() => onChange(candidates.filter((_, j) => j !== i))} aria-label={`Remove agent ${"ABC"[i]}`}>Remove</Button>}
      </div>;
    })}
    {candidates.length < 3 && available.length > 0 && <Button onClick={() => onChange([...candidates, { harness: available[0].id as CompareCandidate["harness"] }])}>Add a third agent</Button>}
    {extras > 0 && <p className="text-warn">Attachments and @ context are not included in comparisons.</p>}
    <label className="flex items-start gap-2"><input type="checkbox" checked={consent} onChange={e => onConsent(e.target.checked)} /><span>I approve running {candidates.length} agents on this task, each on its own allowance.</span></label>
  </section>;
}

/** "+" in the composer: attach files or images, or choose the project (both also inline: paste, drop, @). */
function PlusMenu({ uploads, disabled, projects, onProject, onAddProject }: { uploads: ReturnType<typeof useAttachments>; disabled: boolean; projects: Project[]; onProject: (id: string) => void; onAddProject: () => void }) {
  const [open, setOpen] = useState<null | "menu" | "projects">(null);
  const box = useRef<HTMLDivElement>(null), file = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => { if (!box.current?.contains(e.target as Node)) setOpen(null); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(null); };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("mousedown", away); document.removeEventListener("keydown", esc); };
  }, [open]);
  const item = "block w-full px-3 py-1.5 text-left text-[13px] hover:bg-raised";
  return <div className="relative" ref={box}>
    <Button variant="ghost" aria-label="Add files or choose a project" aria-haspopup="menu" aria-expanded={!!open} disabled={disabled} onClick={() => setOpen(o => (o ? null : "menu"))}>＋</Button>
    <input ref={file} type="file" multiple className="hidden" aria-label="Choose files or images" onChange={e => { uploads.add(e.target.files); e.target.value = ""; }} />
    {open && <div role="menu" className="absolute bottom-full left-0 z-30 mb-1 max-h-72 w-64 overflow-y-auto rounded-lg border border-line bg-panel py-1 shadow-xl">
      {open === "menu" ? <>
        <button type="button" role="menuitem" className={item} onClick={() => { setOpen(null); file.current?.click(); }}>Files or images…<span className="block text-[11px] text-faint">Or paste or drop them here</span></button>
        <button type="button" role="menuitem" className={item} onClick={() => setOpen("projects")}>Project…<span className="block text-[11px] text-faint">Or type @ and a project name</span></button>
      </> : <>
        {projects.map(p => <button key={p.id} type="button" role="menuitem" className={item} onClick={() => { onProject(p.id); setOpen(null); }}><span className="block truncate">{p.name}</span><span className="block truncate text-[11px] text-faint">{p.path}</span></button>)}
        <div className="my-1 border-t border-line" />
        <button type="button" role="menuitem" className={`${item} text-dim`} onClick={() => { setOpen(null); onAddProject(); }}>+ Add a project…</button>
      </>}
    </div>}
  </div>;
}
