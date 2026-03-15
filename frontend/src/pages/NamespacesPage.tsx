import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { IconBuilding, IconUser, IconPlus } from "@tabler/icons-react";
import { toast } from "sonner";
import { listNamespaces, createNamespace, type Namespace, ApiError } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

function NamespaceCard({ ns }: { ns: Namespace }) {
  const Icon = ns.kind === "personal" ? IconUser : IconBuilding;

  return (
    <Link
      to="/namespaces/$nsId"
      params={{ nsId: ns.id }}
      className="rounded-lg border bg-card px-5 py-4 flex items-center justify-between hover:bg-accent/50 transition-colors"
    >
      <div className="flex items-center gap-4 min-w-0">
        <Icon className="size-8 shrink-0 text-muted-foreground opacity-60" />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-medium">{ns.display_name ?? ns.slug}</span>
            <span className="text-xs font-mono bg-secondary px-1.5 py-0.5 rounded text-muted-foreground">
              {ns.slug}
            </span>
            <span className="text-xs text-muted-foreground">{ns.kind}</span>
          </div>
          <p className="text-xs text-muted-foreground mt-0.5">
            {ns.vcpu_limit / 1000} vCPUs · {ns.mem_limit_mb / 1024} GB RAM · {ns.vm_limit} VMs
          </p>
        </div>
      </div>
    </Link>
  );
}

function CreateNamespaceDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [displayName, setDisplayName] = useState("");

  const mutation = useMutation({
    mutationFn: () => createNamespace(slug, displayName || undefined),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["namespaces"] });
      toast.success("namespace created");
      onOpenChange(false);
      setSlug("");
      setDisplayName("");
    },
    onError: (e) => {
      toast.error(e instanceof ApiError ? e.message : "failed to create namespace");
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>new namespace</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4 pt-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="ns-slug">slug</Label>
            <Input
              id="ns-slug"
              placeholder="my-org"
              value={slug}
              onChange={(e) => setSlug(e.target.value.toLowerCase())}
            />
            <p className="text-xs text-muted-foreground">
              lowercase letters, numbers, and hyphens only · globally unique
            </p>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="ns-display">display name</Label>
            <Input
              id="ns-display"
              placeholder="My Org (optional)"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
            />
          </div>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!slug || mutation.isPending}
            className="self-end"
          >
            create
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function NamespacesPage() {
  const [createOpen, setCreateOpen] = useState(false);
  const { data: namespaces = [], isLoading, error } = useQuery({
    queryKey: ["namespaces"],
    queryFn: listNamespaces,
  });

  if (isLoading)
    return (
      <div className="flex flex-col gap-2">
        {Array.from({ length: 3 }).map((_, i) => (
          <Skeleton key={i} className="h-[68px] rounded-lg" />
        ))}
      </div>
    );
  if (error) return <p className="text-destructive text-sm">failed to load namespaces</p>;

  const orgs = namespaces.filter((n) => n.kind !== "personal");
  const personal = namespaces.find((n) => n.kind === "personal");

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-xl font-semibold">namespaces</h1>
          <p className="text-sm text-muted-foreground mt-1">
            quota groups and access scopes for your VMs
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateOpen(true)}>
          <IconPlus className="size-4" />
          new org
        </Button>
      </div>

      {personal && (
        <div className="flex flex-col gap-2">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">personal</p>
          <NamespaceCard ns={personal} />
        </div>
      )}

      {orgs.length > 0 && (
        <div className="flex flex-col gap-2">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">orgs</p>
          {orgs.map((ns) => (
            <NamespaceCard key={ns.id} ns={ns} />
          ))}
        </div>
      )}

      {namespaces.length === 0 && (
        <div className="flex flex-col items-center gap-2 py-24 text-muted-foreground">
          <IconBuilding className="size-10 opacity-30" />
          <p className="text-sm">no namespaces yet</p>
        </div>
      )}

      <CreateNamespaceDialog open={createOpen} onOpenChange={setCreateOpen} />
    </div>
  );
}
