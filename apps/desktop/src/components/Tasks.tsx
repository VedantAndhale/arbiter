// Linear-style tasks. A task is the "what"; threads are agents working on it.
// Status follows the agents automatically (started → In Progress, agent done
// → In Review); you close tasks out as Done or Canceled.

import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { Priority, Project, Task, TaskStatus, Thread } from "../api";
import { Button, Segmented, StatusDot } from "./ui";
import { AccessControls, ModelPicker, useRememberedConfig } from "./RunControls";

export const STATUSES: { id: TaskStatus; label: string; icon: string; color: string }[] = [
  { id: "backlog", label: "Backlog", icon: "◌", color: "text-faint" },
  { id: "todo", label: "Todo", icon: "○", color: "text-dim" },
  { id: "in_progress", label: "In Progress", icon: "◐", color: "text-warn" },
  { id: "in_review", label: "In Review", icon: "◑", color: "text-accent" },
  { id: "done", label: "Done", icon: "●", color: "text-ok" },
  { id: "canceled", label: "Canceled", icon: "⊘", color: "text-faint" },
];
const statusMeta = (s: TaskStatus) => STATUSES.find((x) => x.id === s)!;

export const PRIORITIES: { id: Priority; label: string; icon: string; color: string }[] = [
  { id: "urgent", label: "Urgent", icon: "!", color: "text-bad" },
  { id: "high", label: "High", icon: "▮▮▮", color: "text-dim" },
  { id: "medium", label: "Medium", icon: "▮▮▯", color: "text-dim" },
  { id: "low", label: "Low", icon: "▮▯▯", color: "text-faint" },
  { id: "none", label: "No priority", icon: "–", color: "text-faint" },
];
const priorityMeta = (p: Priority) => PRIORITIES.find((x) => x.id === p)!;

type View = "list" | "board";

function storedView(): View {
  try {
    return localStorage.getItem("arb.tasks.view") === "board" ? "board" : "list";
  } catch {
    return "list";
  }
}

export function TasksView({
  projects,
  threads,
  onOpenThread,
}: {
  projects: Project[];
  threads: Thread[];
  onOpenThread: (id: string) => void;
}) {
  const api = useApi();
  const qc = useQueryClient();
  const [projectId, setProjectId] = useState(projects[0]?.id ?? "");
  const [view, setViewState] = useState<View>(storedView);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const quickAdd = useRef<HTMLInputElement>(null);
  const tasks = useQuery({ queryKey: ["tasks", projectId], queryFn: () => api.tasks(projectId), enabled: !!projectId });
  const create = useMutation({
    mutationFn: (title: string) => api.createTask({ project_id: projectId, title }),
    onSuccess: (t) => {
      setDraft("");
      qc.invalidateQueries({ queryKey: ["tasks"] });
      setSelected(t.id);
    },
  });
  const patch = useMutation({
    mutationFn: ({ id, ...b }: { id: string } & Parameters<typeof api.patchTask>[1]) => api.patchTask(id, b),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["tasks"] }),
  });

  const setView = (v: View) => {
    setViewState(v);
    try {
      localStorage.setItem("arb.tasks.view", v);
    } catch {
      /* per-session only */
    }
  };

  // Linear-style: press C anywhere (outside inputs) to create a task.
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const typing = (e.target as HTMLElement)?.closest("input, textarea, select, [contenteditable]");
      if (!typing && e.key.toLowerCase() === "c" && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        quickAdd.current?.focus();
      }
      if (e.key === "Escape") setSelected(null);
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  const threadById = useMemo(() => new Map(threads.map((t) => [t.id, t])), [threads]);
  const list = tasks.data ?? [];
  const task = list.find((t) => t.id === selected);

  return (
    <div className="flex h-full">
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-11 shrink-0 items-center gap-3 border-b border-line px-4">
          <span className="font-medium">Tasks</span>
          {projects.length > 1 && (
            <select
              className="h-7 rounded-md border border-line bg-bg px-1.5 text-[12px]"
              value={projectId}
              onChange={(e) => setProjectId(e.target.value)}
            >
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          )}
          <div className="flex-1" />
          <Segmented value={view} options={["list", "board"] as const} onChange={setView} />
        </header>
        <form
          noValidate
          className="flex shrink-0 items-center gap-2 border-b border-line px-4 py-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (draft.trim() && !create.isPending) create.mutate(draft.trim());
          }}
        >
          <span className="text-faint">+</span>
          <input
            ref={quickAdd}
            className="h-7 flex-1 bg-transparent outline-none placeholder:text-faint"
            placeholder="New task title…  (press C to focus, Enter to add)"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
          {create.error && <span className="text-[11px] text-bad">{create.error.message}</span>}
        </form>
        <div className="min-h-0 flex-1 overflow-auto">
          {tasks.isSuccess && list.length === 0 ? (
            <div className="grid h-full place-items-center text-center text-dim">
              <div>
                <div className="mb-1 text-fg">No tasks yet</div>
                Add one above, then hand it to an agent with <b>Start agent</b>.
              </div>
            </div>
          ) : view === "list" ? (
            <ListView tasks={list} threadById={threadById} selected={selected} onSelect={setSelected} onPatch={(id, b) => patch.mutate({ id, ...b })} />
          ) : (
            <BoardView tasks={list} threadById={threadById} onSelect={setSelected} onPatch={(id, b) => patch.mutate({ id, ...b })} />
          )}
        </div>
      </div>
      {task && (
        <TaskPanel
          key={task.id}
          task={task}
          threadById={threadById}
          onClose={() => setSelected(null)}
          onPatch={(b) => patch.mutate({ id: task.id, ...b })}
          onOpenThread={onOpenThread}
        />
      )}
    </div>
  );
}

type Patch = Parameters<ReturnType<typeof useApi>["patchTask"]>[1];

function AgentDots({ task, threadById }: { task: Task; threadById: Map<string, Thread> }) {
  const ts = task.thread_ids.map((id) => threadById.get(id)).filter((t): t is Thread => !!t);
  if (!ts.length) return null;
  return (
    <span className="flex items-center gap-1" title={`${ts.length} agent thread${ts.length > 1 ? "s" : ""}`}>
      {ts.slice(0, 3).map((t) => (
        <StatusDot key={t.id} status={t.status} />
      ))}
    </span>
  );
}

function Labels({ labels }: { labels: string[] }) {
  return (
    <>
      {labels.map((l) => (
        <span key={l} className="rounded-full border border-line px-1.5 text-[10px] text-dim">
          {l}
        </span>
      ))}
    </>
  );
}

function ListView({
  tasks,
  threadById,
  selected,
  onSelect,
  onPatch,
}: {
  tasks: Task[];
  threadById: Map<string, Thread>;
  selected: string | null;
  onSelect: (id: string) => void;
  onPatch: (id: string, b: Patch) => void;
}) {
  // Active work first, like Linear's "My issues".
  const order: TaskStatus[] = ["in_progress", "in_review", "todo", "backlog", "done", "canceled"];
  return (
    <div className="pb-6">
      {order.map((s) => {
        const group = tasks.filter((t) => t.status === s);
        if (!group.length) return null;
        const m = statusMeta(s);
        return (
          <section key={s}>
            <div className="sticky top-0 z-10 flex items-center gap-2 border-b border-line bg-panel px-4 py-1.5 text-[12px]">
              <span className={m.color}>{m.icon}</span>
              <span className="font-medium">{m.label}</span>
              <span className="text-faint">{group.length}</span>
            </div>
            {group.map((t) => (
              <div
                key={t.id}
                onClick={() => onSelect(t.id)}
                className={`group flex cursor-default items-center gap-3 border-b border-line/50 px-4 py-1.5 ${
                  t.id === selected ? "bg-raised" : "hover:bg-raised/50"
                }`}
              >
                <PriorityMenu value={t.priority} onChange={(p) => onPatch(t.id, { priority: p })} />
                <span className="w-16 shrink-0 font-mono text-[11px] text-faint">{t.key}</span>
                <StatusMenu value={t.status} onChange={(st) => onPatch(t.id, { status: st })} />
                <span className="min-w-0 flex-1 truncate">{t.title}</span>
                <Labels labels={t.labels} />
                <AgentDots task={t} threadById={threadById} />
              </div>
            ))}
          </section>
        );
      })}
    </div>
  );
}

function BoardView({
  tasks,
  threadById,
  onSelect,
  onPatch,
}: {
  tasks: Task[];
  threadById: Map<string, Thread>;
  onSelect: (id: string) => void;
  onPatch: (id: string, b: Patch) => void;
}) {
  const [over, setOver] = useState<TaskStatus | null>(null);
  const columns = STATUSES.filter((s) => s.id !== "canceled");
  return (
    <div className="flex h-full gap-3 p-3">
      {columns.map((c) => {
        const col = tasks.filter((t) => t.status === c.id);
        return (
          <div
            key={c.id}
            onDragOver={(e) => {
              e.preventDefault();
              setOver(c.id);
            }}
            onDragLeave={() => setOver((o) => (o === c.id ? null : o))}
            onDrop={(e) => {
              e.preventDefault();
              setOver(null);
              const id = e.dataTransfer.getData("text/task");
              if (id) onPatch(id, { status: c.id });
            }}
            className={`flex w-64 shrink-0 flex-col rounded-lg border ${over === c.id ? "border-accent bg-accent/5" : "border-line bg-panel/40"}`}
          >
            <div className="flex items-center gap-2 px-3 py-2 text-[12px]">
              <span className={c.color}>{c.icon}</span>
              <span className="font-medium">{c.label}</span>
              <span className="text-faint">{col.length}</span>
            </div>
            <div className="min-h-0 flex-1 space-y-2 overflow-y-auto px-2 pb-2">
              {col.map((t) => (
                <div
                  key={t.id}
                  draggable
                  onDragStart={(e) => e.dataTransfer.setData("text/task", t.id)}
                  onClick={() => onSelect(t.id)}
                  className="cursor-grab rounded-md border border-line bg-panel p-2 hover:border-dim/40 active:cursor-grabbing"
                >
                  <div className="mb-1 flex items-center gap-2 text-[11px] text-faint">
                    <span className="font-mono">{t.key}</span>
                    <div className="flex-1" />
                    <AgentDots task={t} threadById={threadById} />
                    <span className={priorityMeta(t.priority).color} title={priorityMeta(t.priority).label}>
                      {priorityMeta(t.priority).icon}
                    </span>
                  </div>
                  <div className="text-[12px] leading-snug">{t.title}</div>
                  {t.labels.length > 0 && (
                    <div className="mt-1.5 flex flex-wrap gap-1">
                      <Labels labels={t.labels} />
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function Menu<T extends string>({
  value,
  options,
  onChange,
  render,
  title,
}: {
  value: T;
  options: { id: T; label: string; icon: string; color: string }[];
  onChange: (v: T) => void;
  render: (o: { icon: string; color: string; label: string }) => React.ReactNode;
  title: string;
}) {
  const cur = options.find((o) => o.id === value)!;
  return (
    <label className="relative shrink-0" title={`${title}: ${cur.label}`} onClick={(e) => e.stopPropagation()}>
      {render(cur)}
      <select
        className="absolute inset-0 cursor-pointer opacity-0"
        value={value}
        onChange={(e) => onChange(e.target.value as T)}
      >
        {options.map((o) => (
          <option key={o.id} value={o.id}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function StatusMenu({ value, onChange }: { value: TaskStatus; onChange: (s: TaskStatus) => void }) {
  return (
    <Menu title="Status" value={value} options={STATUSES} onChange={onChange} render={(o) => <span className={`w-4 text-center ${o.color}`}>{o.icon}</span>} />
  );
}

function PriorityMenu({ value, onChange }: { value: Priority; onChange: (p: Priority) => void }) {
  return (
    <Menu
      title="Priority"
      value={value}
      options={PRIORITIES}
      onChange={onChange}
      render={(o) => <span className={`inline-block w-7 font-mono text-[10px] ${o.color}`}>{o.icon}</span>}
    />
  );
}

function TaskPanel({
  task,
  threadById,
  onClose,
  onPatch,
  onOpenThread,
}: {
  task: Task;
  threadById: Map<string, Thread>;
  onClose: () => void;
  onPatch: (b: Patch) => void;
  onOpenThread: (id: string) => void;
}) {
  const api = useApi();
  const qc = useQueryClient();
  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [labels, setLabels] = useState(task.labels.join(", "));
  const [cfg, setCfg] = useRememberedConfig(task.project_id);
  const start = useMutation({
    // Save unsaved edits first: typing a description and clicking Start right
    // away must not send the agent the stale version.
    mutationFn: async () => {
      if (title.trim() && (title !== task.title || description !== task.description)) {
        await api.patchTask(task.id, { title: title.trim(), description });
      }
      return api.startTask(task.id, { ...cfg, worktree: true });
    },
    onSuccess: ({ thread }) => {
      qc.invalidateQueries({ queryKey: ["tasks"] });
      qc.invalidateQueries({ queryKey: ["threads"] });
      onOpenThread(thread.id);
    },
  });
  const threads = task.thread_ids.map((id) => threadById.get(id)).filter((t): t is Thread => !!t);
  const saveLabels = () => {
    const next = labels.split(",").map((l) => l.trim()).filter(Boolean);
    if (next.join() !== task.labels.join()) onPatch({ labels: next });
  };

  return (
    <aside className="flex w-[380px] shrink-0 flex-col border-l border-line bg-panel">
      <div className="flex h-11 shrink-0 items-center gap-2 border-b border-line px-4">
        <span className="font-mono text-[12px] text-faint">{task.key}</span>
        <div className="flex-1" />
        <button onClick={onClose} className="rounded px-1.5 text-faint hover:bg-raised hover:text-fg" title="Close (Esc)">
          ✕
        </button>
      </div>
      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
        <input
          className="w-full bg-transparent text-base font-semibold outline-none"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={() => title.trim() && title !== task.title && onPatch({ title })}
        />
        <textarea
          aria-label="Task description"
          rows={Math.max(5, Math.min(20, description.split("\n").length))}
          className="min-h-28 w-full resize-none rounded-md border border-line bg-bg p-2 text-[12px] leading-relaxed outline-none placeholder:text-faint focus:border-accent"
          placeholder="Description, acceptance criteria, links… (sent to the agent when you start it)"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          onBlur={() => description !== task.description && onPatch({ description })}
        />
        <div className="grid grid-cols-[80px_1fr] items-center gap-y-2 text-[12px]">
          <span className="text-faint">Status</span>
          <select
            className="h-7 rounded-md border border-line bg-bg px-1.5"
            value={task.status}
            onChange={(e) => onPatch({ status: e.target.value as TaskStatus })}
          >
            {STATUSES.map((s) => (
              <option key={s.id} value={s.id}>
                {s.icon} {s.label}
              </option>
            ))}
          </select>
          <span className="text-faint">Priority</span>
          <select
            className="h-7 rounded-md border border-line bg-bg px-1.5"
            value={task.priority}
            onChange={(e) => onPatch({ priority: e.target.value as Priority })}
          >
            {PRIORITIES.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
          <span className="text-faint">Labels</span>
          <input
            className="h-7 rounded-md border border-line bg-bg px-2 outline-none placeholder:text-faint focus:border-accent"
            placeholder="ui, bug"
            value={labels}
            onChange={(e) => setLabels(e.target.value)}
            onBlur={saveLabels}
            onKeyDown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
          />
        </div>

        <section>
          <div className="mb-1.5 text-[11px] font-medium uppercase tracking-wide text-faint">Agents</div>
          {threads.length === 0 && <div className="mb-2 text-[12px] text-dim">No agent has worked on this yet.</div>}
          <div className="space-y-1">
            {threads.map((t) => (
              <button
                key={t.id}
                onClick={() => onOpenThread(t.id)}
                className="flex w-full items-center gap-2 rounded-md border border-line px-2 py-1.5 text-left text-[12px] hover:bg-raised"
              >
                <StatusDot status={t.status} />
                <span className="min-w-0 flex-1 truncate">{t.title}</span>
                <span className="text-faint">{t.harness}</span>
              </button>
            ))}
          </div>
          <div className="mt-3 rounded-lg border border-line bg-bg">
            <div className="flex flex-wrap items-center gap-1 p-1.5">
              <ModelPicker value={cfg} onChange={setCfg} />
              <AccessControls value={cfg} onChange={setCfg} />
            </div>
            <div className="border-t border-line p-1.5">
              <Button variant="primary" className="w-full justify-center" onClick={() => start.mutate()} disabled={start.isPending}>
                {start.isPending ? "Starting…" : threads.length ? "Start another agent" : "Start agent"}
              </Button>
            </div>
          </div>
          {start.error && <div className="mt-1 text-[11px] text-bad">{start.error.message}</div>}
        </section>
      </div>
    </aside>
  );
}
