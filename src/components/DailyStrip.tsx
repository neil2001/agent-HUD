import { invoke } from "@tauri-apps/api/core";
import type { DailyUsage } from "../types";

type DailyStripProps = {
  usage: DailyUsage;
  divided: boolean;
};

function formatCount(value: number | null): string {
  return value == null ? "—" : String(value);
}

export function DailyStrip({ usage, divided }: DailyStripProps) {
  return (
    <div className={divided ? "daily-strip daily-strip-divided" : "daily-strip"}>
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
        <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
          <path
            d="M4.25 11.75 11.75 4.25"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
          />
          <path
            d="M6.25 4.25h5.5v5.5"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
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
