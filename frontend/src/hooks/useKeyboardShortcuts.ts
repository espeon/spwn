import { useEffect } from "react";
import { useNavigate } from "@tanstack/react-router";

const ROUTES: Record<string, string> = {
  v: "/vms",
  i: "/images",
  n: "/namespaces",
  a: "/admin",
  s: "/account",
};

export function useKeyboardShortcuts(onOpenHelp?: () => void) {
  const navigate = useNavigate();

  useEffect(() => {
    let gPressed = false;
    let timer: ReturnType<typeof setTimeout>;

    function onKey(e: KeyboardEvent) {
      const target = e.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable
      )
        return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      if (e.key === "?") {
        e.preventDefault();
        onOpenHelp?.();
        return;
      }

      if (e.key === "g") {
        gPressed = true;
        clearTimeout(timer);
        timer = setTimeout(() => {
          gPressed = false;
        }, 1000);
        return;
      }

      if (gPressed) {
        clearTimeout(timer);
        gPressed = false;
        const to = ROUTES[e.key];
        if (to) {
          e.preventDefault();
          navigate({ to } as Parameters<typeof navigate>[0]);
        }
      }
    }

    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      clearTimeout(timer);
    };
  }, [navigate, onOpenHelp]);
}
