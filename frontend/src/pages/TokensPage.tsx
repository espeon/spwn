import { useState } from "react";
import { toast } from "sonner";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { listTokens, createToken, deleteToken, type CreatedApiToken, ApiError } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { IconKey, IconTrash, IconCopy, IconCheck } from "@tabler/icons-react";

function formatDate(ts: number) {
  return new Date(ts * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export function TokensPage() {
  const qc = useQueryClient();
  const { data: tokens = [] } = useQuery({
    queryKey: ["tokens"],
    queryFn: listTokens,
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [tokenName, setTokenName] = useState("");
  const [created, setCreated] = useState<CreatedApiToken | null>(null);
  const [copied, setCopied] = useState(false);

  const createMutation = useMutation({
    mutationFn: () => createToken(tokenName.trim()),
    onSuccess: async (data) => {
      await qc.invalidateQueries({ queryKey: ["tokens"] });
      setTokenName("");
      setCreated(data);
    },
    onError: (err: unknown) => {
      toast.error(err instanceof ApiError ? err.message : "failed to create token");
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteToken(id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["tokens"] });
      toast.success("token revoked");
    },
    onError: () => toast.error("failed to revoke token"),
  });

  function openCreate() {
    setTokenName("");
    setCreated(null);
    setCopied(false);
    setCreateOpen(true);
  }

  function closeCreate() {
    setCreateOpen(false);
    setCreated(null);
    setCopied(false);
    setTokenName("");
  }

  async function copyToken() {
    if (!created) return;
    await navigator.clipboard.writeText(created.token);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <>
      <Dialog open={createOpen} onOpenChange={(o) => !o && closeCreate()}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {created ? "token created" : "create API token"}
            </DialogTitle>
            <DialogDescription>
              {created
                ? "copy your token now — it won't be shown again."
                : "give the token a name so you can identify it later."}
            </DialogDescription>
          </DialogHeader>

          {created ? (
            <div className="space-y-3 py-2">
              <Label>token</Label>
              <div className="flex items-center gap-2">
                <code className="flex-1 rounded-md border bg-muted px-3 py-2 text-xs font-mono break-all">
                  {created.token}
                </code>
                <Button variant="outline" size="sm" onClick={copyToken} className="shrink-0">
                  {copied ? (
                    <IconCheck className="size-4 text-green-500" />
                  ) : (
                    <IconCopy className="size-4" />
                  )}
                </Button>
              </div>
            </div>
          ) : (
            <div className="space-y-1.5 py-2">
              <Label htmlFor="token-name">name</Label>
              <Input
                id="token-name"
                value={tokenName}
                onChange={(e) => setTokenName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && tokenName.trim() && createMutation.mutate()}
                placeholder="e.g. laptop, CI"
                autoFocus
              />
            </div>
          )}

          <DialogFooter>
            {created ? (
              <Button onClick={closeCreate}>done</Button>
            ) : (
              <>
                <Button variant="ghost" onClick={closeCreate} disabled={createMutation.isPending}>
                  cancel
                </Button>
                <Button
                  onClick={() => createMutation.mutate()}
                  disabled={!tokenName.trim() || createMutation.isPending}
                >
                  {createMutation.isPending ? "creating..." : "create"}
                </Button>
              </>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="space-y-5 mt-0.5">
        <div className="flex items-center justify-between">
          <h2 className="text-2xl font-semibold">API tokens</h2>
          <Button size="sm" onClick={openCreate}>
            create token
          </Button>
        </div>

        <div className="rounded-lg border bg-card">
          {tokens.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-10 text-muted-foreground">
              <IconKey className="size-8 opacity-40" />
              <p className="text-sm">no tokens yet</p>
              <Button size="sm" variant="outline" onClick={openCreate}>
                create your first token
              </Button>
            </div>
          ) : (
            <div className="divide-y divide-border">
              {tokens.map((token) => (
                <div key={token.id} className="flex items-center justify-between px-4 py-3">
                  <div className="min-w-0">
                    <p className="text-sm font-medium">{token.name}</p>
                    <p className="text-xs text-muted-foreground">
                      created {formatDate(token.created_at)}
                      {token.last_used_at
                        ? ` · last used ${formatDate(token.last_used_at)}`
                        : " · never used"}
                    </p>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive shrink-0"
                    onClick={() =>
                      toast.promise(deleteMutation.mutateAsync(token.id), {
                        loading: "revoking...",
                        success: "token revoked",
                        error: "failed to revoke token",
                      })
                    }
                    disabled={deleteMutation.isPending}
                  >
                    <IconTrash className="size-3.5" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>

        <p className="text-xs text-muted-foreground">
          use tokens with{" "}
          <span className="font-mono">Authorization: Bearer &lt;token&gt;</span>{" "}
          or as the SSH password.
        </p>
      </div>
    </>
  );
}
