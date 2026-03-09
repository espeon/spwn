import { useState } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { getVm, startVm, stopVm, snapshotVm, deleteVm, ApiError } from "@/api";
import { StatusBadge } from "@/components/StatusBadge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
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

  const {
    data: vm,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["vms", vmId],
    queryFn: () => getVm(vmId),
  });

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ["vms", vmId] });
    qc.invalidateQueries({ queryKey: ["vms"] });
  };

  const startMutation = useMutation({
    mutationFn: () => startVm(vmId),
    onSuccess: () => {
      invalidate();
      toast.success("vm starting");
    },
    onError: (err) => toast.error(`failed to start: ${err.message}`),
  });
  const stopMutation = useMutation({
    mutationFn: () => stopVm(vmId),
    onSuccess: () => {
      invalidate();
      toast.success("vm stopping");
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

  const mutError =
    startMutation.error?.message ||
    stopMutation.error?.message ||
    snapshotMutation.error?.message ||
    deleteMutation.error?.message;

  return (
    <>
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
            ["vcores", vm.vcores],
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
              this is permanent. the vm and all its data will be gone.
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
      </div>
    </>
  );
}
