import { useState, useMemo } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { trackVmToast } from "@/hooks/useVmEvents";
import {
  getVm,
  startVm,
  stopVm,
  snapshotVm,
  deleteVm,
  cloneVm,
  resizeVmResources,
  listSnapshots,
  deleteSnapshot,
  restoreSnapshot,
  listVmEvents,
  ApiError,
  type VmEvent,
} from "@/api";
import { StatusBadge } from "@/components/StatusBadge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
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

export function VmDetailPage() {
  const { vmId } = useParams({ from: "/_authed/vms/$vmId" });
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [cloneName, setCloneName] = useState("");
  const [cloneMemory, setCloneMemory] = useState(false);
  const [resizeOpen, setResizeOpen] = useState(false);
  const [resizeVcpus, setResizeVcpus] = useState("");

  const {
    data: vm,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["vms", vmId],
    queryFn: () => getVm(vmId),
  });

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
      return resizeVmResources(vmId, vcpus, undefined);
    },
    onSuccess: () => {
      invalidate();
      setResizeOpen(false);
      toast.success("resources updated");
    },
    onError: (err) => toast.error(`resize failed: ${err.message}`),
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
    return <p className="text-muted-foreground text-sm">loading...</p>;

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
              <p className="text-xs text-muted-foreground">
                e.g. 0.5, 1, 2 — fractional vCPUs allowed
              </p>
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
              disabled={!resizeVcpus || resizeMutation.isPending}
            >
              {resizeMutation.isPending ? "resizing..." : "apply"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="flex items-start justify-between mb-6">
        <div>
          <div className="flex items-center gap-3 mb-1">
            <h1 className="text-xl font-semibold">{vm.name}</h1>
            <StatusBadge status={vm.status} />
          </div>
          <p className="text-sm text-muted-foreground font-mono">
            {vm.subdomain}
          </p>
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

      <div className="grid grid-cols-2 gap-3 mb-6 sm:grid-cols-4">
        {(
          [
            ["vcpus", vm.vcpus / 1000],
            ["memory", `${vm.memory_mb} mb`],
            ["ip", vm.ip_address],
            ["port", vm.exposed_port],
          ] as const
        ).map(([label, value]) => (
          <Card key={label}>
            <CardContent className="px-4 py-3">
              <p className="text-xs text-muted-foreground mb-1">{label}</p>
              <p className="font-mono text-sm">{value}</p>
            </CardContent>
          </Card>
        ))}
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
      </div>

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
    refetchInterval: isError ? false : 10_000,
  });

  const latestError = useMemo(() => {
    if (!isError) return null;
    return events.find((e) => e.event === "error") ?? null;
  }, [events, isError]);

  return (
    <div className="mt-8">
      <h2 className="text-sm font-medium mb-3">events</h2>

      {isError && latestError && (
        <div className="mb-3 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3">
          <p className="text-xs font-medium text-destructive mb-1">
            start failed
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
