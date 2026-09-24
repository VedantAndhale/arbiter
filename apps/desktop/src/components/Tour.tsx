import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "./ui";

type Step = { target?: string; title: string; body: string };

const STEPS: Step[] = [
  { target: "composer", title: "Describe what you want", body: "Write the outcome in your own words. Arbiter works out the approach and only asks when a choice is yours to make." },
  { target: "composer", title: "Point at things", body: "Type @ to mention a file or folder, and / for extra controls such as planning first or comparing agents." },
  { target: "needs-you", title: "Needs you", body: "Questions and approvals collect here, so you can step away while agents work." },
  { target: "add-project", title: "Add a project", body: "Open a folder you already have, or start a new one from a template such as a landing page or a booking site." },
  { target: "search", title: "Find anything", body: "Search tasks and actions from anywhere with Ctrl+K." },
  { title: "Then watch it come together", body: "When a task finishes, Arbiter starts your app in Preview and suggests the next step, like saving the changes." },
];

const KEY = "arbiter.tour.done";

export function tourDone(): boolean {
  try { return localStorage.getItem(KEY) === "1"; } catch { return true; }
}

/** A short spotlight tour over the real interface. */
export function Tour({ onClose }: { onClose: () => void }) {
  const [i, setI] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const card = useRef<HTMLDivElement>(null);
  const [cardHeight, setCardHeight] = useState(180);
  useLayoutEffect(() => { if (card.current) setCardHeight(card.current.offsetHeight); }, [i]);
  const step = STEPS[i];
  const finish = () => { try { localStorage.setItem(KEY, "1"); } catch { /* storage unavailable */ } onClose(); };
  useLayoutEffect(() => {
    const measure = () => {
      const el = step.target ? document.querySelector<HTMLElement>(`[data-tour="${step.target}"]`) : null;
      setRect(el ? el.getBoundingClientRect() : null);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [step.target]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") finish();
      else if (e.key === "ArrowRight") setI(n => Math.min(n + 1, STEPS.length - 1));
      else if (e.key === "ArrowLeft") setI(n => Math.max(n - 1, 0));
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });
  const pad = 8;
  // Below the target, else above it, else pinned to the bottom edge.
  const gap = pad + 8;
  const cardTop = !rect
    ? Math.max(16, window.innerHeight / 2 - cardHeight / 2)
    : rect.bottom + gap + cardHeight <= window.innerHeight - 16
      ? rect.bottom + gap
      : rect.top - gap - cardHeight >= 16
        ? rect.top - gap - cardHeight
        : window.innerHeight - cardHeight - 16;
  const cardLeft = rect ? Math.min(Math.max(16, rect.left), window.innerWidth - 360) : window.innerWidth / 2 - 170;
  return <div className="fixed inset-0 z-50" role="dialog" aria-modal="true" aria-labelledby="tour-title">
    {rect
      ? <div aria-hidden className="pointer-events-none absolute rounded-xl ring-2 ring-accent transition-all duration-300 ease-out motion-reduce:transition-none"
          style={{ top: rect.top - pad, left: rect.left - pad, width: rect.width + pad * 2, height: rect.height + pad * 2, boxShadow: "0 0 0 9999px rgba(0,0,0,.55)" }} />
      : <div aria-hidden className="absolute inset-0 bg-black/55 transition-opacity duration-300" />}
    <div ref={card} className="absolute w-[340px] max-w-[calc(100vw-32px)] rounded-xl border border-line bg-panel p-4 shadow-2xl transition-all duration-300 ease-out motion-reduce:transition-none" style={{ top: cardTop, left: cardLeft }}>
      <p className="text-[11px] text-faint">{i + 1} of {STEPS.length}</p>
      <h2 id="tour-title" className="mt-1 font-semibold">{step.title}</h2>
      <p className="mt-1 text-[13px] text-dim">{step.body}</p>
      <div className="mt-3 flex items-center gap-2">
        <button type="button" className="text-[12px] text-dim hover:text-fg" onClick={finish}>Skip</button>
        <span className="flex-1" />
        {i > 0 && <Button onClick={() => setI(i - 1)}>Back</Button>}
        {i < STEPS.length - 1 ? <Button variant="primary" autoFocus onClick={() => setI(i + 1)}>Next</Button> : <Button variant="primary" autoFocus onClick={finish}>Start building</Button>}
      </div>
    </div>
  </div>;
}
