export type ConcurrencyShare = {
  one: number;
  two: number;
  three: number;
  four_plus: number;
};

export type TurnDurationBuckets = {
  under_30s: number;
  s30_to_2m: number;
  m2_to_5m: number;
  m5_to_15m: number;
  over_15m: number;
};

export type FlowSummary = {
  prompts: number;
  turns: number;
  sessions: number;
  agent_runtime_ms: number;
  wall_clock_agent_ms: number;
  max_concurrent_agents: number;
  concurrency_share: ConcurrencyShare;
  context_switches_during_agent: number;
  premature_checks: number;
  supervision_ms: number;
  return_latency_median_ms: number | null;
  return_latency_p90_ms: number | null;
  not_yet_returned: number;
  median_turn_ms: number | null;
  p90_turn_ms: number | null;
  longest_autonomous_ms: number | null;
  focused_time_while_agents_ms: number;
  turn_duration_buckets: TurnDurationBuckets;
};

export type TimelineEntry = {
  timestamp: number;
  kind: string;
  label: string;
  detail: string | null;
  premature_check: boolean;
};

export type FocusPeriodEntry = {
  start_ms: number;
  end_ms: number;
  app_name: string;
  agent_overlap_ms: number;
  context_switches: number;
  premature_checks: number;
};

export type AgentProjectRollup = {
  project: string;
  sessions: number;
  turns: number;
  runtime_ms: number;
  median_turn_ms: number | null;
  p90_turn_ms: number | null;
  premature_checks: number;
};

export type RiverSegment = {
  start_ms: number;
  end_ms: number;
};

export type RiverLane = {
  app_name: string;
  is_cursor: boolean;
  segments: RiverSegment[];
};

export type RiverPiece = {
  start_ms: number;
  end_ms: number;
  autonomous: boolean;
};

export type RiverAgentBand = {
  label: string;
  open: boolean;
  pieces: RiverPiece[];
};

export type RiverMarker = {
  timestamp: number;
  kind: string;
};

export type FlowRiver = {
  range_start_ms: number;
  range_end_ms: number;
  lanes: RiverLane[];
  agent_bands: RiverAgentBand[];
  markers: RiverMarker[];
};

export type FlowTimeline = {
  entries: TimelineEntry[];
  focus_periods: FocusPeriodEntry[];
  agents: AgentProjectRollup[];
  river: FlowRiver;
};

export type FlowSettings = {
  recording_enabled: boolean;
  retention_days: number;
  excluded_bundle_ids: string[];
};
