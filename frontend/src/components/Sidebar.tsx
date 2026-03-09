import { Server } from "lucide-react";
import {
  Sidebar,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuItem,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
} from "./ui/sidebar";
import { IconInnerShadowTop } from "@tabler/icons-react";
import { Link } from "@tanstack/react-router";
import { NavUser } from "@/components/NavUser";

const navItems = [{ to: "/vms", icon: Server, label: "vms" }] as const;

const navLinkClass =
  "flex items-center gap-2 text-sidebar-foreground/70 hover:text-sidebar-foreground hover:bg-sidebar-accent data-[status=active]:bg-sidebar-accent data-[status=active]:text-sidebar-foreground duration-200 rounded-md px-2 py-2";

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  return (
    <Sidebar collapsible="offcanvas" {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <Link to="/" className={`${navLinkClass} -mt-1`}>
              <IconInnerShadowTop className="size-5!" />
              <span className="text-base font-semibold">spwn</span>
            </Link>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent className="flex flex-col gap-2">
            {navItems.map(({ to, icon: Icon, label }) => (
              <SidebarMenuItem key={to}>
                <Link to={to} className={navLinkClass}>
                  <Icon className="size-5!" />
                  <span className="text-sm font-medium">{label}</span>
                </Link>
              </SidebarMenuItem>
            ))}
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <NavUser />
      </SidebarFooter>
    </Sidebar>
  );
}
