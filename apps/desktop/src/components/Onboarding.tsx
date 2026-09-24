import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../ApiContext";
import { Button, Input } from "./ui";

export function Onboarding() {
  const api = useApi();
  const qc = useQueryClient();
  const [path, setPath] = useState("");
  const add = useMutation({
    mutationFn: () => api.addProject(path.trim()),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["projects"] }),
  });

  return (
    <div className="grid h-full place-items-center overflow-y-auto p-4">
      <form
        noValidate
        className="w-full max-w-md rounded-xl border border-line bg-panel p-6"
        onSubmit={(e) => {
          e.preventDefault();
          if (path.trim() && !add.isPending) add.mutate();
        }}
      >
        <div className="mb-1 text-base font-semibold">Welcome to Arbiter</div>
        <p className="mb-4 text-dim">
          Point Arbiter at a git repository. Each thread gets its own worktree, so agents never step on each other.
        </p>
        <Input autoFocus aria-label="Repository path" placeholder="D:\code\my-repo" value={path} onChange={(e) => setPath(e.target.value)} />
        {add.error && <div role="alert" className="mt-2 text-[12px] text-bad">{add.error.message}</div>}
        <div className="mt-4 flex justify-end">
          <Button variant="primary" disabled={!path.trim() || add.isPending}>
            Add project
          </Button>
        </div>
      </form>
    </div>
  );
}
