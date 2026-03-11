import { useEffect, useRef } from "react";
import { Link } from "@tanstack/react-router";
import { IconGalaxy } from "@tabler/icons-react";
import { Button } from "@/components/ui/button";

export default function HomePage() {
  const cursorRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const line1Ref = useRef<HTMLSpanElement>(null);
  const line2Ref = useRef<HTMLSpanElement>(null);
  const line3Ref = useRef<HTMLSpanElement>(null);
  const geoRectRef = useRef<HTMLDivElement>(null);
  const diagonalRef = useRef<HTMLDivElement>(null);
  const counterBlockRef = useRef<HTMLDivElement>(null);
  const infoRefs = useRef<(HTMLDivElement | null)[]>([]);

  useEffect(() => {
    const reduced = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    const lines = [line1Ref.current, line2Ref.current, line3Ref.current];

    const show = (el: HTMLElement | null, cls = "visible") =>
      el?.classList.add(cls);

    if (reduced) {
      show(gridRef.current);
      lines.forEach((l) => show(l));
      show(geoRectRef.current);
      show(diagonalRef.current);
      show(counterBlockRef.current);
      infoRefs.current.forEach((el) => show(el));
      return;
    }

    const ids: ReturnType<typeof setTimeout>[] = [];
    const t = (fn: () => void, ms: number) => {
      const id = setTimeout(fn, ms);
      ids.push(id);
    };

    t(() => show(gridRef.current), 200);
    t(() => show(line1Ref.current), 400);
    t(() => show(line2Ref.current), 520);
    t(() => show(line3Ref.current), 640);
    t(() => show(geoRectRef.current), 500);
    t(() => show(diagonalRef.current), 700);
    t(() => show(counterBlockRef.current), 900);
    infoRefs.current.forEach((el, i) => t(() => show(el), 1100 + i * 80));

    return () => ids.forEach(clearTimeout);
  }, []);

  // custom cursor
  useEffect(() => {
    const reduced = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    if (reduced || !window.matchMedia("(pointer: fine)").matches) return;

    const cursor = cursorRef.current;
    let mouseX = 0,
      mouseY = 0,
      cx = 0,
      cy = 0;
    let rafId: number;

    const onMove = (e: MouseEvent) => {
      mouseX = e.clientX;
      mouseY = e.clientY;
      cursor?.classList.add("!opacity-100");
    };
    const onLeave = () => cursor?.classList.remove("!opacity-100");

    const loop = () => {
      cx += (mouseX - cx) * 0.12;
      cy += (mouseY - cy) * 0.12;
      if (cursor) {
        cursor.style.left = `${cx}px`;
        cursor.style.top = `${cy}px`;
      }
      rafId = requestAnimationFrame(loop);
    };

    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseleave", onLeave);
    rafId = requestAnimationFrame(loop);

    const interactives = Array.from(document.querySelectorAll("a, button"));
    const enterHandlers = interactives.map((el) => {
      const fn = () =>
        cursor?.classList.add(
          "scale-[4]",
          "!bg-transparent",
          "border",
          "border-destructive",
        );
      el.addEventListener("mouseenter", fn);
      return { el, fn };
    });
    const leaveHandlers = interactives.map((el) => {
      const fn = () =>
        cursor?.classList.remove(
          "scale-[4]",
          "!bg-transparent",
          "border",
          "border-destructive",
        );
      el.addEventListener("mouseleave", fn);
      return { el, fn };
    });

    return () => {
      cancelAnimationFrame(rafId);
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseleave", onLeave);
      enterHandlers.forEach(({ el, fn }) =>
        el.removeEventListener("mouseenter", fn),
      );
      leaveHandlers.forEach(({ el, fn }) =>
        el.removeEventListener("mouseleave", fn),
      );
    };
  }, []);

  return (
    <div className="relative w-full min-h-dvh max-w-560 mx-auto bg-background text-foreground overflow-x-hidden">
      {/* custom cursor */}
      <div
        ref={cursorRef}
        aria-hidden="true"
        className="fixed w-2 h-2 rounded-full bg-foreground pointer-events-none z-9999 opacity-0 -translate-x-1/2 -translate-y-1/2 transition-[transform,background] duration-200 mix-blend-difference"
      />

      {/* subtle radial glow at top */}
      <div
        aria-hidden="true"
        className="fixed inset-0 pointer-events-none z-0"
        style={{
          background:
            "radial-gradient(ellipse 80% 50% at 50% -20%, color-mix(in srgb, var(--destructive) 12%, transparent) 0%, transparent 60%)",
        }}
      />

      {/* grid overlay — fades in on mount */}
      <div
        ref={gridRef}
        aria-hidden="true"
        className="fixed inset-0 pointer-events-none z-0 opacity-0 transition-opacity duration-1000"
      >
        {[
          8.333, 16.666, 25, 33.333, 41.666, 50, 58.333, 66.666, 75, 83.333,
          91.666,
        ].map((x) => (
          <div
            key={`v${x}`}
            className="absolute top-0 bottom-0 w-px"
            style={{ left: `${x}%`, background: "rgba(255,255,255,0.04)" }}
          />
        ))}
        {[16.666, 33.333, 50, 66.666, 83.333].map((y) => (
          <div
            key={`h${y}`}
            className="absolute left-0 right-0 h-px"
            style={{ top: `${y}%`, background: "rgba(255,255,255,0.04)" }}
          />
        ))}
      </div>

      {/* page shell */}
      <div className="relative z-10 flex flex-col min-h-dvh">
        {/* ── header ── */}
        <header className="flex justify-between items-center px-[5vw] pt-6 z-20">
          <Link
            to="/"
            className={`-mt-1 flex items-center gap-2 px-2 py-2 hover:text-sidebar-foreground hover:bg-sidebar-accent data-[status=active]:bg-sidebar-accent data-[status=active]:text-sidebar-foreground duration-200 rounded-md`}
          >
            <IconGalaxy className="size-8!" />
            <span className="text-3xl font-semibold -mt-1.5">spwn</span>
          </Link>
          <Link to="/login">
            <Button variant="ghost" className="text-foreground/80">
              Sign in
            </Button>
          </Link>
        </header>

        {/* ── main hero ── */}
        <main className="flex-1 flex flex-col justify-center px-[5vw] relative pt-8 pb-16">
          {/* diagonal accent bar */}
          <div
            ref={diagonalRef}
            aria-hidden="true"
            className="absolute left-0 top-[20%] w-0.75 h-[30vh] opacity-0 origin-top transition-[transform,opacity] duration-1000 [transition-timing-function:cubic-bezier(0.16,1,0.3,1)] scale-y-0"
            style={{
              background:
                "linear-gradient(to bottom, var(--destructive), transparent)",
            }}
          />

          {/* geo rect */}
          <div
            ref={geoRectRef}
            aria-hidden="true"
            className="absolute right-[6%] top-1/2 -translate-y-1/2 pointer-events-none z-0 origin-left transition-[transform,opacity] duration-1200 [transition-timing-function:cubic-bezier(0.16,1,0.3,1)]"
            style={{
              width: "clamp(180px,30vw,400px)",
              height: "clamp(180px,30vw,400px)",
              border: "2px solid var(--foreground)",
            }}
          >
            <div
              className="absolute"
              style={{
                inset: "15%",
                background: "var(--destructive)",
                opacity: 0.9,
              }}
            />
            <div
              className="absolute"
              style={{
                inset: "25%",
                border: "1px solid var(--muted-foreground)",
              }}
            />
          </div>

          {/* headline */}
          <div className="flex gap-2">
            <IconGalaxy className="inline-block mt-2 text-destructive size-36" />
            <h1
              className="relative z-10 font-black tracking-[-0.03em] uppercase leading-[0.9]"
              style={{ fontSize: "clamp(3rem, 13vw, 11rem)" }}
            >
              {/* each line: outer clips, inner slides up */}
              <span className="block overflow-hidden">
                <span
                  ref={line1Ref}
                  className="block translate-y-full opacity-0 transition-[transform,opacity] duration-1000 [transition-timing-function:cubic-bezier(0.16,1,0.3,1)] [&.visible]:translate-y-0 [&.visible]:opacity-100"
                >
                  Compute you
                </span>
              </span>
              <span className="block overflow-hidden">
                <span
                  ref={line2Ref}
                  className="block translate-y-full opacity-0 transition-[transform,opacity] duration-1000 [transition-delay:100ms] [transition-timing-function:cubic-bezier(0.16,1,0.3,1)] [&.visible]:translate-y-0 [&.visible]:opacity-100"
                >
                  can just <span className="text-[#9ccfd8]">do</span>
                </span>
              </span>
              <span className="block overflow-hidden">
                <span
                  ref={line3Ref}
                  className="block translate-y-full opacity-0 transition-[transform,opacity] duration-1000 [transition-delay:200ms] [transition-timing-function:cubic-bezier(0.16,1,0.3,1)] [&.visible]:translate-y-0 [&.visible]:opacity-100"
                >
                  things with
                </span>
              </span>
            </h1>
          </div>
        </main>
      </div>

      <style>{`
        @keyframes hp-logo-pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.35; }
        }
        @keyframes hp-ripple {
          to { transform: scale(50); opacity: 0; }
        }

        @media (prefers-reduced-motion: reduce) {
          [ref] span,
          [ref] div {
            transition: none !important;
          }
        }
      `}</style>
    </div>
  );
}
