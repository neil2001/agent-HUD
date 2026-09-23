import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./flow.css";
import { dayBoundsMs, formatDayLabel, formatDuration, formatPercent, formatTime } from "./format";
import { AttentionView } from "./AttentionView";
import { ContextRiver } from "./ContextRiver";
import { METRIC_HELP } from "./metricHelp";
import { MetricTooltip } from "./MetricTooltip";
import { PrsView } from "./PrsView";
import { useFlowData } from "./useFlowData";
import type { FlowSettings, FlowSummary, FlowTimeline } from "./types";

type Tab = "overview" | "timeline" | "focus" | "agents" | "prs" | "privacy";

export function FlowApp() {
  const { summary, timeline, settings, loading, error, refresh, saveSettings } =
    useFlowData();
  const [tab, setTab] = useState<Tab>("overview");

  const empty =
    !loading &&
    summary &&
    summary.prompts === 0 &&
    summary.turns === 0 &&
    (timeline?.entries.length ?? 0) === 0;

  const todayTab = tab === "timeline" || tab === "focus" || tab === "agents";

  return (
    <div className="flow-app">
      <aside className="flow-sidebar">
        <header className="flow-header">
          <h1>Agent Flow</h1>
          <p>Flight recorder for human–agent work</p>
        </header>
        <nav className="flow-nav">
          {(
            [
              ["overview", "Overview"],
              ["timeline", "Timeline"],
              ["focus", "Focus"],
              ["agents", "Agents"],
              ["prs", "PRs"],
              ["privacy", "Privacy"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              data-active={tab === id}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </nav>
      </aside>
      <main className="flow-main">
        {error && todayTab && <p className="flow-error">{error}</p>}
        {loading && todayTab && <p className="flow-empty">Loading…</p>}
        {empty && todayTab && (
          <p className="flow-empty">No agent activity recorded today.</p>
        )}
        {tab === "overview" && <Overview />}
        {!loading && timeline && tab === "timeline" && !empty && (
          <TimelineView entries={timeline.entries} />
        )}
        {!loading && timeline && tab === "focus" && !empty && (
          <FocusView periods={timeline.focus_periods} />
        )}
        {!loading && timeline && tab === "agents" && !empty && (
          <AgentsView agents={timeline.agents} />
        )}
        {tab === "prs" && <PrsView />}
        {tab === "privacy" && settings && (
          <PrivacyView settings={settings} onSave={saveSettings} onRefresh={refresh} />
        )}
      </main>
    </div>
  );
}

function Overview() {
  const [dayOffset, setDayOffset] = useState(0);
  const [summary, setSummary] = useState<FlowSummary | null>(null);
  const [timeline, setTimeline] = useState<FlowTimeline | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [startMs] = dayBoundsMs(dayOffset);
  const dayLabel = formatDayLabel(startMs);
  const riverHasMarks =
    timeline != null &&
    (timeline.river.lanes.length > 0 || timeline.river.agent_bands.length > 0);

  useEffect(() => {
    let cancelled = false;
    const [rangeStart, rangeEnd] = dayBoundsMs(dayOffset);
    setLoading(true);
    setError(null);
    setSummary(null);
    setTimeline(null);
    Promise.all([
      invoke<FlowSummary>("get_flow_summary", {
        startMs: rangeStart,
        endMs: rangeEnd,
      }),
      invoke<FlowTimeline>("get_flow_timeline", {
        startMs: rangeStart,
        endMs: rangeEnd,
      }),
    ])
      .then(([nextSummary, nextTimeline]) => {
        if (cancelled) return;
        setSummary(nextSummary);
        setTimeline(nextTimeline);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [dayOffset]);

  return (
    <div className="flow-overview">
      <AttentionView />
      {error && <p className="flow-error">{error}</p>}
      {loading && !timeline && <p className="flow-empty">Loading…</p>}
      {timeline && (
        <ContextRiver
          river={timeline.river}
          dayLabel={dayLabel}
          canGoForward={dayOffset > 0}
          onPreviousDay={() => setDayOffset((current) => current + 1)}
          onNextDay={() => setDayOffset((current) => Math.max(0, current - 1))}
        />
      )}
      {summary && riverHasMarks && (
        <>
          <div className="flow-activity-row">
            <ActivityFigure
              label="Between turns"
              value={
                summary.mean_turn_gap_ms != null
                  ? formatDuration(summary.mean_turn_gap_ms)
                  : "—"
              }
            />
          </div>

          <section className="flow-overview-section">
        <h2 className="flow-section-title">The loop</h2>
        <p className="flow-loop-caption">
          How long until you get back after the agent finishes
        </p>
        <CountHistogram
          rows={[
            ["<1m", summary.return_latency_buckets.under_1m],
            ["1–5m", summary.return_latency_buckets.m1_to_5m],
            ["5–15m", summary.return_latency_buckets.m5_to_15m],
            ["15–30m", summary.return_latency_buckets.m15_to_30m],
            ["30m+", summary.return_latency_buckets.over_30m],
          ]}
        />
        <div className="flow-loop-grid">
          <Stat
            label="Premature checks"
            value={String(summary.premature_checks)}
            help={METRIC_HELP.prematureChecks}
          />
          <Stat
            label="Still out"
            value={String(summary.not_yet_returned)}
            help={METRIC_HELP.notYetReturned}
          />
        </div>
      </section>

      <section className="flow-overview-section">
        <div className="flow-section-heading-row">
          <h2 className="flow-section-heading">Concurrency</h2>
          <span className="flow-section-meta">
            max {summary.max_concurrent_agents}
          </span>
        </div>
        <ShareHistogram
          share={summary.concurrency_share}
          wallClockMs={summary.wall_clock_agent_ms}
        />
      </section>

      <section className="flow-overview-section">
        <h2 className="flow-section-title">Turn duration</h2>
        <CountHistogram
          rows={[
            ["<30s", summary.turn_duration_buckets.under_30s],
            ["30s–2m", summary.turn_duration_buckets.s30_to_2m],
            ["2–5m", summary.turn_duration_buckets.m2_to_5m],
            ["5–15m", summary.turn_duration_buckets.m5_to_15m],
            ["15m+", summary.turn_duration_buckets.over_15m],
          ]}
        />
      </section>
        </>
      )}
    </div>
  );
}

function ActivityFigure({
  label,
  value,
  help,
}: {
  label: string;
  value: string;
  help?: string;
}) {
  return (
    <div className="flow-activity-figure">
      <div className="flow-activity-label">
        <MetricTooltip help={help} compact>
          <span>{label}</span>
        </MetricTooltip>
      </div>
      <div className="flow-activity-value">{value}</div>
    </div>
  );
}

function Stat({
  label,
  value,
  help,
}: {
  label: string;
  value: string;
  help?: string;
}) {
  return (
    <div className="flow-stat">
      <div className="flow-stat-label">
        <MetricTooltip help={help} compact>
          <span>{label}</span>
        </MetricTooltip>
      </div>
      <div className="flow-stat-value">{value}</div>
    </div>
  );
}

function ShareHistogram({
  share,
  wallClockMs,
}: {
  share: FlowSummary["concurrency_share"];
  wallClockMs: number;
}) {
  const rows = [
    ["1", share.one],
    ["2", share.two],
    ["3", share.three],
    ["4+", share.four_plus],
  ] as const;

  return (
    <div className="flow-share-histogram">
      {rows.map(([label, fraction]) => (
        <div className="flow-bar-row flow-bar-row-wide" key={label}>
          <span>{label}</span>
          <div className="flow-bar-track">
            <div
              className="flow-bar-fill"
              style={{ width: `${Math.max(fraction * 100, 0)}%` }}
            />
          </div>
          <span>
            {formatPercent(fraction)} · {formatDuration(Math.round(fraction * wallClockMs))}
          </span>
        </div>
      ))}
    </div>
  );
}

function CountHistogram({
  rows,
}: {
  rows: ReadonlyArray<readonly [string, number]>;
}) {
  const max = Math.max(1, ...rows.map(([, count]) => count));

  return (
    <div>
      {rows.map(([label, count]) => (
        <div className="flow-bar-row" key={label}>
          <span>{label}</span>
          <div className="flow-bar-track">
            <div
              className="flow-bar-fill"
              style={{ width: `${(count / max) * 100}%` }}
            />
          </div>
          <span>{count}</span>
        </div>
      ))}
    </div>
  );
}

function TimelineView({
  entries,
}: {
  entries: import("./types").TimelineEntry[];
}) {
  if (entries.length === 0) {
    return <p className="flow-empty">No events in this period.</p>;
  }
  return (
    <ul className="flow-timeline">
      {entries.map((e, i) => (
        <li key={`${e.timestamp}-${i}`}>
          <time>{formatTime(e.timestamp)}</time>
          <div className={e.premature_check ? "premature" : undefined}>
            {e.label}
            {e.detail ? ` · ${e.detail}` : ""}
            {e.premature_check ? " · premature check" : ""}
          </div>
        </li>
      ))}
    </ul>
  );
}

function FocusView({
  periods,
}: {
  periods: import("./types").FocusPeriodEntry[];
}) {
  if (periods.length === 0) {
    return <p className="flow-empty">No focus periods of 10+ minutes today.</p>;
  }
  return (
    <table className="flow-table">
      <thead>
        <tr>
          <th>Period</th>
          <th>App</th>
          <th>Agent overlap</th>
          <th>Switches</th>
          <th>Premature checks</th>
        </tr>
      </thead>
      <tbody>
        {periods.map((p) => (
          <tr key={p.start_ms}>
            <td>
              {formatTime(p.start_ms)} – {formatTime(p.end_ms)}
            </td>
            <td>{p.app_name}</td>
            <td>{formatDuration(p.agent_overlap_ms)}</td>
            <td>{p.context_switches}</td>
            <td>{p.premature_checks}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function AgentsView({
  agents,
}: {
  agents: import("./types").AgentProjectRollup[];
}) {
  if (agents.length === 0) {
    return <p className="flow-empty">No Cursor agent activity by project.</p>;
  }
  return (
    <table className="flow-table">
      <thead>
        <tr>
          <th>Project</th>
          <th>Turns</th>
          <th>Runtime</th>
          <th>Median turn</th>
          <th>P90 turn</th>
          <th>Premature checks</th>
        </tr>
      </thead>
      <tbody>
        {agents.map((a) => (
          <tr key={a.project}>
            <td>{a.project}</td>
            <td>{a.turns}</td>
            <td>{formatDuration(a.runtime_ms)}</td>
            <td>
              {a.median_turn_ms != null
                ? formatDuration(a.median_turn_ms)
                : "—"}
            </td>
            <td>
              {a.p90_turn_ms != null ? formatDuration(a.p90_turn_ms) : "—"}
            </td>
            <td>{a.premature_checks}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function PrivacyView({
  settings,
  onSave,
  onRefresh,
}: {
  settings: FlowSettings;
  onSave: (s: FlowSettings) => Promise<void>;
  onRefresh: () => void;
}) {
  const [draft, setDraft] = useState(settings);
  const [saving, setSaving] = useState(false);

  const excludedText = draft.excluded_bundle_ids.join("\n");

  return (
    <div className="flow-privacy">
      <label>
        <input
          type="checkbox"
          checked={draft.recording_enabled}
          onChange={(e) =>
            setDraft({ ...draft, recording_enabled: e.target.checked })
          }
        />
        Record agent and focus events locally
      </label>
      <label>
        Retention (days)
        <input
          type="number"
          min={1}
          value={draft.retention_days}
          onChange={(e) =>
            setDraft({
              ...draft,
              retention_days: Math.max(1, Number(e.target.value) || 90),
            })
          }
        />
      </label>
      <label>
        Excluded bundle IDs (one per line)
        <textarea
          value={excludedText}
          onChange={(e) =>
            setDraft({
              ...draft,
              excluded_bundle_ids: e.target.value
                .split("\n")
                .map((s) => s.trim())
                .filter(Boolean),
            })
          }
        />
      </label>
      <button
        type="button"
        disabled={saving}
        onClick={async () => {
          setSaving(true);
          await onSave(draft);
          setSaving(false);
          onRefresh();
        }}
      >
        {saving ? "Saving…" : "Save settings"}
      </button>
      <p className="flow-empty" style={{ textAlign: "left", paddingTop: 24 }}>
        Changes apply to future events only. Data stays on this Mac.
      </p>
    </div>
  );
}
