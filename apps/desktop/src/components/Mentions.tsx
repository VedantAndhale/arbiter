import { type KeyboardEvent, type RefObject, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useApi } from "../ApiContext";

type Item = { kind: "project" | "folder" | "file" | "note"; value: string; label: string; hint?: string };

const LIMIT = 12;

/** The `@word` being typed right before the caret, if any. */
function activeToken(text: string, caret: number): { start: number; query: string } | null {
  const m = /(?:^|\s)@([^\s@]*)$/.exec(text.slice(0, caret));
  return m ? { start: caret - m[1].length - 1, query: m[1] } : null;
}

function rank(path: string, q: string): number {
  const p = path.toLowerCase(), base = p.split("/").pop() ?? p;
  if (base.startsWith(q)) return 0;
  if (p.startsWith(q)) return 1;
  if (base.includes(q)) return 2;
  return p.includes(q) ? 3 : -1;
}

/**
 * Inline `@` mentions for a composer, like Claude Code: typing `@` lists the
 * project's folders and files (and memory notes inside a task), filtered as
 * you type. ↑/↓ move, Enter or Tab inserts, Esc closes.
 */
export function useMentions({ text, setText, input, projectId, threadId, projects, onProject }: {
  text: string;
  setText: (t: string) => void;
  input: RefObject<HTMLTextAreaElement | null>;
  projectId?: string;
  threadId?: string;
  /** When given, `@` also offers projects; picking one calls `onProject`. */
  projects?: { id: string; name: string; path: string }[];
  onProject?: (id: string) => void;
}) {
  const api = useApi();
  const [caret, setCaret] = useState(0);
  const [index, setIndex] = useState(0);
  const [dismissed, setDismissed] = useState<number | null>(null);
  const token = activeToken(text, caret);
  const open = !!token && dismissed !== token.start;
  const files = useQuery({ queryKey: ["project-files", projectId], queryFn: () => api.projectFiles(projectId!), enabled: (open || text.includes("@")) && !!projectId, staleTime: 30_000 });
  const notes = useQuery({ queryKey: ["vault", threadId], queryFn: () => api.vault(threadId!), enabled: open && !!threadId });
  const items = useMemo<Item[]>(() => {
    if (!token) return [];
    const q = token.query.toLowerCase();
    const paths = files.data ?? [];
    const folders = [...new Set(paths.flatMap(p => p.split("/").slice(0, -1).map((_, i, parts) => parts.slice(0, i + 1).join("/"))))];
    const noteItems: Item[] = (notes.data?.notes ?? []).map(n => ({ kind: "note", value: `[[${n.id}]]`, label: n.title, hint: n.id }));
    const projectItems: Item[] = (projects ?? []).filter(p => p.id !== projectId).map(p => ({ kind: "project", value: p.id, label: p.name, hint: "project" }));
    if (!q) {
      // Like a folder listing: top-level folders, then top-level files.
      const top: Item[] = [
        ...folders.filter(f => !f.includes("/")).sort().map(f => ({ kind: "folder" as const, value: f, label: f })),
        ...paths.filter(p => !p.includes("/")).sort().map(p => ({ kind: "file" as const, value: p, label: p })),
      ];
      return [...projectItems.slice(0, 5), ...noteItems.slice(0, 3), ...top].slice(0, LIMIT);
    }
    const scored: [number, Item][] = [
      ...folders.map(f => [rank(f, q), { kind: "folder", value: f, label: f }] as [number, Item]),
      ...paths.map(p => [rank(p, q), { kind: "file", value: p, label: p }] as [number, Item]),
      ...noteItems.map(n => [rank(`${n.hint} ${n.label}`, q), n] as [number, Item]),
      // Projects sort just ahead of files with the same match quality.
      ...projectItems.map(p => { const r = rank(p.label, q); return [r < 0 ? -1 : r - 0.5, p] as [number, Item]; }),
    ];
    return scored.filter(([r]) => r >= -0.5).sort((a, b) => a[0] - b[0] || a[1].label.length - b[1].label.length).slice(0, LIMIT).map(([, i]) => i);
  }, [token, files.data, notes.data, projects, projectId]);

  const pick = (item: Item) => {
    if (!token) return;
    if (item.kind === "project") {
      // A project is a choice, not text: remove the @word and select it.
      const next = text.slice(0, token.start) + text.slice(caret);
      setText(next);
      setCaret(token.start);
      setIndex(0);
      onProject?.(item.value);
      requestAnimationFrame(() => { input.current?.focus(); input.current?.setSelectionRange(token.start, token.start); });
      return;
    }
    const insert = item.kind === "note" ? `${item.value} ` : `@${item.value}${item.kind === "folder" ? "/" : ""} `;
    const next = text.slice(0, token.start) + insert + text.slice(caret);
    setText(next);
    const at = token.start + insert.length;
    setCaret(at);
    setIndex(0);
    requestAnimationFrame(() => { input.current?.focus(); input.current?.setSelectionRange(at, at); });
  };

  /** Call first from the textarea's onKeyDown; true means the key was used. */
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!open || items.length === 0) return false;
    if (e.key === "ArrowDown") { e.preventDefault(); setIndex(i => (i + 1) % items.length); return true; }
    if (e.key === "ArrowUp") { e.preventDefault(); setIndex(i => (i - 1 + items.length) % items.length); return true; }
    if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); pick(items[Math.min(index, items.length - 1)]); return true; }
    if (e.key === "Escape") { e.preventDefault(); setDismissed(token!.start); return true; }
    return false;
  };
  /** Keep the caret position current: wire to onSelect/onKeyUp/onClick/onChange. */
  const track = () => { const el = input.current; if (el) setCaret(el.selectionStart ?? el.value.length); };

  const menu = open && ((files.isPending && !!projectId) || items.length > 0) ? (
    <div role="listbox" aria-label="Mention a project, file, folder or note" className="absolute bottom-full left-0 z-20 mb-1 max-h-72 w-full max-w-md overflow-y-auto rounded-lg border border-line bg-panel py-1 shadow-xl">
      {files.isPending && items.length === 0 && <p role="status" className="px-3 py-1.5 text-[12px] text-dim">Loading project files…</p>}
      {items.map((item, i) => (
        <button key={`${item.kind}:${item.value}`} type="button" role="option" aria-selected={i === index} tabIndex={-1}
          onMouseDown={e => { e.preventDefault(); pick(item); }} onMouseEnter={() => setIndex(i)}
          className={`flex w-full items-center gap-2 px-3 py-1 text-left text-[13px] ${i === index ? "bg-raised text-fg" : "text-dim"}`}>
          <span aria-hidden className="w-4 shrink-0 text-center text-faint">{item.kind === "project" ? "▭" : item.kind === "folder" ? "▸" : item.kind === "note" ? "◆" : "·"}</span>
          <span className="min-w-0 truncate">{item.kind === "folder" ? `${item.label}/` : item.label}</span>
          {item.hint && <span className="ml-auto shrink-0 truncate text-[11px] text-faint">{item.hint}</span>}
        </button>
      ))}
    </div>
  ) : null;

  /** Files and folders currently mentioned in the text, for context paths. */
  const mentioned = useMemo(() => {
    const known = new Set(files.data ?? []);
    const found = [...text.matchAll(/(?:^|\s)@([^\s@]+)/g)].map(m => m[1].replace(/\/$/, ""));
    return [...new Set(found)].filter(p => known.has(p) || (files.data ?? []).some(f => f.startsWith(`${p}/`))).slice(0, 8);
  }, [text, files.data]);

  return { menu, onKeyDown, track, mentioned };
}

export type Command = { name: string; label: string; hint: string; run: () => void };

/**
 * Inline `/` commands: power-user controls that stay invisible until typed.
 * Picking one removes the `/word` from the text and runs it.
 */
export function useCommands({ text, setText, input, commands }: {
  text: string;
  setText: (t: string) => void;
  input: RefObject<HTMLTextAreaElement | null>;
  commands: Command[];
}) {
  const [caret, setCaret] = useState(0);
  const [index, setIndex] = useState(0);
  const [dismissed, setDismissed] = useState<number | null>(null);
  const m = /(?:^|\s)\/([a-z]*)$/i.exec(text.slice(0, caret));
  const token = m ? { start: caret - m[1].length - 1, query: m[1].toLowerCase() } : null;
  const items = token ? commands.filter(c => c.name.startsWith(token.query) || c.label.toLowerCase().includes(token.query)) : [];
  const open = !!token && dismissed !== token.start && items.length > 0;
  const pick = (c: Command) => {
    if (!token) return;
    const next = (text.slice(0, token.start) + text.slice(caret)).replace(/^\s+/, "");
    setText(next);
    setCaret(token.start);
    setIndex(0);
    c.run();
    requestAnimationFrame(() => { input.current?.focus(); const at = Math.min(token.start, next.length); input.current?.setSelectionRange(at, at); });
  };
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!open) return false;
    if (e.key === "ArrowDown") { e.preventDefault(); setIndex(i => (i + 1) % items.length); return true; }
    if (e.key === "ArrowUp") { e.preventDefault(); setIndex(i => (i - 1 + items.length) % items.length); return true; }
    if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); pick(items[Math.min(index, items.length - 1)]); return true; }
    if (e.key === "Escape") { e.preventDefault(); setDismissed(token!.start); return true; }
    return false;
  };
  const track = () => { const el = input.current; if (el) setCaret(el.selectionStart ?? el.value.length); };
  const menu = open ? (
    <div role="listbox" aria-label="Commands" className="absolute bottom-full left-0 z-20 mb-1 w-full max-w-md overflow-hidden rounded-lg border border-line bg-panel py-1 shadow-xl">
      {items.map((c, i) => (
        <button key={c.name} type="button" role="option" aria-selected={i === index} tabIndex={-1}
          onMouseDown={e => { e.preventDefault(); pick(c); }} onMouseEnter={() => setIndex(i)}
          className={`flex w-full items-baseline gap-3 px-3 py-1.5 text-left ${i === index ? "bg-raised" : ""}`}>
          <span className="w-20 shrink-0 font-mono text-[12px] text-accent">/{c.name}</span>
          <span className="min-w-0 text-[13px] text-fg">{c.label}</span>
          <span className="ml-auto hidden shrink-0 text-[11px] text-faint sm:inline">{c.hint}</span>
        </button>
      ))}
    </div>
  ) : null;
  return { menu, onKeyDown, track };
}
