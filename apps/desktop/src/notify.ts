/** Desktop notifications for moments that need the user: approval needed,
 *  work ready for review, or a failure. Off until the user turns them on;
 *  shown only while the window is in the background. */
const KEY = "arbiter.notifications";

export type NotifyState = "unsupported" | "denied" | "off" | "on";

export function notifyState(): NotifyState {
  if (typeof window === "undefined" || !("Notification" in window)) return "unsupported";
  if (Notification.permission === "denied") return "denied";
  let on = false;
  try { on = localStorage.getItem(KEY) === "on"; } catch { /* storage unavailable */ }
  return on && Notification.permission === "granted" ? "on" : "off";
}

export async function setNotifications(on: boolean): Promise<NotifyState> {
  if (notifyState() === "unsupported") return "unsupported";
  if (on && Notification.permission !== "granted") {
    const p = await Notification.requestPermission();
    if (p !== "granted") return p === "denied" ? "denied" : "off";
  }
  try { localStorage.setItem(KEY, on ? "on" : "off"); } catch { /* storage unavailable */ }
  return notifyState();
}

const MESSAGES: Record<string, string> = {
  needs_approval: "needs your decision",
  review: "is ready for review",
  failed: "stopped with a problem",
};

/** A status change worth interrupting for, as text; null otherwise. */
export function attention(status: string): string | null {
  return MESSAGES[status] ?? null;
}

export function notify(title: string, status: string, onClick: () => void) {
  const what = attention(status);
  if (!what || notifyState() !== "on" || (document.hasFocus() && !document.hidden)) return;
  try {
    const n = new Notification(`Arbiter: ${title}`, { body: `This task ${what}.`, tag: `${title}:${status}` });
    n.onclick = () => { window.focus(); onClick(); n.close(); };
  } catch { /* some webviews expose the API but refuse to show */ }
}
