import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useApi } from "../ApiContext";
import type { Api } from "../api";
import { Button } from "./ui";

const inTauri = "__TAURI_INTERNALS__" in window;
const CHECK_EVERY = 6 * 60 * 60 * 1000;

/** An update downloaded in the background, installed when the app closes. */
let ready: Update | null = null;

/** Save memory, then stop the background service so files can be replaced.
 *  Returns false when an agent is working (unless forced). */
async function prepare(api: Api, force: boolean): Promise<boolean> {
  try { await api.memorySync(true, true); } catch { /* no remote or offline: memory is still committed locally */ }
  try { await api.shutdown(force); return true; } catch (e) {
    if (e instanceof Error && /agent is working/i.test(e.message)) return false;
    return true; // the service is already gone
  }
}

/** Called by the close prompt: install a downloaded update on the way out,
 *  unless an agent is still working (then it waits for the next close). */
export async function installOnClose(api: Api): Promise<void> {
  if (!ready) return;
  if (await prepare(api, false)) await ready.install();
}

type State =
  | { kind: "idle" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; update: Update; done: number; total: number | null }
  | { kind: "ready"; update: Update }
  | { kind: "waiting"; update: Update }
  | { kind: "error"; message: string };

/** A slim bar at the top of the window when a new version exists. */
export function UpdateBanner() {
  const api = useApi();
  const setup = useQuery({ queryKey: ["setup"], queryFn: api.setup });
  const auto = setup.data?.preferences.auto_update ?? true;
  const [state, setState] = useState<State>({ kind: "idle" });
  const [hidden, setHidden] = useState(false);
  const busy = useRef(false);

  const download = async (update: Update) => {
    let done = 0, total: number | null = null;
    setState({ kind: "downloading", update, done, total });
    await update.download(e => {
      if (e.event === "Started") total = e.data.contentLength ?? null;
      if (e.event === "Progress") done += e.data.chunkLength;
      setState({ kind: "downloading", update, done, total });
    });
    ready = update;
    setState({ kind: "ready", update });
  };

  useEffect(() => {
    if (!inTauri) return;
    const look = async () => {
      if (busy.current || ready) return;
      busy.current = true;
      try {
        const update = await check();
        if (!update) return;
        setHidden(false);
        if (auto) await download(update); else setState({ kind: "available", update });
      } catch (e) {
        // Offline or no release yet: stay quiet, try again later.
        console.warn("update check failed", e);
      } finally { busy.current = false; }
    };
    void look();
    const timer = window.setInterval(look, CHECK_EVERY);
    return () => window.clearInterval(timer);
  }, [auto]);

  const installNow = async (update: Update, force: boolean) => {
    try {
      if (!ready) await download(update);
      if (!(await prepare(api, force))) { setState({ kind: "waiting", update }); return; }
      await update.install();
      await relaunch();
    } catch (e) {
      setState({ kind: "error", message: e instanceof Error ? e.message : String(e) });
    }
  };

  if (state.kind === "idle" || hidden) return null;
  const version = "update" in state ? state.update.version : "";
  const pct = state.kind === "downloading" && state.total ? Math.round((state.done / state.total) * 100) : null;
  return <div role="status" aria-label="App update" className="flex shrink-0 flex-wrap items-center gap-2 border-b border-accent/40 bg-accent/10 px-3 py-1.5 text-[12px]">
    <span aria-hidden className="text-accent">●</span>
    <span className="flex-1">
      {state.kind === "available" && <>Arbiter {version} is available.</>}
      {state.kind === "downloading" && <>Downloading Arbiter {version}{pct !== null ? ` · ${pct}%` : "…"}</>}
      {state.kind === "ready" && <>Arbiter {version} is ready. It installs when you close Arbiter, or restart now.</>}
      {state.kind === "waiting" && <>An agent is still working. Arbiter {version} installs when it finishes and you close Arbiter.</>}
      {state.kind === "error" && <span className="text-bad">The update did not install: {state.message}</span>}
    </span>
    {state.kind === "available" && <Button variant="primary" onClick={() => installNow(state.update, false)}>Update now</Button>}
    {state.kind === "ready" && <Button variant="primary" onClick={() => installNow(state.update, false)}>Restart now</Button>}
    {state.kind === "waiting" && <Button onClick={() => installNow(state.update, true)} title="Stops the running agent; its work stays in its branch">Update anyway</Button>}
    {state.kind !== "downloading" && <Button variant="ghost" aria-label="Hide until later" onClick={() => setHidden(true)}>Later</Button>}
  </div>;
}
