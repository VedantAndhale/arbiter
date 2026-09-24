import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import { Button, Dialog, Input } from "./ui";

/** Browse folders on this computer (names only) and pick or create one. */
export function FolderPicker({ title, action, start, onPick, onClose }: {
  title: string;
  action: string;
  start?: string;
  onPick: (path: string) => void;
  onClose: () => void;
}) {
  const api = useApi();
  const [path, setPath] = useState(start ?? "");
  const [naming, setNaming] = useState(false), [name, setName] = useState("");
  const folders = useQuery({ queryKey: ["folders", path], queryFn: () => api.folders(path), placeholderData: prev => prev });
  const create = useMutation({ mutationFn: () => api.createFolder(folders.data!.listing.path, name.trim()), onSuccess: r => { setNaming(false); setName(""); setPath(r.path); } });
  const l = folders.data?.listing;
  const crumbs = l ? l.path.split(/[\\/]/).filter(Boolean) : [];
  const sep = l?.path.includes("\\") ? "\\" : "/";
  return <Dialog title={title} onClose={onClose}>
    <div className="space-y-3">
      <div className="flex flex-wrap gap-1">{folders.data?.places.map(p => <Button key={p.path} variant="ghost" onClick={() => setPath(p.path)} aria-pressed={l?.path === p.path}>{p.name}</Button>)}</div>
      <div className="flex items-center gap-2">
        <Button disabled={!l?.parent} onClick={() => l?.parent && setPath(l.parent)} aria-label="Up one folder">↑</Button>
        <nav aria-label="Current folder" className="flex min-w-0 flex-1 flex-wrap items-center gap-0.5 text-[12px]">
          {crumbs.map((c, i) => {
            const target = (l!.path.startsWith("/") ? "/" : "") + crumbs.slice(0, i + 1).join(sep) + (i === 0 && sep === "\\" ? "\\" : "");
            return <span key={i} className="flex items-center gap-0.5">{i > 0 && <span className="text-faint">{sep}</span>}<button type="button" className={`rounded px-1 hover:bg-raised ${i === crumbs.length - 1 ? "text-fg" : "text-dim"}`} onClick={() => setPath(target)}>{c}</button></span>;
          })}
        </nav>
      </div>
      <div className="h-64 overflow-y-auto rounded-lg border border-line" role="list" aria-label="Folders">
        {folders.isPending && <p role="status" className="p-3 text-dim">Loading folders…</p>}
        {folders.error && <p role="alert" className="p-3 text-bad">{folders.error.message}</p>}
        {l && l.entries.length === 0 && <p className="p-3 text-dim">No folders here.</p>}
        {l?.entries.map(e => <button key={e.path} type="button" role="listitem" onClick={() => setPath(e.path)} className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-raised"><span aria-hidden className="text-faint">▸</span><span className="min-w-0 truncate">{e.name}</span></button>)}
        {l?.truncated && <p className="p-2 text-[12px] text-dim">Showing the first 500 folders.</p>}
      </div>
      {naming
        ? <form className="flex gap-2" onSubmit={e => { e.preventDefault(); if (name.trim()) create.mutate(); }}>
            <Input autoFocus aria-label="New folder name" value={name} maxLength={100} onChange={e => setName(e.target.value)} placeholder="my-new-app" />
            <Button type="submit" disabled={!name.trim() || create.isPending}>Create</Button>
            <Button type="button" onClick={() => setNaming(false)}>Cancel</Button>
          </form>
        : <Button disabled={!l} onClick={() => setNaming(true)}>New folder</Button>}
      {create.error && <p role="alert" className="text-[12px] text-bad">{create.error.message}</p>}
      <div className="flex items-center justify-between gap-2 border-t border-line pt-3">
        <span className="min-w-0 truncate text-[12px] text-dim" title={l?.path}>{l?.path}</span>
        <div className="flex shrink-0 gap-2"><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!l} onClick={() => l && onPick(l.path)}>{action}</Button></div>
      </div>
    </div>
  </Dialog>;
}
