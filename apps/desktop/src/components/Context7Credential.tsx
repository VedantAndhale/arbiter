import { useEffect, useState } from "react";
import { useApi } from "../ApiContext";
import type { CredentialStatus } from "../api";
import { Button, Input } from "./ui";

export function Context7Credential() {
  const api=useApi();
  const [status,setStatus]=useState<CredentialStatus|null>(null);
  const [key,setKey]=useState("");
  const [persist,setPersist]=useState(false);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState("");
  const [message,setMessage]=useState("");
  useEffect(()=>{
    const controller=new AbortController();
    api.documentationCredential(controller.signal).then(setStatus).catch(e=>{if(!controller.signal.aborted)setError(e.message);});
    return ()=>controller.abort();
  },[api]);
  async function update(remove=false) {
    setBusy(true);setError("");setMessage("");
    const submitted=key.trim();setKey("");
    try {
      const next=remove?await api.removeDocumentationCredential():await api.saveDocumentationCredential(submitted,persist);
      setStatus(next);
      setMessage(remove?'Stored key removed.':next.source==='os_store'?'Key saved in OS credential storage.':'Key available until the daemon restarts.');
    } catch(e) {setError(e instanceof Error?e.message:'Credential update failed. Re-enter the key to retry.');}
    finally {setBusy(false);}
  }
  return <details className="rounded border border-line p-3">
    <summary className="cursor-pointer">Context7 account {status?.configured?'· Key configured':'· Optional key'}</summary>
    <div className="mt-3 space-y-3">
      <p className="text-[12px] text-dim">Create or find your key on the <a href="https://context7.com/dashboard" target="_blank" rel="noreferrer" className="text-accent underline">Context7 dashboard</a>. Enter it here, never in a chat or documentation question. The key is sent only to your local daemon and Context7 authentication.</p>
      {status?.configured && <p className="text-[12px]">Current source: {status.source==='os_store'?'OS credential storage':status.source==='session'?'this daemon session':'daemon environment'}. The saved value is never returned to this screen.</p>}
      <label className="block space-y-1"><span>Context7 API key</span><Input type="password" autoComplete="new-password" spellCheck={false} maxLength={2500} placeholder="ctx7sk…" value={key} disabled={busy} onChange={e=>setKey(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.nativeEvent.isComposing){e.preventDefault();if(key.trim()&&!busy)void update();}}}/></label>
      {status?.persistent_available?<label className="flex items-start gap-2"><input type="checkbox" checked={persist} disabled={busy} onChange={e=>setPersist(e.target.checked)}/><span>Remember in OS credential storage<span className="block text-[12px] text-dim">Unchecked keeps the new key only until the daemon restarts. An existing OS key remains available after restart.</span></span></label>:<p className="text-[12px] text-dim">Session-only storage is available here. Native persistent credential storage is not available on this platform.</p>}
      <div className="flex flex-wrap gap-2"><Button disabled={busy||!key.trim()} onClick={()=>update()}>Save key</Button>{status?.configured&&status.source!=='environment'&&<Button disabled={busy} onClick={()=>update(true)}>Remove stored key</Button>}</div>
      {status?.source==='environment'&&<p className="text-[12px] text-dim">To remove the environment key, unset CONTEXT7_API_KEY and restart the daemon.</p>}
      {busy&&<p role="status">Updating credential…</p>}{message&&<p role="status">{message}</p>}{error&&<p role="alert" className="break-words text-bad">{error}</p>}
    </div>
  </details>;
}
