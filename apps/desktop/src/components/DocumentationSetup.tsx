import { useEffect, useRef, useState } from "react";
import { useApi } from "../ApiContext";
import { Button, Input } from "./ui";
import { Context7Credential } from "./Context7Credential";

/** Results stay in this mounted panel; no query cache or browser persistence. */
export function DocumentationSetup() {
  const api = useApi();
  const [library, setLibrary] = useState("");
  const [topic, setTopic] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [brief, setBrief] = useState<Awaited<ReturnType<typeof api.documentationQuery>> | null>(null);
  const controller = useRef<AbortController | null>(null);
  const operation = useRef<string | null>(null);
  useEffect(() => () => {controller.current?.abort();if(operation.current)void api.cancelDocumentation(operation.current).catch(()=>{});}, [api]);
  async function cancel() {
    const id=operation.current;
    controller.current?.abort();
    if(!id)return;
    try {await api.cancelDocumentation(id);setMessage("Documentation work cancelled. Requests already sent to Context7 cannot be undone.");}
    catch(e){setError(e instanceof Error?e.message:"Cancellation failed. The request remains bounded by its timeout.");}
    finally {if(operation.current===id){operation.current=null;setBusy(false);}}
  }
  async function run(action: "guide" | "test" | "query") {
    const request = new AbortController();
    controller.current?.abort();
    controller.current = request;
    const id=crypto.randomUUID();operation.current=id;
    setBusy(true); setError(""); setMessage(""); setBrief(null);
    try {
      if (action === "query") {
        const result = await api.documentationQuery(library.trim(), topic.trim(), request.signal,id);
        if (!request.signal.aborted) setBrief(result);
      } else {
        const result = await api.documentationSetup(action === "test", request.signal,id);
        if (!request.signal.aborted) setMessage(result.message);
      }
    } catch (e) {
      if (!request.signal.aborted) setError(e instanceof Error ? e.message : "Request failed. Retry.");
    } finally {
      if (controller.current === request && !request.signal.aborted) {setBusy(false);operation.current=null;}
    }
  }
  return <section className="space-y-3 border-t border-line pt-4" aria-labelledby="documentation-heading">
    <h3 id="documentation-heading" className="font-medium">Local documentation assistance</h3>
    <Context7Credential />
    <p className="text-[12px] text-dim">Save your model and connection choices before using these controls. Your local model handles library selection and summaries. No documentation cache or frontier calls.</p>
    <div className="flex flex-wrap gap-2">
      <Button disabled={busy} onClick={() => run("guide")}>Guide me locally</Button>
      <Button disabled={busy} onClick={() => run("test")}>Test Context7 connection</Button>
    </div>
    <details><summary className="cursor-pointer text-dim">Try a documentation question</summary>
      <div className="mt-3 space-y-3">
        <label className="block space-y-1"><span>Public library name</span><Input value={library} maxLength={100} placeholder="For example, react" onChange={e => setLibrary(e.target.value)} disabled={busy}/></label>
        <label className="block space-y-1"><span>Public API question, including version</span><Input value={topic} maxLength={350} placeholder="How does effect cleanup work in React 18?" onChange={e => setTopic(e.target.value)} disabled={busy}/></label>
        <p className="text-[12px] text-dim">This library name and question are sent to Context7. Keep private code, paths and credentials out. Live requests use the service’s own allowance.</p>
        <Button disabled={busy || !library.trim() || !topic.trim()} onClick={() => run("query")}>Look up with local agent</Button>
      </div>
    </details>
    {busy && <div role="status" className="flex flex-wrap items-center gap-2"><span>Working locally…</span><Button onClick={()=>void cancel()}>Cancel documentation work</Button></div>}
    {error && <p role="alert" className="break-words text-bad">{error}</p>}
    {message && <p role="status" className="break-words">{message}</p>}
    {brief && <article className="space-y-2 rounded border border-line p-3" aria-label="Local documentation findings"><p className="text-[12px] text-dim">{brief.library_id} · Live Context7 lookup</p><p className="whitespace-pre-wrap break-words">{brief.summary}</p><p className="text-[12px] text-warn">{brief.limitations}</p><ul className="space-y-1">{brief.sources.map(source => <li key={source}><a href={source} target="_blank" rel="noreferrer" className="break-all text-accent underline">{source}</a></li>)}</ul><Button onClick={() => setBrief(null)}>Dismiss findings</Button></article>}
    <p className="text-[12px] text-dim">When enabled, the local agent prepares relevant documentation before frontier tasks start. The compact findings still count as frontier input tokens. Failed preparation pauses dispatch.</p>
  </section>;
}
