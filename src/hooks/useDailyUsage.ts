import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { DailyUsage } from "../types";

const EMPTY_USAGE: DailyUsage = {
  sessions: 0,
  turns: 0,
  prs_opened: null,
  prs_merged: null,
};

export function useDailyUsage() {
  const [usage, setUsage] = useState<DailyUsage>(EMPTY_USAGE);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    invoke<DailyUsage>("get_daily_usage")
      .then(setUsage)
      .catch(() => setUsage(EMPTY_USAGE));

    listen<DailyUsage>("daily-usage-changed", (event) => {
      setUsage(event.payload);
    })
      .then((dispose) => {
        unlisten = dispose;
      })
      .catch(() => undefined);

    return () => {
      unlisten?.();
    };
  }, []);

  return usage;
}
