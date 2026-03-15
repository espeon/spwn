import { useState, useEffect, useMemo } from "react";
import { Link } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { trackVmToast } from "@/hooks/useVmEvents";
import {
  listVms,
  listImages,
  listRegions,
  createVm,
  startVm,
  stopVm,
  type CreateVmRequest,
  type RegionInfo,
  type Vm,
} from "@/api";
import { useActiveNamespace } from "@/hooks/useActiveNamespace";
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
import { Copy, FileQuestion, MemoryStick, Pause, Play } from "lucide-react";
import { Loader } from "@/components/ui/loader";
import { Skeleton } from "@/components/ui/skeleton";
import { IconAccessPoint, IconCpu } from "@tabler/icons-react";
import { copyToClipboard, formatDataSize, timeAgo } from "@/lib/utils";

function CreateVmDialog({
  open,
  onClose,
  namespaceId,
}: {
  open: boolean;
  onClose: () => void;
  namespaceId?: string;
}) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [image, setImage] = useState("");
  const [vcpus, setVcpus] = useState(1.0);
  const [memoryMb, setMemoryMb] = useState(512);
  const [region, setRegion] = useState<string>("");
  const [port, setPort] = useState(8080);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [submitError, setSubmitError] = useState<string | null>(null);

  const { data: images = [] } = useQuery({
    queryKey: ["images"],
    queryFn: listImages,
    staleTime: 60_000,
  });

  const { data: regions = [] } = useQuery<RegionInfo[]>({
    queryKey: ["regions"],
    queryFn: listRegions,
    staleTime: 60_000,
  });

  const activeRegions = regions.filter((r) => r.active);
  const defaultImage =
    images.length > 0 ? `${images[0].name}:${images[0].tag}` : "";
  const defaultRegion = activeRegions.length > 0 ? activeRegions[0].name : "";
  const selectedImage = image || defaultImage;
  const selectedRegion = region || defaultRegion;

  useEffect(() => {
    const initAllDefault = () => {
      setName("");
      setImage("");
      setVcpus(1.0);
      setMemoryMb(512);
      setPort(8080);
      setRegion("");
      setFieldErrors({});
      setSubmitError(null);
    };
    if (!open) {
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
    if (!selectedImage.trim()) {
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

  function submit(e: React.SubmitEvent<HTMLFormElement>) {
    e.preventDefault();
    setSubmitError(null);
    const errs = validate();
    if (Object.keys(errs).length > 0) {
      setFieldErrors(errs);
      return;
    }
    setFieldErrors({});
    const req: CreateVmRequest = {
      name: name.trim(),
      image: selectedImage.trim(),
      vcpus: vcpus * 1000,
      memory_mb: memoryMb,
      exposed_port: port,
      ...(selectedRegion ? { region: selectedRegion } : {}),
      ...(namespaceId ? { namespace_id: namespaceId } : {}),
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
                className={`flex flex-wrap gap-1.5 ${fieldErrors.image ? "rounded-md outline outline-destructive p-1" : ""}`}
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
                        selectedImage === val
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
          {activeRegions.length > 0 && (
            <div>
              <Label htmlFor="vm-region">region</Label>
              {activeRegions.length === 1 ? (
                <p className="text-sm py-1 text-muted-foreground font-mono">
                  {activeRegions[0].name}
                </p>
              ) : (
                <select
                  id="vm-region"
                  value={selectedRegion}
                  onChange={(e) => {
                    setRegion(e.target.value);
                    if (fieldErrors.region)
                      setFieldErrors((p) => ({ ...p, region: "" }));
                  }}
                  className={`w-full rounded-md border px-3 py-2 ${
                    fieldErrors.region
                      ? "border-destructive focus-visible:ring-destructive"
                      : "border-input"
                  }`}
                >
                  {activeRegions.map((r) => (
                    <option key={r.name} value={r.name}>
                      {r.name}
                    </option>
                  ))}
                </select>
              )}
              {fieldErrors.region && (
                <p className="text-xs text-destructive">{fieldErrors.region}</p>
              )}
            </div>
          )}
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
  const [search, setSearch] = useState("");
  const { activeNamespace } = useActiveNamespace();
  const {
    data: vms,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["vms", activeNamespace?.id],
    queryFn: () => listVms(activeNamespace?.id),
    refetchInterval: 10_000,
  });

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key !== "n" || e.metaKey || e.ctrlKey) return;
      const target = e.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.tagName === "SELECT" ||
        target.isContentEditable
      ) return;
      setShowCreate(true);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const filtered = vms?.filter((vm) =>
    vm.name.toLowerCase().includes(search.toLowerCase()),
  );

  if (isLoading)
    return (
      <div className="flex flex-col gap-2">
        {Array.from({ length: 3 }).map((_, i) => (
          <Skeleton key={i} className="h-[74px] w-full rounded-lg" />
        ))}
      </div>
    );
  if (error)
    return <p className="text-destructive text-sm">failed to load vms</p>;

  return (
    <>
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-baseline gap-2">
          <h1 className="text-xl font-semibold">virtual machines</h1>
          {vms && vms.length > 0 && (
            <span className="text-sm text-muted-foreground">{vms.length}</span>
          )}
        </div>
        <Button size="sm" onClick={() => setShowCreate(true)}>
          new vm
          <kbd className="ml-1.5 hidden sm:inline-flex h-5 items-center rounded border border-primary-foreground/30 px-1 font-mono text-[10px] opacity-60">n</kbd>
        </Button>
      </div>

      {vms && vms.length > 0 && (
        <Input
          placeholder="filter..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="mb-4 h-8 text-sm"
        />
      )}

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
      ) : filtered && filtered.length === 0 ? (
        <p className="text-sm text-muted-foreground py-8 text-center">no results for "{search}"</p>
      ) : (
        <div className="flex flex-col gap-2">
          {filtered?.map((vm) => (
            <VmRow key={vm.id} vm={vm} />
          ))}
        </div>
      )}

      <CreateVmDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        namespaceId={activeNamespace?.id}
      />
    </>
  );
}

function VmRow({ vm }: { vm: Vm }) {
  const qc = useQueryClient();
  const [localPending, setLocalPending] = useState(false);
  const [copied, setCopied] = useState(false);
  const age = useMemo(() => timeAgo(vm.created_at), [vm.created_at]);

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
            {vm.region && (
              <span className="font-mono bg-secondary px-1.5 py-0.5 rounded">
                {vm.region}
              </span>
            )}
          </div>
        </div>
      </div>
      <div className="flex gap-2 items-center">
        <span className="text-xs text-muted-foreground">{age}</span>
        <button
          className="group flex items-center gap-1 text-xs text-muted-foreground font-mono hover:text-foreground transition-colors"
          onClick={async (e) => {
            e.preventDefault();
            const ok = await copyToClipboard(vm.subdomain);
            if (ok) {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            }
          }}
          title="copy subdomain"
        >
          {vm.subdomain}
          <Copy className={`h-3 w-3 shrink-0 transition-opacity ${copied ? "opacity-100 text-green-500" : "opacity-0 group-hover:opacity-60"}`} />
        </button>
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
      </div>
    </div>
  );
}
