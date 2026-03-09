import { useState, useEffect, type FormEvent } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
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
import { IconTruckLoading } from "@tabler/icons-react";
import { FileQuestion, Loader, Pause, Play } from "lucide-react";

function CreateVmDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [vcores, setVcores] = useState(2);
  const [memoryMb, setMemoryMb] = useState(512);
  const [port, setPort] = useState(8080);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [submitError, setSubmitError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setName("");
      setVcores(2);
      setMemoryMb(512);
      setPort(8080);
      setFieldErrors({});
      setSubmitError(null);
    }
  }, [open]);

  function validate(): Record<string, string> {
    const errs: Record<string, string> = {};
    const trimmed = name.trim();
    if (!trimmed) {
      errs.name = "name is required";
    } else if (trimmed.length > 63) {
      errs.name = "name must be 63 characters or fewer";
    } else if (!/^[a-zA-Z0-9][a-zA-Z0-9\- ]*$/.test(trimmed)) {
      errs.name = "letters, numbers, hyphens, and spaces only";
    }
    if (vcores < 1 || vcores > 8) {
      errs.vcores = "must be between 1 and 8";
    }
    if (memoryMb < 128 || memoryMb > 12288 || memoryMb % 128 !== 0) {
      errs.memory = "must be 128–12288 mb in multiples of 128";
    }
    if (port < 1 || port > 65535) {
      errs.port = "must be between 1 and 65535";
    }
    return errs;
  }

  const mutation = useMutation({
    mutationFn: (req: CreateVmRequest) => createVm(req),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["vms"] });
      onClose();
    },
    onError: () => {},
  });

  function submit(e: FormEvent) {
    e.preventDefault();
    setSubmitError(null);
    const errs = validate();
    if (Object.keys(errs).length > 0) {
      setFieldErrors(errs);
      return;
    }
    setFieldErrors({});
    const req = {
      name: name.trim(),
      vcores,
      memory_mb: memoryMb,
      exposed_port: port,
    };
    const toastId = toast.loading("creating vm...");
    mutation
      .mutateAsync(req)
      .then(() => {
        toast.success("vm created", { id: toastId });
      })
      .catch((err: Error) => {
        const msg = err.message.toLowerCase();
        if (msg.includes("quota") || msg.includes("limit")) {
          toast.error(err.message, { id: toastId });
        } else {
          toast.dismiss(toastId);
          setSubmitError(err.message);
        }
      });
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
              onChange={(e) => {
                setName(e.target.value);
                if (fieldErrors.name)
                  setFieldErrors((p) => ({ ...p, name: "" }));
              }}
              aria-invalid={!!fieldErrors.name}
              className={
                fieldErrors.name
                  ? "border-destructive focus-visible:ring-destructive"
                  : ""
              }
            />
            {fieldErrors.name && (
              <p className="text-xs text-destructive">{fieldErrors.name}</p>
            )}
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
                onChange={(e) => {
                  setVcores(Number(e.target.value));
                  if (fieldErrors.vcores)
                    setFieldErrors((p) => ({ ...p, vcores: "" }));
                }}
                aria-invalid={!!fieldErrors.vcores}
                className={
                  fieldErrors.vcores
                    ? "border-destructive focus-visible:ring-destructive"
                    : ""
                }
              />
              {fieldErrors.vcores && (
                <p className="text-xs text-destructive">{fieldErrors.vcores}</p>
              )}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="vm-memory">memory (mb)</Label>
              <Input
                id="vm-memory"
                type="number"
                min={128}
                max={12288}
                step={128}
                value={memoryMb}
                onChange={(e) => {
                  setMemoryMb(Number(e.target.value));
                  if (fieldErrors.memory)
                    setFieldErrors((p) => ({ ...p, memory: "" }));
                }}
                aria-invalid={!!fieldErrors.memory}
                className={
                  fieldErrors.memory
                    ? "border-destructive focus-visible:ring-destructive"
                    : ""
                }
              />
              {fieldErrors.memory && (
                <p className="text-xs text-destructive">{fieldErrors.memory}</p>
              )}
            </div>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="vm-port">exposed port</Label>
            <Input
              id="vm-port"
              type="number"
              min={1}
              max={65535}
              value={port}
              onChange={(e) => {
                setPort(Number(e.target.value));
                if (fieldErrors.port)
                  setFieldErrors((p) => ({ ...p, port: "" }));
              }}
              aria-invalid={!!fieldErrors.port}
              className={
                fieldErrors.port
                  ? "border-destructive focus-visible:ring-destructive"
                  : ""
              }
            />
            {fieldErrors.port && (
              <p className="text-xs text-destructive">{fieldErrors.port}</p>
            )}
          </div>
          {submitError && (
            <p className="text-sm text-destructive">{submitError}</p>
          )}
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
    onSuccess: () => {
      invalidate();
      toast.success(`${vm.name} starting`);
    },
    onError: (err) => toast.error(`failed to start ${vm.name}: ${err.message}`),
  });
  const stopMutation = useMutation({
    mutationFn: () => stopVm(vm.id),
    onSuccess: () => {
      invalidate();
      toast.success(`${vm.name} stopping`);
    },
    onError: (err) => toast.error(`failed to stop ${vm.name}: ${err.message}`),
  });

  const isTransitioning =
    vm.status === "starting" ||
    vm.status === "stopping" ||
    vm.status === "snapshotting";
  const canStart = vm.status === "stopped" || vm.status === "error";
  const canStop = vm.status === "running";

  return (
    <div className="border rounded-lg px-5 py-4 flex flex-row items-center justify-between w-full">
      <div className="flex flex-col">
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
        </div>
        <div className="mt-2 flex items-center justify-between">
          <div className="flex gap-4 text-xs text-muted-foreground">
            <span>{vm.vcores}c</span>
            <span>{vm.memory_mb}mb</span>
            <span>:{vm.exposed_port}</span>
          </div>
        </div>
      </div>
      <div className="flex gap-2 items-center">
        <Button
          variant="ghost"
          size="icon"
          className="rounded-full"
          disabled={
            isTransitioning || startMutation.isPending || stopMutation.isPending
          }
          onClick={(e) => {
            e.preventDefault();
            if (canStart) startMutation.mutate();
            else if (canStop) stopMutation.mutate();
          }}
        >
          {isTransitioning ||
          startMutation.isPending ||
          stopMutation.isPending ? (
            <Loader className="animate-spin" />
          ) : canStart ? (
            <Play className="h-4 w-4" />
          ) : canStop ? (
            <Pause className="h-4 w-4" />
          ) : (
            <FileQuestion className="h-4 w-4" />
          )}
        </Button>
        <span className="text-xs text-muted-foreground font-mono">
          {vm.subdomain}
        </span>
      </div>
    </div>
  );
}
