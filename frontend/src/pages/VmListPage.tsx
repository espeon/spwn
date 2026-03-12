import { useState, useEffect, type FormEvent } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { trackVmToast } from "@/hooks/useVmEvents";
import {
  listVms,
  listImages,
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
import { FileQuestion, MemoryStick, Pause, Play } from "lucide-react";
import { Loader } from "@/components/ui/loader";
import { IconAccessPoint, IconCpu } from "@tabler/icons-react";
import { formatDataSize } from "@/lib/utils";

function CreateVmDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [image, setImage] = useState("");
  const [vcpus, setVcpus] = useState(1.0);
  const [memoryMb, setMemoryMb] = useState(512);
  const [port, setPort] = useState(8080);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [submitError, setSubmitError] = useState<string | null>(null);

  const { data: images = [] } = useQuery({
    queryKey: ["images"],
    queryFn: listImages,
    staleTime: 60_000,
  });

  useEffect(() => {
    if (images.length > 0 && !image) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setImage(`${images[0].name}:${images[0].tag}`);
    }
  }, [images, image]);

  function initAllDefault() {
    setName("");
    setImage(images.length > 0 ? `${images[0].name}:${images[0].tag}` : "");
    setVcpus(1.0);
    setMemoryMb(512);
    setPort(8080);
    setFieldErrors({});
    setSubmitError(null);
  }
  useEffect(() => {
    if (!open) {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      initAllDefault();
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
    if (!image.trim()) {
      errs.image = "select an image";
    }
    if (vcpus < 0.125 || vcpus > 8) {
      errs.vcpus = "must be between 0.125 and 8";
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
      image: image.trim(),
      vcpus: vcpus * 1000, // convert to millicpus
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
          <div className="space-y-1.5">
            <Label htmlFor="vm-image">image</Label>
            {images.length === 0 ? (
              <p className="text-xs text-muted-foreground py-1">
                no images available —{" "}
                <a href="/images" className="underline underline-offset-2">
                  visit the image catalogue
                </a>
              </p>
            ) : (
              <div
                className={`flex flex-wrap gap-1.5 ${fieldErrors.image ? "rounded-md outline outline-1 outline-destructive p-1" : ""}`}
              >
                {images.map((img) => {
                  const val = `${img.name}:${img.tag}`;
                  return (
                    <button
                      key={img.id}
                      type="button"
                      onClick={() => {
                        setImage(val);
                        if (fieldErrors.image)
                          setFieldErrors((p) => ({ ...p, image: "" }));
                      }}
                      className={`px-2.5 py-1 rounded-md text-xs font-mono border transition-colors ${
                        image === val
                          ? "bg-primary text-primary-foreground border-primary"
                          : "bg-background hover:bg-muted border-input"
                      }`}
                    >
                      {img.name}
                      <span className="opacity-60">:{img.tag}</span>
                    </button>
                  );
                })}
              </div>
            )}
            {fieldErrors.image && (
              <p className="text-xs text-destructive">{fieldErrors.image}</p>
            )}
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>vcpus</Label>
              <div
                className={`flex rounded-md border ${fieldErrors.vcpus ? "border-destructive" : "border-input"}`}
              >
                {[0.125, 0.25, 0.5, 1, 1.5, 2, 3, 4, 5, 6, 7, 8].map(
                  (v, i, arr) => (
                    <button
                      key={v}
                      type="button"
                      onClick={() => {
                        setVcpus(v);
                        if (fieldErrors.vcpus)
                          setFieldErrors((p) => ({ ...p, vcpus: "" }));
                      }}
                      className={`flex-1 py-2 text-xs font-medium transition-colors
                      ${i === 0 ? "rounded-l-md" : ""}
                      ${i === arr.length - 1 ? "rounded-r-md" : "border-r border-input"}
                      ${vcpus === v ? "bg-primary text-primary-foreground" : "bg-background text-foreground hover:bg-muted"}`}
                    >
                      {v}
                    </button>
                  ),
                )}
              </div>
              {fieldErrors.vcpus && (
                <p className="text-xs text-destructive">{fieldErrors.vcpus}</p>
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
  const [localPending, setLocalPending] = useState(false);

  const invalidate = () => qc.invalidateQueries({ queryKey: ["vms"] });

  const startMutation = useMutation({
    mutationFn: () => startVm(vm.id),
    onSuccess: () => {
      invalidate();
      const toastId = toast.loading(`starting ${vm.name}...`);
      trackVmToast(
        vm.id,
        toastId,
        "running",
        `${vm.name} started`,
        `${vm.name} failed to start`,
      );
    },
    onError: (err) => {
      setLocalPending(false);
      toast.error(`failed to start ${vm.name}: ${err.message}`);
    },
  });
  const stopMutation = useMutation({
    mutationFn: () => stopVm(vm.id),
    onSuccess: () => {
      invalidate();
      const toastId = toast.loading(`stopping ${vm.name}...`);
      trackVmToast(
        vm.id,
        toastId,
        "stopped",
        `${vm.name} stopped`,
        `${vm.name} failed to stop`,
      );
    },
    onError: (err) => {
      setLocalPending(false);
      toast.error(`failed to stop ${vm.name}: ${err.message}`);
    },
  });

  const isTransitioning =
    vm.status === "starting" ||
    vm.status === "stopping" ||
    vm.status === "snapshotting";
  const canStart = vm.status === "stopped" || vm.status === "error";
  const canStop = vm.status === "running";

  // once the SSE-driven status reflects a transitioning state, the local
  // optimistic flag is no longer needed
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    if (isTransitioning) setLocalPending(false);
  }, [isTransitioning]);

  const showSpinner =
    localPending ||
    isTransitioning ||
    startMutation.isPending ||
    stopMutation.isPending;

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
            <span className="flex items-center gap-1">
              <IconCpu size={16} />
              {vm.vcpus / 1000} vCPU{vm.vcpus / 1000 > 1 && "s"}
            </span>
            <span className="flex items-center gap-1">
              <MemoryStick size={16} />
              {formatDataSize(vm.memory_mb * 1024 * 1024)}
            </span>
            <span className="flex items-center gap-1">
              <IconAccessPoint size={16} />
              Port {vm.exposed_port}
            </span>
          </div>
        </div>
      </div>
      <div className="flex gap-2 items-center">
        <Button
          variant="ghost"
          size="icon"
          className={`rounded-full group opacity-50 transition-all hover:opacity-100 data-[state=open]:bg-transparent ${
            showSpinner ? "cursor-not-allowed" : ""
          } ${vm.status === "error" ? "text-destructive-foreground" : ""} ${vm.status === "running" && "hover:text-destructive"}`}
          disabled={showSpinner}
          onClick={(e) => {
            e.preventDefault();
            if (canStart) {
              setLocalPending(true);
              startMutation.mutate();
            } else if (canStop) {
              setLocalPending(true);
              stopMutation.mutate();
            }
          }}
        >
          {showSpinner ? (
            <Loader size={16} />
          ) : canStart ? (
            <Play className="h-4 w-4 fill-accent-foreground" />
          ) : canStop ? (
            <Pause
              className={`h-4 w-4 ${vm.status === "running" ? "group-hover:fill-destructive fill-accent-foreground" : "fill-accent-foreground"}`}
            />
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
