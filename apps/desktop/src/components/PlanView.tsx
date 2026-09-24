import { useState } from "react";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { VaultButton } from "./Vault";
import { useApi } from "../ApiContext";

import type { Plan, PlanNode, PlanState, PlanNodeState, Project, Thread } from "../api";
import { Landing } from "./DiffView";

import { Button, Dialog, Input } from "./ui";

const colors:Record<string,string>={queued:"text-dim",running:"text-accent",checking:"text-accent",blocked:"text-warn",merging:"text-accent",merged:"text-ok"};

/** Plain words for step states. */
const WORDS:Record<string,string>={queued:"Waiting",running:"Working",checking:"Checking",blocked:"Needs you",merging:"Adding it in",merged:"Done"};
const ICONS:Record<string,string>={queued:"○",running:"●",checking:"●",blocked:"!",merging:"●",merged:"✓"};

const label=(s:string)=>s.replaceAll("_"," ");

/** Name of a step's project; `undefined` is the task's own project. */
function useProjectName(main:string){
  const api=useApi();
  const projects=useQuery({queryKey:["projects"],queryFn:api.projects});
  return (id?:string|null)=>(projects.data??[]).find((p:Project)=>p.id===(id??main))?.name??"project";
}

export function PlanView({thread,state,onOpen}:{thread:Thread;state:PlanState;onOpen:(id:string)=>void}) {
  const threadId=thread.id;
  const api=useApi(),qc=useQueryClient();
  const projectName=useProjectName(thread.project_id);
  const multi=(state.plan?.projects?.length??0)>0;
  const [diffProject,setDiffProject]=useState<string|undefined>(undefined);
  const usageMode=useQuery({queryKey:["setup"],queryFn:api.setup});
  // The checklist is the plan; the map, timeline and combined diff are extras.
  const [view,setView]=useState<"steps"|"map"|"activity"|"diff">("steps");
  const [open,setOpen]=useState<string|null>(null);
  const [changeMenu,setChangeMenu]=useState(false);
  const [editing,setEditing]=useState(false);
  const [scope,setScope]=useState("");
  const [budgeting,setBudgeting]=useState(false);
  const action=useMutation({mutationFn:(op:"approve"|"pause"|"resume"|"refine"|"scope")=>op==="approve"?api.approvePlan(threadId,state.revision):op==="refine"?api.refinePlan(threadId):op==="scope"?api.approveScope(threadId,open!,scope.split("\n").map(s=>s.trim()).filter(Boolean)):api.controlPlan(threadId,op),onSuccess:()=>{qc.invalidateQueries({queryKey:["plan",threadId]});qc.invalidateQueries({queryKey:["threads"]});setScope("");}});
  const plan=state.plan!;
  const complete=Object.values(state.nodes).filter(n=>n.status==="merged").length;
  const running=Object.values(state.nodes).filter(n=>["running","checking","merging"].includes(n.status)).length;
  const stage=state.completed?"Finished":state.paused?"Needs your attention":state.planner?"Arbiter is planning":state.approved?"Working on it":"Here is the plan";
  const status=(id:string)=>state.nodes[id]?.status??"queued";
  return <div className="h-full min-h-0 overflow-y-auto">
    <div className="mx-auto max-w-3xl space-y-4 px-4 py-5">
      <header className="space-y-2">
        <p className="text-[12px] text-dim">{stage}</p>
        <h1 className="text-lg font-semibold">{plan.title}</h1>
        <div className="flex flex-wrap items-center gap-2">
          {!state.approved && <>
            <Button variant="primary" disabled={action.isPending || !!state.planner} onClick={()=>action.mutate("approve")}>Approve plan</Button>
            <div className="relative">
              <Button disabled={action.isPending || !!state.planner} aria-expanded={changeMenu} onClick={()=>setChangeMenu(v=>!v)}>Change the plan</Button>
              {changeMenu && <div className="absolute left-0 z-10 mt-1 w-56 rounded-lg border border-line bg-panel py-1 shadow-xl">
                <button type="button" className="block w-full px-3 py-1.5 text-left hover:bg-raised" onClick={()=>{setChangeMenu(false);setEditing(true);}}>Edit steps myself</button>
                <button type="button" className="block w-full px-3 py-1.5 text-left hover:bg-raised" onClick={()=>{setChangeMenu(false);action.mutate("refine");}}>Ask the planner to revise it</button>
              </div>}
            </div>
          </>}
          {state.approved && !state.completed && <Button variant={state.paused?"primary":"default"} disabled={action.isPending} onClick={()=>action.mutate(state.paused?"resume":"pause")}>{state.paused?"Resume":"Pause"}</Button>}
          {state.planner && <Button variant="ghost" onClick={()=>onOpen(state.planner!)}>See the planner</Button>}
          <span className="flex-1" />
          <span className="text-[12px] text-dim">{complete} of {plan.nodes.length} done{running?` · ${running} working`:""}</span>
        </div>
        <progress aria-label="Plan completion" max={plan.nodes.length} value={complete} className="h-1 w-full accent-accent" />
        {!state.approved && !state.planner && <p className="text-[12px] text-dim">Approving lets agents work on these steps, each in its own copy. Their changes come together for you to review at the end.</p>}
        {state.paused && <p role="alert" className="rounded-lg border border-warn/40 bg-panel p-3 text-warn">{state.paused}</p>}
        {state.completed && <p className="text-ok">All steps are done and the checks passed. Review the combined changes before saving them.</p>}
        {action.error && <p role="alert" className="text-bad">{action.error.message}</p>}
        <details className="text-[12px] text-dim"><summary className="cursor-pointer">What you asked for</summary><p className="mt-2 whitespace-pre-wrap break-words">{plan.goal}</p></details>
      </header>

      <nav aria-label="Plan views" className="flex flex-wrap items-center gap-3 text-[12px]">
        {view!=="steps" && <button type="button" className="text-accent hover:underline" onClick={()=>setView("steps")}>← Steps</button>}
        {view!=="map" && <button type="button" className="text-dim hover:text-fg" onClick={()=>setView("map")}>Show map</button>}
        {view!=="activity" && <button type="button" className="text-dim hover:text-fg" onClick={()=>setView("activity")}>Timeline</button>}
        {view!=="diff" && state.path && <button type="button" className="text-dim hover:text-fg" onClick={()=>setView("diff")}>{state.completed?"Review and save changes":"All changes"}</button>}
        <span className="flex-1" /><VaultButton threadId={threadId}/>
      </nav>

      {multi && view==="steps" && <p className="text-[12px] text-dim">This plan works across {1+(plan.projects?.length??0)} projects: {[projectName(null),...(plan.projects??[]).map(p=>projectName(p))].join(", ")}. A step that needs another project's work waits for it and gets its handoff.</p>}
      {view==="map" ? <AgentMap plan={plan} state={state} selected={open??""} onSelect={id=>{setOpen(id);setView("steps");}} projectName={multi?projectName:undefined}/>
      : view==="activity" ? <PlanTimeline threadId={threadId}/>
      : view==="diff" ? <div className="space-y-4">
        {multi && <div role="tablist" aria-label="Project" className="flex flex-wrap gap-1">{[undefined,...(plan.projects??[])].map(p=><button key={p??"main"} type="button" role="tab" aria-selected={diffProject===p} onClick={()=>setDiffProject(p)} className={`rounded-md px-2 py-1 text-[12px] ${diffProject===p?"bg-raised text-fg":"text-dim hover:text-fg"}`}>{projectName(p)}</button>)}</div>}
        <PlanDiff key={diffProject??"main"} threadId={threadId} project={diffProject}/>
        {state.completed && <Landing key={`land-${diffProject??"main"}`} thread={thread} busy={false} project={diffProject} startOpen/>}
      </div>
      : <ol aria-label="Plan steps" className="divide-y divide-line rounded-xl border border-line">
        {plan.nodes.map((n,i)=>{const st=status(n.id),prog=state.nodes[n.id],expanded=open===n.id;return <li key={n.id}>
          <button type="button" aria-expanded={expanded} onClick={()=>setOpen(expanded?null:n.id)} className="flex w-full items-start gap-3 px-4 py-3 text-left hover:bg-raised/50">
            <span aria-hidden className={`mt-0.5 w-4 text-center ${colors[st]} ${["running","checking","merging"].includes(st)?"animate-pulse":""}`}>{ICONS[st]}</span>
            <span className="min-w-0 flex-1"><span className="block font-medium">{i+1}. {n.title}</span><span className="block truncate text-[12px] text-dim">{multi && <span className="mr-1 rounded bg-raised px-1 text-faint">{projectName(n.project)}</span>}{prog?.reason || (n.dependencies.length?`Starts after step ${n.dependencies.map(d=>plan.nodes.findIndex(m=>m.id===d)+1).join(", ")}`:"Starts after approval")}</span></span>
            <span className={`shrink-0 text-[12px] ${colors[st]}`}>{WORDS[st]??label(st)}</span>
          </button>
          {expanded && <div className="space-y-3 px-11 pb-4 text-[13px]">
            <p className="whitespace-pre-wrap text-dim">{n.goal}</p>
            {(prog?.thread || prog?.repair) && <div className="flex flex-wrap gap-2">{prog?.thread && <Button variant="primary" onClick={()=>onOpen(prog.thread!)}>Open this step's agent</Button>}{prog?.repair && <Button onClick={()=>onOpen(prog.repair!)}>Open merge repair</Button>}</div>}
            {prog?.handoff && <p><span className="text-dim">What it did: </span>{prog.handoff.summary}</p>}
            <details className="text-[12px]"><summary className="cursor-pointer text-dim">Details</summary>
              <Detail title="Can change" lines={[...n.scope,...(prog?.extra_scope??[])]}/><Detail title="Checks" lines={n.checks.length?n.checks:["No explicit commands; agent checks still apply"]}/><Detail title="Won't do" lines={n.non_goals}/>
              <h3 className="mt-5 text-[12px] uppercase tracking-wide text-dim">Agent</h3><p className="mt-1">{n.harness} · {n.model??"default model"}</p><p className="mt-1 text-dim">{n.model_reason}</p>
              {usageMode.data?.preferences.mode==="paid" && <p className="mt-2 text-dim">{n.token_budget?`${n.token_budget.toLocaleString()} token cap`:"No token cap"} · {n.budget_usd?`$${n.budget_usd} cost cap`:"No cost cap"}</p>}
              {prog?.handoff && <HandoffView state={prog}/>}
              {prog?.brief && <details className="mt-4"><summary className="cursor-pointer text-dim">Exact agent brief</summary><pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-words font-mono">{prog.brief}</pre></details>}
            </details>
            {state.paused && state.approved && <div className="space-y-2 border-t border-line pt-3">
              <Button onClick={()=>setBudgeting(true)}>Review spending limits</Button>
              <form noValidate onSubmit={e=>{e.preventDefault();action.mutate("scope");}}><label className="block text-[12px] text-dim" htmlFor={`scope-${n.id}`}>Let this step change more files (one path or pattern per line)</label><textarea id={`scope-${n.id}`} rows={3} value={scope} onChange={e=>setScope(e.target.value)} className="my-2 w-full resize-none rounded-lg border border-line bg-bg p-2"/><Button type="submit" disabled={!scope.trim() || action.isPending}>Allow these files</Button></form>
            </div>}
          </div>}
        </li>;})}
      </ol>}
      {state.branch && view==="steps" && <p className="text-[12px] text-faint">Changes collect on <code className="break-all">{state.branch}</code>{multi?` in each project (${[projectName(null),...Object.keys(state.integrations??{}).map(p=>projectName(p))].join(", ")})`:""}</p>}
    </div>
    {budgeting && <BudgetEditor threadId={threadId} plan={plan} selected={open??plan.nodes[0].id} onClose={()=>setBudgeting(false)}/>}
    {editing && <PlanEditor threadId={threadId} plan={plan} projectName={projectName} onClose={()=>setEditing(false)}/>}
  </div>;
}

function Detail({title,lines}:{title:string;lines:string[]}) {return lines.length?<div className="mt-5"><h3 className="text-[12px] uppercase tracking-wide text-dim">{title}</h3><ul className="mt-2 space-y-1 text-[12px]">{lines.map((s,i)=><li key={i} className="break-words font-mono">{s}</li>)}</ul></div>:null;}

function HandoffView({state}:{state:PlanNodeState}) {const h=state.handoff!;return <section className="mt-5 border-t border-line pt-4"><h3 className="text-[12px] uppercase tracking-wide text-dim">Handoff</h3><p className="mt-2">{h.summary}</p><Detail title="Files" lines={h.files}/><Detail title="Decisions" lines={h.decisions}/><Detail title="Interfaces" lines={h.interfaces}/><Detail title="Open issues" lines={h.open_issues}/></section>;}

function AgentMap({plan,state,selected,onSelect,projectName}:{plan:Plan;state:PlanState;selected:string;onSelect:(id:string)=>void;projectName?:(id?:string|null)=>string}) {

  const levels=new Map<string,number>();

  for(let pass=0;pass<plan.nodes.length;pass++)for(const n of plan.nodes)if(n.dependencies.every(d=>levels.has(d)))levels.set(n.id,n.dependencies.length?Math.max(...n.dependencies.map(d=>levels.get(d)!))+1:0);

  const columns=Math.max(...levels.values(),0)+1;

  const rows=Math.max(...Array.from({length:columns},(_,i)=>plan.nodes.filter(n=>levels.get(n.id)===i).length));

  const points=new Map(plan.nodes.map(n=>{const col=levels.get(n.id)??0;const row=plan.nodes.filter(m=>levels.get(m.id)===col).findIndex(m=>m.id===n.id);return [n.id,{x:col*256,y:row*130}];}));

  return <div className="overflow-auto rounded-lg border border-line bg-bg p-4"><div className="relative" style={{width:columns*256-24,minHeight:Math.max(240,rows*130)}}>

    <svg aria-hidden="true" className="pointer-events-none absolute inset-0" width={columns*256} height={rows*130}>{plan.nodes.flatMap(n=>n.dependencies.map(d=>{const a=points.get(d)!,b=points.get(n.id)!;return <path key={`${d}-${n.id}`} d={`M ${a.x+228} ${a.y+48} C ${a.x+244} ${a.y+48}, ${b.x-16} ${b.y+48}, ${b.x} ${b.y+48}`} fill="none" stroke="currentColor" className="text-dim" strokeWidth="1.5"/>;}))}</svg>

    {plan.nodes.map((n,i)=>{const p=points.get(n.id)!,s=state.nodes[n.id];return <button key={n.id} aria-pressed={selected===n.id} onClick={()=>onSelect(n.id)} style={{left:p.x,top:p.y,width:228}} className={`absolute rounded-lg border bg-panel p-3 text-left ${selected===n.id?"border-accent":"border-line hover:border-dim"}`}><div className="flex items-center justify-between text-[12px]"><span className="font-mono text-dim">{String(i+1).padStart(2,"0")}</span><span className={colors[s?.status??"queued"]}>{label(s?.status??"queued")}</span></div><p className="mt-2 truncate font-medium" title={n.title}>{n.title}</p><p className="mt-1 truncate text-[12px] text-dim">{projectName && <span className="mr-1 text-faint">{projectName(n.project)} ·</span>}{s?.reason || `${n.harness} · ${n.model??"default"}`}</p></button>;})}

  </div></div>;

}

function PlanEditor({threadId,plan,projectName,onClose}:{threadId:string;plan:Plan;projectName:(id?:string|null)=>string;onClose:()=>void}) {

  const api=useApi(),qc=useQueryClient();const [draft,setDraft]=useState(()=>structuredClone(plan));const [index,setIndex]=useState(0); const [validation,setValidation]=useState("");

  const save=useMutation({mutationFn:()=>api.savePlan(threadId,{...draft,nodes:draft.nodes.map(n=>({...n,scope:n.scope.filter(Boolean),may_read:n.may_read.filter(Boolean),non_goals:n.non_goals.filter(Boolean),checks:n.checks.filter(Boolean),dependencies:n.dependencies.filter(Boolean)}))}),onSuccess:()=>{qc.invalidateQueries({queryKey:["plan",threadId]});onClose();}});

  const step=draft.nodes[index];

  const change=(patch:Partial<PlanNode>)=>setDraft(d=>({...d,nodes:d.nodes.map((n,i)=>i===index?{...n,...patch}:n)}));

  const lines=(key:"scope"|"may_read"|"non_goals"|"checks"|"dependencies",title:string)=><label className="block text-[12px] text-dim">{title}<textarea rows={3} value={step[key].join("\n")} onChange={e=>change({[key]:e.target.value.split("\n")})} className="resize-none mt-1 w-full rounded border border-line bg-bg p-2 font-mono text-fg"/></label>;

  return <Dialog title="Edit plan before approval" onClose={()=>!save.isPending&&onClose()}><form noValidate className="space-y-3" onSubmit={e=>{e.preventDefault();if(!e.currentTarget.checkValidity()){setValidation("Review the numeric limits. Use positive budgets, whole token counts, and 1–6 parallel agents.");return;}setValidation("");setDraft(d=>({...d,nodes:d.nodes.map(n=>({...n,scope:n.scope.filter(Boolean),may_read:n.may_read.filter(Boolean),non_goals:n.non_goals.filter(Boolean),checks:n.checks.filter(Boolean),dependencies:n.dependencies.filter(Boolean)}))}));save.mutate();}}>

    <label className="block text-[12px] text-dim">Plan title<Input value={draft.title} onChange={e=>setDraft({...draft,title:e.target.value})}/></label>

    <label className="block text-[12px] text-dim">Plan goal<textarea value={draft.goal} onChange={e=>setDraft({...draft,goal:e.target.value})} rows={2} className="resize-none mt-1 w-full rounded border border-line bg-bg p-2 text-fg"/></label>

    <div className="flex gap-3"><label className="text-[12px] text-dim">Max parallel agents<Input type="number" min="1" max="6" value={draft.concurrency} onChange={e=>setDraft({...draft,concurrency:Number(e.target.value)})}/></label><label className="text-[12px] text-dim">Plan budget ($)<Input type="number" min="0.01" step="0.01" value={draft.budget_usd??""} placeholder="Uncapped" onChange={e=>setDraft({...draft,budget_usd:e.target.value?Number(e.target.value):null})}/></label></div>

    <div className="flex gap-2 border-t border-line pt-3"><select aria-label="Step to edit" value={index} onChange={e=>setIndex(Number(e.target.value))} className="min-w-0 flex-1 rounded border border-line bg-bg p-1">{draft.nodes.map((n,i)=><option key={i} value={i}>{i+1}. {n.title}</option>)}</select><Button type="button" disabled={draft.nodes.length>=24} onClick={()=>{const id=`step-${Date.now().toString(36)}`;setDraft({...draft,nodes:[...draft.nodes,{id,title:"New step",goal:"",scope:["src/**"],may_read:["**"],non_goals:[],checks:[],dependencies:[],harness:"auto",model:null,model_reason:"Automatic assignment",tool_profile:"implementation",budget_usd:null,token_budget:null}]});setIndex(draft.nodes.length);}}>Add step</Button></div>

    <label className="block text-[12px] text-dim">Step ID<Input value={step.id} onChange={e=>change({id:e.target.value})}/></label><label className="block text-[12px] text-dim">Title<Input value={step.title} onChange={e=>change({title:e.target.value})}/></label>

    {(draft.projects?.length??0)>0 && <label className="block text-[12px] text-dim">Project<select aria-label="Step project" value={step.project??""} onChange={e=>change({project:e.target.value||null})} className="mt-1 block w-full rounded border border-line bg-bg p-2 text-fg"><option value="">{projectName(null)}</option>{(draft.projects??[]).map(p=><option key={p} value={p}>{projectName(p)}</option>)}</select></label>}
    <label className="block text-[12px] text-dim">Goal<textarea value={step.goal} onChange={e=>change({goal:e.target.value})} rows={2} className="resize-none mt-1 w-full rounded border border-line bg-bg p-2 text-fg"/></label>

    {lines("scope","Writable scope — one pattern per line")}{lines("may_read","Read context — one pattern per line")}{lines("dependencies","Dependencies — step IDs, one per line")}{lines("checks","Acceptance commands — one per line")}{lines("non_goals","Non-goals — one per line")}

    <div className="flex gap-2"><label className="text-[12px] text-dim">Harness<select value={step.harness} onChange={e=>change({harness:e.target.value as PlanNode["harness"]})} className="mt-1 block rounded border border-line bg-bg p-2 text-fg"><option>auto</option><option>claude</option><option>codex</option></select></label><label className="min-w-0 flex-1 text-[12px] text-dim">Model override<Input value={step.model??""} onChange={e=>change({model:e.target.value||null})} placeholder="Harness default"/></label></div>

    <label className="block text-[12px] text-dim">Tools<select className="ml-2 rounded border border-line bg-bg p-2 text-fg" value={step.tool_profile} onChange={e=>change({tool_profile:e.target.value as PlanNode["tool_profile"]})}><option value="implementation">Implementation</option><option value="research">Research</option></select></label>

    <label className="block text-[12px] text-dim">Assignment reason<Input value={step.model_reason} onChange={e=>change({model_reason:e.target.value})}/></label>

    <div className="flex gap-2"><label className="text-[12px] text-dim">Step budget ($)<Input type="number" min="0.01" step="0.01" value={step.budget_usd??""} onChange={e=>change({budget_usd:e.target.value?Number(e.target.value):null})}/></label><label className="text-[12px] text-dim">Token cap<Input type="number" min="1" step="1" value={step.token_budget??""} onChange={e=>change({token_budget:e.target.value?Number(e.target.value):null})}/></label></div>

    <Button type="button" disabled={draft.nodes.length===1} onClick={()=>{setDraft({...draft,nodes:draft.nodes.filter((_,i)=>i!==index)});setIndex(0);}}>Remove this step</Button>

    {validation && <p role="alert" className="text-bad">{validation}</p>}{save.error && <p role="alert" className="text-bad">{save.error.message}</p>}<div className="flex justify-end gap-2 border-t border-line pt-3"><Button type="button" disabled={save.isPending} onClick={onClose}>Cancel</Button><Button type="submit" variant="primary" disabled={save.isPending}>{save.isPending?"Saving…":"Save revision"}</Button></div>

  </form></Dialog>;

}

function PlanTimeline({threadId}:{threadId:string}) {

  const api=useApi();

  const query=useQuery({queryKey:["events",threadId],queryFn:()=>api.events(threadId),refetchInterval:2000});

  const entries=(query.data??[]).filter(e=>e.kind.type==="plan" && !["context","brief_compiled"].includes(e.kind.event.type as string));

  if(query.isPending)return <p role="status" className="text-dim">Loading timeline…</p>;

  if(query.error)return <div role="alert">{query.error.message} <Button onClick={()=>query.refetch()}>Retry timeline</Button></div>;

  return <ol aria-label="Execution timeline" className="space-y-4">{entries.map(e=>{

    const data:Record<string,unknown>=e.kind.type==="plan"?e.kind.event:{};

    const node=typeof data.node==="string"?data.node:null;

    const reason=typeof data.reason==="string"?data.reason:"";

    return <li key={e.id} className="border-l-2 border-line pl-4"><time className="text-[12px] text-dim" dateTime={e.ts}>{new Date(e.ts).toLocaleString()}</time><p className="mt-1 font-medium">{node?`${node} · `:""}{label(String(data.type))}{typeof data.status==="string"?` · ${data.status}`:""}</p>{reason&&<p className="mt-1 text-[12px] text-dim">{reason}</p>}</li>;

  })}</ol>;

}

function PlanDiff({threadId,project}:{threadId:string;project?:string}) {

  const api=useApi();

  const query=useQuery({queryKey:["plan-diff",threadId,project??""],queryFn:()=>api.planDiff(threadId,project)});

  return <section aria-label="Integrated changes"><div className="mb-3 flex items-center justify-between"><h2 className="font-semibold">Changes on the plan branch</h2><Button disabled={query.isFetching} onClick={()=>query.refetch()}>Refresh changes</Button></div><p className="mb-4 text-[12px] text-dim">This includes integrated steps. Open an agent to inspect its unfinished changes.</p>{query.isPending&&<p role="status">Loading changes…</p>}{query.error&&<p role="alert" className="text-bad">{query.error.message}</p>}{query.isSuccess&&!query.data&&<p className="text-dim">No integrated changes yet.</p>}<pre className="overflow-x-auto rounded border border-line text-[12px] leading-5">{query.data?.split("\n").map((line,i)=><div key={i} className={`px-3 ${line.startsWith("+")?"bg-ok/10 text-ok":line.startsWith("-")?"bg-bad/10 text-bad":line.startsWith("@@")?"text-accent":"text-dim"}`}>{line||" "}</div>)}</pre></section>;

}

function BudgetEditor({threadId,plan,selected,onClose}:{threadId:string;plan:Plan;selected:string;onClose:()=>void}) {

  const api=useApi(),qc=useQueryClient();

  const [target,setTarget]=useState(selected);
  const [validation,setValidation]=useState("");

  const current=plan.nodes.find(n=>n.id===target);

  const [usd,setUsd]=useState(String(current?.budget_usd??""));

  const [tokens,setTokens]=useState(String(current?.token_budget??""));

  const save=useMutation({mutationFn:()=>api.approvePlanBudget(threadId,target||null,usd?Number(usd):null,tokens&&target?Number(tokens):null),onSuccess:()=>{qc.invalidateQueries({queryKey:["plan",threadId]});onClose();}});

  return <Dialog title="Review spending limits" onClose={onClose}><form noValidate className="space-y-4" onSubmit={e=>{e.preventDefault();if(!e.currentTarget.checkValidity()){setValidation("Enter a positive cost cap or whole token count, or leave the field empty.");return;}setValidation("");save.mutate();}}><p className="text-dim">Dollar thresholds use reported cost, not subscription allowance, and may overshoot. Codex does not report prices. Token caps apply to cumulative usage, including earlier turns. Leave a field empty to remove that cap. Resume the plan separately after reviewing.</p><label className="block">Budget for<select className="mt-1 w-full rounded border border-line bg-bg p-2" value={target} onChange={e=>{const id=e.target.value;const n=plan.nodes.find(n=>n.id===id);setTarget(id);setUsd(String(id?n?.budget_usd??"":plan.budget_usd??""));setTokens(String(n?.token_budget??""));}}><option value="">Whole plan</option>{plan.nodes.map(n=><option key={n.id} value={n.id}>{n.title}</option>)}</select></label><label className="block">Cost cap ($)<Input type="number" min="0.01" step="0.01" value={usd} onChange={e=>setUsd(e.target.value)} placeholder="Uncapped"/></label>{target&&<label className="block">Token cap<Input type="number" min="1" step="1" value={tokens} onChange={e=>setTokens(e.target.value)} placeholder="Uncapped"/></label>}{validation&&<p role="alert" className="text-bad">{validation}</p>}{save.error&&<p role="alert" className="text-bad">{save.error.message}</p>}<Button type="submit" variant="primary" disabled={save.isPending}>{save.isPending?"Saving…":"Approve budget change"}</Button></form></Dialog>;

}

