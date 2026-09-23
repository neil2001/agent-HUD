import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { FlowSettings, FlowSummary, FlowTimeline } from "./types";
import { todayBoundsMs } from "./format";

export function useFlowData() {
  const [summary, setSummary] = useState<FlowSummary | null>(null);
  const [timeline, setTimeline] = useState<FlowTimeline | null>(null);
  const [settings, setSettings] = useState<FlowSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    const [startMs, dayEnd] = todayBoundsMs();
    const endMs = Math.min(dayEnd, Date.now());
    try {
      const [s, t, cfg] = await Promise.all([
        invoke<FlowSummary>("get_flow_summary", {
          startMs,
          endMs,
        }),
        invoke<FlowTimeline>("get_flow_timeline", {
          startMs,
          endMs,
        }),
        invoke<FlowSettings>("get_flow_settings"),
      ]);
      setSummary(s);
      setTimeline(t);
      setSettings(cfg);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const saveSettings = useCallback(
    async (next: FlowSettings) => {
      await invoke("set_flow_settings", { settings: next });
      setSettings(next);
    },
    [],
  );

  return {
    summary,
    timeline,
    settings,
    loading,
    error,
    refresh,
    saveSettings,
  };
}
