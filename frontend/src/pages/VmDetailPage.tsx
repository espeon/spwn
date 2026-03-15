import { useState, useMemo, useEffect } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { trackVmToast } from "@/hooks/useVmEvents";
import { addRecentVm } from "@/hooks/useRecentVms";
import {
  getVm,
  getConfig,
  startVm,
  stopVm,
  snapshotVm,
  deleteVm,
  cloneVm,
  patchVm,
  resizeVmResources,
  listSnapshots,
  deleteSnapshot,
  restoreSnapshot,
  listVmEvents,
  ApiError,
  type VmEvent,
} from "@/api";
import { ConsolePanel } from "@/components/ConsolePanel";
import { StatusBadge } from "@/components/StatusBadge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Copy, Terminal, Pencil, Check, X, ExternalLink } from "lucide-react";
import { formatUptime, timeAgo } from "@/lib/utils";
import { copyToClipboard } from "@/lib/utils";

export function VmDetailPage() {
  const { vmId } = useParams({ from: "/_authed/vms/$vmId" });
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [consoleOpen, setConsoleOpen] = useState(false);
  const [copiedSubdomain, setCopiedSubdomain] = useState(false);
  const [copiedSsh, setCopiedSsh] = useState(false);
  const [copiedUrl, setCopiedUrl] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [cloneName, setCloneName] = useState("");
  const [cloneMemory, setCloneMemory] = useState(false);
  const [resizeOpen, setResizeOpen] = useState(false);
  const [resizeVcpus, setResizeVcpus] = useState("");
  const [resizeMemory, setResizeMemory] = useState("");
  const [portEditing, setPortEditing] = useState(false);
  const [portValue, setPortValue] = useState("");
  const [renaming, setRenaming] = useState(false);
  const [renameName, setRenameName] = useState("");

  const { data: config } = useQuery({
    queryKey: ["config"],
    queryFn: getConfig,
    staleTime: Infinity,
  });

  const {
    data: vm,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["vms", vmId],
    queryFn: () => getVm(vmId),
  });

  useEffect(() => {
    if (vm) {
      addRecentVm({
        id: vm.id,
        name: vm.name,
        status: vm.status,
        subdomain: vm.subdomain,
      });
    }
  }, [vm?.id]);

  useEffect(() => {
    const prev = document.title;
    document.title = vm ? `${vm.name} — spwn` : "spwn";
    return () => {
      document.title = prev;
    };
  }, [vm?.name]);

  const { data: snapshots, isLoading: snapshotsLoading } = useQuery({
    queryKey: ["snapshots", vmId],
    queryFn: () => listSnapshots(vmId),
  });

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ["vms", vmId] });
    qc.invalidateQueries({ queryKey: ["vms"] });
    qc.invalidateQueries({ queryKey: ["snapshots", vmId] });
  };

  const startMutation = useMutation({
    mutationFn: () => startVm(vmId),
    onSuccess: () => {
      invalidate();
      const toastId = toast.loading("starting vm...");
      trackVmToast(
        vmId,
        toastId,
        "running",
        "vm started",
        "vm failed to start",
      );
    },
    onError: (err) => toast.error(`failed to start: ${err.message}`),
  });
  const stopMutation = useMutation({
    mutationFn: () => stopVm(vmId),
    onSuccess: () => {
      invalidate();
      const toastId = toast.loading("stopping vm...");
      trackVmToast(vmId, toastId, "stopped", "vm stopped", "vm failed to stop");
    },
    onError: (err) => toast.error(`failed to stop: ${err.message}`),
  });
  const snapshotMutation = useMutation({
    mutationFn: () => snapshotVm(vmId),
    onSuccess: () => invalidate(),
  });
  const deleteMutation = useMutation({
    mutationFn: () => deleteVm(vmId),
    onSuccess: () => navigate({ to: "/vms" }),
  });

  const cloneMutation = useMutation({
    mutationFn: () => cloneVm(vmId, cloneName.trim(), cloneMemory),
    onSuccess: (newVm) => {
      qc.invalidateQueries({ queryKey: ["vms"] });
      setCloneOpen(false);
      setCloneName("");
      setCloneMemory(false);
      toast.success(`clone "${newVm.name}" created`);
      navigate({ to: "/vms/$vmId", params: { vmId: newVm.id } });
    },
    onError: (err) =>
      toast.error(
        `clone failed: ${err instanceof ApiError ? err.message : err.message}`,
      ),
  });

  const resizeMutation = useMutation({
    mutationFn: () => {
      const vcpus = resizeVcpus
        ? Math.round(parseFloat(resizeVcpus) * 1000)
        : undefined;
      const memory_mb = resizeMemory ? parseInt(resizeMemory, 10) : undefined;
      return resizeVmResources(vmId, vcpus, memory_mb);
    },
    onSuccess: () => {
      invalidate();
      setResizeOpen(false);
      toast.success("resources updated");
    },
    onError: (err) => toast.error(`resize failed: ${err.message}`),
  });

  const patchPortMutation = useMutation({
    mutationFn: (port: number) => patchVm(vmId, { exposed_port: port }),
    onSuccess: (updated) => {
      qc.setQueryData(["vms", vmId], updated);
      qc.setQueriesData<{ id: string; exposed_port: number }[]>(
        { queryKey: ["vms"] },
        (old) => {
          if (!Array.isArray(old)) return old;
          return old.map((v) =>
            v.id === vmId ? { ...v, exposed_port: updated.exposed_port } : v,
          );
        },
      );
      setPortEditing(false);
    },
    onError: (err) => toast.error(`port update failed: ${err.message}`),
  });

  const renameMutation = useMutation({
    mutationFn: (name: string) => patchVm(vmId, { name }),
    onSuccess: (updated) => {
      qc.setQueryData(["vms", vmId], updated);
      qc.setQueriesData<{ id: string; name: string }[]>(
        { queryKey: ["vms"] },
        (old) => {
          if (!Array.isArray(old)) return old;
          return old.map((v) =>
            v.id === vmId ? { ...v, name: updated.name } : v,
          );
        },
      );
      setRenaming(false);
    },
    onError: (err) => toast.error(`rename failed: ${err.message}`),
  });

  const deleteSnapMutation = useMutation({
    mutationFn: (snapId: string) => deleteSnapshot(vmId, snapId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["snapshots", vmId] });
      toast.success("snapshot deleted");
    },
    onError: (err) => toast.error(`failed to delete snapshot: ${err.message}`),
  });

  const restoreSnapMutation = useMutation({
    mutationFn: (snapId: string) => restoreSnapshot(vmId, snapId),
    onSuccess: () => {
      invalidate();
      const toastId = toast.loading("restoring snapshot...");
      trackVmToast(vmId, toastId, "running", "vm restored", "restore failed");
    },
    onError: (err) => toast.error(`failed to restore: ${err.message}`),
  });

  if (isLoading)
    return (
      <div className="space-y-6">
        <div className="flex items-start justify-between">
          <div className="space-y-2">
            <div className="flex items-center gap-3">
              <Skeleton className="h-7 w-40" />
              <Skeleton className="h-5 w-16 rounded-full" />
            </div>
            <Skeleton className="h-4 w-56" />
          </div>
        </div>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} className="h-[62px] rounded-lg" />
          ))}
        </div>
        <Skeleton className="h-10 w-full rounded-md" />
        <div className="flex gap-3">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-9 w-20 rounded-md" />
          ))}
        </div>
      </div>
    );

  if (error) {
    const status = error instanceof ApiError ? error.status : null;
    if (status === 404) {
      return (
        <div className="text-center py-24 text-muted-foreground">
          <p className="text-sm">this vm no longer exists.</p>
          <button
            onClick={() => navigate({ to: "/vms" })}
            className="mt-2 text-sm underline underline-offset-4 hover:no-underline"
          >
            back to vms
          </button>
        </div>
      );
    }
    return <p className="text-destructive text-sm">failed to load vm</p>;
  }

  if (!vm) return null;

  const isTransitioning =
    vm.status === "starting" ||
    vm.status === "stopping" ||
    vm.status === "snapshotting";
  const canStart = vm.status === "stopped" || vm.status === "error";
  const canStop = vm.status === "running";
  const canSnapshot = vm.status === "running";
  const canClone = vm.status === "stopped" || vm.status === "running";

  function openClone() {
    if (!vm) return console.error("VM not found");
    setCloneName(`${vm.name}-clone`);
    setCloneMemory(vm.status === "running");
    setCloneOpen(true);
  }

  function openResize() {
    if (!vm) return console.error("VM not found");
    setResizeVcpus(String(vm.vcpus / 1000));
    setResizeMemory(String(vm.memory_mb));
    setResizeOpen(true);
  }

  return (
    <>
      <Dialog open={cloneOpen} onOpenChange={setCloneOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>clone {vm.name}</DialogTitle>
            <DialogDescription>
              creates a new VM with a copy of this VM's disk. if you include
              memory, the clone will start running in the same state.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label htmlFor="clone-name">clone name</Label>
              <Input
                id="clone-name"
                value={cloneName}
                onChange={(e) => setCloneName(e.target.value)}
                placeholder="my-clone"
                autoFocus
              />
            </div>
            {vm.status === "running" && (
              <label className="flex items-center gap-2 text-sm cursor-pointer select-none">
                <input
                  type="checkbox"
                  className="rounded"
                  checked={cloneMemory}
                  onChange={(e) => setCloneMemory(e.target.checked)}
                />
                include memory state (clone starts running)
              </label>
            )}
            {cloneMemory && (
              <p className="text-xs text-muted-foreground">
                the clone will boot with the source's network state — you may
                need to reconfigure networking inside the guest.
              </p>
            )}
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setCloneOpen(false)}
              disabled={cloneMutation.isPending}
            >
              cancel
            </Button>
            <Button
              onClick={() => cloneMutation.mutate()}
              disabled={!cloneName.trim() || cloneMutation.isPending}
            >
              {cloneMutation.isPending ? "cloning..." : "clone"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={resizeOpen} onOpenChange={setResizeOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>resize {vm.name}</DialogTitle>
            <DialogDescription>
              {vm.status === "running"
                ? "cpu changes apply immediately. memory changes require a restart."
                : "changes take effect on next start."}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-2">
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="resize-vcpus">vcpus</Label>
                <Input
                  id="resize-vcpus"
                  type="number"
                  min="0.1"
                  step="0.1"
                  value={resizeVcpus}
                  onChange={(e) => setResizeVcpus(e.target.value)}
                  placeholder="1"
                  autoFocus
                />
                <p className="text-xs text-muted-foreground">fractional ok</p>
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="resize-memory">memory (mb)</Label>
                <Input
                  id="resize-memory"
                  type="number"
                  min="128"
                  step="128"
                  value={resizeMemory}
                  onChange={(e) => setResizeMemory(e.target.value)}
                  placeholder="512"
                />
                <p className="text-xs text-muted-foreground">
                  multiples of 128
                </p>
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setResizeOpen(false)}
              disabled={resizeMutation.isPending}
            >
              cancel
            </Button>
            <Button
              onClick={() => resizeMutation.mutate()}
              disabled={
                (!resizeVcpus && !resizeMemory) || resizeMutation.isPending
              }
            >
              {resizeMutation.isPending ? "resizing..." : "apply"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="flex items-start justify-between mb-6">
        <div>
          <div className="flex items-center gap-3 mb-1">
            {renaming ? (
              <form
                className="flex items-center gap-1"
                onSubmit={(e) => {
                  e.preventDefault();
                  const trimmed = renameName.trim();
                  if (trimmed && trimmed !== vm.name)
                    renameMutation.mutate(trimmed);
                  else setRenaming(false);
                }}
              >
                <Input
                  autoFocus
                  className="h-7 text-xl font-semibold px-1 w-48"
                  value={renameName}
                  onChange={(e) => setRenameName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") setRenaming(false);
                  }}
                  disabled={renameMutation.isPending}
                />
                <button
                  type="submit"
                  className="p-1 hover:text-green-500 transition-colors"
                  disabled={renameMutation.isPending}
                >
                  <Check className="size-3.5" />
                </button>
                <button
                  type="button"
                  className="p-1 hover:text-destructive transition-colors"
                  onClick={() => setRenaming(false)}
                >
                  <X className="size-3.5" />
                </button>
              </form>
            ) : (
              <button
                className="group flex items-center gap-1.5"
                onClick={() => {
                  setRenameName(vm.name);
                  setRenaming(true);
                }}
                title="rename vm"
              >
                <h1 className="text-xl font-semibold">{vm.name}</h1>
                <Pencil className="size-3.5 opacity-0 group-hover:opacity-40 transition-opacity text-muted-foreground" />
              </button>
            )}
            <StatusBadge status={vm.status} />
          </div>
          <button
            className="group flex items-center gap-1.5 text-sm text-muted-foreground font-mono hover:text-foreground transition-colors mt-1"
            onClick={async () => {
              const ok = await copyToClipboard(vm.subdomain);
              if (ok) {
                setCopiedSubdomain(true);
                setTimeout(() => setCopiedSubdomain(false), 1500);
              }
            }}
            title="copy subdomain"
          >
            {vm.subdomain}
            <Copy
              className={`h-3.5 w-3.5 shrink-0 transition-opacity ${copiedSubdomain ? "opacity-100 text-green-500" : "opacity-0 group-hover:opacity-60"}`}
            />
          </button>
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setConfirmDelete(true)}
          disabled={deleteMutation.isPending || isTransitioning}
          className="text-destructive hover:text-destructive hover:bg-destructive/10"
        >
          delete
        </Button>
      </div>

      <div className="grid grid-cols-2 gap-3 mb-6 sm:grid-cols-3 lg:grid-cols-6">
        {(
          [
            ["vcpus", String(vm.vcpus / 1000)],
            ["memory", `${vm.memory_mb} mb`],
            ["ip", vm.ip_address],
            ["image", vm.image],
            ["region", vm.region ?? "—"],
          ] as const
        ).map(([label, value]) => (
          <Card key={label}>
            <CardContent className="px-4 py-3">
              <p className="text-xs text-muted-foreground mb-1">{label}</p>
              <p className="font-mono text-sm truncate" title={value}>
                {value}
              </p>
            </CardContent>
          </Card>
        ))}
        <Card className="group">
          <CardContent className="px-4 py-3">
            <p className="text-xs text-muted-foreground mb-1">port</p>
            {portEditing ? (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  const p = parseInt(portValue, 10);
                  if (p >= 1 && p <= 65535 && p !== vm.exposed_port)
                    patchPortMutation.mutate(p);
                  else setPortEditing(false);
                }}
              >
                <Input
                  autoFocus
                  type="number"
                  min={1}
                  max={65535}
                  className="h-6 text-sm font-mono px-1 w-full"
                  value={portValue}
                  onChange={(e) => setPortValue(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") setPortEditing(false);
                  }}
                  onBlur={() => setPortEditing(false)}
                  disabled={patchPortMutation.isPending}
                />
              </form>
            ) : (
              <button
                className="flex items-center gap-1 w-full group/port"
                onClick={() => {
                  setPortValue(String(vm.exposed_port));
                  setPortEditing(true);
                }}
                title="edit exposed port"
              >
                <span className="font-mono text-sm truncate">
                  {vm.exposed_port}
                </span>
                <Pencil className="size-3 shrink-0 opacity-0 group-hover/port:opacity-40 transition-opacity text-muted-foreground" />
              </button>
            )}
          </CardContent>
        </Card>
      </div>

      <div className="flex gap-4 text-xs text-muted-foreground mb-6">
        <span>created {timeAgo(vm.created_at)}</span>
        {vm.status === "running" && vm.last_started_at && (
          <span className="text-green-500/70">
            up {formatUptime(vm.last_started_at)}
          </span>
        )}
      </div>

      <Dialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>delete {vm.name}?</DialogTitle>
            <DialogDescription>
              this is permanent. the vm and all its data will be gone!
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setConfirmDelete(false)}
              disabled={deleteMutation.isPending}
            >
              cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() =>
                toast.promise(deleteMutation.mutateAsync(), {
                  loading: "deleting vm...",
                  success: "vm deleted",
                  error: (err) => `failed to delete: ${err.message}`,
                })
              }
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending ? "deleting..." : "delete"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {config &&
        (() => {
          const [gwHost, gwPort] = config.ssh_gateway_addr.split(":");
          const sshCmd = `ssh ${vm.id}@${gwHost} -p ${gwPort ?? "22"}`;
          // const publicUrl = `${config.public_url.replace(/\/$/, "")}`;
          const vmUrl = `https://${vm.subdomain}`;
          return (
            <div className="flex flex-col gap-2 mb-6">
              <button
                className="group flex items-center gap-2 w-full rounded-md border bg-muted/40 px-4 py-2.5 text-xs font-mono text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                onClick={async () => {
                  const ok = await copyToClipboard(sshCmd);
                  if (ok) {
                    setCopiedSsh(true);
                    setTimeout(() => setCopiedSsh(false), 1500);
                  }
                }}
                title="copy ssh command"
              >
                <Terminal className="h-3.5 w-3.5 shrink-0 opacity-60" />
                <span className="flex-1 text-left">{sshCmd}</span>
                <Copy
                  className={`h-3.5 w-3.5 shrink-0 transition-opacity ${copiedSsh ? "opacity-100 text-green-500" : "opacity-0 group-hover:opacity-60"}`}
                />
              </button>
              <div className="group flex items-center gap-2 w-full rounded-md border bg-muted/40 px-4 py-2.5 text-xs font-mono text-muted-foreground">
                <ExternalLink className="h-3.5 w-3.5 shrink-0 opacity-60" />
                <a
                  href={vmUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex-1 text-left hover:text-foreground transition-colors truncate"
                >
                  {vmUrl}
                </a>
                <button
                  onClick={async () => {
                    const ok = await copyToClipboard(vmUrl);
                    if (ok) {
                      setCopiedUrl(true);
                      setTimeout(() => setCopiedUrl(false), 1500);
                    }
                  }}
                  title="copy url"
                >
                  <Copy
                    className={`h-3.5 w-3.5 shrink-0 transition-opacity ${copiedUrl ? "opacity-100 text-green-500" : "opacity-0 group-hover:opacity-60"}`}
                  />
                </button>
              </div>
            </div>
          );
        })()}

      <div className="flex gap-3">
        <Button
          variant="outline"
          onClick={() => startMutation.mutate()}
          disabled={!canStart || isTransitioning || startMutation.isPending}
          className="border-green-800 text-green-400 hover:bg-green-950 hover:text-green-300 disabled:opacity-40"
        >
          {startMutation.isPending ? "starting..." : "start"}
        </Button>
        <Button
          variant="outline"
          onClick={() => stopMutation.mutate()}
          disabled={!canStop || isTransitioning || stopMutation.isPending}
        >
          {stopMutation.isPending ? "stopping..." : "stop"}
        </Button>
        <Button
          variant="outline"
          onClick={() =>
            toast.promise(snapshotMutation.mutateAsync(), {
              loading: "taking snapshot...",
              success: "snapshot taken",
              error: (err) => `failed to snapshot: ${err.message}`,
            })
          }
          disabled={
            !canSnapshot || isTransitioning || snapshotMutation.isPending
          }
          className="border-blue-800 text-blue-400 hover:bg-blue-950 hover:text-blue-300 disabled:opacity-40"
        >
          {snapshotMutation.isPending ? "snapshotting..." : "snapshot"}
        </Button>
        <Button
          variant="outline"
          onClick={openClone}
          disabled={!canClone || isTransitioning || cloneMutation.isPending}
        >
          {cloneMutation.isPending ? "cloning..." : "clone"}
        </Button>
        <Button
          variant="outline"
          onClick={openResize}
          disabled={isTransitioning || resizeMutation.isPending}
        >
          resize
        </Button>
        <Button
          variant="outline"
          onClick={() => setConsoleOpen((o) => !o)}
          disabled={vm.status !== "running"}
        >
          {consoleOpen ? "hide console" : "console"}
        </Button>
      </div>

      {consoleOpen && vm.status === "running" && (
        <div className="mt-4 h-80 rounded-md border overflow-hidden">
          <ConsolePanel vmId={vmId} open={consoleOpen} />
        </div>
      )}

      <EventLog vmId={vmId} status={vm.status} />

      <div className="mt-8">
        <h2 className="text-sm font-medium mb-3">snapshots</h2>
        {snapshotsLoading ? (
          <p className="text-xs text-muted-foreground">loading...</p>
        ) : !snapshots?.length ? (
          <p className="text-xs text-muted-foreground">no snapshots yet</p>
        ) : (
          <div className="space-y-2">
            {snapshots.map((snap) => (
              <div
                key={snap.id}
                className="flex items-center justify-between rounded-md border px-4 py-3"
              >
                <div>
                  <p className="text-sm font-mono">
                    {snap.label ?? snap.id.slice(0, 8)}
                  </p>
                  <p className="text-xs text-muted-foreground mt-0.5">
                    {(snap.size_bytes / 1024 / 1024).toFixed(1)} mb &middot;{" "}
                    {new Date(snap.created_at * 1000).toLocaleString()}
                  </p>
                </div>
                <div className="flex gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      toast.promise(restoreSnapMutation.mutateAsync(snap.id), {
                        loading: "restoring...",
                        success: "restored",
                        error: (err) => `restore failed: ${err.message}`,
                      })
                    }
                    disabled={
                      vm.status !== "stopped" ||
                      restoreSnapMutation.isPending ||
                      deleteSnapMutation.isPending
                    }
                  >
                    restore
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => deleteSnapMutation.mutate(snap.id)}
                    disabled={
                      deleteSnapMutation.isPending ||
                      restoreSnapMutation.isPending
                    }
                    className="text-destructive hover:text-destructive hover:bg-destructive/10"
                  >
                    delete
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </>
  );
}

function eventColor(event: string): string {
  switch (event) {
    case "started":
      return "text-green-500";
    case "stopped":
      return "text-muted-foreground";
    case "error":
    case "crashed":
      return "text-destructive";
    case "snapshot_taken":
      return "text-blue-400";
    default:
      return "text-muted-foreground";
  }
}

function EventLog({ vmId, status }: { vmId: string; status: string }) {
  const isError = status === "error";

  const { data: events = [], isLoading } = useQuery({
    queryKey: ["vm-events", vmId],
    queryFn: () => listVmEvents(vmId, 20),
    refetchInterval: 10_000,
  });

  const latestError = useMemo(() => {
    if (!isError) return null;
    return (
      events.find((e) => e.event === "crashed" || e.event === "error") ?? null
    );
  }, [events, isError]);

  return (
    <div className="mt-8">
      <h2 className="text-sm font-medium mb-3">events</h2>

      {isError && latestError && (
        <div className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3">
          <p className="text-xs font-medium text-destructive mb-1">
            {latestError.event === "crashed" ? "vm crashed" : "start failed"}
          </p>
          <p className="text-xs font-mono text-destructive/90 break-all">
            {latestError.metadata ?? "unknown error"}
          </p>
        </div>
      )}

      {isLoading ? (
        <p className="text-xs text-muted-foreground">loading...</p>
      ) : events.length === 0 ? (
        <p className="text-xs text-muted-foreground">no events yet</p>
      ) : (
        <div className="rounded-md border divide-y divide-border text-xs">
          {events.map((ev) => (
            <EventRow key={ev.id} event={ev} />
          ))}
        </div>
      )}
    </div>
  );
}

function EventRow({ event }: { event: VmEvent }) {
  const [expanded, setExpanded] = useState(false);
  const time = useMemo(
    () => new Date(event.created_at * 1000).toLocaleString(),
    [event.created_at],
  );

  return (
    <div
      className={`px-4 py-2.5 ${event.metadata ? "cursor-pointer hover:bg-muted/40" : ""}`}
      onClick={() => event.metadata && setExpanded((e) => !e)}
    >
      <div className="flex items-center justify-between gap-3">
        <span className={`font-medium ${eventColor(event.event)}`}>
          {event.event}
        </span>
        <span className="text-muted-foreground shrink-0">{time}</span>
      </div>
      {expanded && event.metadata && (
        <p className="mt-1.5 font-mono text-muted-foreground break-all">
          {event.metadata}
        </p>
      )}
    </div>
  );
}
