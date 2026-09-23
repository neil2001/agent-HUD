import { type ReactNode, useMemo, useState } from "react";
import "./flow.css";
import { formatDuration, formatPercent, formatTime } from "./format";
import { ContextRiver } from "./ContextRiver";
import { METRIC_HELP } from "./metricHelp";
import { MetricTooltip } from "./MetricTooltip";
import { useFlowData } from "./useFlowData";
import type { FlowSettings, FlowSummary, FlowTimeline, TurnDurationBuckets } from "./types";

type Tab = "overview" | "timeline" | "focus" | "agents" | "privacy";

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

  return (
    <div className="flow-app">
      <header className="flow-header">
        <h1>Agent Flow</h1>
        <p>Flight recorder for human–agent work · today</p>
      </header>
      <nav className="flow-nav">
        {(
          [
            ["overview", "Overview"],
            ["timeline", "Timeline"],
            ["focus", "Focus"],
            ["agents", "Agents"],
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
      <main className="flow-main">
        {error && <p className="flow-error">{error}</p>}
        {loading && <p className="flow-empty">Loading…</p>}
        {empty && tab !== "privacy" && (
          <p className="flow-empty">No agent activity recorded today.</p>
        )}
        {!loading && summary && timeline && tab === "overview" && !empty && (
          <Overview summary={summary} timeline={timeline} />
        )}
        {!loading && timeline && tab === "timeline" && !empty && (
          <TimelineView entries={timeline.entries} />
        )}
        {!loading && timeline && tab === "focus" && !empty && (
          <FocusView periods={timeline.focus_periods} />
        )}
        {!loading && timeline && tab === "agents" && !empty && (
          <AgentsView agents={timeline.agents} />
        )}
        {tab === "privacy" && settings && (
          <PrivacyView settings={settings} onSave={saveSettings} onRefresh={refresh} />
        )}
      </main>
    </div>
  );
}

function Overview({
  summary,
  timeline,
}: {
  summary: FlowSummary;
  timeline: FlowTimeline;
}) {
  const buckets = summary.turn_duration_buckets;
  const maxBucket = Math.max(
    buckets.under_30s,
    buckets.s30_to_2m,
    buckets.m2_to_5m,
    buckets.m5_to_15m,
    buckets.over_15m,
    1,
  );

  return (
    <div className="flow-overview">
      <ContextRiver river={timeline.river} />
      <div className="flow-activity-row">
        <ActivityFigure
          label="Sessions"
          value={String(summary.sessions)}
          help={METRIC_HELP.sessions}
        />
        <ActivityFigure
          label="Turns"
          value={String(summary.turns)}
          help={METRIC_HELP.turns}
        />
      </div>

      <section className="flow-overview-section">
        <h2 className="flow-section-heading">The loop</h2>
        <p className="flow-loop-caption">
          Prompt → other work → return after the agent finishes
        </p>
        <div className="flow-loop-grid">
          <Stat
            label="Median return"
            value={
              summary.return_latency_median_ms != null
                ? formatDuration(summary.return_latency_median_ms)
                : "—"
            }
            help={METRIC_HELP.medianReturnLatency}
          />
          <Stat
            label="P90 return"
            value={
              summary.return_latency_p90_ms != null
                ? formatDuration(summary.return_latency_p90_ms)
                : "—"
            }
            help={METRIC_HELP.p90ReturnLatency}
          />
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
          <MetricTooltip help={METRIC_HELP.maxConcurrent}>
            <span className="flow-section-meta">
              max {summary.max_concurrent_agents}
            </span>
          </MetricTooltip>
        </div>
        <ShareHistogram share={summary.concurrency_share} />
      </section>

      <section className="flow-overview-section">
        <SectionTitle help={METRIC_HELP.turnDurationSection}>
          Turn duration
        </SectionTitle>
        <DurationHistogram buckets={buckets} max={maxBucket} />
      </section>
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
    <MetricTooltip help={help}>
      <div className="flow-activity-figure">
        <div className="flow-activity-label">{label}</div>
        <div className="flow-activity-value">{value}</div>
      </div>
    </MetricTooltip>
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
    <MetricTooltip help={help}>
      <div className="flow-stat">
        <div className="flow-stat-label">{label}</div>
        <div className="flow-stat-value">{value}</div>
      </div>
    </MetricTooltip>
  );
}

function SectionTitle({
  children,
  help,
}: {
  children: ReactNode;
  help?: string;
}) {
  return (
    <MetricTooltip help={help}>
      <h2 className="flow-section-title">{children}</h2>
    </MetricTooltip>
  );
}

function ShareHistogram({ share }: { share: FlowSummary["concurrency_share"] }) {
  const rows = useMemo(
    () =>
      [
        ["1", share.one, METRIC_HELP.concurrencyOne],
        ["2", share.two, METRIC_HELP.concurrencyTwo],
        ["3", share.three, METRIC_HELP.concurrencyThree],
        ["4+", share.four_plus, METRIC_HELP.concurrencyFourPlus],
      ] as const,
    [share],
  );

  return (
    <div className="flow-share-histogram">
      {rows.map(([label, fraction, help]) => (
        <MetricTooltip key={label} help={help}>
          <div className="flow-bar-row">
            <span>{label}</span>
            <div className="flow-bar-track">
              <div
                className="flow-bar-fill"
                style={{ width: `${Math.max(fraction * 100, 0)}%` }}
              />
            </div>
            <span>{formatPercent(fraction)}</span>
          </div>
        </MetricTooltip>
      ))}
    </div>
  );
}

function DurationHistogram({
  buckets,
  max,
}: {
  buckets: TurnDurationBuckets;
  max: number;
}) {
  const rows = useMemo(
    () =>
      [
        ["<30s", buckets.under_30s],
        ["30s–2m", buckets.s30_to_2m],
        ["2–5m", buckets.m2_to_5m],
        ["5–15m", buckets.m5_to_15m],
        ["15m+", buckets.over_15m],
      ] as const,
    [buckets],
  );

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
