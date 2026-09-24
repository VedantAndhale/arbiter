import { useEffect, useMemo, useState } from "react";
import type { Thread } from "../api";
import { ACTIONS, type ActionId, comboOf, label, rebind, saveBindings, usable } from "../keymap";
import { type NotifyState, notifyState, setNotifications } from "../notify";
import { Button, Dialog, Input } from "./ui";

type Item = { key: string; label: string; hint?: string; run: () => void };

/** Ctrl+K: run an app action or jump to a task by title. */
export function Palette({ bindings, threads, run, openThread, onShortcuts, onClose }: {
  bindings: Record<ActionId, string>;
  threads: Thread[];
  run: (id: ActionId) => void;
  openThread: (id: string) => void;
  onShortcuts: () => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState(""), [index, setIndex] = useState(0);
  const [notifications, setNotify] = useState<NotifyState>(notifyState);
  const items = useMemo<Item[]>(() => {
    const q = query.trim().toLowerCase();
    const actions: Item[] = ACTIONS.filter(a => a.id !== "palette").map(a => ({ key: a.id, label: a.label, hint: bindings[a.id] ? label(bindings[a.id]) : undefined, run: () => run(a.id) }));
    actions.push({ key: "shortcuts", label: "Keyboard shortcuts…", run: onShortcuts });
    if (notifications !== "unsupported") actions.push({
      key: "notify",
      label: notifications === "on" ? "Turn off desktop notifications" : notifications === "denied" ? "Desktop notifications are blocked by the system" : "Turn on desktop notifications",
      run: () => { if (notifications !== "denied") setNotifications(notifications !== "on").then(setNotify); },
    });
    const tasks: Item[] = threads
      .filter(t => !q || t.title.toLowerCase().includes(q))
      .slice(0, 20)
      .map(t => ({ key: t.id, label: t.title, hint: t.status.replace("_", " "), run: () => openThread(t.id) }));
    return [...actions.filter(a => !q || a.label.toLowerCase().includes(q)), ...tasks];
  }, [query, bindings, threads, run, openThread, onShortcuts, notifications]);
  useEffect(() => setIndex(0), [query]);
  const choose = (item?: Item) => {
    if (!item) return;
    // Notification toggles keep the palette open so the result is visible.
    if (item.key !== "notify") onClose();
    item.run();
  };
  return <Dialog title="Command palette" onClose={onClose}>
    <Input autoFocus aria-label="Search actions and tasks" role="combobox" aria-expanded="true" aria-controls="palette-list" aria-activedescendant={items[index] ? `palette-${items[index].key}` : undefined}
      value={query} onChange={e => setQuery(e.target.value)} placeholder="Type an action or a task name…"
      onKeyDown={e => {
        if (e.key === "ArrowDown") { e.preventDefault(); setIndex(i => Math.min(i + 1, items.length - 1)); }
        else if (e.key === "ArrowUp") { e.preventDefault(); setIndex(i => Math.max(i - 1, 0)); }
        else if (e.key === "Enter") { e.preventDefault(); choose(items[index]); }
      }} />
    <ul id="palette-list" role="listbox" aria-label="Results" className="mt-2 max-h-80 overflow-y-auto">
      {items.map((item, i) => <li key={item.key} id={`palette-${item.key}`} role="option" aria-selected={i === index}>
        <button type="button" tabIndex={-1} onMouseEnter={() => setIndex(i)} onClick={() => choose(item)} className={`flex w-full items-center gap-3 rounded px-2 py-1.5 text-left ${i === index ? "bg-raised" : ""}`}>
          <span className="min-w-0 flex-1 truncate">{item.label}</span>
          {item.hint && <span className="shrink-0 text-[12px] text-dim">{item.hint}</span>}
        </button>
      </li>)}
      {items.length === 0 && <li className="px-2 py-3 text-dim">No matching actions or tasks.</li>}
    </ul>
    {notifications === "on" && <p className="mt-2 text-[12px] text-dim">Notifications appear when a task needs you while Arbiter is in the background.</p>}
  </Dialog>;
}

/** View and change the app shortcuts. A shortcut needs Ctrl (⌘) or Alt so it
 *  never fires while typing. */
export function Shortcuts({ bindings, onChange, onClose }: { bindings: Record<ActionId, string>; onChange: (b: Record<ActionId, string>) => void; onClose: () => void }) {
  const [capturing, setCapturing] = useState<ActionId | null>(null), [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!capturing) return;
    const h = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") { setCapturing(null); return; }
      const combo = comboOf(e);
      if (!combo) return;
      if (!usable(combo)) { setError("Include Ctrl (⌘) or Alt so the shortcut does not fire while typing."); return; }
      const next = rebind(bindings, capturing, combo);
      saveBindings(next);
      onChange(next);
      setCapturing(null);
      setError(null);
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  }, [capturing, bindings, onChange]);
  const reset = () => { const d = Object.fromEntries(ACTIONS.map(a => [a.id, a.binding])) as Record<ActionId, string>; saveBindings(d); onChange(d); };
  return <Dialog title="Keyboard shortcuts" onClose={() => capturing ? setCapturing(null) : onClose()}>
    <ul className="space-y-1">{ACTIONS.map(a => <li key={a.id} className="flex items-center gap-3">
      <span className="min-w-0 flex-1">{a.label}</span>
      <kbd className="shrink-0 rounded border border-line px-1.5 text-[12px] text-dim">{capturing === a.id ? "Press keys…" : label(bindings[a.id])}</kbd>
      <Button onClick={() => { setError(null); setCapturing(a.id); }} aria-label={`Change shortcut for ${a.label}`}>Change</Button>
      {bindings[a.id] && <Button onClick={() => { const next = rebind(bindings, a.id, ""); saveBindings(next); onChange(next); }} aria-label={`Clear shortcut for ${a.label}`}>Clear</Button>}
    </li>)}</ul>
    {capturing && <p role="status" className="mt-2 text-[12px] text-dim">Press the new shortcut, or Escape to cancel. A shortcut already in use moves to this action.</p>}
    {error && <p role="alert" className="mt-2 text-[12px] text-warn">{error}</p>}
    <div className="mt-3 flex justify-between gap-2"><Button onClick={reset}>Restore defaults</Button><Button onClick={onClose}>Done</Button></div>
  </Dialog>;
}
