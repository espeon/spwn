import { useCallback, useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { listNamespaces, type Namespace } from "@/api";

const STORAGE_KEY = "spwn:active_namespace_id";

export function useActiveNamespace(): {
  namespaces: Namespace[];
  activeNamespace: Namespace | null;
  setActiveNamespaceId: (id: string) => void;
  isLoading: boolean;
} {
  const { data: namespaces = [], isLoading } = useQuery({
    queryKey: ["namespaces"],
    queryFn: listNamespaces,
  });

  const [activeId, setActiveId] = useState<string | null>(() =>
    localStorage.getItem(STORAGE_KEY)
  );

  const setActiveNamespaceId = useCallback((id: string) => {
    localStorage.setItem(STORAGE_KEY, id);
    setActiveId(id);
  }, []);

  useEffect(() => {
    if (!namespaces.length) return;
    const found = namespaces.find((n) => n.id === activeId);
    if (!found) {
      const personal = namespaces.find((n) => n.kind === "personal");
      if (personal) {
        setActiveNamespaceId(personal.id);
      }
    }
  }, [namespaces, activeId, setActiveNamespaceId]);

  const activeNamespace =
    namespaces.find((n) => n.id === activeId) ??
    namespaces.find((n) => n.kind === "personal") ??
    null;

  return { namespaces, activeNamespace, setActiveNamespaceId, isLoading };
}
