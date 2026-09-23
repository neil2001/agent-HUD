import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ATTENTION_SPANS,
  attentionBoundsMs,
  formatAttentionRange,
  formatDuration,
  formatTurnsPerHour,
  type AttentionSpan,
} from "./format";
import { METRIC_HELP } from "./metricHelp";
import { MetricTooltip } from "./MetricTooltip";
import type { AttentionReport } from "./types";

function childName(label: string): string {
  const mark = " · ";
  const split = label.indexOf(mark);
  const name = split >= 0 ? label.slice(split + mark.length).trim() : label;
  return name || label;
}

export function AttentionView() {
  const [span, setSpan] = useState<AttentionSpan>("1d");
  const [stepsBack, setStepsBack] = useState(0);
  const [report, setReport] = useState<AttentionReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const range = useMemo(() => {
    const [startMs, endMs] = attentionBoundsMs(span, stepsBack);
    return {
      startMs,
      endMs,
      label: formatAttentionRange(startMs, endMs),
    };
  }, [span, stepsBack]);
  const { startMs, endMs, label: rangeLabel } = range;

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setReport(null);
    invoke<AttentionReport>("get_attention", { startMs, endMs })
      .then((next) => {
        if (!cancelled) setReport(next);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setReport(null);
          setError(String(err));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [startMs, endMs]);

  return (
    <div className="flow-attention">
      <div className="flow-attention-top">
        <MetricTooltip help={METRIC_HELP.attentionFocused}>
          <div className="flow-activity-figure flow-attention-hero">
            <div className="flow-activity-value">
              {report ? formatDuration(report.focused_ms) : "—"}
            </div>
            <div className="flow-activity-label">focused</div>
          </div>
        </MetricTooltip>
        <div className="flow-attention-controls">
          <div className="flow-range flow-range-stepper" role="group" aria-label="Earlier or later">
            <button
              type="button"
              aria-label="Previous window"
              onClick={() => setStepsBack((current) => current + 1)}
            >
              ‹
            </button>
            <span className="flow-range-label">{rangeLabel}</span>
            <button
              type="button"
              aria-label="Next window"
              disabled={stepsBack === 0}
              onClick={() => setStepsBack((current) => Math.max(0, current - 1))}
            >
              ›
            </button>
          </div>
          <div className="flow-range" role="group" aria-label="Time window">
            {ATTENTION_SPANS.map(([id, label]) => (
              <button
                key={id}
                type="button"
                data-active={span === id}
                onClick={() => {
                  setSpan(id);
                  setStepsBack(0);
                }}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {report && (
        <div className="flow-attention-metrics">
          <MetricTooltip help={METRIC_HELP.attentionSessions}>
            <Figure label="sessions" value={String(report.sessions)} />
          </MetricTooltip>
          <Figure label="turns" value={String(report.turns)} />
          <Figure label="agent" value={formatDuration(report.agent_runtime_ms)} />
          <Figure label="turns/hr" value={formatTurnsPerHour(report.turns_per_hour)} />
        </div>
      )}

      {error && <p className="flow-error">{error}</p>}
      {loading && !report && <p className="flow-empty">Loading…</p>}
      {report && report.apps.length === 0 && (
        <p className="flow-empty">No focus recorded in this window.</p>
      )}
      {report && report.apps.length > 0 && <AttentionBars report={report} />}
    </div>
  );
}

function Figure({ label, value }: { label: string; value: string }) {
  return (
    <div className="flow-activity-figure flow-attention-metric">
      <div className="flow-activity-value">{value}</div>
      <div className="flow-activity-label">{label}</div>
    </div>
  );
}

function AttentionBars({ report }: { report: AttentionReport }) {
  const scale = Math.max(...report.apps.map((app) => app.focused_ms), 1);

  return (
    <div className="flow-attention-bars">
      {report.apps.map((app) => (
        <div className="flow-attention-group" key={app.app_name}>
          <BarRow
            label={app.app_name}
            focusedMs={app.focused_ms}
            scale={scale}
          />
          {app.children.map((child, index) => (
            <BarRow
              key={`${app.app_name}:${child.label}:${index}`}
              label={childName(child.label)}
              title={child.label}
              focusedMs={child.focused_ms}
              scale={scale}
              child
            />
          ))}
        </div>
      ))}
    </div>
  );
}

function BarRow({
  label,
  title,
  focusedMs,
  scale,
  child = false,
}: {
  label: string;
  title?: string;
  focusedMs: number;
  scale: number;
  child?: boolean;
}) {
  const width = Math.max(0, Math.min(100, (focusedMs / scale) * 100));
  return (
    <div className={child ? "flow-attention-row flow-attention-row-child" : "flow-attention-row"}>
      <span className="flow-attention-name" title={title ?? label}>
        {label}
      </span>
      <div className="flow-bar-track">
        {width > 0 && (
          <div
            className={child ? "flow-bar-fill flow-attention-fill-child" : "flow-bar-fill"}
            style={{ width: `${width}%` }}
          />
        )}
      </div>
      <span className="flow-attention-duration">{formatDuration(focusedMs)}</span>
    </div>
  );
}
