import type { Project, Thread } from "../api";
import { StatusDot } from "./ui";
import { inboxItems } from "./Inbox";

export type View = { kind: "draft"; project?: string } | { kind: "tasks" } | { kind: "inbox" } | { kind: "thread"; id: string };

interface Props {
  projects: Project[];
  threads: Thread[];
  view: View;
  onView: (v: View) => void;
  live: boolean;
  onAddProject: () => void;
}

export function Sidebar({ projects, threads, view, onView, live, onAddProject }: Props) {
  const active = threads.filter((t) => t.status === "running" || t.status === "healing").length;
  const inbox = inboxItems(threads).length;
  return (
    <aside className="flex h-full w-64 max-w-full shrink-0 flex-col border-r border-line bg-panel">
      <div className="flex h-11 items-center justify-between px-3">
        <span className="font-semibold tracking-tight">Arbiter</span>
        <span className="flex items-center gap-1.5 text-[11px] text-faint" title="Connection to arbiterd">
          <span className={`size-1.5 rounded-full ${live ? "bg-ok" : "bg-bad"}`} />
          {live ? (active ? `${active} running` : "live") : "offline"}
        </span>
      </div>
      <div className="space-y-0.5 px-2 pb-2">
        <NavButton active={view.kind === "draft"} onClick={() => onView({ kind: "draft" })} hint="">
          ✎ New task
        </NavButton>
        <NavButton tour="needs-you" active={view.kind === "inbox"} onClick={() => onView({ kind: "inbox" })} hint={inbox ? `${inbox} waiting` : ""}>
          ✉ Needs you
        </NavButton>
        {/* The board earns its place once there is enough work to organize. */}
        {(threads.length >= 6 || view.kind === "tasks") && <NavButton active={view.kind === "tasks"} onClick={() => onView({ kind: "tasks" })} hint="">
          ☰ Board
        </NavButton>}
      </div>
      <nav className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {projects.map((p) => {
          const roots = threads.filter((t) => t.project_id === p.id && !t.plan_root);
          const ts = roots.flatMap(t=>[t,...threads.filter(child=>child.plan_root===t.id)]);
          return (
            <section key={p.id} className="mb-3">
              <div className="group flex items-center gap-1 px-2 py-1">
                <span className="min-w-0 flex-1 truncate text-[11px] font-medium uppercase tracking-wide text-faint" title={p.path}>{p.name}</span>
                <button type="button" aria-label={`New task in ${p.name}`} title={`New task in ${p.name}`} onClick={() => onView({ kind: "draft", project: p.id })}
                  className="rounded px-1 text-faint opacity-60 hover:bg-raised hover:text-fg group-hover:opacity-100 focus:opacity-100">+</button>
              </div>
              {ts.length === 0 && <div className="px-2 py-1 text-[12px] text-faint">No threads yet</div>}
              {ts.map((t) => (
                <button
                  key={t.id}
                  onClick={() => onView({ kind: "thread", id: t.id })}
                  className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left ${
                    view.kind === "thread" && view.id === t.id ? "bg-raised text-fg" : "text-dim hover:bg-raised/60 hover:text-fg"
                  }`}
                >
                  {t.plan_root && <span className="pl-3 text-faint" aria-label="Plan step">↳</span>}<StatusDot status={t.status} />
                  <span className="min-w-0 flex-1 truncate">{t.title}</span>
                </button>
              ))}
            </section>
          );
        })}
        <button type="button" data-tour="add-project" onClick={onAddProject} className="mt-1 w-full rounded-md px-2 py-1.5 text-left text-[12px] text-dim hover:bg-raised/60 hover:text-fg">+ Add project</button>
      </nav>
    </aside>
  );
}

function NavButton({ active, onClick, hint, children, tour }: { active: boolean; onClick: () => void; hint: string; children: React.ReactNode; tour?: string }) {
  return (
    <button
      data-tour={tour}
      onClick={onClick}
      className={`flex w-full items-center rounded-md px-2 py-1.5 text-left ${active ? "bg-raised text-fg" : "text-dim hover:bg-raised/60 hover:text-fg"}`}
    >
      <span className="flex-1">{children}</span>
      {hint && <span className="text-[10px] text-accent">{hint}</span>}
    </button>
  );
}
