import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { ElementDescriptor, PreviewCapture, Viewport } from "../api";
import { ScreenshotViewer } from "./ScreenshotViewer";
import { Button, Input } from "./ui";

const SIZES = { phone: 390, tablet: 768, desktop: 0 } as const;

/** The system browser in the desktop app; a new tab in a plain browser. */
async function openExternal(url: string) {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch {
    window.open(url, "_blank", "noopener");
  }
}

/**
 * The task's app. Live shows the real, interactive page; Inspect keeps the
 * screenshot tools (element descriptors, scripted clicks, page checks,
 * captures). Arbiter starts the app itself after a task finishes.
 */
export function Preview({ threadId, onAttach }: { threadId: string; onAttach: (text: string) => void }) {
  const api = useApi(), qc = useQueryClient();
  const key = ["preview", threadId];
  const status = useQuery({ queryKey: key, queryFn: () => api.preview(threadId), retry: false, refetchInterval: q => q.state.data?.url ? false : 5000 });
  const [view, setView] = useState<"live" | "inspect">("live");
  const [size, setSize] = useState<keyof typeof SIZES>("desktop");
  const [path, setPath] = useState("/");
  const [reload, setReload] = useState(0);
  const start = useMutation({ mutationFn: () => api.startPreview(threadId), onSuccess: v => qc.setQueryData(key, v) });
  const url = status.data?.url ?? null;
  const open = () => { if (url) void openExternal(url + path); };
  if (!url) return <NotRunning threadId={threadId} starting={start.isPending} error={start.error?.message ?? null} onStart={() => start.mutate()} />;
  return <section className="flex min-h-0 flex-1 flex-col" aria-label="App preview">
    <div className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
      <div role="radiogroup" aria-label="Preview mode" className="inline-flex rounded-lg border border-line p-0.5">
        {(["live", "inspect"] as const).map(v => <button key={v} type="button" role="radio" aria-checked={view === v} onClick={() => setView(v)} className={`rounded-md px-3 py-1 text-[12px] ${view === v ? "bg-raised text-fg" : "text-dim hover:text-fg"}`}>{v === "live" ? "Live" : "Inspect"}</button>)}
      </div>
      {view === "live" && <>
        <form className="flex min-w-32 flex-1" onSubmit={e => { e.preventDefault(); setReload(r => r + 1); }}>
          <Input aria-label="Page path" className="w-full font-mono" value={path} onChange={e => setPath(e.target.value.startsWith("/") ? e.target.value : `/${e.target.value}`)} />
        </form>
        <div role="radiogroup" aria-label="Screen size" className="inline-flex rounded-lg border border-line p-0.5">
          {(Object.keys(SIZES) as (keyof typeof SIZES)[]).map(k => <button key={k} type="button" role="radio" aria-checked={size === k} onClick={() => setSize(k)} className={`rounded-md px-2.5 py-1 text-[12px] capitalize ${size === k ? "bg-raised text-fg" : "text-dim hover:text-fg"}`}>{k}</button>)}
        </div>
        <Button variant="ghost" onClick={() => setReload(r => r + 1)} title="Reload the page">Reload</Button>
        <Button variant="ghost" onClick={open}>Open in browser</Button>
      </>}
    </div>
    {view === "live"
      ? <div className="min-h-0 flex-1 overflow-auto bg-raised/40 p-3">
          <iframe key={reload} title="Your app" src={url + path} className="mx-auto block h-full min-h-[480px] rounded-lg border border-line bg-white" style={{ width: SIZES[size] ? `${SIZES[size]}px` : "100%" }} />
        </div>
      : <Inspector threadId={threadId} onAttach={onAttach} />}
  </section>;
}

/** Shown until the app runs: what happens, a start button, and the command. */
function NotRunning({ threadId, starting, error, onStart }: { threadId: string; starting: boolean; error: string | null; onStart: () => void }) {
  const api = useApi(), qc = useQueryClient();
  const cmd = useQuery({ queryKey: ["preview-command", threadId], queryFn: () => api.previewCommand(threadId) });
  const [editing, setEditing] = useState(false), [draft, setDraft] = useState("");
  const save = useMutation({ mutationFn: (dev: string | null) => api.setPreviewCommand(threadId, dev), onSuccess: v => { qc.setQueryData(["preview-command", threadId], v); setEditing(false); } });
  const effective = cmd.data?.effective;
  return <section aria-label="App preview" className="grid min-h-0 flex-1 place-items-center overflow-y-auto p-6">
    <div className="w-full max-w-md space-y-3 text-center">
      <h2 className="text-base font-semibold">See your app here</h2>
      <p className="text-dim">{effective ? "Arbiter starts your app automatically when a task finishes. You can also start it now." : "Arbiter could not tell how to start this app yet. Tell it the start command, or ask the agent to set one up."}</p>
      {effective && <Button variant="primary" disabled={starting} onClick={onStart}>{starting ? "Starting your app…" : "Start my app"}</Button>}
      {error && <p role="alert" className="text-[12px] text-bad">{error}</p>}
      <div className="rounded-lg border border-line p-3 text-left text-[12px]">
        {editing
          ? <form className="space-y-2" onSubmit={e => { e.preventDefault(); save.mutate(draft.trim() || null); }}>
              <label className="block space-y-1"><span className="text-dim">Start command. Use {"{port}"} where the port goes.</span><Input autoFocus className="w-full font-mono" value={draft} maxLength={300} onChange={e => setDraft(e.target.value)} placeholder="npm run dev -- --port {port}" /></label>
              <div className="flex justify-end gap-2">{cmd.data?.custom && <Button type="button" onClick={() => save.mutate(null)}>Use detected</Button>}<Button type="button" onClick={() => setEditing(false)}>Cancel</Button><Button type="submit" variant="primary" disabled={save.isPending}>Save</Button></div>
              {save.error && <p role="alert" className="text-bad">{save.error.message}</p>}
            </form>
          : <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate"><span className="text-dim">Start command: </span><span className="font-mono">{effective === "builtin:static" ? "built-in file server" : effective ?? "not set"}</span>{cmd.data?.custom && <span className="text-dim"> (yours)</span>}</span>
              <Button onClick={() => { setDraft(cmd.data?.custom ?? cmd.data?.detected?.replace("builtin:static", "") ?? ""); setEditing(true); }}>Change</Button>
            </div>}
      </div>
    </div>
  </section>;
}

/** Screenshot-based inspection: element descriptors, clicks, checks, captures. */
function Inspector({ threadId, onAttach }: { threadId: string; onAttach: (text: string) => void }) {
  const api = useApi(), qc = useQueryClient();
  const key = ["preview", threadId];
  const status = useQuery({ queryKey: key, queryFn: () => api.preview(threadId), retry: false });
  const [route, setRoute] = useState("/");
  const [frame, setFrame] = useState("");
  const [frameError, setFrameError] = useState("");
  const [frameLoading, setFrameLoading] = useState(false);
  const [frameRoute, setFrameRoute] = useState("/");
  const [revision, setRevision] = useState(0);
  const [mode, setMode] = useState<"review"|"inspect"|"interact">("review");
  const [viewport, setViewport] = useState<Viewport>({width:1280,height:800});
  const [sizeError, setSizeError] = useState("");
  const [custom, setCustom] = useState({width:"1280",height:"800"});
  const [capture, setCapture] = useState<PreviewCapture | null>(null);
  const [capturedAt, setCapturedAt] = useState("");
  const captures = useQuery({queryKey:["captures",threadId],queryFn:()=>api.captures(threadId)});
  const resize = useMutation({mutationFn:(v:Viewport)=>api.resizePreview(threadId,v),onSuccess:v=>{setFrame("");setViewport(v);setSelected(null);setRevision(r=>r+1);qc.invalidateQueries({queryKey:key});}});
  const saveCapture = useMutation({mutationFn:()=>api.capturePreview(threadId),onSuccess:()=>{qc.invalidateQueries({queryKey:["captures",threadId]});}});
  const [selector, setSelector] = useState("");
  const [typed, setTyped] = useState("");
  const [selected, setSelected] = useState<ElementDescriptor | null>(null);
  const [checks, setChecks] = useState<{ issues: string[] } | null>(null);

  const frameUrl = useRef("");
  useEffect(() => () => { if (frameUrl.current) URL.revokeObjectURL(frameUrl.current); }, []);
  const start = useMutation({ mutationFn: () => api.startPreview(threadId), onSuccess: v => qc.setQueryData(key, v) });
  const stop = useMutation({ mutationFn: () => api.stopPreview(threadId), onSuccess: () => { qc.setQueryData(key, { url: null }); setFrame(""); setSelected(null); } });
  const action = useMutation({
    mutationFn: (body: Record<string, unknown>) => api.browser<ElementDescriptor & { route?: string; issues?: string[] }>(threadId, body),
    onSuccess: (value, body) => {
      if (body.op === "inspect") { setSelected(value); setSelector(value.selector); }
      else { setFrame("");setSelected(null); setRevision(r => r + 1); }
      if (value.route) setRoute(value.route);
      if (value.issues) setChecks({ issues: value.issues });
    },
  });
  const running = !!status.data?.url;
  const busy = start.isPending || stop.isPending || action.isPending || resize.isPending || saveCapture.isPending;
  useEffect(() => {
    if (!running && !capture) { setFrameLoading(false); return; }
    const c = new AbortController();
    setFrameError("");
    setFrameLoading(true);
    const request = capture ? api.attachment(capture.id).then(blob=>({blob,width:capture.width,height:capture.height,route:capture.route})) : api.previewImage(threadId, AbortSignal.any([c.signal, AbortSignal.timeout(45_000)]));
    request.then(image => {
      if (c.signal.aborted) return;
      const objectUrl = URL.createObjectURL(image.blob);
      if (frameUrl.current) URL.revokeObjectURL(frameUrl.current);
      frameUrl.current = objectUrl; setFrame(objectUrl); setViewport({width:image.width,height:image.height});
      setFrameRoute(image.route);
      setCapturedAt(capture?.ts ?? new Date().toISOString());
    }).catch(e => { if (!c.signal.aborted) setFrameError(e.message); }).finally(() => { if (!c.signal.aborted) setFrameLoading(false); });
    return () => c.abort();
  }, [api, threadId, running, revision, capture]);
  const error = start.error ?? stop.error ?? resize.error ?? saveCapture.error ?? action.error ?? status.error;
  const run = (body: Record<string, unknown>) => { if (!busy) action.mutate(body); };
  return <section className="flex min-h-0 flex-1 flex-col" aria-label="App preview">
    <div className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
      <Input aria-label="Preview route" className="min-w-32 flex-1 font-mono" value={route} onChange={e => setRoute(e.target.value)} onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing && running) run({ op: "goto", route }); }} />
      {running ? <>
        <Button disabled={busy} onClick={() => run({ op: "goto", route })}>Go</Button>
        <Button disabled={busy} onClick={() => { setCapture(null); setSelected(null); setRevision(r => r + 1); }}>Refresh view</Button>
        <Button disabled={busy || !!capture} onClick={()=>saveCapture.mutate()}>{saveCapture.isPending ? "Saving…" : "Save capture"}</Button>
        <Button disabled={busy} onClick={() => stop.mutate()}>Stop preview</Button>
      </> : <Button variant="primary" disabled={busy || status.isPending} onClick={() => start.mutate()}>{start.isPending ? "Starting…" : "Start preview"}</Button>}
    </div>
    <div className="min-h-0 flex-1 overflow-auto p-3">
      <div className="mb-2 min-h-5 text-[12px] text-dim" role="status">{frameLoading ? "Refreshing screenshot…" : frameError ? "Screenshot unavailable or stale. Retry before interacting." : busy ? "Working…" : capture ? "Saved capture — this image does not update." : running ? mode === "review" ? "Screenshot review. Refresh view to capture the latest page." : mode === "inspect" ? "Pick an element, or enter its CSS selector below." : "Click the preview to interact. Use the field controls below to type." : "Start your app here to inspect elements and check browser errors."}</div>
      {(error || frameError) && <div role="alert" className="mb-3 rounded border border-bad/40 p-2 text-bad">{error?.message ?? frameError}<Button className="ml-2" onClick={() => { void status.refetch(); setRevision(r => r + 1); }}>Retry</Button></div>}
      {(running || capture) && <>
        {running && !capture && <div className="mb-3 flex flex-wrap items-end gap-2">
          <label className="text-[12px] text-dim">Website viewport<select aria-label="Website viewport" className="ml-2 rounded border border-line bg-bg p-2 text-fg" disabled={busy} value={["1280x800","768x1024","390x844"].includes(`${viewport.width}x${viewport.height}`)?`${viewport.width}x${viewport.height}`:"custom"} onChange={e=>{if(e.target.value!=="custom"){const [width,height]=e.target.value.split("x").map(Number);resize.mutate({width,height});}}}><option value="1280x800">Desktop · 1280 × 800</option><option value="768x1024">Tablet · 768 × 1024</option><option value="390x844">Phone · 390 × 844</option><option value="custom">Custom size</option></select></label>
          <details><summary className="cursor-pointer text-[12px] text-dim">Custom dimensions</summary><form noValidate className="mt-2 flex flex-wrap items-end gap-2" onSubmit={e=>{e.preventDefault();const width=Number(custom.width),height=Number(custom.height); if(!Number.isInteger(width)||width<320||width>2560||!Number.isInteger(height)||height<240||height>1600){setSizeError("Use whole pixels: width 320–2560, height 240–1600.");e.currentTarget.querySelector<HTMLInputElement>("input")?.focus();return;}setSizeError("");resize.mutate({width,height});}}><label className="w-24 text-[12px]">Width<Input type="number" aria-invalid={!!sizeError} aria-describedby="preview-size-error" value={custom.width} onChange={e=>setCustom({...custom,width:e.target.value})}/></label><label className="w-24 text-[12px]">Height<Input type="number" aria-invalid={!!sizeError} aria-describedby="preview-size-error" value={custom.height} onChange={e=>setCustom({...custom,height:e.target.value})}/></label><Button type="submit" disabled={busy}>Apply size</Button></form><p id="preview-size-error" role="alert" className="text-[12px] text-bad">{sizeError}</p></details>
          <div className="flex flex-wrap gap-1" aria-label="Preview mode">{(["review","inspect","interact"] as const).map(m=><Button key={m} aria-pressed={mode===m} variant={mode===m?"primary":"default"} onClick={()=>{setMode(m);setSelected(null);}}>{m === "review" ? "Review image" : m === "inspect" ? "Inspect elements" : "Interact"}</Button>)}</div>
        </div>}
        {frame ? <ScreenshotViewer src={frame} viewport={viewport} caption={`${capture?"Saved capture":"Latest frame"} · ${capture?.route??frameRoute} · ${capturedAt?new Date(capturedAt).toLocaleString():"Loading"}`} selected={selected} mode={capture?"review":mode} busy={busy || frameLoading || !!frameError} onPoint={(x,y)=>run({op:mode==="inspect"?"inspect":"click",x,y})}/> : <p role="status">{frameError?"Preview unavailable.":"Loading preview…"}</p>}
        {running && !capture && <>
        <div className="my-3 flex flex-wrap gap-2">
          <Button disabled={busy} onClick={() => run({ op:"scroll", dy:-500 })}>Scroll up</Button>
          <Button disabled={busy} onClick={() => run({ op:"scroll", dy:500 })}>Scroll down</Button>
          <Button disabled={busy} onClick={() => run({ op:"errors", route })}>Check page</Button>
        </div>
        <div className="flex flex-wrap gap-2">
          <Input id="preview-selector" aria-label="Element CSS selector" className="min-w-40 flex-1 font-mono" value={selector} placeholder="CSS selector, e.g. #submit" onChange={e => setSelector(e.target.value)} />
          <Button disabled={busy || !selector.trim()} onClick={() => run({ op:"inspect",selector })}>Inspect element</Button>
          <Button disabled={busy || !selector.trim()} onClick={() => run({ op:"click",selector })}>Click element</Button>
        </div>
        <div className="mt-2 flex gap-2"><Input aria-label="Text to enter in selected field" value={typed} onChange={e => setTyped(e.target.value)} placeholder="Text to enter in the selected field" /><Button disabled={busy || !selector.trim()} onClick={() => run({ op:"type",selector,text:typed })}>Type text</Button></div>
        {selected && <div className="mt-3 rounded border border-accent/40 bg-panel p-3">
          <div className="mb-2 flex flex-wrap items-center gap-2"><strong className="flex-1">Selected element</strong><Button variant="primary" onClick={() => onAttach(selected.descriptor)}>Add to message</Button></div>
          <pre className="whitespace-pre-wrap break-words font-mono text-[12px] text-dim">{selected.descriptor}</pre>
          {!selected.source && <p className="mt-2 text-[12px] text-dim">Source location is unavailable in this app’s development metadata.</p>}
        </div>}
        {checks && <div className="mt-3 rounded border border-line p-3" role="status"><strong>{checks.issues.length ? `${checks.issues.length} browser issues` : "No browser issues found"}</strong>{checks.issues.map((s,i) => <p key={i} className="mt-1 break-words font-mono text-[12px] text-dim">{s}</p>)}</div>}
        </>}
      </>}
      <section className="mt-5 border-t border-line pt-3" aria-label="Saved captures"><h3 className="font-semibold">Saved captures</h3><p className="mt-1 text-[12px] text-dim">Latest 100 captures. Saving does not send the image to an agent.</p>{saveCapture.isSuccess&&<p role="status" className="mt-2 text-ok">Capture saved.</p>}{captures.isPending&&<p role="status">Loading captures…</p>}{captures.error&&<p role="alert" className="text-bad">{captures.error.message} <Button onClick={()=>captures.refetch()}>Retry</Button></p>}{captures.data?.length===0&&<p className="mt-2 text-dim">No captures yet. Start the preview and save a frame.</p>}
        {capture&&<Button className="mt-2" onClick={()=>{setCapture(null);setFrame("");setRevision(r=>r+1);}}>Return to latest frame</Button>}
        <ul className="mt-3 space-y-2">{captures.data?.map((c,i)=><li key={`${c.id}-${c.ts}-${i}`}><Button className="h-auto min-h-8 max-w-full text-left" onClick={()=>{setCapture(c);setFrame("");setSelected(null);}}>{c.route} · {c.width} × {c.height} · {new Date(c.ts).toLocaleString()}</Button></li>)}</ul>
      </section>
    </div>
  </section>;
}
