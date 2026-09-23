import { invoke } from "@tauri-apps/api/core";
import type { DailyUsage } from "../types";

type DailyStripProps = {
  usage: DailyUsage;
};

function formatCount(value: number | null): string {
  return value == null ? "—" : String(value);
}

export function DailyStrip({ usage }: DailyStripProps) {
  return (
    <div className="daily-strip">
      <Metric label="sessions" value={String(usage.sessions)} />
      <Metric label="turns" value={String(usage.turns)} />
      <Metric label="opened" value={formatCount(usage.prs_opened)} />
      <Metric label="merged" value={formatCount(usage.prs_merged)} />
      <button
        type="button"
        className="flow-open-button"
        aria-label="Open Agent Flow"
        onClick={() => {
          invoke("open_flow").catch(() => undefined);
        }}
      >
        <svg width="15" height="15" viewBox="0 0 16 16" aria-hidden="true">
          <circle cx="8" cy="3.25" r="1.15" fill="currentColor" />
          <circle cx="8" cy="8" r="1.15" fill="currentColor" />
          <circle cx="8" cy="12.75" r="1.15" fill="currentColor" />
        </svg>
      </button>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="daily-metric">
      <span className="daily-metric-value">{value}</span>
      <span className="daily-metric-label">{label}</span>
    </div>
  );
}
