import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { useApi } from "../ApiContext";
import type { MemoryStatus } from "../api";
import { Button, Dialog, Input } from "./ui";
import { installOnClose } from "./Updates";

const ago = (secs: number | null | undefined) => {
  if (!secs) return "never";
  const s = Math.max(0, Date.now() / 1000 - secs);
  return s < 90 ? "just now" : s < 5400 ? `${Math.round(s / 60)} minutes ago` : s < 129600 ? `${Math.round(s / 3600)} hours ago` : `${Math.round(s / 86400)} days ago`;
};

/** Settings card: where your memory lives and where it is backed up. */
export function MemoryCard() {
  const api = useApi(), qc = useQueryClient();
  const status = useQuery({ queryKey: ["memory"], queryFn: api.memory, refetchInterval: 60000 });
  const [url, setUrl] = useState("");
  const done = (s: MemoryStatus) => qc.setQueryData(["memory"], s);
  const github = useMutation({ mutationFn: () => api.memoryGithub("arbiter-memory"), onSuccess: done });
  const remote = useMutation({ mutationFn: () => api.memoryRemote(url.trim()), onSuccess: s => { setUrl(""); done(s); } });
  const sync = useMutation({ mutationFn: () => api.memorySync(true, false), onSuccess: done });
  const s = status.data;
  const error = github.error ?? remote.error ?? sync.error ?? status.error;
  return <section aria-label="Your memory" className="space-y-2 rounded-lg border border-line p-3">
    <p className="font-medium">Your memory</p>
    <p className="text-[12px] text-dim">Arbiter keeps your notes, your guidance for agents and a backup of your history in its own Git repository, separate from your projects, so nothing personal reaches a repository you share.</p>
    {s && <>
      <p className="break-all font-mono text-[12px]">{s.path}</p>
      <p className="text-[12px]">{s.remote
        ? <>Backed up to <span className="font-mono">{s.remote}</span> · last push {ago(s.last_push)}{(s.unpushed ?? 0) > 0 ? ` · ${s.unpushed} change${s.unpushed === 1 ? "" : "s"} not pushed yet` : ""}</>
        : <span className="text-warn">Only on this computer. Add a backup so a lost or broken computer does not lose it.</span>}</p>
      <p className="text-[12px] text-dim">Tell agents how you like to work in <span className="font-mono">guidance/about-me.md</span>, and add per-project instructions or check overrides under <span className="font-mono">projects/</span>.{s.guidance ? " Your guidance is in use." : ""}</p>
      {!s.remote && <div className="space-y-2">
        {s.gh && <Button variant="primary" disabled={github.isPending} onClick={() => github.mutate()}>{github.isPending ? "Creating…" : "Create a private GitHub repository"}</Button>}
        <form className="flex gap-2" onSubmit={e => { e.preventDefault(); if (url.trim()) remote.mutate(); }}>
          <Input aria-label="Git remote URL" placeholder={s.gh ? "Or paste a git remote URL" : "Paste a git remote URL (for example a private GitHub repository)"} value={url} onChange={e => setUrl(e.target.value)} />
          <Button type="submit" className="shrink-0 whitespace-nowrap" disabled={remote.isPending || !url.trim()}>{remote.isPending ? "Connecting…" : "Use it"}</Button>
        </form>
      </div>}
      {s.remote && <Button disabled={sync.isPending} onClick={() => sync.mutate()}>{sync.isPending ? "Saving…" : "Save to the remote now"}</Button>}
      {(s.committed_in_projects ?? []).length > 0 && <div className="rounded border border-warn/40 p-2 text-[12px]">
        <p className="text-warn">Older Arbiter files are committed in these projects. Arbiter no longer writes them; remove them with a normal commit if your team does not need them.</p>
        {s.committed_in_projects.map(c => <p key={c.project}>{c.project}: <span className="font-mono">{c.files.join(", ")}</span></p>)}
      </div>}
    </>}
    {status.isPending && <p role="status" className="text-[12px] text-dim">Checking your memory…</p>}
    {error && <p role="alert" className="text-[12px] text-bad">{error.message}</p>}
  </section>;
}

/** In the desktop app, closing the window asks to save memory to the remote
 *  first. A second close within a few seconds always closes (see the shell). */
export function CloseGuard() {
  const api = useApi();
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // A downloaded update installs on the way out (it exits the app itself).
  const close = () => { void installOnClose(api).catch(() => undefined).finally(() => getCurrentWindow().destroy()); };
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const off = listen("arbiter://close-requested", async () => {
      try {
        const s = await api.memory();
        if (s.remote && ((s.unpushed ?? 0) > 0 || s.dirty)) { setAsking(true); return; }
        await api.memorySync(false, true);
      } catch { /* the daemon may be gone; close anyway */ }
      close();
    });
    return () => { void off.then(f => f()); };
  }, [api]);
  const finish = async (push: boolean) => {
    setBusy(true); setError(null);
    try { await api.memorySync(push, true); close(); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); setBusy(false); }
  };
  if (!asking) return null;
  return <Dialog title="Save your memory before closing?" onClose={() => !busy && setAsking(false)}>
    <p className="text-[13px] text-dim">Your notes and history have changes that are not on your backup yet. If you close without saving, they stay on this computer and Arbiter asks again next time.</p>
    {error && <p role="alert" className="mt-2 text-[12px] text-bad">{error}</p>}
    <div className="mt-3 flex flex-wrap justify-end gap-2">
      <Button disabled={busy} onClick={() => (error ? close() : finish(false))}>{error ? "Close anyway" : "Close without saving"}</Button>
      <Button variant="primary" disabled={busy} onClick={() => finish(true)}>{busy ? "Saving…" : "Save and close"}</Button>
    </div>
  </Dialog>;
}
