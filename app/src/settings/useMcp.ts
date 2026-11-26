import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { api } from "../api";
import type { McpStatus } from "../types";

const RECENT_AGENT_MILLISECONDS = 15000;

export function useMcp(): McpStatus | null {
  const [status, setStatus] = useState<McpStatus | null>(null);

  useEffect(() => {
    let active = true;
    void api
      .mcpStatus()
      .then((current) => active && setStatus(current))
      .catch(() => undefined);
    const subscription = listen<McpStatus>("mcp", ({ payload }) => setStatus(payload));
    return () => {
      active = false;
      void subscription.then((unlisten) => unlisten());
    };
  }, []);

  return status;
}

export function agentActive(status: McpStatus | null, now: number): boolean {
  return status?.clients.some((client) => now - client.lastSeen < RECENT_AGENT_MILLISECONDS) ?? false;
}
