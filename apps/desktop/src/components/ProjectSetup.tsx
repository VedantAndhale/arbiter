import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import type { Adoption, Baseline, ProjectCheck } from "../api";
import { Button, Input } from "./ui";
import { FolderPicker } from "./FolderPicker";

export function Onboarding({ onClose }: { onClose?:()=>void }) {
  const api = useApi(), qc = useQueryClient();
  const [path,setPath]=useState("");
  const [proposal,setProposal]=useState<Adoption|null>(null),[checks,setChecks]=useState<ProjectCheck[]>([]),[baseline,setBaseline]=useState<Baseline|null>(null);
  const [registered,setRegistered]=useState(false),[consent,setConsent]=useState(false);
  const [initialFiles,setInitialFiles]=useState<string[]>([]),[gitConsent,setGitConsent]=useState(false);
  const [picker,setPicker]=useState<null|"open">(null),[typed,setTyped]=useState(false);
  const target=useRef<string|null>(null);
  const [resumeError,setResumeError]=useState<string|null>(null),[resuming,setResuming]=useState(false),[committed,setCommitted]=useState<number|null>(null);
  function saveProposal(p:Adoption|null) {
    setProposal(p);
    const url=new URL(window.location.href);
    if(p) url.searchParams.set('adoption',p.id); else url.searchParams.delete('adoption');
    window.history.replaceState(null,'',url);
  }
  function close() {saveProposal(null);onClose?.();}
  useEffect(()=>{
    const id=new URLSearchParams(window.location.search).get('adoption');
    if(!id)return;
    let active=true;setResuming(true);
    api.getAdoption(id).then(async p=>{
      const overview=await api.projectOverview(p.inventory.root);
      if(active){setProposal(p);setPath(p.inventory.root);setChecks(overview.checks);setBaseline(overview.baseline);}
    }).catch(e=>{if(active)setResumeError(e.message);}).finally(()=>{if(active)setResuming(false);});
    return ()=>{active=false;};
  },[api]);
  const action=useMutation({mutationFn:async(kind:string)=>{
    // Existing folders: inspection only; purpose and stack matter for new projects.
    if(kind==='inspect') { const at=(target.current??path).trim(); target.current=null; setPath(at); const r=await api.inspectProject(at,"","");saveProposal(r.proposal);setChecks(r.checks);setBaseline(null);setConsent(false);setInitialFiles([]);setGitConsent(false);setCommitted(null); }
    if(kind==='apply' && proposal) saveProposal(await api.applyAdoption(proposal.id));
    if(kind==='rollback' && proposal) saveProposal(await api.applyAdoption(proposal.id,true));
    if(kind==='baseline' && proposal) setBaseline(await api.projectBaseline(proposal.inventory.root,proposal.inventory.fingerprint));
    if(kind==='register' && proposal) {await api.addProject(proposal.inventory.root);setRegistered(true);}
    if(kind==='initialize' && proposal) {saveProposal(await api.initializeProject(proposal.id,initialFiles));setCommitted(initialFiles.length);}
    if(kind==='done') {await qc.invalidateQueries({queryKey:['projects']});close();}
  }});
  // One step: Arbiter adds a repository or empty folder directly and only
  // asks for review when loose files need choosing for the first version.
  const quick=useMutation({mutationFn:(p:string)=>api.quickAdd(p),onSuccess:async r=>{
    if(r.status==='added'){await qc.invalidateQueries({queryKey:['projects']});close();return;}
    setPath(r.proposal.inventory.root);saveProposal(r.proposal);setChecks(r.checks);setBaseline(null);setConsent(false);setInitialFiles([]);setGitConsent(false);setCommitted(null);
  }});
  const busy=action.isPending||resuming||quick.isPending;
  return <div className="h-full overflow-y-auto p-4 sm:p-6"><div className="mx-auto max-w-2xl space-y-5">
    {(proposal||onClose) && <div className="flex flex-wrap items-center justify-between gap-3"><div>{proposal && <><h1 className="text-lg font-semibold">One more step</h1><p className="mt-1 text-dim">This folder has files but no saved versions yet. Choose what goes into the first one.</p></>}</div>{onClose && <Button disabled={busy} onClick={close}>Close</Button>}</div>}
    {resumeError && <p role="alert" className="text-bad">{resumeError} <Button onClick={()=>{saveProposal(null);setResumeError(null);}}>Start a new inspection</Button></p>}
    {!proposal ? <section className="space-y-3 rounded-xl border border-line bg-panel p-5">
      <h2 className="text-base font-semibold">Add a project</h2>
      <p className="text-dim">Pick the folder for your project, or make a new one inside the picker. Arbiter sets it up; what to build comes from your first message.</p>
      {typed
        ? <form noValidate className="flex flex-wrap gap-2" onSubmit={e=>{e.preventDefault();if(path.trim()&&!busy)quick.mutate(path.trim());}}>
            <Input autoFocus aria-label="Project folder path" className="min-w-0 flex-1" value={path} onChange={e=>setPath(e.target.value)} placeholder="D:\code\my-project" disabled={busy}/>
            <Button type="submit" variant="primary" disabled={!path.trim()||busy}>{busy?'Adding…':'Add'}</Button>
          </form>
        : <Button variant="primary" disabled={busy} onClick={()=>setPicker("open")}>{busy?'Adding…':'Choose folder…'}</Button>}
      <button type="button" className="block text-[12px] text-dim underline-offset-2 hover:text-fg hover:underline" onClick={()=>setTyped(t=>!t)}>{typed?'Browse instead':'Type a path instead'}</button>
      {quick.error && <p role="alert" className="text-[12px] text-bad">{quick.error.message}</p>}
      {picker && <FolderPicker title="Choose your project folder" action="Use this folder" start={path} onClose={()=>setPicker(null)} onPick={p=>{setPicker(null);quick.mutate(p);}}/>}
    </section> : <>
      <section className="space-y-3 rounded-lg border border-line bg-panel p-4"><h2 className="font-semibold break-all">{proposal.inventory.root}</h2>
        <p className="text-dim">{proposal.inventory.git?'Git repository':proposal.inventory.empty?'Empty folder':'Folder without Git'} · {proposal.state.replaceAll('_',' ')}</p>
        {proposal.inventory.warnings.map(w=><p key={w} className="text-[12px] text-warn">{w}</p>)}
        {proposal.inventory.instructions.length>0 && <details><summary className="cursor-pointer text-dim">Preserved instructions ({proposal.inventory.instructions.length})</summary><ul className="mt-2 space-y-1 font-mono text-[12px]">{proposal.inventory.instructions.map(p=><li key={p} className="break-all">{p}</li>)}</ul></details>}
        <h3 className="font-medium">Proposed additions</h3>
        {proposal.changes.length===0 && <p className="text-dim">The relevant setup files already exist. No additions needed.</p>}
        {proposal.changes.map(c=><details key={c.path} className="rounded border border-line p-2"><summary className="cursor-pointer break-all font-mono">+ {c.path}</summary><pre className="mt-2 whitespace-pre-wrap break-words text-[12px] text-dim">{c.content}</pre></details>)}
        <div className="flex flex-wrap gap-2">
          {['proposed','applying'].includes(proposal.state) && <Button variant="primary" disabled={busy} onClick={()=>action.mutate('apply')}>{proposal.state==='applying'?'Resume applying additions':'Apply reviewed additions'}</Button>}
          {['applied','applying'].includes(proposal.state) && <Button disabled={busy} onClick={()=>action.mutate('rollback')}>Undo setup additions</Button>}
          <Button disabled={busy} onClick={()=>{saveProposal(null);setRegistered(false);action.reset();}}>Inspect again</Button>
        </div>
      </section>
      <section className="space-y-3 rounded-lg border border-line bg-panel p-4"><h2 className="font-semibold">Baseline checks</h2><p className="text-[12px] text-dim">These commands execute project code and may access the network or change files. Review them before running. Dependency installation and automatic fixes are excluded.</p>
        {checks.length===0 ? <p className="text-dim">No checks discovered. Record check commands in the project before coding.</p>:<ul className="space-y-2">{checks.map(c=><li key={c.name}><span>{c.name}</span><code className="ml-2 break-all text-dim">{c.cmd}</code></li>)}</ul>}
        {checks.length>0 && <><label className="flex items-start gap-2"><input type="checkbox" checked={consent} onChange={e=>setConsent(e.target.checked)} disabled={busy}/><span>I trust this project and approve these commands.</span></label><Button disabled={!consent||busy} onClick={()=>action.mutate('baseline')}>Run baseline checks</Button></>}
        {baseline && <div role="status" className="space-y-2"><p>{baseline.status==='passed'?'All checks passed':baseline.status==='no_checks'?'No checks ran':'Some checks already fail before any agent changes'} · {baseline.checks_run} {baseline.checks_run===1?'check':'checks'} · {new Date(baseline.at*1000).toLocaleString()}</p>{baseline.fingerprint&&proposal&&baseline.fingerprint!==proposal.inventory.fingerprint&&<p className="text-warn">These results are from an earlier project configuration. Run the checks again.</p>}{baseline.status==='existing_failures'&&<p className="text-[12px] text-dim">Agents are told about failures that existed before their changes, so self-healing does not chase them.</p>}{baseline.results.map(r=>r.ok?<p key={r.name} className="text-ok">✓ {r.name} · {(r.duration_ms/1000).toFixed(1)}s</p>:<details key={r.name} className="rounded border border-line p-2"><summary className="cursor-pointer text-warn">✗ {r.name}: {r.timed_out?'timed out':'failed before agent changes'}</summary>{r.cmd&&<p className="mt-2 break-all font-mono text-[12px] text-dim">{r.cmd}</p>}{r.evidence?<pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-words text-[12px]">{r.evidence}</pre>:<p className="mt-2 text-[12px] text-dim">No output was captured. Run the command in a terminal to see details.</p>}</details>)}</div>}
      </section>
      <section className="space-y-2 rounded-lg border border-line bg-panel p-4"><h2 className="font-semibold">Detected dependencies</h2><p className="text-dim">These are version hints. Use local documentation assistance in Settings to look up any public library with Context7.</p>{proposal.inventory.profiles.length===0?<p className="text-dim">No supported manifest detected yet.</p>:proposal.inventory.profiles.map((p)=><div key={`${p.manifest}-${p.name}`} className="space-y-1"><a className="text-accent underline" href={p.source} target="_blank" rel="noreferrer">{p.name} · {p.version}</a><p className="text-[12px] text-dim">{p.exact?'Lockfile version':'Requirement or unknown version'} · {p.manifest}</p></div>)}</section>
      {!proposal.inventory.git && proposal.state==='applied' && <section className="space-y-3 rounded-lg border border-line bg-panel p-4"><h2 className="font-semibold">Start Git history</h2><p className="text-dim">Select the files to include in the first commit. Unselected files stay on disk but will not be available in isolated agent branches. Hidden files and likely credentials are excluded from this list.</p><div className="flex flex-wrap items-center gap-2 text-[12px] text-dim"><span>{initialFiles.length} of {proposal.inventory.initial_files.length} selected</span><Button disabled={busy} onClick={()=>setInitialFiles(initialFiles.length===proposal.inventory.initial_files.length?[]:[...proposal.inventory.initial_files])}>{initialFiles.length===proposal.inventory.initial_files.length?'Select none':'Select all'}</Button></div><div className="max-h-64 space-y-2 overflow-y-auto">{proposal.inventory.initial_files.map(file=><label key={file} className="flex items-start gap-2"><input type="checkbox" checked={initialFiles.includes(file)} disabled={busy} onChange={e=>setInitialFiles(old=>e.target.checked?[...old,file]:old.filter(p=>p!==file))}/><span className="break-all font-mono text-[12px]">{file}</span></label>)}</div><label className="flex items-start gap-2"><input type="checkbox" checked={gitConsent} onChange={e=>setGitConsent(e.target.checked)} disabled={busy}/><span>I reviewed these files for secrets and approve a local initial commit. Nothing is published.</span></label><Button disabled={!gitConsent||busy} onClick={()=>action.mutate('initialize')}>Initialize Git and commit selected files</Button></section>}
      {committed!==null && proposal.inventory.git && !registered && <p role="status" className="text-ok">Git history started with {committed} {committed===1?'file':'files'} in a local commit. Add the project to the workspace to begin.</p>}
      <div className="flex flex-wrap gap-2">{registered?<Button variant="primary" onClick={()=>action.mutate('done')} disabled={busy}>Open workspace</Button>:<Button variant="primary" disabled={busy||!proposal.inventory.git||proposal.state!=='applied'} onClick={()=>action.mutate('register')}>Add to workspace</Button>}</div>
    </>}
    {busy && <p role="status" className="text-dim">Working…</p>}
    {action.error && <div role="alert" className="rounded border border-bad p-3 text-bad">{action.error.message}</div>}
  </div></div>;
}
