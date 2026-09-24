/** Rebindable app shortcuts. Bindings are a per-viewer convenience kept in
 *  localStorage; the defaults apply whenever storage is unavailable. */
export type ActionId = "palette" | "newTask" | "tasks" | "inbox" | "toggleSidebar" | "setup" | "addProject" | "stopAgent" | "tour";

export const ACTIONS: { id: ActionId; label: string; binding: string }[] = [
  { id: "palette", label: "Open command palette", binding: "mod+k" },
  { id: "newTask", label: "New task", binding: "mod+n" },
  { id: "tasks", label: "Open the board", binding: "mod+t" },
  { id: "inbox", label: "Show what needs you", binding: "mod+i" },
  { id: "toggleSidebar", label: "Show or hide the sidebar", binding: "mod+b" },
  { id: "setup", label: "Open setup and usage", binding: "mod+," },
  { id: "addProject", label: "Add a project", binding: "" },
  { id: "stopAgent", label: "Stop the open task's agent", binding: "mod+shift+." },
  { id: "tour", label: "Take the tour", binding: "" },
];

const KEY = "arbiter.keys";

export function loadBindings(): Record<ActionId, string> {
  const defaults = Object.fromEntries(ACTIONS.map(a => [a.id, a.binding])) as Record<ActionId, string>;
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    for (const a of ACTIONS) if (typeof saved[a.id] === "string") defaults[a.id] = saved[a.id];
  } catch { /* storage unavailable: defaults */ }
  return defaults;
}

export function saveBindings(b: Record<ActionId, string>) {
  try { localStorage.setItem(KEY, JSON.stringify(b)); } catch { /* storage unavailable */ }
}

/** Assign `binding` to `id`, clearing any other action that used it. */
export function rebind(b: Record<ActionId, string>, id: ActionId, binding: string): Record<ActionId, string> {
  const next = { ...b };
  for (const k of Object.keys(next) as ActionId[]) if (binding && next[k] === binding) next[k] = "";
  next[id] = binding;
  return next;
}

/** Normalized combo for a key event, e.g. "mod+shift+k"; null for bare modifiers. */
export function comboOf(e: KeyboardEvent): string | null {
  if (["Control", "Meta", "Shift", "Alt"].includes(e.key)) return null;
  const parts = [];
  if (e.ctrlKey || e.metaKey) parts.push("mod");
  if (e.altKey) parts.push("alt");
  if (e.shiftKey) parts.push("shift");
  parts.push(e.key === " " ? "space" : e.key.length === 1 ? e.key.toLowerCase() : e.key.toLowerCase());
  return parts.join("+");
}

/** App shortcuts need a modifier, so typing in the composer never triggers them. */
export function usable(combo: string) {
  return combo.startsWith("mod+") || combo.startsWith("alt+");
}

export function label(binding: string) {
  if (!binding) return "Not set";
  const mac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
  return binding.split("+").map(p => p === "mod" ? (mac ? "⌘" : "Ctrl") : p === "shift" ? "Shift" : p === "alt" ? (mac ? "⌥" : "Alt") : p.length === 1 ? p.toUpperCase() : p[0].toUpperCase() + p.slice(1)).join(mac ? "" : "+");
}
