import { type DragEvent as ReactDragEvent, useEffect, useRef, useState } from "react";
import { useApi } from "../ApiContext";
import type { Attachment, AttachmentRef } from "../api";
import { Button } from "./ui";

type Upload = { key: string; file: File; status: "uploading" | "ready" | "error"; value?: Attachment; error?: string };
export function useAttachments() {
  const api = useApi();
  const [files, setFiles] = useState<Upload[]>([]);
  const controllers = useRef(new Map<string, AbortController>());
  const filesRef = useRef(files);
  filesRef.current = files;
  const [error, setError] = useState("");
  useEffect(() => { const active = controllers.current; return () => active.forEach(c => c.abort()); }, []);
  const upload = async (entry: Upload) => {
    const c = new AbortController();
    controllers.current.set(entry.key, c);
    setFiles(old => old.map(f => f.key === entry.key ? { ...f, status: "uploading", error: undefined } : f));
    try {
      const value = await api.upload(entry.file, AbortSignal.any([c.signal, AbortSignal.timeout(120_000)]));
      setFiles(old => old.map(f => f.key === entry.key ? { ...f, status: "ready", value } : f));
    } catch (e) {
      if (!c.signal.aborted) setFiles(old => old.map(f => f.key === entry.key ? { ...f, status: "error", error: e instanceof Error ? e.message : "Upload failed" } : f));
    } finally { controllers.current.delete(entry.key); }
  };
  const add = (selected: FileList | File[] | null) => {
    if (!selected) return;
    const incoming = Array.from(selected);
    if (incoming.length + filesRef.current.length > 10) { setError("Attach up to 10 files per message."); return; }
    setError("");
    const entries: Upload[] = incoming.map(file => ({ key: crypto.randomUUID(), file, status: "uploading" }));
    setFiles(old => [...old, ...entries]);
    for (const f of entries) {
      if (!f.file.size || f.file.size > 50 * 1024 * 1024) {
        setFiles(old => old.map(x => x.key === f.key ? { ...x, status: "error", error: f.file.size ? "File exceeds 50 MB" : "File is empty" } : x));
      } else void upload(f);
    }
  };
  const remove = (key: string) => { controllers.current.get(key)?.abort(); setFiles(old => old.filter(f => f.key !== key)); };
  const clear = () => { controllers.current.forEach(c => c.abort()); setFiles([]); setError(""); };
  return { files, error, add, remove, clear, retry: upload, busy: files.some(f => f.status === "uploading"), blocked: files.some(f => f.status !== "ready"), ids: [...new Set(files.flatMap(f => f.value ? [f.value.id] : []))] };
}

export function AttachmentPicker({ uploads, disabled = false }: { uploads: ReturnType<typeof useAttachments>; disabled?: boolean }) {
  const input = useRef<HTMLInputElement>(null);
  const [drag, setDrag] = useState(false);
  return <div className={`border-t border-dashed px-3 py-2 ${drag ? "border-accent bg-raised" : "border-line"}`}
    onDragOver={e => { e.preventDefault(); if (!disabled) setDrag(true); }} onDragLeave={() => setDrag(false)}
    onDrop={e => { e.preventDefault(); setDrag(false); if (!disabled) uploads.add(e.dataTransfer.files); }}>
    <div className="flex flex-wrap items-center gap-2">
      <Button onClick={() => input.current?.click()} disabled={disabled || uploads.files.length >= 10}>Attach files</Button>
      <span className="text-[11px] text-dim">Drop files · up to 10, 50 MB each</span>
      <input ref={input} type="file" multiple className="hidden" aria-label="Choose attachments" onChange={e => { uploads.add(e.target.files); e.target.value = ""; }} />
    </div>
    <div aria-live="polite" className="space-y-1">
      {uploads.files.map(f => <div key={f.key} className="mt-2 flex flex-wrap items-center gap-2 text-[11px]">
        <span className="min-w-0 max-w-64 truncate" title={f.file.name}>{f.file.name}</span>
        <span className="text-dim">{Math.ceil(f.file.size / 1024).toLocaleString()} KB</span>
        <span className={f.status === "error" ? "text-bad" : "text-dim"}>{f.status === "uploading" ? "Uploading…" : f.status === "ready" ? f.value?.note : f.error}</span>
        {f.status === "error" && <Button variant="ghost" onClick={() => void uploads.retry(f)} disabled={disabled || !f.file.size || f.file.size > 50 * 1024 * 1024}>Retry</Button>}
        <Button variant="ghost" onClick={() => uploads.remove(f.key)} disabled={disabled} aria-label={`Remove ${f.file.name}`}>{f.status === "uploading" ? "Cancel" : "Remove"}</Button>
      </div>)}
      {uploads.error && <p role="alert" className="mt-1 text-[11px] text-bad">{uploads.error}</p>}
    </div>
  </div>;
}

/** Drop files anywhere on a composer. */
export function useDropFiles(uploads: ReturnType<typeof useAttachments>, disabled = false) {
  const [drag, setDrag] = useState(false);
  return {
    drag,
    handlers: {
      onDragOver: (e: ReactDragEvent) => { if (e.dataTransfer.types.includes("Files")) { e.preventDefault(); if (!disabled) setDrag(true); } },
      onDragLeave: (e: ReactDragEvent) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setDrag(false); },
      onDrop: (e: ReactDragEvent) => { if (!e.dataTransfer.files.length) return; e.preventDefault(); setDrag(false); if (!disabled) uploads.add(e.dataTransfer.files); },
    },
  };
}

/** Compact toolbar button; drop and paste work on the whole composer. */
export function AttachButton({ uploads, disabled = false }: { uploads: ReturnType<typeof useAttachments>; disabled?: boolean }) {
  const input = useRef<HTMLInputElement>(null);
  return <>
    <Button variant="ghost" onClick={() => input.current?.click()} disabled={disabled || uploads.files.length >= 10} title="Attach files (or drop them here) · up to 10, 50 MB each" aria-label="Attach files">＋ Attach</Button>
    <input ref={input} type="file" multiple className="hidden" aria-label="Choose attachments" onChange={e => { uploads.add(e.target.files); e.target.value = ""; }} />
  </>;
}

/** Attached files as small chips inside the composer. */
export function AttachmentChips({ uploads, disabled = false }: { uploads: ReturnType<typeof useAttachments>; disabled?: boolean }) {
  if (!uploads.files.length && !uploads.error) return null;
  return <div aria-live="polite" className="flex flex-wrap gap-1.5 px-3 pb-1">
    {uploads.files.map(f => <span key={f.key} tabIndex={f.file.type.startsWith("image/") ? 0 : undefined} className={`group relative flex max-w-full items-center gap-1.5 rounded-md border px-2 py-0.5 text-[11px] ${f.status === "error" ? "border-bad/50" : "border-line"}`} title={f.status === "ready" ? f.value?.note : f.error ?? undefined}>
      {f.file.type.startsWith("image/") && <ImagePreview file={f.file} />}
      <span className="min-w-0 max-w-48 truncate">{f.file.name}</span>
      <span className={f.status === "error" ? "text-bad" : "text-faint"}>{f.status === "uploading" ? "uploading…" : f.status === "error" ? "failed" : `${Math.ceil(f.file.size / 1024).toLocaleString()} KB`}</span>
      {f.status === "error" && <button type="button" className="text-dim hover:text-fg" onClick={() => void uploads.retry(f)} disabled={disabled || !f.file.size || f.file.size > 50 * 1024 * 1024}>Retry</button>}
      <button type="button" className="text-dim hover:text-fg" onClick={() => uploads.remove(f.key)} disabled={disabled} aria-label={`Remove ${f.file.name}`}>×</button>
    </span>)}
    {uploads.error && <p role="alert" className="w-full text-[11px] text-bad">{uploads.error}</p>}
  </div>;
}

/** Thumbnail and a larger view on hover or focus, to check the right image is attached. */
function ImagePreview({ file }: { file: File }) {
  const [url, setUrl] = useState("");
  useEffect(() => {
    const u = URL.createObjectURL(file);
    setUrl(u);
    return () => URL.revokeObjectURL(u);
  }, [file]);
  if (!url) return null;
  return <>
    <img src={url} alt="" className="size-4 rounded object-cover" />
    <span role="tooltip" className="pointer-events-none absolute bottom-full left-0 z-40 mb-2 hidden rounded-lg border border-line bg-panel p-1 shadow-xl group-hover:block group-focus:block">
      <img src={url} alt={`Preview of ${file.name}`} className="block max-h-60 max-w-72 rounded object-contain" />
    </span>
  </>;
}

export function AttachmentLinks({ items }: { items: AttachmentRef[] }) {
  const api = useApi();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const download = async (a: AttachmentRef) => {
    setBusy(a.id); setError("");
    try {
      const blob = await api.attachment(a.id), url = URL.createObjectURL(blob);
      const link = document.createElement("a"); link.href = url;
      link.download = blob.type.startsWith("text/") && a.kind !== "text" ? `${a.name}.txt` : a.name;
      link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (e) { setError(e instanceof Error ? e.message : "Could not download attachment"); }
    finally { setBusy(null); }
  };
  return <div className="mt-2 space-y-1">
    {items.map(a => <div key={a.id} className="flex flex-wrap items-center gap-2"><Button variant="ghost" disabled={busy !== null} onClick={() => void download(a)}>{a.name}</Button><span className="text-[11px] text-dim">{a.note}</span></div>)}
    {error && <p role="alert" className="text-bad">{error}</p>}
  </div>;
}
