import { useState } from "react";
import { toast } from "sonner";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { listSshKeys, addSshKey, deleteSshKey, ApiError } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { IconKey, IconTrash } from "@tabler/icons-react";

export function SshKeysPage() {
  const qc = useQueryClient();
  const { data: keys = [] } = useQuery({
    queryKey: ["ssh-keys"],
    queryFn: listSshKeys,
  });

  const [addOpen, setAddOpen] = useState(false);
  const [keyName, setKeyName] = useState("");
  const [keyData, setKeyData] = useState("");
  const [addError, setAddError] = useState<string | null>(null);

  const addMutation = useMutation({
    mutationFn: () => addSshKey(keyName.trim(), keyData.trim()),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["ssh-keys"] });
      setAddOpen(false);
      setKeyName("");
      setKeyData("");
      setAddError(null);
      toast.success("key added");
    },
    onError: (err: unknown) => {
      setAddError(
        err instanceof ApiError ? err.message : "failed to add key",
      );
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => deleteSshKey(id),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["ssh-keys"] });
      toast.success("key removed");
    },
    onError: () => toast.error("failed to remove key"),
  });

  function openAdd() {
    setKeyName("");
    setKeyData("");
    setAddError(null);
    setAddOpen(true);
  }

  const canSubmit =
    keyName.trim().length > 0 &&
    keyData.trim().startsWith("ssh-") &&
    !addMutation.isPending;

  return (
    <>
      <Dialog open={addOpen} onOpenChange={setAddOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>add SSH key</DialogTitle>
            <DialogDescription>
              paste your public key below. you can then authenticate to VMs via
              the SSH gateway using the corresponding private key.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label htmlFor="key-name">name</Label>
              <Input
                id="key-name"
                value={keyName}
                onChange={(e) => setKeyName(e.target.value)}
                placeholder="e.g. laptop"
                autoFocus
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="key-data">public key</Label>
              <textarea
                id="key-data"
                value={keyData}
                onChange={(e) => setKeyData(e.target.value)}
                placeholder="ssh-ed25519 AAAA..."
                rows={4}
                className="w-full rounded-md border bg-transparent px-3 py-2 text-sm font-mono resize-none focus:outline-none focus:ring-1 focus:ring-ring"
              />
            </div>
            {addError && (
              <p className="text-sm text-destructive">{addError}</p>
            )}
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setAddOpen(false)}
              disabled={addMutation.isPending}
            >
              cancel
            </Button>
            <Button
              onClick={() => addMutation.mutate()}
              disabled={!canSubmit}
            >
              {addMutation.isPending ? "adding..." : "add key"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="space-y-5 mt-0.5">
        <div className="flex items-center justify-between">
          <h2 className="text-2xl font-semibold">SSH keys</h2>
          <Button size="sm" onClick={openAdd}>
            add key
          </Button>
        </div>

        <div className="rounded-lg border bg-card">
          {keys.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-10 text-muted-foreground">
              <IconKey className="size-8 opacity-40" />
              <p className="text-sm">no SSH keys yet</p>
              <Button size="sm" variant="outline" onClick={openAdd}>
                add your first key
              </Button>
            </div>
          ) : (
            <div className="divide-y divide-border">
              {keys.map((key, i) => (
                <div key={key.id}>
                  {i === 0 && <Separator className="hidden" />}
                  <div className="flex items-center justify-between px-4 py-3">
                    <div className="min-w-0">
                      <p className="text-sm font-medium">{key.name}</p>
                      <p className="text-xs font-mono text-muted-foreground truncate">
                        {key.fingerprint}
                      </p>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive shrink-0"
                      onClick={() =>
                        toast.promise(deleteMutation.mutateAsync(key.id), {
                          loading: "removing...",
                          success: "key removed",
                          error: "failed to remove key",
                        })
                      }
                      disabled={deleteMutation.isPending}
                    >
                      <IconTrash className="size-3.5" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        <p className="text-xs text-muted-foreground">
          connect via{" "}
          <span className="font-mono">
            ssh &lt;vm-name&gt;@spwn.run -p 2222
          </span>{" "}
          using any registered key, or your bearer token as the password.
        </p>
      </div>
    </>
  );
}
