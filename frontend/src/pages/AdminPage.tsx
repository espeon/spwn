import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  listHosts,
  listAdminVms,
  setHostStatus,
  adminMigrateVm,
  type Host,
  type AdminVm,
} from "@/api";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";

function resourcePercent(used: number, total: number): number {
  if (total === 0) return 0;
  return Math.round((used / total) * 100);
}

function ResourceBar({
  used,
  total,
  label,
  unit = "",
}: {
  used: number;
  total: number;
  label: string;
  unit?: string;
}) {
  const pct = resourcePercent(used, total);
  const color =
    pct > 85 ? "bg-red-500" : pct > 60 ? "bg-yellow-500" : "bg-green-500";
  return (
    <div className="flex flex-col gap-1">
      <div className="flex justify-between text-xs text-muted-foreground">
        <span>{label}</span>
        <span>
          {used}
          {unit} / {total}
          {unit} ({pct}%)
        </span>
      </div>
      <div className="h-1.5 w-full rounded-full bg-muted">
        <div
          className={`h-full rounded-full ${color} transition-all`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

const STATUS_CYCLE: Record<string, { next: string; label: string }> = {
  active: { next: "draining", label: "drain" },
  draining: { next: "active", label: "re-activate" },
  offline: { next: "active", label: "re-activate" },
};

const STATUS_COLOR: Record<string, string> = {
  active: "text-green-500",
  draining: "text-yellow-500",
  offline: "text-muted-foreground",
};

const VM_STATUS_COLOR: Record<string, string> = {
  running: "text-green-500",
  stopped: "text-muted-foreground",
  starting: "text-yellow-500",
  stopping: "text-yellow-500",
  error: "text-red-500",
};

function VmRow({
  vm,
  onMigrate,
}: {
  vm: AdminVm;
  onMigrate: (vm: AdminVm) => void;
}) {
  return (
    <div className="flex items-center justify-between py-2 px-3 rounded-md hover:bg-muted/50 text-sm">
      <div className="flex items-center gap-3 min-w-0">
        <span
          className={`text-xs font-medium w-16 shrink-0 ${VM_STATUS_COLOR[vm.status] ?? "text-muted-foreground"}`}
        >
          {vm.status}
        </span>
        <div className="min-w-0">
          <span className="font-medium truncate block">{vm.name}</span>
          <span className="text-xs text-muted-foreground font-mono truncate block">
            {vm.username}
          </span>
        </div>
      </div>
      <div className="flex items-center gap-4 shrink-0 text-xs text-muted-foreground">
        <span className="w-16 text-right">{(vm.vcpus / 1000).toFixed(1)}c</span>
        <span className="w-14 text-right">{vm.memory_mb}mb</span>
        <span className="w-16 text-right">{vm.disk_usage_mb}mb disk</span>
        <button
          onClick={() => onMigrate(vm)}
          className="text-xs px-2 py-0.5 rounded border hover:bg-muted transition-colors"
        >
          migrate
        </button>
      </div>
    </div>
  );
}

function HostCard({
  host,
  vms,
  onMigrateVm,
}: {
  host: Host;
  vms: AdminVm[];
  hosts: Host[];
  onMigrateVm: (vm: AdminVm) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const qc = useQueryClient();

  const { mutate: changeStatus, isPending } = useMutation({
    mutationFn: (status: string) => setHostStatus(host.id, status),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["admin", "hosts"] });
      toast.success(`host ${host.name} status updated`);
    },
    onError: (e) => toast.error(e.message),
  });

  const action = STATUS_CYCLE[host.status];
  const timeSince = Math.floor(Date.now() / 1000 - host.last_seen_at);
  const lastSeen =
    timeSince < 60
      ? `${timeSince}s ago`
      : timeSince < 3600
        ? `${Math.floor(timeSince / 60)}m ago`
        : `${Math.floor(timeSince / 3600)}h ago`;

  const provisionedVcpus = vms.reduce((s, v) => s + v.vcpus, 0);
  const provisionedMem = vms.reduce((s, v) => s + v.memory_mb, 0);
  const totalDisk = vms.reduce((s, v) => s + v.disk_usage_mb, 0);

  return (
    <div className="rounded-lg border bg-card flex flex-col">
      <div className="p-4 flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <div className="flex flex-col">
            <span className="font-medium">{host.name}</span>
            <span className="text-xs text-muted-foreground font-mono">
              {host.address}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <span
              className={`text-xs font-medium ${STATUS_COLOR[host.status] ?? "text-muted-foreground"}`}
            >
              {host.status}
            </span>
            {action && (
              <button
                onClick={() => changeStatus(action.next)}
                disabled={isPending}
                className="text-xs px-2 py-1 rounded border hover:bg-muted transition-colors disabled:opacity-50"
              >
                {action.label}
              </button>
            )}
          </div>
        </div>

        <div className="flex flex-col gap-2">
          <ResourceBar
            used={host.vcpu_used}
            total={host.vcpu_total}
            label="vcpu active"
          />
          {provisionedVcpus > host.vcpu_used && (
            <ResourceBar
              used={provisionedVcpus}
              total={host.vcpu_total}
              label="vcpu provisioned"
            />
          )}
          <ResourceBar
            used={host.mem_used_mb}
            total={host.mem_total_mb}
            label="mem active"
            unit="mb"
          />
          {provisionedMem > host.mem_used_mb && (
            <ResourceBar
              used={provisionedMem}
              total={host.mem_total_mb}
              label="mem provisioned"
              unit="mb"
            />
          )}
        </div>

        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>last seen {lastSeen}</span>
          <span>
            {vms.length} vm{vms.length !== 1 ? "s" : ""} &middot; {totalDisk}mb
            disk
          </span>
        </div>
      </div>

      {vms.length > 0 && (
        <div className="border-t">
          <button
            onClick={() => setExpanded((e) => !e)}
            className="w-full px-4 py-2 text-xs text-muted-foreground hover:text-foreground text-left transition-colors"
          >
            {expanded
              ? "▲ hide vms"
              : `▼ show ${vms.length} vm${vms.length !== 1 ? "s" : ""}`}
          </button>
          {expanded && (
            <div className="px-1 pb-2">
              {vms.map((vm) => (
                <VmRow key={vm.id} vm={vm} onMigrate={onMigrateVm} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function UnassignedVms({
  vms,
  onMigrate,
}: {
  vms: AdminVm[];
  hosts: Host[];
  onMigrate: (vm: AdminVm) => void;
}) {
  if (vms.length === 0) return null;
  return (
    <div>
      <h2 className="text-sm font-medium text-muted-foreground mb-2">
        unassigned ({vms.length})
      </h2>
      <div className="rounded-lg border bg-card/50 px-1 py-2 flex flex-col">
        {vms.map((vm) => (
          <VmRow key={vm.id} vm={vm} onMigrate={onMigrate} />
        ))}
      </div>
    </div>
  );
}

export function AdminPage() {
  const [migrateVm, setMigrateVm] = useState<AdminVm | null>(null);
  const [migrateTarget, setMigrateTarget] = useState("");
  const qc = useQueryClient();

  const {
    data: hosts,
    isLoading: hostsLoading,
    error: hostsError,
  } = useQuery({
    queryKey: ["admin", "hosts"],
    queryFn: listHosts,
    refetchInterval: 15_000,
  });

  const { data: vms, isLoading: vmsLoading } = useQuery({
    queryKey: ["admin", "vms"],
    queryFn: listAdminVms,
    refetchInterval: 15_000,
  });

  const { mutate: doMigrate, isPending: migrating } = useMutation({
    mutationFn: () => adminMigrateVm(migrateVm!.id, migrateTarget),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["admin", "vms"] });
      qc.invalidateQueries({ queryKey: ["admin", "hosts"] });
      setMigrateVm(null);
      setMigrateTarget("");
      toast.success(`migration of ${migrateVm?.name} started`);
    },
    onError: (e) => toast.error(e.message),
  });

  if (hostsLoading || vmsLoading) {
    return <div className="p-6 text-muted-foreground text-sm">loading...</div>;
  }

  if (hostsError) {
    return (
      <div className="p-6 text-destructive text-sm">
        failed to load admin data
      </div>
    );
  }

  const vmsByHost = new Map<string, AdminVm[]>();
  const unassigned: AdminVm[] = [];

  for (const vm of vms ?? []) {
    if (vm.host_id) {
      const list = vmsByHost.get(vm.host_id) ?? [];
      list.push(vm);
      vmsByHost.set(vm.host_id, list);
    } else {
      unassigned.push(vm);
    }
  }

  const totalVms = vms?.length ?? 0;
  const totalDisk = vms?.reduce((s, v) => s + v.disk_usage_mb, 0) ?? 0;
  const activeHosts = hosts?.filter((h) => h.status === "active").length ?? 0;

  return (
    <>
      <Dialog
        open={!!migrateVm}
        onOpenChange={(open) => {
          if (!open) {
            setMigrateVm(null);
            setMigrateTarget("");
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>migrate {migrateVm?.name}</DialogTitle>
            <DialogDescription>
              move this vm to a different host. it will be stopped, migrated,
              and left in stopped state on the target host.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2 py-2">
            <Label htmlFor="migrate-target">target host</Label>
            <select
              id="migrate-target"
              value={migrateTarget}
              onChange={(e) => setMigrateTarget(e.target.value)}
              className="w-full rounded-md border bg-background px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-ring"
            >
              <option value="">select host...</option>
              {hosts
                ?.filter((h) => h.id !== migrateVm?.host_id)
                .map((h) => (
                  <option key={h.id} value={h.id}>
                    {h.name} ({h.status})
                  </option>
                ))}
            </select>
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => {
                setMigrateVm(null);
                setMigrateTarget("");
              }}
              disabled={migrating}
            >
              cancel
            </Button>
            <Button
              onClick={() => doMigrate()}
              disabled={!migrateTarget || migrating}
            >
              {migrating ? "migrating..." : "migrate"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="p-6 flex flex-col gap-6">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="text-xl font-semibold">admin</h1>
            <p className="text-sm text-muted-foreground mt-1">
              host cluster overview
            </p>
          </div>
          <div className="flex gap-6 text-right">
            <div>
              <p className="text-sm font-medium">
                {activeHosts}/{hosts?.length ?? 0}
              </p>
              <p className="text-xs text-muted-foreground">hosts active</p>
            </div>
            <div>
              <p className="text-sm font-medium">{totalVms}</p>
              <p className="text-xs text-muted-foreground">total vms</p>
            </div>
            <div>
              <p className="text-sm font-medium">
                {(totalDisk / 1024).toFixed(1)}gb
              </p>
              <p className="text-xs text-muted-foreground">disk used</p>
            </div>
          </div>
        </div>

        {hosts && hosts.length === 0 && (
          <p className="text-sm text-muted-foreground">no hosts registered</p>
        )}

        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
          {hosts?.map((h) => (
            <HostCard
              key={h.id}
              host={h}
              vms={vmsByHost.get(h.id) ?? []}
              hosts={hosts}
              onMigrateVm={setMigrateVm}
            />
          ))}
        </div>

        <UnassignedVms
          vms={unassigned}
          hosts={hosts ?? []}
          onMigrate={setMigrateVm}
        />
      </div>
    </>
  );
}
