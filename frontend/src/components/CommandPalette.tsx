import { useEffect, useState, useCallback } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  Server,
  Box,
  FolderOpen,
  User,
  Key,
  Fingerprint,
  Clock,
  Hash,
} from "lucide-react";
import { listVms, startVm, stopVm, listNamespaces, type Vm } from "@/api";
import { getRecentVms, type RecentVm } from "@/hooks/useRecentVms";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import {
  Command,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandSeparator,
} from "@/components/ui/command";
import {
  IconPlayerPlayFilled,
  IconPlayerStopFilled,
} from "@tabler/icons-react";

const VM_STATUS_DOT: Record<string, string> = {
  running: "bg-green-500",
  stopped: "bg-muted-foreground/30",
  starting: "bg-yellow-400",
  stopping: "bg-yellow-400",
  error: "bg-destructive",
};

const VM_STATUS_BADGE: Record<string, string> = {
  running: "text-green-600 dark:text-green-400 bg-green-500/10",
  stopped: "text-muted-foreground bg-muted/60",
  starting: "text-yellow-600 dark:text-yellow-400 bg-yellow-400/10",
  stopping: "text-yellow-600 dark:text-yellow-400 bg-yellow-400/10",
  error: "text-destructive bg-destructive/10",
};

function StatusDot({ status }: { status: string }) {
  return (
    <span
      className={`inline-block size-1.5 rounded-full shrink-0 ${VM_STATUS_DOT[status] ?? "bg-muted-foreground/30"}`}
    />
  );
}

function StatusBadge({ status }: { status: string }) {
  return (
    <span
      className={`text-[10px] font-mono px-1.5 py-0.5 rounded shrink-0 ${VM_STATUS_BADGE[status] ?? "text-muted-foreground bg-muted/60"}`}
    >
      {status}
    </span>
  );
}

function IconBox({ children }: { children: React.ReactNode }) {
  return (
    <span className="flex size-5 items-center justify-center rounded text-muted-foreground shrink-0">
      {children}
    </span>
  );
}

type VmItemProps = {
  vm: Vm | RecentVm;
  onSelect: () => void;
  icon?: React.ReactNode;
};
function VmItem({ vm, onSelect, icon }: VmItemProps) {
  return (
    <CommandItem
      value={`vm ${vm.name} ${"subdomain" in vm ? vm.subdomain : ""} ${vm.id}`}
      onSelect={onSelect}
    >
      <IconBox>{icon ?? <Server className="size-3.5" />}</IconBox>
      <span className="flex-1 min-w-0 flex items-center gap-2">
        <StatusDot status={vm.status} />
        <span className="font-medium truncate">{vm.name}</span>
        {"subdomain" in vm && vm.subdomain && (
          <span className="text-xs text-muted-foreground font-mono truncate">
            {vm.subdomain}
          </span>
        )}
      </span>
      <StatusBadge status={vm.status} />
    </CommandItem>
  );
}

type ActionItemProps = {
  vm: Vm;
  action: "start" | "stop";
  onSelect: () => void;
};
function ActionItem({ vm, action, onSelect }: ActionItemProps) {
  return (
    <CommandItem
      value={`action ${action} ${vm.name} ${vm.id}`}
      onSelect={onSelect}
    >
      <IconBox>
        {action === "start" ? (
          <IconPlayerPlayFilled className="size-3.5" />
        ) : (
          <IconPlayerStopFilled className="size-3.5" />
        )}
      </IconBox>
      <span className="flex-1">
        {action} <span className="font-medium">{vm.name}</span>
      </span>
    </CommandItem>
  );
}

const PAGES = [
  {
    value: "vms dashboard",
    label: "vms",
    path: "/vms",
    icon: <Server className="size-3.5" />,
  },
  {
    value: "images",
    label: "images",
    path: "/images",
    icon: <Box className="size-3.5" />,
  },
  {
    value: "namespaces",
    label: "namespaces",
    path: "/namespaces",
    icon: <FolderOpen className="size-3.5" />,
  },
  {
    value: "account identity",
    label: "account",
    path: "/account/identity",
    icon: <User className="size-3.5" />,
  },
  {
    value: "ssh keys",
    label: "ssh keys",
    path: "/account/ssh-keys",
    icon: <Key className="size-3.5" />,
  },
  {
    value: "tokens",
    label: "tokens",
    path: "/account/tokens",
    icon: <Fingerprint className="size-3.5" />,
  },
] as const;

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const navigate = useNavigate();
  const qc = useQueryClient();

  const { data: vms = [] } = useQuery({
    queryKey: ["vms"],
    queryFn: () => listVms(),
    enabled: open,
  });

  const { data: namespaces = [] } = useQuery({
    queryKey: ["namespaces"],
    queryFn: listNamespaces,
    enabled: open,
  });

  const [recents, setRecents] = useState<RecentVm[]>([]);
  useEffect(() => {
    if (open) setRecents(getRecentVms());
  }, [open]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!open) setSearch("");
  }, [open]);

  const go = useCallback(
    (path: string) => {
      setOpen(false);
      navigate({ to: path } as Parameters<typeof navigate>[0]);
    },
    [navigate],
  );

  const goVm = useCallback(
    (vm: Vm | RecentVm) => {
      setOpen(false);
      navigate({ to: "/vms/$vmId", params: { vmId: vm.id } });
    },
    [navigate],
  );

  const handleStart = useCallback(
    async (vm: Vm) => {
      setOpen(false);
      try {
        await startVm(vm.id);
        qc.invalidateQueries({ queryKey: ["vms"] });
        toast.success(`starting ${vm.name}`);
      } catch {
        toast.error(`failed to start ${vm.name}`);
      }
    },
    [qc],
  );

  const handleStop = useCallback(
    async (vm: Vm) => {
      setOpen(false);
      try {
        await stopVm(vm.id);
        qc.invalidateQueries({ queryKey: ["vms"] });
        toast.success(`stopping ${vm.name}`);
      } catch {
        toast.error(`failed to stop ${vm.name}`);
      }
    },
    [qc],
  );

  const recentIds = new Set(recents.map((r) => r.id));
  const nonRecentVms = vms.filter((v) => !recentIds.has(v.id));
  const stoppedVms = vms.filter((v) => v.status === "stopped");
  const runningVms = vms.filter((v) => v.status === "running");

  const isSearching = search !== "";

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="overflow-hidden p-0 shadow-lg max-w-lg">
        <Command
          className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-muted-foreground [&_[cmdk-group]:not([hidden])_~[cmdk-group]]:pt-0 [&_[cmdk-group]]:px-2 [&_[cmdk-input-wrapper]_svg]:h-4 [&_[cmdk-input-wrapper]_svg]:w-4 [&_[cmdk-input]]:h-12 [&_[cmdk-item]]:px-2 [&_[cmdk-item]]:py-2"
          shouldFilter={isSearching || recents.length === 0}
        >
          <CommandInput
            placeholder="search vms, namespaces, pages…"
            value={search}
            onValueChange={setSearch}
          />
          <CommandList>
            <CommandEmpty>
              <span className="text-muted-foreground">
                no results for &ldquo;{search}&rdquo;
              </span>
            </CommandEmpty>

            {isSearching ? (
              // flat list — cmdk filters and sorts everything together
              <CommandGroup>
                {vms.map((vm) => (
                  <VmItem key={vm.id} vm={vm} onSelect={() => goVm(vm)} />
                ))}
                {stoppedVms.map((vm) => (
                  <ActionItem
                    key={`start-${vm.id}`}
                    vm={vm}
                    action="start"
                    onSelect={() => handleStart(vm)}
                  />
                ))}
                {runningVms.map((vm) => (
                  <ActionItem
                    key={`stop-${vm.id}`}
                    vm={vm}
                    action="stop"
                    onSelect={() => handleStop(vm)}
                  />
                ))}
                {namespaces.map((ns) => (
                  <CommandItem
                    key={ns.id}
                    value={`namespace ${ns.slug} ${ns.display_name ?? ""}`}
                    onSelect={() => go(`/namespaces/${ns.id}`)}
                  >
                    <IconBox>
                      <Hash className="size-3.5" />
                    </IconBox>
                    <span className="font-medium">
                      {ns.display_name ?? ns.slug}
                    </span>
                  </CommandItem>
                ))}
                {PAGES.map((p) => (
                  <CommandItem
                    key={p.value}
                    value={p.value}
                    onSelect={() => go(p.path)}
                  >
                    <IconBox>{p.icon}</IconBox>
                    <span>{p.label}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            ) : (
              // grouped view for empty search
              <>
                {recents.length > 0 && (
                  <CommandGroup heading="recent">
                    {recents.map((vm) => (
                      <VmItem
                        key={`recent-${vm.id}`}
                        vm={vm}
                        onSelect={() => goVm(vm)}
                        icon={<Clock className="size-3.5" />}
                      />
                    ))}
                  </CommandGroup>
                )}

                {nonRecentVms.length > 0 && (
                  <>
                    {recents.length > 0 && <CommandSeparator />}
                    <CommandGroup heading="vms">
                      {nonRecentVms.map((vm) => (
                        <VmItem key={vm.id} vm={vm} onSelect={() => goVm(vm)} />
                      ))}
                    </CommandGroup>
                  </>
                )}

                {(stoppedVms.length > 0 || runningVms.length > 0) && (
                  <>
                    <CommandSeparator />
                    <CommandGroup heading="actions">
                      {stoppedVms.map((vm) => (
                        <ActionItem
                          key={`start-${vm.id}`}
                          vm={vm}
                          action="start"
                          onSelect={() => handleStart(vm)}
                        />
                      ))}
                      {runningVms.map((vm) => (
                        <ActionItem
                          key={`stop-${vm.id}`}
                          vm={vm}
                          action="stop"
                          onSelect={() => handleStop(vm)}
                        />
                      ))}
                    </CommandGroup>
                  </>
                )}

                {namespaces.length > 0 && (
                  <>
                    <CommandSeparator />
                    <CommandGroup heading="namespaces">
                      {namespaces.map((ns) => (
                        <CommandItem
                          key={ns.id}
                          value={`namespace ${ns.slug} ${ns.display_name ?? ""}`}
                          onSelect={() => go(`/namespaces/${ns.id}`)}
                        >
                          <IconBox>
                            <Hash className="size-3.5" />
                          </IconBox>
                          <span className="font-medium">
                            {ns.display_name ?? ns.slug}
                          </span>
                        </CommandItem>
                      ))}
                    </CommandGroup>
                  </>
                )}

                <CommandSeparator />
                <CommandGroup heading="pages">
                  {PAGES.map((p) => (
                    <CommandItem
                      key={p.value}
                      value={p.value}
                      onSelect={() => go(p.path)}
                    >
                      <IconBox>{p.icon}</IconBox>
                      <span>{p.label}</span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}
          </CommandList>

          <div className="border-t px-3 py-2 flex items-center gap-4 text-[10px] text-muted-foreground select-none">
            <span className="flex items-center gap-1">
              <kbd className="font-mono bg-muted px-1 rounded">↑↓</kbd> navigate
            </span>
            <span className="flex items-center gap-1">
              <kbd className="font-mono bg-muted px-1 rounded">↵</kbd> select
            </span>
            <span className="flex items-center gap-1">
              <kbd className="font-mono bg-muted px-1 rounded">esc</kbd> close
            </span>
            <span className="ml-auto flex items-center gap-1"></span>
          </div>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
