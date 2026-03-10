import { useTheme } from "@/hooks/useTheme";
import { Outlet } from "@tanstack/react-router";

export function RootLayout() {
  useTheme();
  return <Outlet />;
}
