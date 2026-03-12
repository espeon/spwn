import { Server, ShieldCheck, Box } from "lucide-react";
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
import { IconGalaxy } from "@tabler/icons-react";
import { Link } from "@tanstack/react-router";
import { NavUser } from "@/components/NavUser";
import { useQuery } from "@tanstack/react-query";
import { getMe } from "@/api";

const navLinkClass =
  "flex items-center gap-2 text-sidebar-foreground/70 hover:text-sidebar-foreground hover:bg-sidebar-accent data-[status=active]:bg-sidebar-accent data-[status=active]:text-sidebar-foreground duration-200 rounded-md px-2 py-2";

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  const { data: me } = useQuery({ queryKey: ["me"], queryFn: getMe });

  return (
    <Sidebar collapsible="offcanvas" {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem className="marker:invisible">
            <Link
              to="/"
              className={`-mt-1 flex items-center gap-2 px-2 py-2 hover:text-sidebar-foreground hover:bg-sidebar-accent data-[status=active]:bg-sidebar-accent data-[status=active]:text-sidebar-foreground duration-200 rounded-md`}
            >
              <IconGalaxy className="size-5!" />
              <span className="text-lg font-semibold -mt-1">spwn</span>
            </Link>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent className="flex flex-col gap-2">
            <SidebarMenuItem className="text-white/0">
              <Link to="/vms" className={navLinkClass}>
                <Server className="size-5!" />
                <span className="text-sm font-medium">vms</span>
              </Link>
            </SidebarMenuItem>
            <SidebarMenuItem className="text-white/0">
              <Link to="/images" className={navLinkClass}>
                <Box className="size-5!" />
                <span className="text-sm font-medium">images</span>
              </Link>
            </SidebarMenuItem>
            {me?.role === "superadmin" && (
              <SidebarMenuItem className="text-white/0">
                <Link to="/admin" className={navLinkClass}>
                  <ShieldCheck className="size-5!" />
                  <span className="text-sm font-medium">admin</span>
                </Link>
              </SidebarMenuItem>
            )}
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <NavUser />
      </SidebarFooter>
    </Sidebar>
  );
}
