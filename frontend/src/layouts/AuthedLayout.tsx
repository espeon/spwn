import { Outlet } from "@tanstack/react-router";
import { useVmEvents } from "@/hooks/useVmEvents";
import { useTheme } from "@/hooks/useTheme";
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { AppSidebar } from "@/components/Sidebar";

export function AuthedLayout() {
  useVmEvents();
  useTheme();

  return (
    <SidebarProvider
      style={
        {
          "--sidebar-width": "calc(var(--spacing) * 64)",
          "--header-height": "calc(var(--spacing) * 12)",
        } as React.CSSProperties
      }
    >
      <AppSidebar variant="inset" />
      <SidebarInset>
        <header className="flex h-12 items-center gap-2 border-b px-4">
          <SidebarTrigger />
        </header>

        <div className="flex flex-1 flex-col max-w-5xl mx-auto w-full">
          <div className="flex flex-col p-4 md:p-6">
            <Outlet />
          </div>
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
}
