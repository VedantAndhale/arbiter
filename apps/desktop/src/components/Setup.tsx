import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { SetupPreferences } from "../api";
import { Button, Input } from "./ui";
import { LocalModelSettings } from "./Intake";
import { DocumentationSetup } from "./DocumentationSetup";
import { Prerequisites } from "./Prerequisites";
import { MemoryCard } from "./Memory";

const steps = ["Your work", "Accounts & usage", "Local assistance", "Ready to begin"];

export function Setup({ onClose }: { onClose?:()=>void }) {
  const api=useApi(), qc=useQueryClient();
  const state=useQuery({queryKey:["setup"],queryFn:api.setup,refetchInterval:30000});
  const models=useQuery({queryKey:["local-models"],queryFn:()=>api.localModels()});
  const [draft,setDraft]=useState<SetupPreferences|null>(null);
  const heading=useRef<HTMLHeadingElement>(null);
  // A finished setup reopens as settings: start at the first page.
  useEffect(()=>{ if(state.data && !draft) setDraft(state.data.preferences.complete?{...state.data.preferences,step:0}:state.data.preferences); },[state.data,draft]);
  useEffect(()=>{heading.current?.focus();},[draft?.step]);
  const save=useMutation({mutationFn:api.saveSetup,onSuccess:data=>{qc.setQueryData(["setup"],data);setDraft(data.preferences);if(data.preferences.complete)onClose?.();}});
  const refresh=useMutation({mutationFn:api.refreshSetup,onSuccess:data=>qc.setQueryData(["setup"],data)});
  // One click: check installed coding tools, then use the subscription when a
  // signed-in account exists, otherwise keep cloud agents off. Never paid usage.
  const automatic=useMutation({mutationFn:async()=>{
    let d=state.data!;
    if(d.preferences.network){ try{ d=await api.refreshSetup(); }catch{ /* keep last known status */ } }
    else { d=await api.setup(); }
    const signedIn=d.accounts.filter(a=>a.installed&&a.auth==='subscription'&&!a.conflict).map(a=>a.harness==='claude'?'Claude Code':'Codex');
    // Save onto the latest stored settings, so an older copy on this page
    // can never be rejected as out of date.
    const saved=await api.saveSetup({...d.preferences,mode:signedIn.length?'subscription':'local',paid_consent:false,step:3,complete:true});
    return {saved,signedIn,first:!d.preferences.complete};
  },onSuccess:({saved,first})=>{qc.setQueryData(["setup"],saved);setDraft(first?saved.preferences:{...saved.preferences,step:0});if(first)onClose?.();}});
  const automaticResult=automatic.data&&(automatic.data.signedIn.length
    ?`Done. Cloud agents use your ${automatic.data.signedIn.join(" and ")} subscription${automatic.data.signedIn.length>1?"s":""}. Paid usage stays off.`
    :"No signed-in Claude Code or Codex was found, so only local agents run. Sign in under \"Your computer\" below, then press this again.");
  if(state.isPending || !draft && !state.error) return <p role="status" className="p-6">Loading setup…</p>;
  if(state.error) return <div role="alert" className="p-6">{state.error.message} <Button onClick={()=>state.refetch()}>Retry</Button></div>;
  if(!draft || !state.data) return null;
  const p=draft, data=state.data, busy=save.isPending||refresh.isPending||automatic.isPending;
  const change=(patch:Partial<SetupPreferences>)=>setDraft({...p,...patch});
  const settled=data.preferences.complete;
  // During first-run setup each step is saved; afterwards moving between
  // pages never un-completes setup, and "Save changes" keeps it complete.
  const advance=(step:number)=>settled?setDraft({...p,step}):save.mutate({...p,step,complete:false});
  return <div className="h-full overflow-y-auto px-4 py-6 sm:px-8">
    <div className="mx-auto w-full max-w-2xl space-y-5">
      <header className="flex items-start justify-between gap-3"><div>{!settled && <p className="text-[12px] text-dim">{`Arbiter setup · ${p.step+1} of ${steps.length}`}</p>}<h1 ref={heading} tabIndex={-1} className="mt-1 text-lg font-semibold">{settled?'Settings':steps[p.step]}</h1></div>{onClose && settled && <Button onClick={onClose}>Close</Button>}</header>
      {!settled && <ol aria-label="Setup progress" className="flex flex-wrap gap-x-5 gap-y-2 text-[12px] text-dim">{steps.map((s,i)=><li key={s} aria-current={i===p.step?"step":undefined} className={i===p.step?"font-medium text-fg":""}>{settled?<button type="button" className="hover:text-fg" onClick={()=>setDraft({...p,step:i})}>{i===3?'Summary':s}</button>:`${i+1}. ${s}`}</li>)}</ol>}
      <fieldset disabled={busy} className="min-w-0 space-y-4 rounded-lg border border-line bg-panel p-4 sm:p-6">
      {(settled||p.step===0) && <>
        <section className="space-y-2 rounded-lg border border-accent/40 bg-accent/5 p-4">
          <h2 className="font-medium">Set up automatically</h2>
          <p className="text-[12px] text-dim">Arbiter checks the coding tools installed on this computer. If you are signed in to Claude Code or Codex, it uses that subscription; otherwise cloud agents stay off. Paid usage is never turned on. You can change anything later in Settings.</p>
          <Button variant="primary" disabled={busy} onClick={()=>automatic.mutate()}>{automatic.isPending?'Checking your tools…':'Set up automatically'}</Button>
          {automatic.error && <p role="alert" className="text-[12px] text-bad">{automatic.error.message}</p>}
          {automaticResult && <p role="status" className={`text-[12px] ${automatic.data?.signedIn.length?"text-ok":"text-warn"}`}>{automaticResult}</p>}
        </section>
        <h2 className="pt-2 font-medium">Or choose yourself</h2>
        <p className="text-dim">A few preferences help us set up your workspace. These questions run locally.</p>
        <label className="block space-y-2"><span>Your main work</span><Input maxLength={500} value={p.purpose} onChange={e=>change({purpose:e.target.value})} /></label>
        <fieldset className="space-y-2"><legend className="mb-2 font-medium">How should agents use your accounts?</legend>
          {([['subscription','Use my subscription','Pause when allowance or extra-credit status cannot be verified.'],['local','Keep cloud agents off','Use local intake and explicitly selected local editing/review models.']] as const).map(([mode,label,help])=><label key={mode} className="flex gap-3 rounded border border-line p-3"><input type="radio" name="usage-mode" checked={p.mode===mode} onChange={()=>change({mode,paid_consent:false})}/><span>{label}<span className="mt-1 block text-[12px] text-dim">{help}</span></span></label>)}
        </fieldset>
        <label className="flex items-start gap-2"><input type="checkbox" checked={p.network} onChange={e=>change({network:e.target.checked})}/><span>Allow account checks and model downloads<span className="block text-[12px] text-dim">Cloud agents still follow your usage choice. This setting is not an operating-system network sandbox.</span></span></label>
        <details><summary className="cursor-pointer text-dim">Advanced usage settings</summary><div className="mt-3 space-y-3">
          <label className="flex items-start gap-2"><input type="checkbox" checked={p.mode==='paid'} onChange={e=>change({mode:e.target.checked?'paid':'subscription',paid_consent:false})}/><span>Enable paid API or provider-credit usage</span></label>
          {p.mode==='paid' && <label className="flex items-start gap-2 text-warn"><input type="checkbox" checked={p.paid_consent} onChange={e=>change({paid_consent:e.target.checked})}/><span>I authorize metered usage. Arbiter's reported-cost thresholds are not hard billing caps.</span></label>}
          <label className="flex items-start gap-2"><input type="checkbox" checked={p.strict_allowance??false} onChange={e=>change({strict_allowance:e.target.checked})}/><span>Strict protection<span className="block text-[12px] text-dim">Only run cloud agents when the account reports its remaining allowance and confirms paid extras are off. Most CLIs cannot report this, so agents will often stay paused.</span></span></label>
          <label className="flex items-start gap-2"><input type="checkbox" checked={p.auto_update??true} onChange={e=>change({auto_update:e.target.checked})}/><span>Update Arbiter automatically<span className="block text-[12px] text-dim">New versions download in the background and install when you close Arbiter. Turn off to be asked first.</span></span></label>
          <label className="flex items-center gap-3">Maximum cloud agents<select aria-label="Maximum cloud agents" value={p.max_cloud_runs} onChange={e=>change({max_cloud_runs:Number(e.target.value)})} className="rounded border border-line bg-bg p-2">{[1,2,3].map(n=><option key={n}>{n}</option>)}</select></label>
        </div></details>
      </>}
      {settled && <hr className="border-line"/>}
      {(settled||p.step===1) && <>
        <Prerequisites />
        <MemoryCard />
        {!p.network && <p className="text-warn">Account checks are off because network access is disabled.</p>}
      </>}
      {settled && <hr className="border-line"/>}
      {(settled||p.step===2) && <>
        <h2 className="font-medium">Start small on this computer</h2>
        <p>{data.hardware.logical_cpus} logical CPUs · {data.hardware.os} · {data.hardware.arch}</p>
        <p className="text-dim">{data.hardware.memory_bytes?`${(data.hardware.memory_bytes/2**30).toFixed(1)} GB memory`:'Memory capacity unavailable'} · {data.hardware.free_disk_bytes?`${(data.hardware.free_disk_bytes/2**30).toFixed(1)} GB free for models`:'Free disk space unavailable'}</p>
        <p className="text-dim">Start with the small Potion classifier. A question-writing model is optional; benchmark it before relying on it. You can keep standard local rules and install a model later.</p>
        <p className="text-[12px] text-dim">Model setup shows download sizes, licenses and measured speed. Installed generation models can code locally once they pass the coding check in Local models. Intake speed does not establish coding or review quality.</p>
        <LocalModelSettings />
        <label className="block space-y-2"><span>Local documentation model</span><select className="w-full rounded border border-line bg-bg p-2" value={p.documentation_model??''} onChange={e=>change({documentation_model:e.target.value||null})}><option value="">Choose an installed generation model</option>{models.data?.models.filter(m=>m.ready&&m.model.id!=='potion').map(m=><option key={m.model.id} value={m.model.id}>{m.model.name}</option>)}</select></label>
        <label className="flex items-start gap-2"><input type="checkbox" checked={p.context7_enabled??false} onChange={e=>change({context7_enabled:e.target.checked})}/><span>Allow local agents to query Context7<span className="block text-[12px] text-dim">Public documentation questions leave this computer. No persistent documentation cache; no frontier retrieval calls.</span></span></label>
        <label className="flex items-start gap-2"><input type="checkbox" checked={p.web_research_enabled??false} onChange={e=>change({web_research_enabled:e.target.checked})}/><span>Let a local model look things up online for agents<span className="block text-[12px] text-dim">It searches and reads pages on this computer, then gives Claude or Codex a short cited summary instead of whole pages. Their own web tools are switched off while this is on. Only public questions are sent to the search engine.</span></span></label>
        {p.web_research_enabled && <SignedInSites />}
        <Button disabled={busy} onClick={()=>save.mutate(settled?{...p,step:3,complete:true}:{...p,complete:false})}>Save assistance settings</Button>
        <DocumentationSetup />
        <label className="block space-y-2"><span>Independent local reviewer</span><select className="w-full rounded border border-line bg-bg p-2" value={p.reviewer_model??''} onChange={e=>change({reviewer_model:e.target.value||null})}><option value="">Off until a capable model is available</option>{models.data?.models.filter(m=>m.ready&&m.model.id!=='potion').map(m=><option key={m.model.id} value={m.model.id}>{m.model.name}</option>)}</select></label><p className="text-[12px] text-dim">One assessment at plan approval and final review. It reads requirements and checks independently; documentation verification is unavailable until live evidence is supplied. Findings are advisory; missing sources or model failures are shown. Review quality still needs evaluation.</p>
      </>}
      {!settled && p.step===3 && <>
        <h2 className="font-medium">Your workspace preferences</h2>
        <dl className="space-y-2"><div><dt className="text-dim">Work</dt><dd className="break-words">{p.purpose}</dd></div><div><dt className="text-dim">Usage</dt><dd>{p.mode==='subscription'?'Protected subscription usage':p.mode==='local'?'Cloud agents off':'Paid usage authorized'}</dd></div></dl>
        <p className="text-dim">You can add a repository and prepare tasks. Agent execution stays blocked until account checks pass. Setup can be reopened from the workspace.</p>
        {data.accounts.map(a=><p key={a.harness} className="text-[12px] text-dim">{a.harness}: {a.auth} · {a.windows.length?'Allowance reported':'Allowance unknown'}{data.readiness.find(r=>r.harness===a.harness)?.blocked && ` · ${data.readiness.find(r=>r.harness===a.harness)?.blocked}`}</p>)}
        <p className="text-[12px] text-warn">Saving changed preferences stops active agents so the new policy applies. Their work remains available.</p>
      </>}
      {(save.error||refresh.error) && <p role="alert" className="text-bad">{save.error?.message??refresh.error?.message} <Button onClick={async()=>{const latest=await state.refetch();if(latest.data)setDraft(latest.data.preferences);save.reset();refresh.reset();}}>Reload saved setup</Button></p>}
      {settled
        ? <footer className="flex flex-wrap justify-end gap-3 border-t border-line pt-4"><Button disabled={busy} onClick={()=>onClose?.()}>Close</Button><Button variant="primary" disabled={busy||!p.purpose.trim()||(p.mode==='paid'&&!p.paid_consent)} onClick={()=>save.mutate({...p,step:3,complete:true})}>{save.isPending?'Saving…':'Save changes'}</Button></footer>
        : <footer className="flex flex-wrap justify-between gap-3 border-t border-line pt-4"><Button disabled={p.step===0||busy} onClick={()=>advance(p.step-1)}>Back</Button><Button variant="primary" disabled={busy||!p.purpose.trim()||(p.mode==='paid'&&!p.paid_consent)} onClick={()=>p.step===3?save.mutate({...p,complete:true}):advance(p.step+1)}>{save.isPending?'Saving…':p.step===3?'Save and open workspace':'Save and continue'}</Button></footer>}
      </fieldset>
    </div>
  </div>;
}


/** Sites the local researcher may read while signed in. The user signs in in a separate browser window; Arbiter never sees the password. */
function SignedInSites() {
  const api = useApi(), qc = useQueryClient();
  const sites = useQuery({ queryKey: ["web-sites"], queryFn: api.webSites });
  const [url, setUrl] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const add = useMutation({ mutationFn: () => api.addWebSite(url), onSuccess: r => { setUrl(""); setNote(`A browser window opened for ${r.host}. Sign in there, then close it.`); qc.invalidateQueries({ queryKey: ["web-sites"] }); } });
  const remove = useMutation({ mutationFn: (host: string) => api.removeWebSite(host), onSuccess: r => { setNote(r.note ?? "Signed out."); qc.invalidateQueries({ queryKey: ["web-sites"] }); } });
  return <div className="space-y-2 rounded-lg border border-line p-3">
    <p className="font-medium">Sites you are signed into</p>
    <p className="text-[12px] text-dim">For pages that need an account, such as your issue tracker. You sign in yourself in a separate Arbiter browser window. A local model checks each summary from these sites; if it looks private, the task asks you before Claude or Codex sees it.</p>
    {(sites.data?.sites ?? []).map(h => <div key={h} className="flex items-center gap-2 text-[13px]"><span className="flex-1 truncate">{h}</span><Button variant="ghost" disabled={remove.isPending} onClick={() => remove.mutate(h)}>Sign out</Button></div>)}
    <form className="flex gap-2" onSubmit={e => { e.preventDefault(); if (url.trim()) add.mutate(); }}>
      <Input aria-label="Site address" placeholder="https://github.com" value={url} onChange={e => setUrl(e.target.value)} />
      <Button type="submit" className="shrink-0 whitespace-nowrap" disabled={add.isPending || !url.trim()}>Sign in…</Button>
    </form>
    {(add.error || remove.error) && <p role="alert" className="text-[12px] text-bad">{(add.error ?? remove.error)?.message}</p>}
    {note && <p className="text-[12px] text-dim">{note}</p>}
  </div>;
}
