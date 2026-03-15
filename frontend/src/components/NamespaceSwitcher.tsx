import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import {
  IconBuilding,
  IconUser,
  IconCheck,
  IconChevronDown,
  IconPlus,
  IconSettings,
} from "@tabler/icons-react";
import { useActiveNamespace } from "@/hooks/useActiveNamespace";
import { createNamespace, ApiError } from "@/api";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Namespace } from "@/api";

function NamespaceIcon({ ns }: { ns: Namespace }) {
  return ns.kind === "personal" ? (
    <IconUser className="size-4 shrink-0" />
  ) : (
    <IconBuilding className="size-4 shrink-0" />
  );
}

function NewOrgDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const qc = useQueryClient();
  const [slug, setSlug] = useState("");
  const [displayName, setDisplayName] = useState("");
  const mutation = useMutation({
    mutationFn: () => createNamespace(slug, displayName || undefined),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["namespaces"] });
      toast.success("namespace created");
      onOpenChange(false);
      setSlug("");
      setDisplayName("");
    },
    onError: (e) => {
      toast.error(
        e instanceof ApiError ? e.message : "failed to create namespace",
      );
    },
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>new namespace</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4 pt-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="sw-slug">slug</Label>
            <Input
              id="sw-slug"
              placeholder="my-org"
              value={slug}
              onChange={(e) => setSlug(e.target.value.toLowerCase())}
            />
            <p className="text-xs text-muted-foreground">
              lowercase alphanumeric and hyphens · globally unique
            </p>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="sw-display">display name</Label>
            <Input
              id="sw-display"
              placeholder="My Org (optional)"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
            />
          </div>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!slug || mutation.isPending}
            className="self-end"
          >
            create
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function NamespaceSwitcher() {
  const { namespaces, activeNamespace, setActiveNamespaceId } =
    useActiveNamespace();
  const navigate = useNavigate();
  const [newOrgOpen, setNewOrgOpen] = useState(false);

  if (!activeNamespace) return null;

  const personal = namespaces.find((n) => n.kind === "personal");
  const orgs = namespaces.filter((n) => n.kind !== "personal");

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-sm hover:bg-sidebar-accent transition-colors">
            <NamespaceIcon ns={activeNamespace} />
            <span className="flex-1 text-left font-medium truncate">
              {activeNamespace.display_name ?? activeNamespace.slug}
            </span>
            <IconChevronDown className="size-3.5 text-muted-foreground shrink-0" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-56">
          {personal && (
            <DropdownMenuItem
              onClick={() => setActiveNamespaceId(personal.id)}
              className="flex items-center gap-2"
            >
              <IconUser className="size-4 shrink-0 text-muted-foreground" />
              <span className="flex-1 truncate">
                {personal.display_name ?? personal.slug}
              </span>
              {activeNamespace.id === personal.id && (
                <IconCheck className="size-3.5 shrink-0" />
              )}
            </DropdownMenuItem>
          )}
          {orgs.length > 0 && (
            <>
              <DropdownMenuSeparator />
              {orgs.map((ns) => (
                <DropdownMenuItem
                  key={ns.id}
                  onClick={() => setActiveNamespaceId(ns.id)}
                  className="flex items-center gap-2"
                >
                  <IconBuilding className="size-4 shrink-0 text-muted-foreground" />
                  <span className="flex-1 truncate">
                    {ns.display_name ?? ns.slug}
                  </span>
                  {activeNamespace.id === ns.id && (
                    <IconCheck className="size-3.5 shrink-0" />
                  )}
                </DropdownMenuItem>
              ))}
            </>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem
            onClick={() =>
              navigate({
                to: "/namespaces/$nsId",
                params: { nsId: activeNamespace.id },
              })
            }
            className="flex items-center gap-2"
          >
            <IconSettings className="size-4 shrink-0 text-muted-foreground" />
            <span>manage namespace</span>
          </DropdownMenuItem>
          <DropdownMenuItem
            onClick={() => setNewOrgOpen(true)}
            className="flex items-center gap-2"
          >
            <IconPlus className="size-4 shrink-0 text-muted-foreground" />
            <span>new org</span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <NewOrgDialog open={newOrgOpen} onOpenChange={setNewOrgOpen} />
    </>
  );
}
