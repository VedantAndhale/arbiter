import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import { Button, Input } from "./ui";

/** The tools Arbiter needs on this computer, with guided installs. */
export function Prerequisites() {
  const api = useApi(), qc = useQueryClient();
  const state = useQuery({
    queryKey: ["prerequisites"],
    queryFn: api.prerequisites,
    refetchInterval: q => (q.state.data?.tools.some(t => t.job?.running) ? 3000 : false),
  });
  const [confirm, setConfirm] = useState<string | null>(null), [approved, setApproved] = useState(false);
  const install = useMutation({ mutationFn: (id: string) => api.installPrerequisite(id), onSuccess: () => { setConfirm(null); setApproved(false); qc.invalidateQueries({ queryKey: ["prerequisites"] }); } });
  const signIn = useMutation({ mutationFn: (id: string) => api.signIn(id) });
  // Re-check accounts too, so sign-in and allowance are current.
  const recheck = useMutation({ mutationFn: async () => { await api.refreshSetup().catch(() => null); await state.refetch(); qc.invalidateQueries({ queryKey: ["setup"] }); } });
  const [name, setName] = useState(""), [email, setEmail] = useState("");
  const identity = useMutation({ mutationFn: () => api.setGitIdentity(name, email), onSuccess: () => qc.invalidateQueries({ queryKey: ["prerequisites"] }) });
  if (state.isPending) return <p role="status" className="text-dim">Checking this computer…</p>;
  if (state.error) return <p role="alert" className="text-bad">{state.error.message} <Button onClick={() => state.refetch()}>Retry</Button></p>;
  const git = state.data.tools.find(t => t.id === "git");
  const needsIdentity = git?.installed && (!state.data.git_identity.name || !state.data.git_identity.email);
  return <section aria-label="Your computer" className="space-y-3">
    <div className="flex items-center justify-between gap-2"><h3 className="font-medium">Your computer</h3><Button variant="ghost" onClick={() => recheck.mutate()} disabled={recheck.isPending || state.isFetching}>{recheck.isPending || state.isFetching ? "Checking…" : "Check again"}</Button></div>
    <ul className="divide-y divide-line rounded-lg border border-line">
      {state.data.tools.map(t => <li key={t.id} className="space-y-2 p-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className={t.installed ? "text-ok" : t.required ? "text-warn" : "text-faint"} aria-hidden>{t.installed ? "✓" : "○"}</span>
          <span className="font-medium">{t.name}</span>
          <span className="text-[12px] text-dim">{t.installed ? t.version : t.required ? "Needed" : "Not installed"}</span>
          <span className="flex-1" />
          {!t.installed && !t.job?.running && (t.can_install
            ? <Button onClick={() => { setConfirm(t.id); setApproved(false); }}>Install</Button>
            : <a className="text-[12px] text-accent underline" href={t.manual_url} target="_blank" rel="noreferrer">How to install</a>)}
          {t.installed && t.sign_in && (t.signed_in
            ? <span className="text-[12px] text-ok">Signed in{t.account?.plan ? ` · ${t.account.plan} plan` : ""}</span>
            : t.account ? <Button disabled={signIn.isPending} onClick={() => signIn.mutate(t.id)}>Sign in</Button>
            : <span className="text-[12px] text-dim">Not checked yet</span>)}
        </div>
        <p className="text-[12px] text-dim">{t.why}</p>
        {t.signed_in && t.account && <div className="space-y-0.5 text-[12px]">
          {t.account.allowance.map(a => <p key={a} className="text-dim">{a}</p>)}
          {t.account.allowance.length === 0 && <p className="text-faint">{t.account.note}</p>}
          {t.account.blocked ? <p className="text-warn">Paused: {t.account.blocked}</p> : <p className="text-ok">Ready for work</p>}
        </div>}
        {t.job && <p role="status" className={`text-[12px] ${t.job.running ? "text-dim" : t.job.ok ? "text-ok" : "text-bad"} whitespace-pre-wrap`}>{t.job.message}</p>}
        {confirm === t.id && <div className="space-y-2 rounded-md border border-line bg-bg p-2 text-[12px]">
          <label className="flex items-start gap-2"><input type="checkbox" checked={approved} onChange={e => setApproved(e.target.checked)} /><span>Install {t.name} from its official source ({t.id === "claude" || t.id === "codex" ? "npm" : "the system package manager"}). The package's own license applies.</span></label>
          <div className="flex gap-2"><Button variant="primary" disabled={!approved || install.isPending} onClick={() => install.mutate(t.id)}>{install.isPending ? "Starting…" : `Install ${t.name}`}</Button><Button onClick={() => setConfirm(null)}>Cancel</Button></div>
          {install.error && <p role="alert" className="text-bad">{install.error.message}</p>}
        </div>}
      </li>)}
    </ul>
    {signIn.isSuccess && <p role="status" className="text-[12px] text-dim">A sign-in window opened. Finish there, then press Check again.</p>}
    {signIn.error && <p role="alert" className="text-[12px] text-bad">{signIn.error.message}</p>}
    {needsIdentity && <form className="space-y-2 rounded-lg border border-line p-3" onSubmit={e => { e.preventDefault(); identity.mutate(); }}>
      <p className="font-medium">Who is saving the work?</p>
      <p className="text-[12px] text-dim">Git records a name and email with each saved version. They stay on this computer unless you publish.</p>
      <div className="grid gap-2 sm:grid-cols-2"><Input aria-label="Your name" value={name} onChange={e => setName(e.target.value)} placeholder="Your name" /><Input aria-label="Your email" type="email" value={email} onChange={e => setEmail(e.target.value)} placeholder="you@example.com" /></div>
      <Button type="submit" disabled={!name.trim() || !email.trim() || identity.isPending}>Save</Button>
      {identity.error && <p role="alert" className="text-[12px] text-bad">{identity.error.message}</p>}
    </form>}
  </section>;
}
