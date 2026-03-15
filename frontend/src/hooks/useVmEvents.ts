import { useEffect, useRef } from "react";
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

export function useVmEvents() {
  const qc = useQueryClient();
  const trackedRef = useRef(trackedToasts);

  useEffect(() => {
    const es = new EventSource("/api/events");

    es.addEventListener("vm_status", (e: MessageEvent) => {
      const { vm_id, status } = JSON.parse(e.data) as {
        vm_id: string;
        status: string;
      };

      qc.setQueryData<Vm>(["vms", vm_id], (old) =>
        old ? { ...old, status: status as Vm["status"] } : old,
      );
      qc.setQueriesData<Vm[]>({ queryKey: ["vms"] }, (old) => {
        if (!Array.isArray(old)) return old;
        return old.map((vm) =>
          vm.id === vm_id ? { ...vm, status: status as Vm["status"] } : vm,
        );
      });

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

    return () => es.close();
  }, [qc]);
}
