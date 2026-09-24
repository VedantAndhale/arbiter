import { useEffect, useId, useRef, type ReactNode, type ButtonHTMLAttributes, type InputHTMLAttributes } from "react";
import type { ThreadStatus } from "../api";

export function Button({ className = "", variant = "default", onClick, ...p }: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "primary" | "ghost";
}) {
  const styles = {
    default: "border border-line bg-raised hover:bg-line",
    primary: "bg-accent text-bg font-medium hover:brightness-110",
    ghost: "hover:bg-raised text-dim hover:text-fg",
  }[variant];
  return (
    <button
      onClick={onClick}
      className={`inline-flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12px] transition disabled:opacity-40 ${styles} ${className}`}
      {...p}
    />
  );
}

export function Input({ className = "", ...p }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={`h-8 w-full rounded-md border border-line bg-bg px-2.5 outline-none placeholder:text-faint focus:border-accent ${className}`}
      {...p}
    />
  );
}

export function Dialog({ title, onClose, children, variant = "default" }: { title: string; onClose: () => void; children: ReactNode; variant?: "default" | "viewer" | "navigation" }) {
  const ref = useRef<HTMLDialogElement>(null), id = useId();
  const returnFocus = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const close = useRef(onClose); close.current = onClose;
  useEffect(() => {
    const d = ref.current!;
    d.showModal();
    return () => {
      d.close();
      if (returnFocus.current?.isConnected) returnFocus.current.focus();
    };
  }, []);
  return <dialog ref={ref} aria-labelledby={id} onCancel={e => { e.preventDefault(); close.current(); }} className={`max-h-[95dvh] overflow-auto rounded-lg border border-line bg-panel p-4 text-fg shadow-xl backdrop:bg-black/60 ${variant === "viewer" ? "m-auto w-[96vw]" : variant === "navigation" ? "m-0 h-dvh w-[min(20rem,95vw)]" : "m-auto w-[min(32rem,94vw)]"}`}>
    <div className="sticky top-0 z-10 mb-3 flex items-center justify-between gap-3 bg-panel py-1"><h2 id={id} className="text-base font-semibold">{title}</h2>{variant !== "default" && <Button onClick={onClose} aria-label={`Close ${title}`}>Close</Button>}</div>{children}
  </dialog>;
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  titles,
}: {
  value: T;
  options: readonly T[];
  onChange: (v: T) => void;
  titles?: Partial<Record<T, string>>;
}) {
  return (
    <div className="flex rounded-md border border-line p-0.5">
      {options.map((o) => (
        <button
          type="button"
          key={o}
          title={titles?.[o]}
          onClick={() => onChange(o)}
          className={`rounded px-2 py-0.5 text-[11px] ${value === o ? "bg-raised text-fg" : "text-faint hover:text-dim"}`}
        >
          {o}
        </button>
      ))}
    </div>
  );
}

const STATUS: Record<ThreadStatus, { label: string; color: string }> = {
  idle: { label: "Idle", color: "bg-faint" },
  running: { label: "Working", color: "bg-accent animate-pulse" },
  healing: { label: "Fixing problems", color: "bg-warn animate-pulse" },
  needs_approval: { label: "Needs you", color: "bg-warn" },
  review: { label: "Ready for you", color: "bg-ok" },
  failed: { label: "Stopped", color: "bg-bad" },
  merged: { label: "Done", color: "bg-ok/50" },
};

export function StatusDot({ status }: { status: ThreadStatus }) {
  return <span className={`inline-block size-2 shrink-0 rounded-full ${STATUS[status].color}`} title={STATUS[status].label} />;
}

export function StatusChip({ status }: { status: ThreadStatus }) {
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full border border-line px-2 py-0.5 text-[11px] text-dim">
      <StatusDot status={status} />
      {STATUS[status].label}
    </span>
  );
}

export function fmtTokens(n: number): string {
  return n >= 1_000_000 ? `${(n / 1e6).toFixed(1)}M` : n >= 1000 ? `${(n / 1e3).toFixed(1)}k` : `${n}`;
}
