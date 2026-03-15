import { useState } from "react";
import { useParams } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { IconUser, IconTrash, IconUserPlus } from "@tabler/icons-react";
import { toast } from "sonner";
import {
  getNamespace,
  listNamespaceMembers,
  addNamespaceMember,
  removeNamespaceMember,
  getMe,
  ApiError,
  type NamespaceMember,
} from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";

function AddMemberDialog({
  nsId,
  open,
  onOpenChange,
}: {
  nsId: string;
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const qc = useQueryClient();
  const [username, setUsername] = useState("");
  const [role, setRole] = useState("member");

  const mutation = useMutation({
    mutationFn: () => addNamespaceMember(nsId, username, role),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["namespace-members", nsId] });
      toast.success(`added ${username}`);
      onOpenChange(false);
      setUsername("");
      setRole("member");
    },
    onError: (e) => {
      toast.error(e instanceof ApiError ? e.message : "failed to add member");
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>add member</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4 pt-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="add-username">username</Label>
            <Input
              id="add-username"
              placeholder="alice"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="add-role">role</Label>
            <select
              id="add-role"
              value={role}
              onChange={(e) => setRole(e.target.value)}
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <option value="member">member</option>
              <option value="owner">owner</option>
            </select>
          </div>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!username || mutation.isPending}
            className="self-end"
          >
            add
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function NamespaceDetailPage() {
  const { nsId } = useParams({ from: "/_authed/namespaces/$nsId" });
  const qc = useQueryClient();
  const [addOpen, setAddOpen] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState<NamespaceMember | null>(null);

  const { data: me } = useQuery({ queryKey: ["me"], queryFn: getMe });
  const { data: ns, isLoading: nsLoading } = useQuery({
    queryKey: ["namespace", nsId],
    queryFn: () => getNamespace(nsId),
  });
  const { data: members = [], isLoading: membersLoading } = useQuery({
    queryKey: ["namespace-members", nsId],
    queryFn: () => listNamespaceMembers(nsId),
  });

  const removeMutation = useMutation({
    mutationFn: (accountId: string) => removeNamespaceMember(nsId, accountId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["namespace-members", nsId] });
      toast.success("member removed");
    },
    onError: (e) => {
      toast.error(e instanceof ApiError ? e.message : "failed to remove member");
    },
  });

  if (nsLoading || membersLoading) return <p className="text-muted-foreground text-sm">loading...</p>;
  if (!ns) return <p className="text-destructive text-sm">namespace not found</p>;

  const myMembership = members.find((m) => m.account_id === me?.id);
  const isOwner = myMembership?.role === "owner";

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between">
        <div>
          <div className="flex items-center gap-2">
            <h1 className="text-xl font-semibold">{ns.display_name ?? ns.slug}</h1>
            <span className="text-xs font-mono bg-secondary px-1.5 py-0.5 rounded text-muted-foreground">
              {ns.slug}
            </span>
            <span className="text-xs text-muted-foreground">{ns.kind}</span>
          </div>
          <p className="text-sm text-muted-foreground mt-1">
            {ns.vcpu_limit / 1000} vCPUs · {ns.mem_limit_mb / 1024} GB RAM · {ns.vm_limit} VMs
          </p>
        </div>
        {isOwner && ns.kind !== "personal" && (
          <Button size="sm" onClick={() => setAddOpen(true)}>
            <IconUserPlus className="size-4" />
            add member
          </Button>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
          members · {members.length}
        </p>
        {members.map((m) => (
          <div
            key={m.account_id}
            className="rounded-lg border bg-card px-5 py-3 flex items-center justify-between"
          >
            <div className="flex items-center gap-3">
              <IconUser className="size-5 text-muted-foreground opacity-60" />
              <div>
                <span className="text-sm font-medium">{m.username}</span>
                <span className="ml-2 text-xs font-mono text-muted-foreground">{m.account_id}</span>
                <span className="ml-2 text-xs text-muted-foreground">{m.role}</span>
              </div>
            </div>
            {isOwner && ns.kind !== "personal" && m.account_id !== me?.id && (
              <Button
                variant="ghost"
                size="sm"
                className="text-destructive hover:text-destructive"
                onClick={() => setConfirmRemove(m)}
                disabled={removeMutation.isPending}
              >
                <IconTrash className="size-4" />
              </Button>
            )}
          </div>
        ))}
      </div>

      <AddMemberDialog nsId={nsId} open={addOpen} onOpenChange={setAddOpen} />

      <Dialog open={!!confirmRemove} onOpenChange={(o) => !o && setConfirmRemove(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>remove {confirmRemove?.username}?</DialogTitle>
            <DialogDescription>
              they will lose access to this namespace immediately.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setConfirmRemove(null)} disabled={removeMutation.isPending}>
              cancel
            </Button>
            <Button
              variant="destructive"
              disabled={removeMutation.isPending}
              onClick={() => {
                if (!confirmRemove) return;
                removeMutation.mutate(confirmRemove.account_id, {
                  onSuccess: () => setConfirmRemove(null),
                });
              }}
            >
              {removeMutation.isPending ? "removing..." : "remove"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
