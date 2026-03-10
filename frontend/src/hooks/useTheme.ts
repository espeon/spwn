import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { getMe } from "@/api";
import { useBrowserState } from "./useBrowserState";

export function useTheme() {
  // is dark theme or not?
  const isDefaultDark = window.matchMedia(
    "(prefers-color-scheme: dark)",
  ).matches;

  const defaultTheme = isDefaultDark ? "evergarden-winter" : "catppuccin-latte";

  const { data: me } = useQuery({ queryKey: ["me"], queryFn: getMe });
  const [thm, setThm] = useBrowserState("theme", defaultTheme);

  // if we get 'me' let's set browser state
  useEffect(() => {
    if (me?.theme) {
      setThm(me.theme);
    }
  }, [me?.theme, setThm]);

  console.log("Current theme:", me?.theme ?? thm ?? defaultTheme);

  useEffect(() => {
    const theme = me?.theme ?? thm ?? defaultTheme;
    document.documentElement.setAttribute("data-theme", theme);
  }, [me?.theme, thm, defaultTheme]);
}
