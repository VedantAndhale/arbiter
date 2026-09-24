import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { EditorView, basicSetup } from "codemirror";
import { keymap } from "@codemirror/view";
import { oneDark } from "@codemirror/theme-one-dark";
import { javascript } from "@codemirror/lang-javascript";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { useApi } from "../ApiContext";
import { Button, Input } from "./ui";

function language(path: string) {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (["js", "mjs", "cjs", "jsx"].includes(ext)) return javascript({ jsx: true });
  if (["ts", "tsx", "mts"].includes(ext)) return javascript({ jsx: ext === "tsx", typescript: true });
  if (["css", "scss"].includes(ext)) return css();
  if (["html", "htm", "svelte", "vue"].includes(ext)) return html();
  if (ext === "json") return json();
  if (["md", "mdx"].includes(ext)) return markdown();
  if (ext === "py") return python();
  if (ext === "rs") return rust();
  return [];
}

/** Quick edits in the task's copy: pick a file, change it, save (Ctrl+S). */
export function FileEditor({ threadId, initialPath }: { threadId: string; initialPath?: string | null }) {
  const api = useApi();
  const files = useQuery({ queryKey: ["working-files", threadId], queryFn: () => api.workingFiles(threadId) });
  const [filter, setFilter] = useState(""), [path, setPath] = useState<string | null>(initialPath ?? null);
  useEffect(() => { if (initialPath) setPath(initialPath); }, [initialPath]);
  const shown = useMemo(() => (files.data ?? []).filter(f => f.toLowerCase().includes(filter.toLowerCase())).slice(0, 300), [files.data, filter]);
  return <div className="flex min-h-0 flex-1">
    <div className="flex w-56 shrink-0 flex-col border-r border-line">
      <Input aria-label="Find a file" className="m-2" value={filter} onChange={e => setFilter(e.target.value)} placeholder="Find a file…" />
      <ul className="min-h-0 flex-1 overflow-y-auto pb-2 text-[12px]">
        {files.isPending && <li className="px-3 text-dim">Loading…</li>}
        {shown.map(f => <li key={f}><button type="button" onClick={() => setPath(f)} title={f}
          className={`block w-full truncate px-3 py-0.5 text-left font-mono ${f === path ? "bg-raised text-fg" : "text-dim hover:text-fg"}`}>{f}</button></li>)}
      </ul>
    </div>
    {path ? <Editor key={path} threadId={threadId} path={path} /> : <div className="grid flex-1 place-items-center text-dim">Pick a file to view or edit it.</div>}
  </div>;
}

function Editor({ threadId, path }: { threadId: string; path: string }) {
  const api = useApi();
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const [hash, setHash] = useState(""), [dirty, setDirty] = useState(false), [loadError, setLoadError] = useState("");
  const save = useMutation({
    mutationFn: () => api.writeFile(threadId, path, view.current!.state.doc.toString(), hash),
    onSuccess: r => { setHash(r.hash); setDirty(false); },
  });
  const saveRef = useRef(() => {});
  saveRef.current = () => { if (dirty && !save.isPending) save.mutate(); };
  useEffect(() => {
    let live = true;
    api.readFile(threadId, path).then(file => {
      if (!live || !host.current) return;
      setHash(file.hash);
      view.current = new EditorView({
        parent: host.current,
        doc: file.content,
        extensions: [
          basicSetup, oneDark, language(path),
          keymap.of([{ key: "Mod-s", preventDefault: true, run: () => { saveRef.current(); return true; } }]),
          EditorView.updateListener.of(u => { if (u.docChanged) setDirty(true); }),
          EditorView.theme({ "&": { height: "100%", fontSize: "12px" }, ".cm-scroller": { fontFamily: "ui-monospace, Consolas, monospace" } }),
        ],
      });
    }).catch(e => live && setLoadError(e.message));
    return () => { live = false; view.current?.destroy(); view.current = null; };
  }, [api, threadId, path]);
  return <div className="flex min-w-0 flex-1 flex-col">
    <div className="flex h-8 shrink-0 items-center gap-2 border-b border-line px-3 text-[12px]">
      <span className="min-w-0 flex-1 truncate font-mono">{path}{dirty && <span className="text-warn"> ●</span>}</span>
      {save.error && <span role="alert" className="truncate text-bad">{save.error.message}</span>}
      <Button variant={dirty ? "primary" : "ghost"} disabled={!dirty || save.isPending} onClick={() => save.mutate()}>{save.isPending ? "Saving…" : "Save"}</Button>
    </div>
    {loadError ? <p role="alert" className="p-3 text-bad">{loadError}</p> : <div ref={host} className="min-h-0 flex-1 overflow-hidden" />}
  </div>;
}
