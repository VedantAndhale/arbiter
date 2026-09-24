// Everything that needs a human, and nothing else. Agents run autonomously;
// they surface here when they need approval, got stuck, or finished work
// that is ready for review. "Done for now" settles an item until it changes.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { Project, Thread, ThreadStatus } from "../api";
import { Button, StatusDot } from "./ui";

const GROUPS: { status: ThreadStatus; title: string; hint: string }[] = [
  { status: "needs_approval", title: "Needs approval", hint: "Paused on a budget or a decision" },
  { status: "failed", title: "Stuck or failed", hint: "Healing gave up, or the agent crashed twice" },
  { status: "review", title: "Ready for review", hint: "Agent finished and checks passed" },
];

export function inboxItems(threads: Thread[]): Thread[] {
  return threads.filter((t) => !t.settled && GROUPS.some((g) => g.status === t.status));
}

function ago(iso: string): string {
  const s = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

export function Inbox({ threads, projects, onOpen }: { threads: Thread[]; projects: Project[]; onOpen: (id: string) => void }) {
  const api = useApi();
  const qc = useQueryClient();
  const settle = useMutation({
    mutationFn: (id: string) => api.settle(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["threads"] }),
  });
  const items = inboxItems(threads);
  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? "";

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-11 shrink-0 items-center gap-3 border-b border-line px-4">
        <span className="font-medium">Needs you</span>
        <span className="text-faint">{items.length}</span>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {items.length === 0 && (
          <div className="grid h-full place-items-center text-center text-dim">
            <div>
              <div className="mb-1 text-fg">All clear</div>
              Agents that need you, got stuck, or finished work show up here.
            </div>
          </div>
        )}
        {GROUPS.map((g) => {
          const group = items.filter((t) => t.status === g.status);
          if (!group.length) return null;
          return (
            <section key={g.status}>
              <div className="sticky top-0 flex items-baseline gap-2 border-b border-line bg-panel px-4 py-1.5 text-[12px]">
                <span className="font-medium">{g.title}</span>
                <span className="text-faint">{group.length}</span>
                <span className="text-[11px] text-faint">· {g.hint}</span>
              </div>
              {group.map((t) => (
                <div key={t.id} className="group flex items-center gap-3 border-b border-line/50 px-4 py-2 hover:bg-raised/50">
                  <StatusDot status={t.status} />
                  <button onClick={() => onOpen(t.id)} className="min-w-0 flex-1 text-left">
                    <div className="truncate">{t.title}</div>
                    <div className="text-[11px] text-faint">
                      {projectName(t.project_id)} · {t.harness} · ${t.cost_usd.toFixed(2)}
                      {t.heal_attempts > 0 && ` · ${t.heal_attempts} heal attempt${t.heal_attempts > 1 ? "s" : ""}`} · {ago(t.updated_at)}
                    </div>
                  </button>
                  <Button variant="ghost" onClick={() => onOpen(t.id)}>
                    Open
                  </Button>
                  <Button variant="ghost" onClick={() => settle.mutate(t.id)} title="Hide until something changes">
                    Done for now
                  </Button>
                </div>
              ))}
            </section>
          );
        })}
      </div>
    </div>
  );
}
