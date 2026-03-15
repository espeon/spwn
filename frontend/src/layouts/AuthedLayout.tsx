import { Outlet, Link, useRouterState } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useState } from "react";
import { useVmEvents } from "@/hooks/useVmEvents";
import { useTheme } from "@/hooks/useTheme";
import { useKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { AppSidebar } from "@/components/Sidebar";
import { CommandPalette } from "@/components/CommandPalette";
import { Search } from "lucide-react";
import { getVm, getNamespace } from "@/api";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const PATH_LABELS: Record<string, string> = {
  vms: "vms",
  images: "images",
  namespaces: "namespaces",
  account: "account",
  identity: "identity",
  "ssh-keys": "ssh keys",
  tokens: "tokens",
  themes: "themes",
  admin: "admin",
};

const PATH_TITLES: Record<string, string> = {
  "/vms": "vms — spwn",
  "/images": "images — spwn",
  "/namespaces": "namespaces — spwn",
  "/account/identity": "account — spwn",
  "/account/ssh-keys": "ssh keys — spwn",
  "/account/tokens": "tokens — spwn",
  "/account/themes": "themes — spwn",
  "/admin": "admin — spwn",
};

function VmCrumb({ id }: { id: string }) {
  const { data: vm } = useQuery({
    queryKey: ["vms", id],
    queryFn: () => getVm(id),
    staleTime: 30_000,
  });
  return <>{vm?.name ?? id.slice(0, 8) + "…"}</>;
}

function NamespaceCrumb({ id }: { id: string }) {
  const { data: ns } = useQuery({
    queryKey: ["namespace", id],
    queryFn: () => getNamespace(id),
    staleTime: 30_000,
  });
  return <>{ns ? (ns.display_name ?? ns.slug) : id.slice(0, 8) + "…"}</>;
}

function CrumbLabel({
  seg,
  parent,
}: {
  seg: string;
  parent: string | undefined;
}) {
  if (parent === "vms") return <VmCrumb id={seg} />;
  if (parent === "namespaces") return <NamespaceCrumb id={seg} />;
  return <>{PATH_LABELS[seg] ?? seg}</>;
}

function Breadcrumbs() {
  const { location } = useRouterState();
  const segments = location.pathname.split("/").filter(Boolean);
  if (segments.length === 0) return null;

  return (
    <nav className="flex items-center gap-1 text-xs text-muted-foreground">
      {segments.map((seg, i) => {
        const path = "/" + segments.slice(0, i + 1).join("/");
        const isLast = i === segments.length - 1;
        const parent = segments[i - 1];
        return (
          <span key={path} className="flex items-center gap-1">
            {i > 0 && <span className="opacity-40">/</span>}
            {isLast ? (
              <span className="text-foreground">
                <CrumbLabel seg={seg} parent={parent} />
              </span>
            ) : (
              <Link
                to={path as "/"}
                className="hover:text-foreground transition-colors"
              >
                <CrumbLabel seg={seg} parent={parent} />
              </Link>
            )}
          </span>
        );
      })}
    </nav>
  );
}

const SHORTCUTS = [
  { keys: ["g", "v"], desc: "go to vms" },
  { keys: ["g", "i"], desc: "go to images" },
  { keys: ["g", "n"], desc: "go to namespaces" },
  { keys: ["g", "a"], desc: "go to admin" },
  { keys: ["g", "s"], desc: "go to account" },
  { keys: ["⌘", "k"], desc: "open command palette" },
  { keys: ["?"], desc: "show keyboard shortcuts" },
];

export function AuthedLayout() {
  const { connected } = useVmEvents();
  useTheme();
  const [helpOpen, setHelpOpen] = useState(false);
  const openHelp = useCallback(() => setHelpOpen(true), []);
  useKeyboardShortcuts(openHelp);
  const { location } = useRouterState();

  useEffect(() => {
    const title = PATH_TITLES[location.pathname] ?? "spwn";
    document.title = title;
  }, [location.pathname]);

  return (
    <>
    <Dialog open={helpOpen} onOpenChange={setHelpOpen}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>keyboard shortcuts</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-1 pt-1">
          {SHORTCUTS.map(({ keys, desc }) => (
            <div key={desc} className="flex items-center justify-between py-1.5 text-sm">
              <span className="text-muted-foreground">{desc}</span>
              <span className="flex items-center gap-1">
                {keys.map((k) => (
                  <kbd key={k} className="font-mono text-xs bg-muted px-1.5 py-0.5 rounded border">
                    {k}
                  </kbd>
                ))}
              </span>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
    <SidebarProvider
      style={
        {
          "--sidebar-width": "calc(var(--spacing) * 64)",
          "--header-height": "calc(var(--spacing) * 12)",
        } as React.CSSProperties
      }
    >
      <CommandPalette />
      <AppSidebar variant="inset" />
      <SidebarInset>
        <header className="flex h-12 items-center gap-2 border-b px-4">
          <SidebarTrigger />
          <Breadcrumbs />
          <div className="flex-1" />
          <span
            title={connected ? "live updates connected" : "connecting…"}
            className={`size-2 rounded-full shrink-0 transition-colors ${connected ? "bg-green-500" : "bg-muted-foreground/30"}`}
          />
          <button
            onClick={() => {
              window.dispatchEvent(
                new KeyboardEvent("keydown", { key: "k", metaKey: true }),
              );
            }}
            className="hidden sm:flex items-center gap-1.5 text-xs text-muted-foreground border rounded-md px-2 py-1 hover:bg-muted transition-colors"
          >
            <Search className="w-3.5 h-3.5" />
            <kbd className="font-mono text-[10px] bg-muted px-1 rounded">
              ⌘K
            </kbd>
          </button>
        </header>

        <div className="flex flex-1 flex-col max-w-5xl mx-auto w-full">
          <div className="flex flex-col p-4 md:p-6">
            <Outlet />
          </div>
        </div>
      </SidebarInset>
    </SidebarProvider>
    </>
  );
}
