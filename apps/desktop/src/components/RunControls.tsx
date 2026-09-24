// Composer toolbar controls: which agent runs a thread and how. Modeled on
// t3code's composer footer: provider + model + effort in one picker, plus
// access level and a plan-mode toggle (Shift+Tab).

import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { HarnessChoice, HarnessInfo, ModelInfo, PermissionMode, RunConfig } from "../api";

export const DEFAULT_CONFIG: RunConfig = { harness: "auto", model: null, effort: null, permission: "safe", tool_profile: "auto" };

export function useHarnesses() {
  const api = useApi();
  return useQuery({ queryKey: ["harnesses"], queryFn: () => api.harnesses(), staleTime: 10 * 60_000 });
}

/** Last-used config per project, so new threads start where you left off. */
export function useRememberedConfig(projectId: string | undefined): [RunConfig, (c: RunConfig) => void] {
  const key = `arb.cfg.${projectId ?? "none"}`;
  const read = (): RunConfig => {
    try {
      return { ...DEFAULT_CONFIG, ...JSON.parse(localStorage.getItem(key) ?? "{}") };
    } catch {
      return DEFAULT_CONFIG;
    }
  };
  const [cfg, setCfg] = useState<RunConfig>(read);
  useEffect(() => setCfg(read()), [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const save = (c: RunConfig) => {
    setCfg(c);
    try {
      localStorage.setItem(key, JSON.stringify(c));
    } catch {
      /* storage unavailable: remember for this session only */
    }
  };
  return [cfg, save];
}

function modelOf(harnesses: HarnessInfo[] | undefined, cfg: RunConfig): ModelInfo | undefined {
  return harnesses?.find((h) => h.id === cfg.harness)?.models.find((m) => m.id === cfg.model);
}

export function configLabel(harnesses: HarnessInfo[] | undefined, cfg: RunConfig): string {
  if (cfg.harness === "auto") return "Auto";
  const h = harnesses?.find((x) => x.id === cfg.harness);
  const model = modelOf(harnesses, cfg)?.name ?? cfg.model ?? "Default";
  return [h?.name ?? cfg.harness, model, cfg.effort].filter(Boolean).join(" · ");
}

function useClickOutside(onOutside: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const h = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && onOutside();
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [onOutside]);
  return ref;
}

/**
 * Provider rail on the left, models on the right, effort underneath.
 * `lockedHarness` pins the provider once a thread's agent has started.
 */
export function ModelPicker({
  value,
  onChange,
  lockedHarness,
}: {
  value: RunConfig;
  onChange: (c: RunConfig) => void;
  lockedHarness?: "claude" | "codex" | "local";
}) {
  const harnesses = useHarnesses();
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<HarnessChoice>(lockedHarness ?? value.harness);
  const ref = useClickOutside(() => setOpen(false));
  useEffect(() => {
    if (open) setTab(lockedHarness ?? value.harness);
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const list = harnesses.data ?? [];
  const current = list.find((h) => h.id === tab);
  const selectedModel = modelOf(list, value);
  const efforts = value.harness !== "auto" ? (selectedModel?.efforts ?? []) : [];

  const pick = (harness: HarnessChoice, model: ModelInfo | null) => {
    const sameHarness = harness === value.harness;
    const keepEffort = sameHarness && model?.efforts.includes(value.effort ?? "") ? value.effort : null;
    onChange({ ...value, harness, model: model?.id ?? null, effort: keepEffort });
    if (!model?.efforts.length) setOpen(false);
  };

  return (
    <div className="relative" ref={ref}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex h-7 items-center gap-1.5 rounded-md px-2 text-[12px] text-dim hover:bg-raised hover:text-fg"
        title="Agent and model (Ctrl+Shift+M)"
      >
        <span className={`size-1.5 rounded-full ${value.harness === "auto" ? "bg-accent" : value.harness === "claude" ? "bg-warn" : "bg-ok"}`} />
        {configLabel(list, value)}
        <span className="text-faint">▾</span>
      </button>
      {open && (
        <div className="absolute bottom-9 left-0 z-20 flex w-[460px] overflow-hidden rounded-lg border border-line bg-panel shadow-2xl">
          <div className="w-36 shrink-0 border-r border-line p-1">
            {!lockedHarness && (
              <RailItem active={tab === "auto"} onClick={() => (setTab("auto"), pick("auto", null))} label="Auto" hint="Arbiter picks" />
            )}
            {list.map((h) => (
              <RailItem
                key={h.id}
                active={tab === h.id}
                disabled={!h.installed || (lockedHarness !== undefined && lockedHarness !== h.id)}
                onClick={() => setTab(h.id)}
                label={h.name}
                hint={!h.installed ? "not installed" : lockedHarness && lockedHarness !== h.id ? "locked" : undefined}
              />
            ))}
          </div>
          <div className="min-w-0 flex-1 p-1">
            {tab === "auto" ? (
              <p className="p-2 text-[12px] leading-relaxed text-dim">
                Arbiter routes each new thread to an installed harness, preferring the one with the fewest running agents, so
                work spreads across your Claude and Codex subscriptions. The choice and reason are shown in the thread.
              </p>
            ) : (
              <>
                <div className="max-h-64 overflow-y-auto">
                  {current?.models.map((m) => {
                    const active = value.harness === tab && value.model === m.id;
                    return (
                      <button
                        type="button"
                        key={m.id ?? "default"}
                        onClick={() => pick(tab, m)}
                        className={`block w-full rounded-md px-2 py-1.5 text-left ${active ? "bg-raised" : "hover:bg-raised/60"}`}
                      >
                        <div className="flex items-center gap-2 text-[12px]">
                          <span className={active ? "text-fg" : "text-dim"}>{m.name}</span>
                          {active && <span className="text-accent">✓</span>}
                        </div>
                        {m.description && <div className="truncate text-[11px] text-faint">{m.description}</div>}
                      </button>
                    );
                  })}
                </div>
                {current?.note && <div className="px-2 py-1 text-[11px] text-warn">{current.note}</div>}
                {efforts.length > 0 && value.harness === tab && (
                  <div className="mt-1 flex flex-wrap items-center gap-1 border-t border-line px-1 pt-1.5">
                    <span className="mr-1 text-[11px] text-faint">Effort</span>
                    {[null, ...efforts].map((e) => (
                      <button
                        type="button"
                        key={e ?? "default"}
                        onClick={() => onChange({ ...value, effort: e })}
                        className={`rounded px-1.5 py-0.5 text-[11px] ${value.effort === e ? "bg-raised text-fg" : "text-faint hover:text-dim"}`}
                      >
                        {e ?? `default${selectedModel?.default_effort ? ` (${selectedModel.default_effort})` : ""}`}
                      </button>
                    ))}
                  </div>
                )}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function RailItem({ active, disabled, onClick, label, hint }: { active: boolean; disabled?: boolean; onClick: () => void; label: string; hint?: string }) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={`block w-full rounded-md px-2 py-1.5 text-left text-[12px] disabled:opacity-40 ${active ? "bg-raised text-fg" : "text-dim hover:bg-raised/60"}`}
    >
      {label}
      {hint && <div className="text-[10px] text-faint">{hint}</div>}
    </button>
  );
}

const ACCESS: Record<Exclude<PermissionMode, "plan">, { label: string; hint: string }> = {
  safe: { label: "Supervised", hint: "Reads and edits files; Claude asks here before actions requiring approval" },
  auto: { label: "Full access", hint: "Anything goes inside the worktree (Ctrl+Shift+A to toggle)" },
};

/** Access level plus plan toggle. Plan mode remembers the access level to return to. */
export function AccessControls({ value, onChange }: { value: RunConfig; onChange: (c: RunConfig) => void }) {
  const [lastAccess, setLastAccess] = useState<"safe" | "auto">(value.permission === "auto" ? "auto" : "safe");
  const plan = value.permission === "plan";
  const access = plan ? lastAccess : (value.permission as "safe" | "auto");
  return (
    <>
      <button
        type="button"
        disabled={plan}
        onClick={() => {
          const next = access === "safe" ? "auto" : "safe";
          setLastAccess(next);
          onChange({ ...value, permission: next });
        }}
        title={ACCESS[access].hint}
        className={`flex h-7 items-center gap-1 rounded-md px-2 text-[12px] hover:bg-raised disabled:opacity-40 ${access === "auto" ? "text-warn" : "text-dim"}`}
      >
        {access === "auto" ? "⚡" : "🛡"} {ACCESS[access].label}
      </button>
      <button
        type="button"
        onClick={() => onChange({ ...value, permission: plan ? lastAccess : "plan" })}
        title="Plan mode: read-only, investigate and propose (Shift+Tab)"
        className={`h-7 rounded-md px-2 text-[12px] ${plan ? "bg-accent/15 text-accent" : "text-dim hover:bg-raised"}`}
      >
        Plan
      </button>
    </>
  );
}

export function ToolProfileControl({ value, onChange }: { value: RunConfig; onChange: (c: RunConfig) => void }) {
  return <select aria-label="Agent tool profile" title="Implementation keeps coding tools; research also enables web search in lean mode" value={value.tool_profile ?? "auto"} onChange={e => onChange({ ...value, tool_profile: e.target.value as RunConfig["tool_profile"] })} className="h-7 rounded border border-line bg-bg px-1.5 text-[12px] text-dim">
    <option value="auto">Tools: Auto</option><option value="implementation">Tools: Implementation</option><option value="research">Tools: Research</option>
  </select>;
}

/** Keyboard shortcuts shared by composers: Shift+Tab plan, Ctrl+Shift+A access. */
export function handleConfigKeys(e: React.KeyboardEvent, value: RunConfig, onChange: (c: RunConfig) => void): boolean {
  if (e.key === "Tab" && e.shiftKey) {
    e.preventDefault();
    onChange({ ...value, permission: value.permission === "plan" ? "safe" : "plan" });
    return true;
  }
  if (e.key.toLowerCase() === "a" && e.shiftKey && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    onChange({ ...value, permission: value.permission === "auto" ? "safe" : "auto" });
    return true;
  }
  return false;
}
