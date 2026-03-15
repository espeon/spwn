import { useEffect, useRef, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

interface Props {
  vmId: string;
  open: boolean;
}

export function ConsolePanel({ vmId, open }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return;

    const proto = window.location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${window.location.host}/api/vms/${vmId}/console`);
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onopen = () => {
      termRef.current?.writeln("\r\x1b[32mconnected\x1b[0m");
    };

    ws.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        termRef.current?.write(new Uint8Array(e.data));
      }
    };

    ws.onclose = () => {
      termRef.current?.writeln("\r\x1b[31mdisconnected\x1b[0m");
    };

    ws.onerror = () => {
      termRef.current?.writeln("\r\x1b[31mconnection error\x1b[0m");
    };
  }, [vmId]);

  // Mount terminal once.
  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Fira Code", monospace',
      theme: {
        background: "#0a0a0a",
        foreground: "#e4e4e4",
        cursor: "#e4e4e4",
      },
    });

    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();

    term.onData((data) => {
      const ws = wsRef.current;
      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(new TextEncoder().encode(data));
      }
    });

    termRef.current = term;
    fitRef.current = fit;

    return () => {
      term.dispose();
      wsRef.current?.close();
    };
  }, []);

  // Connect/disconnect based on open state.
  useEffect(() => {
    if (open) {
      connect();
      requestAnimationFrame(() => fitRef.current?.fit());
    } else {
      wsRef.current?.close();
      wsRef.current = null;
    }
  }, [open, connect]);

  // Refit on resize.
  useEffect(() => {
    const observer = new ResizeObserver(() => fitRef.current?.fit());
    if (containerRef.current) observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, []);

  return (
    <div
      ref={containerRef}
      className="h-full w-full rounded-md overflow-hidden bg-[#0a0a0a]"
    />
  );
}
