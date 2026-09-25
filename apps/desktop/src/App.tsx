import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Api, ArbEvent, connect, type Thread } from "./api";
import { Sidebar, type View } from "./components/Sidebar";
import { ThreadView } from "./components/ThreadView";
import { Onboarding } from "./components/ProjectSetup";
import { Setup } from "./components/Setup";
import { Draft } from "./components/Draft";
import { TasksView } from "./components/Tasks";
import { Inbox } from "./components/Inbox";
import { Button, Dialog } from "./components/ui";
import { useMediaQuery } from "./useMediaQuery";
import { Palette, Shortcuts } from "./components/Palette";
import { type ActionId, comboOf, label, loadBindings } from "./keymap";
import { notify } from "./notify";
import { Tour, tourDone } from "./components/Tour";

import { ApiContext, useApi } from "./ApiContext";
import { CloseGuard } from "./components/Memory";
import { UpdateBanner } from "./components/Updates";

export default function App() {
  const [api, setApi] = useState<Api | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    connect()
      .then((c) => { const a = new Api(c); a.reconnect = connect; setApi(a); })
      .catch((e) => setError(String(e?.message ?? e)));
  }, []);

  if (error) return <Splash title="Can't reach arbiterd" detail={error} />;
  if (!api) return <Splash title="Starting Arbiter…" />;
  return (
    <ApiContext.Provider value={api}>
      <Shell />
      <CloseGuard />
    </ApiContext.Provider>
  );
}

function Shell() {
  const api = useApi();
  const qc = useQueryClient();
  const [live, setLive] = useState(false);
  const [setupOpen,setSetupOpen]=useState(false);
  const [projectOpen,setProjectOpen]=useState(()=>new URLSearchParams(window.location.search).has('adoption'));
  const setup=useQuery({queryKey:["setup"],queryFn:api.setup});
  const narrow = useMediaQuery("(max-width: 1023px)");
  const [navigation, setNavigation] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [view, setView] = useState<View>(() => {
    const id = new URLSearchParams(window.location.search).get("thread");
    return id ? { kind: "thread", id } : { kind: "draft" };
  });
  useEffect(() => {
    const url = new URL(window.location.href);
    if (view.kind === "thread") url.searchParams.set("thread", view.id);
    else url.searchParams.delete("thread");
    window.history.replaceState(null, "", url);
  }, [view]);
  const [palette, setPalette] = useState(false), [shortcuts, setShortcuts] = useState(false);
  const [bindings, setBindings] = useState(loadBindings);
  const [tour, setTour] = useState(false);
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const threads = useQuery({ queryKey: ["threads"], queryFn: api.threads });

  // One socket for the whole app: append events into per-thread caches and
  // refresh summaries. On lag, refetch everything rather than guess.
  useEffect(
    () =>
      api.subscribe((e) => {
        if ("type" in e) {
          qc.invalidateQueries(e.type === "tasks_changed" ? { queryKey: ["tasks"] } : undefined);
          return;
        }
        const ev = e as ArbEvent;
        if (ev.kind.type.startsWith("vault_")) qc.invalidateQueries({queryKey:["vault"]});
        if (ev.kind.type.startsWith("outcome_") || ev.kind.type==="routing_preference") qc.invalidateQueries({queryKey:["learning"]});
        if (ev.kind.type==="preview_captured") qc.invalidateQueries({queryKey:["captures",ev.thread_id]});
        if (ev.kind.type === "plan") qc.invalidateQueries({queryKey:["plan",ev.thread_id]});
        if (ev.kind.type === "status_changed") {
          const title = qc.getQueryData<Thread[]>(["threads"])?.find(t => t.id === ev.thread_id)?.title ?? "A task";
          notify(title, ev.kind.status, () => setView({ kind: "thread", id: ev.thread_id }));
        }
        qc.setQueryData<ArbEvent[]>(["events", ev.thread_id], (old) =>
          old && !old.some((o) => o.seq === ev.seq) ? [...old, ev] : old,
        );
        qc.invalidateQueries({ queryKey: ["threads"] });
      }, setLive),
    [api, qc],
  );

  const thread = view.kind === "thread" ? threads.data?.find((t) => t.id === view.id) : undefined;
  const runAction = (id: ActionId) => {
    if (id === "palette") setPalette(true);
    else if (id === "newTask") setView({ kind: "draft" });
    else if (id === "tasks") setView({ kind: "tasks" });
    else if (id === "inbox") setView({ kind: "inbox" });
    else if (id === "toggleSidebar") narrow ? setNavigation(v => !v) : setCollapsed(v => !v);
    else if (id === "setup") setSetupOpen(true);
    else if (id === "addProject") setProjectOpen(true);
    else if (id === "stopAgent" && thread) api.stop(thread.id).catch(() => { /* nothing running */ });
    else if (id === "tour") { setView({ kind: "draft" }); setTour(true); }
  };
  const runRef = useRef(runAction);
  runRef.current = runAction;
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const combo = comboOf(e);
      if (!combo) return;
      const hit = (Object.keys(bindings) as ActionId[]).find(id => bindings[id] === combo);
      if (!hit) return;
      e.preventDefault();
      runRef.current(hit);
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [bindings]);
  // First visit to a workspace with a project: offer the tour once.
  const ready = !!setup.data?.preferences.complete && (projects.data?.length ?? 0) > 0 && !setupOpen && !projectOpen;
  useEffect(() => { if (ready && !tourDone()) { setView({ kind: "draft" }); setTour(true); } }, [ready]);
  // Waiting-on-you count in the window title, even without notifications.
  const waiting = (threads.data ?? []).filter(t => t.status === "needs_approval").length;
  useEffect(() => { document.title = waiting ? `(${waiting}) Arbiter` : "Arbiter"; }, [waiting]);

  if(setup.isPending) return <Splash title="Loading workspace preferences…"/>;
  if(setup.error) return <div role="alert" className="p-6">{setup.error.message} <Button onClick={()=>setup.refetch()}>Retry</Button></div>;
  if(setupOpen || !setup.data?.preferences.complete) return <Setup onClose={()=>setSetupOpen(false)}/>;
  if(projectOpen) return <Onboarding onClose={()=>{setProjectOpen(false);setView({kind:"draft"});}}/>;
  if (projects.isSuccess && projects.data.length === 0) return <div className="flex h-full flex-col"><div className="flex justify-end p-3"><Button variant="ghost" onClick={()=>setSetupOpen(true)}>Settings</Button></div><div className="min-h-0 flex-1"><Onboarding /></div></div>;

  const openThread = (id: string) => setView({ kind: "thread", id });
  return (
    <div className="flex h-full min-h-0">
      {tour && <Tour onClose={() => setTour(false)} />}
      {palette && <Palette bindings={bindings} threads={threads.data ?? []} run={runAction} openThread={openThread} onShortcuts={() => setShortcuts(true)} onClose={() => setPalette(false)} />}
      {shortcuts && <Shortcuts bindings={bindings} onChange={setBindings} onClose={() => setShortcuts(false)} />}
      {!narrow && !collapsed && <Sidebar projects={projects.data ?? []} threads={threads.data ?? []} view={view} onView={setView} live={live} onAddProject={()=>setProjectOpen(true)} />}
      {narrow && navigation && <Dialog title="Navigation" variant="navigation" onClose={()=>setNavigation(false)}><Sidebar projects={projects.data ?? []} threads={threads.data ?? []} view={view} onView={v=>{setView(v);setNavigation(false);}} live={live} onAddProject={()=>{setNavigation(false);setProjectOpen(true);}}/></Dialog>}
      <main className="flex min-h-0 min-w-0 flex-1 flex-col">
        <UpdateBanner />
        <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-1.5">
          <Button variant="ghost" aria-label={narrow ? "Open navigation" : collapsed ? "Show sidebar" : "Hide sidebar"} aria-expanded={narrow?navigation:!collapsed} onClick={()=>narrow?setNavigation(true):setCollapsed(v=>!v)}>☰</Button>
          <span className="flex-1" />
          {!live && <span role="status" className="text-[12px] text-warn">Reconnecting…</span>}
          <Button variant="ghost" data-tour="search" onClick={()=>setPalette(true)} title="Search actions and tasks">Search <kbd className="ml-1 text-[11px] text-faint">{label(bindings.palette)}</kbd></Button>
          <Button variant="ghost" aria-label="Settings" onClick={()=>setSetupOpen(true)} title={setup.data.preferences.mode==='subscription'?'Subscription protected':setup.data.preferences.mode==='local'?'Cloud agents off':'Paid usage enabled'}>Settings</Button>
        </div>
        <div className="min-h-0 min-w-0 flex-1">
        {view.kind === "draft" && projects.data && <Draft key={view.project ?? "new"} projects={projects.data} initialProject={view.project} onAddProject={() => setProjectOpen(true)} onCreated={openThread} />}
        {view.kind === "inbox" && (
          <Inbox threads={threads.data ?? []} projects={projects.data ?? []} onOpen={openThread} />
        )}
        {view.kind === "tasks" && projects.data && (
          <TasksView projects={projects.data} threads={threads.data ?? []} onOpenThread={openThread} />
        )}
        {view.kind === "thread" &&
          (thread ? (
            <ThreadView key={thread.id} thread={thread} onSelect={openThread} />
          ) : (
            <div className="grid h-full place-items-center text-dim">{threads.isSuccess ? <div>Task unavailable. <button className="text-accent underline" onClick={()=>setView({kind:"draft"})}>Create a task</button></div> : "Loading thread…"}</div>
          ))}
        </div>
      </main>
    </div>
  );
}

function Splash({ title, detail }: { title: string; detail?: string }) {
  return (
    <div className="grid h-full place-items-center">
      <div className="max-w-md text-center">
        <div className="mb-2 text-lg font-semibold">{title}</div>
        {detail && <div className="font-mono text-xs text-dim">{detail}</div>}
      </div>
    </div>
  );
}



