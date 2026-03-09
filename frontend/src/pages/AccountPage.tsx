import { Outlet } from "@tanstack/react-router";
import { Link } from "@tanstack/react-router";
import { cn } from "@/lib/utils";
import { IconPalette, IconUserCircle } from "@tabler/icons-react";

const NAV_ITEMS = [
  { to: "/account/identity", label: "identity", icon: IconUserCircle },
  { to: "/account/themes", label: "themes", icon: IconPalette },
] as const;

export function AccountLayout() {
  return (
    <div className="flex flex-col md:flex-row gap-6 md:gap-8">
      <h2 className="text-2xl md:hidden font-semibold tracking-tight ml-2">
        settings
      </h2>
      <nav className="flex flex-row md:flex-col gap-0.5 shrink-0 md:w-36 md:sticky md:top-6 md:self-start md:pt-0.5">
        <p className="hidden md:block text-2xl font-semibold px-2 pb-2">
          settings
        </p>
        {NAV_ITEMS.map(({ to, label, icon: Icon }) => (
          <Link
            key={to}
            to={to}
            className={cn(
              "flex items-center",
              "text-sm px-3 py-1.5 md:rounded-md transition-colors -mr-0.5 md:mr-0",
              "text-muted-foreground hover:text-foreground hover:bg-accent/50",
              "md:text-left",
              "[&.active]:text-foreground [&.active]:font-medium",
              "md:[&.active]:bg-accent md:[&.active]:text-accent-foreground",
              "border-b [&.active]:border-b-accent-foreground ",
              "md:border-b-0 md:[&.active]:border-0",
            )}
          >
            <Icon className="mr-2 size-5 hidden md:block" />
            {label}
          </Link>
        ))}
        <div className="md:hidden flex-1 border-b border-border self-end" />
      </nav>

      <div className="flex-1 min-w-0">
        <Outlet />
      </div>
    </div>
  );
}
