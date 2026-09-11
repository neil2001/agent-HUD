import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { AgentSession } from "../types";

export function useSessions() {
  const [sessions, setSessions] = useState<AgentSession[]>([]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    invoke<AgentSession[]>("get_sessions")
      .then(setSessions)
      .catch(() => setSessions([]));

    listen<AgentSession[]>("sessions-changed", (event) => {
      setSessions(event.payload);
    })
      .then((dispose) => {
        unlisten = dispose;
      })
      .catch(() => undefined);

    return () => {
      unlisten?.();
    };
  }, []);

  return sessions;
}
