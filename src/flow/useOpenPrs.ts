import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { OpenPrList } from "./types";

export const OPEN_PR_WINDOWS = [30, 60, 90] as const;
export type OpenPrWindow = (typeof OPEN_PR_WINDOWS)[number];

export function useOpenPrs(days: OpenPrWindow) {
  const [list, setList] = useState<OpenPrList | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setList(null);
    invoke<OpenPrList>("list_open_prs", { days })
      .then((next) => {
        if (!cancelled) setList(next);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [days]);

  return { list, loading, error };
}
