import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { Vm } from "@/api";

interface TrackedToast {
  toastId: string | number;
  successStatus: string;
  successMsg: string;
  errorMsg: string;
}

const trackedToasts = new Map<string, TrackedToast>();

export function trackVmToast(
  vmId: string,
  toastId: string | number,
  successStatus: string,
  successMsg: string,
  errorMsg: string,
) {
  trackedToasts.set(vmId, { toastId, successStatus, successMsg, errorMsg });
}

export function useVmEvents(): { connected: boolean } {
  const qc = useQueryClient();
  const trackedRef = useRef(trackedToasts);
  const [connected, setConnected] = useState(false);
  const everConnectedRef = useRef(false);

  useEffect(() => {
    const es = new EventSource("/api/events");

    es.addEventListener("open", () => {
      if (everConnectedRef.current) {
        toast.success("reconnected", { duration: 2000 });
      }
      everConnectedRef.current = true;
      setConnected(true);
    });
    es.addEventListener("error", () => setConnected(false));

    es.addEventListener("vm_status", (e: MessageEvent) => {
      const { vm_id, status, last_started_at } = JSON.parse(e.data) as {
        vm_id: string;
        status: string;
        last_started_at?: number | null;
      };

      const patch: Partial<Vm> = { status: status as Vm["status"] };
      if (last_started_at !== undefined) patch.last_started_at = last_started_at;

      qc.setQueryData<Vm>(["vms", vm_id], (old) =>
        old ? { ...old, ...patch } : old,
      );
      qc.setQueriesData<Vm[]>({ queryKey: ["vms"] }, (old) => {
        if (!Array.isArray(old)) return old;
        return old.map((vm) =>
          vm.id === vm_id ? { ...vm, ...patch } : vm,
        );
      });

      qc.setQueriesData<{ id: string; status: string }[]>(
        { queryKey: ["admin", "vms"] },
        (old) => {
          if (!Array.isArray(old)) return old;
          return old.map((vm) => (vm.id === vm_id ? { ...vm, status } : vm));
        },
      );

      qc.invalidateQueries({ queryKey: ["namespace-usage"] });

      const tracked = trackedRef.current.get(vm_id);
      if (!tracked) return;

      if (status === tracked.successStatus) {
        toast.success(tracked.successMsg, { id: tracked.toastId });
        trackedRef.current.delete(vm_id);
      } else if (status === "error") {
        toast.error(tracked.errorMsg, { id: tracked.toastId });
        trackedRef.current.delete(vm_id);
      }
    });

    es.addEventListener("snapshot_complete", (e: MessageEvent) => {
      const { vm_id } = JSON.parse(e.data) as { vm_id: string; snap_id: string };
      qc.invalidateQueries({ queryKey: ["snapshots", vm_id] });
    });

    return () => es.close();
  }, [qc]);

  return { connected };
}
