import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useApi } from "../ApiContext";
import { Button } from "./ui";

/** A shell in the task's copy of the project. Output stays with you; agents never see it. */
export function TerminalDrawer({ threadId, onClose }: { threadId: string; onClose: () => void }) {
  const api = useApi();
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<"connecting" | "open" | "closed">("connecting");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!host.current) return;
    const css = getComputedStyle(document.documentElement);
    const term = new Terminal({
      cursorBlink: true,
      fontFamily: "ui-monospace, SFMono-Regular, Consolas, monospace",
      fontSize: 12,
      theme: { background: css.getPropertyValue("--color-bg").trim() || "#0b0c0f", foreground: css.getPropertyValue("--color-fg").trim() || "#e6e6e6" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current);
    fit.fit();
    const ws = new WebSocket(api.terminalUrl(threadId, term.cols, term.rows));
    ws.binaryType = "arraybuffer";
    ws.onopen = () => { setState("open"); term.focus(); };
    ws.onmessage = e => {
      if (typeof e.data === "string") { if (e.data.includes('"exit"')) setState("closed"); return; }
      term.write(new Uint8Array(e.data as ArrayBuffer));
    };
    ws.onclose = () => setState(s => (s === "open" ? "closed" : s === "connecting" ? "closed" : s));
    const input = term.onData(data => { if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "input", data })); });
    const observer = new ResizeObserver(() => {
      fit.fit();
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
    });
    observer.observe(host.current);
    return () => { observer.disconnect(); input.dispose(); ws.close(); term.dispose(); };
  }, [api, threadId, attempt]);
  const end = async () => { await api.closeTerminal(threadId).catch(() => {}); onClose(); };
  return <section aria-label="Terminal" className="flex h-[38%] min-h-40 shrink-0 flex-col border-t border-line bg-bg">
    <div className="flex h-8 shrink-0 items-center gap-2 px-3 text-[12px]">
      <span className="font-medium">Terminal</span>
      <span className="text-faint">{state === "connecting" ? "Starting…" : state === "closed" ? "Ended" : "In this task's copy of the project"}</span>
      <span className="flex-1" />
      {state === "closed" && <Button variant="ghost" onClick={() => { setState("connecting"); setAttempt(a => a + 1); }}>Restart</Button>}
      <Button variant="ghost" onClick={end} title="End the shell">End</Button>
      <Button variant="ghost" aria-label="Hide terminal (Ctrl+J)" onClick={onClose}>✕</Button>
    </div>
    <div ref={host} className="min-h-0 flex-1 px-2 pb-1" />
  </section>;
}
