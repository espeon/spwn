import { useState, type FormEvent } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  listVms,
  createVm,
  startVm,
  stopVm,
  type CreateVmRequest,
  type Vm,
} from "@/api";
import { StatusBadge } from "@/components/StatusBadge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";

function CreateVmDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [vcores, setVcores] = useState(1);
  const [memoryMb, setMemoryMb] = useState(512);
  const [port, setPort] = useState(8080);
  const [error, setError] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: (req: CreateVmRequest) => createVm(req),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["vms"] });
      onClose();
    },
    onError: (err) => setError(err.message),
  });

  function submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    mutation.mutate({ name, vcores, memory_mb: memoryMb, exposed_port: port });
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>new vm</DialogTitle>
        </DialogHeader>
        <form id="create-vm-form" onSubmit={submit} className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="vm-name">name</Label>
            <Input
              id="vm-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="vm-vcores">vcores</Label>
              <Input
                id="vm-vcores"
                type="number"
                min={1}
                max={8}
                value={vcores}
                onChange={(e) => setVcores(Number(e.target.value))}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="vm-memory">memory (mb)</Label>
              <Input
                id="vm-memory"
                type="number"
                min={128}
                step={128}
                value={memoryMb}
                onChange={(e) => setMemoryMb(Number(e.target.value))}
              />
            </div>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="vm-port">exposed port</Label>
            <Input
              id="vm-port"
              type="number"
              value={port}
              onChange={(e) => setPort(Number(e.target.value))}
            />
          </div>
          {error && <p className="text-sm text-destructive">{error}</p>}
        </form>
        <DialogFooter>
          <Button variant="outline" type="button" onClick={onClose}>
            cancel
          </Button>
          <Button
            type="submit"
            form="create-vm-form"
            disabled={mutation.isPending}
          >
            {mutation.isPending ? "creating..." : "create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function VmListPage() {
  const [showCreate, setShowCreate] = useState(false);
  const {
    data: vms,
    isLoading,
    error,
  } = useQuery({ queryKey: ["vms"], queryFn: listVms });

  if (isLoading)
    return <p className="text-muted-foreground text-sm">loading...</p>;
  if (error)
    return <p className="text-destructive text-sm">failed to load vms</p>;

  return (
    <>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-xl font-semibold">virtual machines</h1>
        <Button size="sm" onClick={() => setShowCreate(true)}>
          new vm
        </Button>
      </div>

      {vms && vms.length === 0 ? (
        <div className="text-center py-24 text-muted-foreground">
          <p className="text-sm">no vms yet.</p>
          <button
            onClick={() => setShowCreate(true)}
            className="mt-3 text-sm underline underline-offset-4 hover:no-underline"
          >
            create your first vm
          </button>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {vms?.map((vm) => (
            <VmRow key={vm.id} vm={vm} />
          ))}
        </div>
      )}

      <CreateVmDialog open={showCreate} onClose={() => setShowCreate(false)} />
    </>
  );
}

function VmRow({ vm }: { vm: Vm }) {
  const qc = useQueryClient();

  const invalidate = () => qc.invalidateQueries({ queryKey: ["vms"] });

  const startMutation = useMutation({
    mutationFn: () => startVm(vm.id),
    onSuccess: invalidate,
  });
  const stopMutation = useMutation({
    mutationFn: () => stopVm(vm.id),
    onSuccess: invalidate,
  });

  const isTransitioning =
    vm.status === "starting" ||
    vm.status === "stopping" ||
    vm.status === "snapshotting";
  const canStart = vm.status === "stopped" || vm.status === "error";
  const canStop = vm.status === "running";

  return (
    <div className="border rounded-lg px-5 py-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Link
            to="/vms/$vmId"
            params={{ vmId: vm.id }}
            className="font-medium hover:underline underline-offset-4"
          >
            {vm.name}
          </Link>
          <StatusBadge status={vm.status} />
          <span className="text-xs text-muted-foreground font-mono bg-secondary px-1.5 py-0.5 rounded">
            {vm.image}
          </span>
        </div>
        <span className="text-xs text-muted-foreground font-mono">
          {vm.subdomain}
        </span>
      </div>
      <div className="mt-2 flex items-center justify-between">
        <div className="flex gap-4 text-xs text-muted-foreground">
          <span>{vm.vcores}c</span>
          <span>{vm.memory_mb}mb</span>
          <span>:{vm.exposed_port}</span>
        </div>
        <div className="flex gap-1.5">
          <Button
            variant="outline"
            size="sm"
            className="h-6 text-xs px-2 border-green-800 text-green-400 hover:bg-green-950 hover:text-green-300 disabled:opacity-40"
            disabled={!canStart || isTransitioning || startMutation.isPending}
            onClick={(e) => {
              e.preventDefault();
              startMutation.mutate();
            }}
          >
            {startMutation.isPending ? "starting..." : "start"}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="h-6 text-xs px-2 disabled:opacity-40"
            disabled={!canStop || isTransitioning || stopMutation.isPending}
            onClick={(e) => {
              e.preventDefault();
              stopMutation.mutate();
            }}
          >
            {stopMutation.isPending ? "stopping..." : "stop"}
          </Button>
        </div>
      </div>
    </div>
  );
}
