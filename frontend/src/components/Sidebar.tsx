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
import { NamespaceSwitcher } from "@/components/NamespaceSwitcher";
import { useQuery } from "@tanstack/react-query";
import { getMe, listVms } from "@/api";

const navLinkClass =
  "flex items-center gap-2 text-sidebar-foreground/70 hover:text-sidebar-foreground hover:bg-sidebar-accent data-[status=active]:bg-sidebar-accent data-[status=active]:text-sidebar-foreground duration-200 rounded-md px-2 py-2";

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  const { data: me } = useQuery({ queryKey: ["me"], queryFn: getMe });
  const { data: vms = [] } = useQuery({
    queryKey: ["vms"],
    queryFn: () => listVms(),
    staleTime: 30_000,
  });
  const runningCount = vms.filter((v) => v.status === "running").length;

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
        <NamespaceSwitcher />
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent className="flex flex-col gap-2">
            <SidebarMenuItem className="text-white/0">
              <Link to="/vms" className={navLinkClass}>
                <Server className="size-5!" />
                <span className="text-sm font-medium">vms</span>
                <span className="ml-auto flex items-center gap-1.5">
                  {runningCount > 0 && (
                    <span className="text-[10px] font-mono bg-primary/15 text-primary rounded px-1.5 py-0.5 leading-none">
                      {runningCount}
                    </span>
                  )}
                  <span className="text-[10px] font-mono text-muted-foreground/40 leading-none">
                    gv
                  </span>
                </span>
              </Link>
            </SidebarMenuItem>
            <SidebarMenuItem className="text-white/0">
              <Link to="/images" className={navLinkClass}>
                <Box className="size-5!" />
                <span className="text-sm font-medium">images</span>
                <span className="ml-auto text-[10px] font-mono text-muted-foreground/40 leading-none">
                  gi
                </span>
              </Link>
            </SidebarMenuItem>
            {me?.role === "superadmin" && (
              <SidebarMenuItem className="text-white/0">
                <Link to="/admin" className={navLinkClass}>
                  <ShieldCheck className="size-5!" />
                  <span className="text-sm font-medium">admin</span>
                  <span className="ml-auto text-[10px] font-mono text-muted-foreground/40 leading-none">
                    ga
                  </span>
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
