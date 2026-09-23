use serde::{Deserialize, Serialize};
use super::event::{FlowEvent, FlowEventType};
use super::focus_macos::is_agent_host_app;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSummary {
    pub prompts: u64,
    pub turns: u64,
    pub sessions: u64,
    pub agent_runtime_ms: i64,
    pub wall_clock_agent_ms: i64,
    pub max_concurrent_agents: u32,
    pub concurrency_share: ConcurrencyShare,
    pub context_switches_during_agent: u64,
    pub premature_checks: u64,
    pub supervision_ms: i64,
    pub return_latency_median_ms: Option<i64>,
    pub return_latency_p90_ms: Option<i64>,
    pub not_yet_returned: u64,
    pub median_turn_ms: Option<i64>,
    pub p90_turn_ms: Option<i64>,
    pub longest_autonomous_ms: Option<i64>,
    pub focused_time_while_agents_ms: i64,
    pub turn_duration_buckets: TurnDurationBuckets,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyShare {
    pub one: f64,
    pub two: f64,
    pub three: f64,
    pub four_plus: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnDurationBuckets {
    pub under_30s: u64,
    pub s30_to_2m: u64,
    pub m2_to_5m: u64,
    pub m5_to_15m: u64,
    pub over_15m: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub timestamp: i64,
    pub kind: String,
    pub label: String,
    pub detail: Option<String>,
    pub premature_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusPeriodEntry {
    pub start_ms: i64,
    pub end_ms: i64,
    pub app_name: String,
    pub agent_overlap_ms: i64,
    pub context_switches: u64,
    pub premature_checks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProjectRollup {
    pub project: String,
    pub sessions: u64,
    pub turns: u64,
    pub runtime_ms: i64,
    pub median_turn_ms: Option<i64>,
    pub p90_turn_ms: Option<i64>,
    pub premature_checks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowTimeline {
    pub entries: Vec<TimelineEntry>,
    pub focus_periods: Vec<FocusPeriodEntry>,
    pub agents: Vec<AgentProjectRollup>,
    pub river: FlowRiver,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowRiver {
    pub range_start_ms: i64,
    pub range_end_ms: i64,
    pub lanes: Vec<RiverLane>,
    pub agent_bands: Vec<RiverAgentBand>,
    pub markers: Vec<RiverMarker>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiverLane {
    pub app_name: String,
    pub is_cursor: bool,
    pub segments: Vec<RiverSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiverSegment {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiverAgentBand {
    pub label: String,
    pub open: bool,
    pub pieces: Vec<RiverPiece>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiverPiece {
    pub start_ms: i64,
    pub end_ms: i64,
    pub autonomous: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiverMarker {
    pub timestamp: i64,
    pub kind: String,
}

#[derive(Clone)]
struct TurnInterval {
    session_id: String,
    turn_id: String,
    start: i64,
    end: i64,
    closed: bool,
    project: String,
    title: String,
}

struct FocusPoint {
    timestamp: i64,
    bundle_id: String,
    app_name: String,
}

pub fn compute_summary(events: &[FlowEvent], start_ms: i64, end_ms: i64) -> FlowSummary {
    let turns = build_turns(events, end_ms);
    let completed = completed_turns(&turns);
    let wall_intervals = wall_clock_intervals(&turns);

    let prompts = events
        .iter()
        .filter(|e| e.event_type == FlowEventType::TurnStarted)
        .filter(|e| in_range(e.timestamp, start_ms, end_ms))
        .count() as u64;
    let turn_finished = events
        .iter()
        .filter(|e| e.event_type == FlowEventType::TurnFinished)
        .filter(|e| in_range(e.timestamp, start_ms, end_ms))
        .count() as u64;
    let sessions = events
        .iter()
        .filter(|e| e.event_type == FlowEventType::SessionStarted)
        .filter(|e| in_range(e.timestamp, start_ms, end_ms))
        .count() as u64;

    let agent_runtime_ms = completed.iter().map(|t| t.end - t.start).sum();

    let wall_clock_agent_ms = union_length(&wall_intervals);

    let concurrency = concurrency_stats(&wall_intervals);
    let focus_points = focus_points(events);
    let premature = count_premature_checks(&focus_points, &wall_intervals);
    let supervision_ms = supervision_time(&focus_points, &wall_intervals);
    let ctx_switches = context_switches_during_agent(&focus_points, &wall_intervals, start_ms, end_ms);

    let turn_durations: Vec<i64> = completed.iter().map(|t| t.end - t.start).collect();

    let (return_median, return_p90, not_returned) =
        return_latency_stats(events, &completed, end_ms);

    let autonomous: Vec<i64> = completed
        .iter()
        .filter(|t| premature_for_turn(t, &focus_points) == 0)
        .map(|t| t.end - t.start)
        .collect();

    let focused_while_agents =
        focused_time_while_agents(&focus_points, &wall_intervals, end_ms);

    FlowSummary {
        prompts,
        turns: turn_finished,
        sessions,
        agent_runtime_ms,
        wall_clock_agent_ms,
        max_concurrent_agents: concurrency.max,
        concurrency_share: concurrency.share,
        context_switches_during_agent: ctx_switches,
        premature_checks: premature,
        supervision_ms,
        return_latency_median_ms: return_median,
        return_latency_p90_ms: return_p90,
        not_yet_returned: not_returned,
        median_turn_ms: percentile(&turn_durations, 50),
        p90_turn_ms: percentile(&turn_durations, 90),
        longest_autonomous_ms: autonomous.iter().max().copied(),
        focused_time_while_agents_ms: focused_while_agents,
        turn_duration_buckets: bucket_turns(&turn_durations),
    }
}

pub fn compute_timeline(events: &[FlowEvent], start_ms: i64, end_ms: i64) -> FlowTimeline {
    let turns = build_turns(events, end_ms);
    let wall_intervals = wall_clock_intervals(&turns);
    let focus_points = focus_points(events);

    let mut entries = Vec::new();
    for event in events {
        if !in_range(event.timestamp, start_ms, end_ms) {
            continue;
        }
        match event.event_type {
            FlowEventType::TurnStarted => {
                let title = event.payload.get("title").and_then(|v| v.as_str()).unwrap_or("");
                entries.push(TimelineEntry {
                    timestamp: event.timestamp,
                    kind: "turn_started".to_string(),
                    label: "Agent working".to_string(),
                    detail: Some(title.to_string()),
                    premature_check: false,
                });
            }
            FlowEventType::TurnFinished => {
                entries.push(TimelineEntry {
                    timestamp: event.timestamp,
                    kind: "turn_finished".to_string(),
                    label: "Agent finished".to_string(),
                    detail: None,
                    premature_check: false,
                });
            }
            FlowEventType::AppFocused => {
                let app = event.payload["app_name"].as_str().unwrap_or("Unknown");
                let bundle = event.payload["bundle_id"].as_str().unwrap_or("");
                let premature = is_premature_focus_at(
                    event.timestamp,
                    bundle,
                    app,
                    &focus_points,
                    &wall_intervals,
                );
                entries.push(TimelineEntry {
                    timestamp: event.timestamp,
                    kind: "app_focused".to_string(),
                    label: format!("Focus: {}", app),
                    detail: None,
                    premature_check: premature,
                });
            }
            _ => {}
        }
    }
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let focus_periods = focus_periods(&focus_points, &wall_intervals, start_ms, end_ms);
    let agents = project_rollups(&turns, &focus_points, start_ms, end_ms);
    let river = compute_river(events, start_ms, end_ms);

    FlowTimeline {
        entries,
        focus_periods,
        agents,
        river,
    }
}

struct FocusInterval {
    start: i64,
    end: i64,
    bundle_id: String,
    app_name: String,
}

pub fn compute_river(events: &[FlowEvent], start_ms: i64, end_ms: i64) -> FlowRiver {
    let focus = clean_focus_points(&focus_points(events));
    let intervals = focus_intervals(&focus, start_ms, end_ms);
    let turns = build_turns(events, end_ms);
    let wall_intervals = wall_clock_intervals(&turns);

    let mut lanes = lanes_from_intervals(&intervals);
    collapse_lanes(&mut lanes);

    let agent_bands = agent_bands_from_turns(&turns, &intervals, start_ms, end_ms);
    let mut markers = river_markers(events, &focus, &turns, &wall_intervals, start_ms, end_ms);
    markers.sort_by(|a, b| {
        a.timestamp
            .cmp(&b.timestamp)
            .then_with(|| marker_rank(&a.kind).cmp(&marker_rank(&b.kind)))
    });

    let (range_start_ms, range_end_ms) =
        river_range(&lanes, &agent_bands, &markers, start_ms, end_ms);

    FlowRiver {
        range_start_ms,
        range_end_ms,
        lanes,
        agent_bands,
        markers,
    }
}

fn marker_rank(kind: &str) -> u8 {
    match kind {
        "premature_check" => 0,
        _ => 1,
    }
}

fn clean_app_name(name: &str) -> String {
    let mut trimmed = name.trim();
    while let Some(rest) = trimmed.strip_prefix(',') {
        trimmed = rest.trim_start();
    }
    trimmed.to_string()
}

fn clean_bundle_id(bundle: &str) -> String {
    let trimmed = bundle.trim().trim_end_matches(',').trim();
    if trimmed.is_empty() || trimmed.contains("missing value") || trimmed.contains(',') {
        String::new()
    } else {
        trimmed.to_string()
    }
}

fn clean_focus_points(focus: &[FocusPoint]) -> Vec<FocusPoint> {
    focus
        .iter()
        .filter_map(|point| {
            let app_name = clean_app_name(&point.app_name);
            if app_name.is_empty() {
                return None;
            }
            Some(FocusPoint {
                timestamp: point.timestamp,
                bundle_id: clean_bundle_id(&point.bundle_id),
                app_name,
            })
        })
        .collect()
}

fn focus_intervals(focus: &[FocusPoint], start_ms: i64, end_ms: i64) -> Vec<FocusInterval> {
    let mut out = Vec::new();
    for i in 0..focus.len() {
        let start = focus[i].timestamp;
        let end = focus.get(i + 1).map(|p| p.timestamp).unwrap_or(end_ms);
        if end <= start {
            continue;
        }
        let start = start.max(start_ms);
        let end = end.min(end_ms);
        if end <= start {
            continue;
        }
        out.push(FocusInterval {
            start,
            end,
            bundle_id: focus[i].bundle_id.clone(),
            app_name: focus[i].app_name.clone(),
        });
    }
    out
}

fn interval_is_cursor(interval: &FocusInterval) -> bool {
    is_agent_host_app(&interval.bundle_id, &interval.app_name)
}

fn lanes_from_intervals(intervals: &[FocusInterval]) -> Vec<RiverLane> {
    use std::collections::BTreeMap;
    let mut grouped: BTreeMap<String, RiverLane> = BTreeMap::new();
    for interval in intervals {
        let lane = grouped
            .entry(interval.app_name.clone())
            .or_insert(RiverLane {
                app_name: interval.app_name.clone(),
                is_cursor: false,
                segments: Vec::new(),
            });
        if interval_is_cursor(interval) {
            lane.is_cursor = true;
        }
        lane.segments.push(RiverSegment {
            start_ms: interval.start,
            end_ms: interval.end,
        });
    }
    let mut lanes: Vec<RiverLane> = grouped.into_values().collect();
    for lane in &mut lanes {
        lane.segments.sort_by_key(|s| s.start_ms);
    }
    lanes
}

fn lane_duration(lane: &RiverLane) -> i64 {
    lane.segments.iter().map(|s| s.end_ms - s.start_ms).sum()
}

fn collapse_lanes(lanes: &mut Vec<RiverLane>) {
    if lanes.len() <= 6 {
        sort_lanes(lanes);
        return;
    }

    let mut cursor = Vec::new();
    let mut rest = Vec::new();
    for lane in lanes.drain(..) {
        if lane.is_cursor {
            cursor.push(lane);
        } else {
            rest.push(lane);
        }
    }
    rest.sort_by(|a, b| {
        lane_duration(b)
            .cmp(&lane_duration(a))
            .then_with(|| a.app_name.cmp(&b.app_name))
    });
    let keep_count = 5.min(rest.len());
    let dropped = rest.split_off(keep_count);
    let mut other_segments = Vec::new();
    for lane in dropped {
        other_segments.extend(lane.segments);
    }
    other_segments = union_segments(other_segments);

    let mut kept = cursor;
    kept.extend(rest);
    if !other_segments.is_empty() {
        kept.push(RiverLane {
            app_name: "Other".to_string(),
            is_cursor: false,
            segments: other_segments,
        });
    }
    *lanes = kept;
    sort_lanes(lanes);
}

fn union_segments(mut segments: Vec<RiverSegment>) -> Vec<RiverSegment> {
    if segments.is_empty() {
        return segments;
    }
    segments.sort_by_key(|s| s.start_ms);
    let mut out = vec![segments[0].clone()];
    for segment in segments.into_iter().skip(1) {
        let last = out.last_mut().unwrap();
        if segment.start_ms <= last.end_ms {
            last.end_ms = last.end_ms.max(segment.end_ms);
        } else {
            out.push(segment);
        }
    }
    out
}

fn sort_lanes(lanes: &mut [RiverLane]) {
    lanes.sort_by(|a, b| {
        let rank = |lane: &RiverLane| if lane.app_name == "Other" { 2 } else if lane.is_cursor { 0 } else { 1 };
        rank(a).cmp(&rank(b)).then_with(|| match rank(a) {
            0 => a.app_name.cmp(&b.app_name),
            1 => lane_duration(b)
                .cmp(&lane_duration(a))
                .then_with(|| a.app_name.cmp(&b.app_name)),
            _ => std::cmp::Ordering::Equal,
        })
    });
}

fn band_label(project: &str, title: &str) -> String {
    match (project.is_empty(), title.is_empty()) {
        (true, true) => "Agent".to_string(),
        (true, false) => title.to_string(),
        (false, true) => project.to_string(),
        (false, false) => format!("{project} · {title}"),
    }
}

fn agent_bands_from_turns(
    turns: &[TurnInterval],
    intervals: &[FocusInterval],
    start_ms: i64,
    end_ms: i64,
) -> Vec<RiverAgentBand> {
    let mut bands = Vec::new();
    for turn in turns {
        if turn.end <= start_ms || turn.start >= end_ms {
            continue;
        }
        let start = turn.start.max(start_ms);
        let end = turn.end.min(end_ms);
        if end <= start {
            continue;
        }
        bands.push(RiverAgentBand {
            label: band_label(&turn.project, &turn.title),
            open: !turn.closed,
            pieces: pieces_for_span(start, end, intervals),
        });
    }
    merge_same_label_bands(bands)
}

fn band_span(band: &RiverAgentBand) -> Option<(i64, i64)> {
    let start = band.pieces.iter().map(|p| p.start_ms).min()?;
    let end = band.pieces.iter().map(|p| p.end_ms).max()?;
    Some((start, end))
}

fn merge_same_label_bands(mut bands: Vec<RiverAgentBand>) -> Vec<RiverAgentBand> {
    bands.sort_by(|a, b| {
        band_span(a)
            .map(|s| s.0)
            .cmp(&band_span(b).map(|s| s.0))
            .then_with(|| a.label.cmp(&b.label))
    });
    let mut merged: Vec<RiverAgentBand> = Vec::new();
    for band in bands {
        let Some((start, _)) = band_span(&band) else {
            continue;
        };
        if let Some(existing) = merged.iter_mut().find(|candidate| {
            candidate.label == band.label
                && band_span(candidate).is_some_and(|(_, end)| end <= start)
        }) {
            existing.pieces.extend(band.pieces);
            existing.pieces.sort_by_key(|p| p.start_ms);
            existing.open = existing.open || band.open;
        } else {
            merged.push(band);
        }
    }
    merged
}

fn pieces_for_span(start: i64, end: i64, intervals: &[FocusInterval]) -> Vec<RiverPiece> {
    let mut pieces = Vec::new();
    let mut cursor = start;
    for interval in intervals {
        if interval.end <= start || interval.start >= end {
            continue;
        }
        let overlap_start = interval.start.max(start);
        let overlap_end = interval.end.min(end);
        if overlap_end <= overlap_start {
            continue;
        }
        if overlap_start > cursor {
            pieces.push(RiverPiece {
                start_ms: cursor,
                end_ms: overlap_start,
                autonomous: false,
            });
        }
        let autonomous = !is_agent_host(&interval.bundle_id, &interval.app_name);
        pieces.push(RiverPiece {
            start_ms: overlap_start,
            end_ms: overlap_end,
            autonomous,
        });
        cursor = overlap_end;
    }
    if cursor < end {
        pieces.push(RiverPiece {
            start_ms: cursor,
            end_ms: end,
            autonomous: false,
        });
    }
    merge_pieces(pieces)
}

fn merge_pieces(pieces: Vec<RiverPiece>) -> Vec<RiverPiece> {
    let mut out: Vec<RiverPiece> = Vec::new();
    for piece in pieces {
        if piece.end_ms <= piece.start_ms {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if last.autonomous == piece.autonomous && piece.start_ms <= last.end_ms {
                last.end_ms = last.end_ms.max(piece.end_ms);
                continue;
            }
        }
        out.push(piece);
    }
    out
}

fn river_markers(
    events: &[FlowEvent],
    focus: &[FocusPoint],
    turns: &[TurnInterval],
    wall: &[(i64, i64)],
    start_ms: i64,
    end_ms: i64,
) -> Vec<RiverMarker> {
    let mut markers = Vec::new();
    for point in focus {
        if !in_range(point.timestamp, start_ms, end_ms) {
            continue;
        }
        if is_premature_focus_at(point.timestamp, &point.bundle_id, &point.app_name, focus, wall)
        {
            markers.push(RiverMarker {
                timestamp: point.timestamp,
                kind: "premature_check".to_string(),
            });
        }
    }

    for turn in turns.iter().filter(|t| t.closed) {
        let finish = turn.end;
        let on_agent_at_finish = focus
            .iter()
            .filter(|p| p.timestamp <= finish)
            .last()
            .map(|p| is_agent_host(&p.bundle_id, &p.app_name))
            .unwrap_or(false);
        if on_agent_at_finish {
            continue;
        }
        let next_turn_start = events
            .iter()
            .filter(|e| e.event_type == FlowEventType::TurnStarted)
            .filter(|e| e.session_id.as_deref() == Some(turn.session_id.as_str()))
            .find(|e| e.timestamp > finish)
            .map(|e| e.timestamp);
        let deadline = next_turn_start.unwrap_or(end_ms);
        if let Some(point) = focus.iter().find(|p| {
            p.timestamp > finish
                && p.timestamp < deadline
                && is_agent_host(&p.bundle_id, &p.app_name)
        }) {
            if in_range(point.timestamp, start_ms, end_ms) {
                markers.push(RiverMarker {
                    timestamp: point.timestamp,
                    kind: "return_after_finish".to_string(),
                });
            }
        }
    }
    markers
}

fn river_range(
    lanes: &[RiverLane],
    bands: &[RiverAgentBand],
    markers: &[RiverMarker],
    start_ms: i64,
    end_ms: i64,
) -> (i64, i64) {
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for lane in lanes {
        for segment in &lane.segments {
            starts.push(segment.start_ms);
            ends.push(segment.end_ms);
        }
    }
    for band in bands {
        for piece in &band.pieces {
            starts.push(piece.start_ms);
            ends.push(piece.end_ms);
        }
    }
    for marker in markers {
        starts.push(marker.timestamp);
    }
    let mut range_start = starts.into_iter().min().unwrap_or(start_ms);
    let mut range_end = ends.into_iter().max().unwrap_or(end_ms);
    range_start = range_start.clamp(start_ms, end_ms);
    range_end = range_end.clamp(start_ms, end_ms);
    if range_end <= range_start {
        range_end = range_start + 1;
    }
    (range_start, range_end)
}

fn in_range(ts: i64, start: i64, end: i64) -> bool {
    ts >= start && ts < end
}

fn build_turns(events: &[FlowEvent], end_ms: i64) -> Vec<TurnInterval> {
    let mut turns = Vec::new();
    let mut open: std::collections::HashMap<String, TurnInterval> =
        std::collections::HashMap::new();

    for event in events {
        match event.event_type {
            FlowEventType::TurnStarted => {
                let sid = event.session_id.clone().unwrap_or_default();
                let tid = event.turn_id.clone().unwrap_or_default();
                let project = event
                    .payload
                    .get("project")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = event
                    .payload
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                open.insert(
                    tid.clone(),
                    TurnInterval {
                        session_id: sid,
                        turn_id: tid,
                        start: event.timestamp,
                        end: event.timestamp,
                        closed: false,
                        project,
                        title,
                    },
                );
            }
            FlowEventType::TurnFinished => {
                let tid = event.turn_id.clone().unwrap_or_default();
                if let Some(mut turn) = open.remove(&tid) {
                    turn.end = event.timestamp;
                    turn.closed = true;
                    turns.push(turn);
                }
            }
            _ => {}
        }
    }

    for turn in open.values() {
        let mut t = turn.clone();
        t.end = end_ms;
        turns.push(t);
    }

    turns.sort_by_key(|t| t.start);
    turns
}

fn completed_turns(turns: &[TurnInterval]) -> Vec<&TurnInterval> {
    turns.iter().filter(|t| t.closed).collect()
}

fn wall_clock_intervals(turns: &[TurnInterval]) -> Vec<(i64, i64)> {
    turns.iter().map(|t| (t.start, t.end)).collect()
}

fn union_length(intervals: &[(i64, i64)]) -> i64 {
    if intervals.is_empty() {
        return 0;
    }
    let mut sorted = intervals.to_vec();
    sorted.sort_by_key(|i| i.0);
    let mut total = 0i64;
    let mut cur_start = sorted[0].0;
    let mut cur_end = sorted[0].1;
    for (s, e) in sorted.iter().skip(1) {
        if *s <= cur_end {
            cur_end = cur_end.max(*e);
        } else {
            total += cur_end - cur_start;
            cur_start = *s;
            cur_end = *e;
        }
    }
    total += cur_end - cur_start;
    total
}

struct ConcurrencyResult {
    max: u32,
    share: ConcurrencyShare,
}

fn concurrency_stats(intervals: &[(i64, i64)]) -> ConcurrencyResult {
    if intervals.is_empty() {
        return ConcurrencyResult {
            max: 0,
            share: ConcurrencyShare {
                one: 0.0,
                two: 0.0,
                three: 0.0,
                four_plus: 0.0,
            },
        };
    }

    let mut events: Vec<(i64, i32)> = Vec::new();
    for (s, e) in intervals {
        events.push((*s, 1));
        events.push((*e, -1));
    }
    events.sort_by_key(|(t, _)| *t);

    let wall = union_length(intervals);
    if wall == 0 {
        return ConcurrencyResult {
            max: 0,
            share: ConcurrencyShare {
                one: 0.0,
                two: 0.0,
                three: 0.0,
                four_plus: 0.0,
            },
        };
    }

    let mut active = 0i32;
    let mut max = 0u32;
    let mut time_at = [0i64; 4];
    let mut prev_t = events[0].0;

    for (t, delta) in events {
        if t > prev_t && active > 0 {
            let dt = t - prev_t;
            let idx = (active as usize).min(4) - 1;
            time_at[idx] += dt;
        }
        active += delta;
        max = max.max(active as u32);
        prev_t = t;
    }

    ConcurrencyResult {
        max,
        share: ConcurrencyShare {
            one: time_at[0] as f64 / wall as f64,
            two: time_at[1] as f64 / wall as f64,
            three: time_at[2] as f64 / wall as f64,
            four_plus: time_at[3] as f64 / wall as f64,
        },
    }
}

fn focus_points(events: &[FlowEvent]) -> Vec<FocusPoint> {
    events
        .iter()
        .filter(|e| e.event_type == FlowEventType::AppFocused)
        .map(|e| FocusPoint {
            timestamp: e.timestamp,
            bundle_id: e.payload["bundle_id"].as_str().unwrap_or("").to_string(),
            app_name: e.payload["app_name"].as_str().unwrap_or("").to_string(),
        })
        .collect()
}

fn inside_wall(ts: i64, wall: &[(i64, i64)]) -> bool {
    wall.iter().any(|(s, e)| ts >= *s && ts < *e)
}

fn context_switches_during_agent(
    focus: &[FocusPoint],
    wall: &[(i64, i64)],
    start_ms: i64,
    end_ms: i64,
) -> u64 {
    let mut count = 0u64;
    let mut prev_bundle: Option<String> = None;
    for p in focus {
        if !in_range(p.timestamp, start_ms, end_ms) {
            continue;
        }
        if !inside_wall(p.timestamp, wall) {
            prev_bundle = Some(p.bundle_id.clone());
            continue;
        }
        if let Some(prev) = &prev_bundle {
            if prev != &p.bundle_id {
                count += 1;
            }
        }
        prev_bundle = Some(p.bundle_id.clone());
    }
    count
}

fn is_agent_host(bundle_id: &str, app_name: &str) -> bool {
    is_agent_host_app(bundle_id, app_name)
}

fn is_premature_focus_at(
    ts: i64,
    bundle_id: &str,
    app_name: &str,
    focus: &[FocusPoint],
    wall: &[(i64, i64)],
) -> bool {
    if !inside_wall(ts, wall) || !is_agent_host(bundle_id, app_name) {
        return false;
    }
    let prev = focus.iter().filter(|p| p.timestamp < ts).last();
    match prev {
        Some(p) if is_agent_host(&p.bundle_id, &p.app_name) => false,
        _ => true,
    }
}

fn count_premature_checks(focus: &[FocusPoint], wall: &[(i64, i64)]) -> u64 {
    focus
        .iter()
        .filter(|p| is_premature_focus_at(p.timestamp, &p.bundle_id, &p.app_name, focus, wall))
        .count() as u64
}

fn premature_for_turn(turn: &TurnInterval, focus: &[FocusPoint]) -> u64 {
    let wall = vec![(turn.start, turn.end)];
    focus
        .iter()
        .filter(|p| {
            p.timestamp >= turn.start
                && p.timestamp < turn.end
                && is_premature_focus_at(p.timestamp, &p.bundle_id, &p.app_name, focus, &wall)
        })
        .count() as u64
}

fn supervision_time(focus: &[FocusPoint], wall: &[(i64, i64)]) -> i64 {
    if focus.is_empty() || wall.is_empty() {
        return 0;
    }
    let mut total = 0i64;
    for i in 0..focus.len() {
        let start = focus[i].timestamp;
        let end = focus.get(i + 1).map(|p| p.timestamp).unwrap_or(i64::MAX);
        if !is_agent_host(&focus[i].bundle_id, &focus[i].app_name) {
            continue;
        }
        for (ws, we) in wall {
            let overlap_start = start.max(*ws);
            let overlap_end = end.min(*we);
            if overlap_end > overlap_start {
                total += overlap_end - overlap_start;
            }
        }
    }
    total
}

fn return_latency_stats(
    events: &[FlowEvent],
    completed: &[&TurnInterval],
    end_ms: i64,
) -> (Option<i64>, Option<i64>, u64) {
    let mut latencies = Vec::new();
    let mut not_returned = 0u64;

    let all_focus = focus_points(events);

    for turn in completed {
        let finish = turn.end;
        let on_agent_at_finish = all_focus
            .iter()
            .filter(|p| p.timestamp <= finish)
            .last()
            .map(|p| is_agent_host(&p.bundle_id, &p.app_name))
            .unwrap_or(false);

        if on_agent_at_finish {
            latencies.push(0);
            continue;
        }

        let next_turn_start = events
            .iter()
            .filter(|e| e.event_type == FlowEventType::TurnStarted)
            .filter(|e| e.session_id.as_deref() == Some(turn.session_id.as_str()))
            .find(|e| e.timestamp > finish)
            .map(|e| e.timestamp);

        let deadline = next_turn_start.unwrap_or(end_ms);

        let next_agent_focus = all_focus
            .iter()
            .find(|p| p.timestamp > finish && p.timestamp < deadline && is_agent_host(&p.bundle_id, &p.app_name));

        if let Some(p) = next_agent_focus {
            latencies.push(p.timestamp - finish);
        } else if next_turn_start.is_some() {
            not_returned += 1;
        } else if finish < end_ms {
            not_returned += 1;
        }
    }

    (
        percentile(&latencies, 50),
        percentile(&latencies, 90),
        not_returned,
    )
}

fn focused_time_while_agents(
    focus: &[FocusPoint],
    wall: &[(i64, i64)],
    end_ms: i64,
) -> i64 {
    let periods = focus_periods_raw(focus, end_ms);
    let mut total = 0i64;
    for (start, end, _) in periods {
        if end - start < 10 * 60 * 1000 {
            continue;
        }
        for (ws, we) in wall {
            let os = start.max(*ws);
            let oe = end.min(*we);
            if oe > os {
                total += oe - os;
            }
        }
    }
    total
}

fn focus_periods_raw(focus: &[FocusPoint], end_ms: i64) -> Vec<(i64, i64, String)> {
    if focus.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..focus.len() {
        let start = focus[i].timestamp;
        let end = focus.get(i + 1).map(|p| p.timestamp).unwrap_or(end_ms);
        out.push((start, end, focus[i].app_name.clone()));
    }
    out
}

fn focus_periods(
    focus: &[FocusPoint],
    wall: &[(i64, i64)],
    start_ms: i64,
    end_ms: i64,
) -> Vec<FocusPeriodEntry> {
    let raw = focus_periods_raw(focus, end_ms);
    let mut out = Vec::new();
    for (start, end, app_name) in raw {
        if end - start < 10 * 60 * 1000 {
            continue;
        }
        if end <= start_ms || start >= end_ms {
            continue;
        }
        let clipped_start = start.max(start_ms);
        let clipped_end = end.min(end_ms);
        let mut agent_overlap = 0i64;
        for (ws, we) in wall {
            let os = clipped_start.max(*ws);
            let oe = clipped_end.min(*we);
            if oe > os {
                agent_overlap += oe - os;
            }
        }
        let switches = context_switches_during_agent(focus, wall, clipped_start, clipped_end);
        let premature = focus
            .iter()
            .filter(|p| p.timestamp >= clipped_start && p.timestamp < clipped_end)
            .filter(|p| is_premature_focus_at(p.timestamp, &p.bundle_id, &p.app_name, focus, wall))
            .count() as u64;
        out.push(FocusPeriodEntry {
            start_ms: clipped_start,
            end_ms: clipped_end,
            app_name,
            agent_overlap_ms: agent_overlap,
            context_switches: switches,
            premature_checks: premature,
        });
    }
    out
}

fn project_rollups(
    turns: &[TurnInterval],
    focus: &[FocusPoint],
    start_ms: i64,
    end_ms: i64,
) -> Vec<AgentProjectRollup> {
    use std::collections::HashMap;
    let mut map: HashMap<String, AgentProjectRollup> = HashMap::new();

    for turn in turns {
        if !turn.closed || turn.start >= end_ms || turn.end <= start_ms {
            continue;
        }
        let entry = map.entry(turn.project.clone()).or_insert(AgentProjectRollup {
            project: turn.project.clone(),
            sessions: 0,
            turns: 0,
            runtime_ms: 0,
            median_turn_ms: None,
            p90_turn_ms: None,
            premature_checks: 0,
        });
        entry.turns += 1;
        entry.runtime_ms += turn.end - turn.start;
        entry.premature_checks += premature_for_turn(turn, focus);
    }

    let mut result: Vec<AgentProjectRollup> = map.into_values().collect();
    for rollup in &mut result {
        let durations: Vec<i64> = turns
            .iter()
            .filter(|t| t.project == rollup.project && t.closed)
            .map(|t| t.end - t.start)
            .collect();
        rollup.median_turn_ms = percentile(&durations, 50);
        rollup.p90_turn_ms = percentile(&durations, 90);
    }
    result.sort_by(|a, b| a.project.cmp(&b.project));
    result
}

fn bucket_turns(durations: &[i64]) -> TurnDurationBuckets {
    let mut buckets = TurnDurationBuckets {
        under_30s: 0,
        s30_to_2m: 0,
        m2_to_5m: 0,
        m5_to_15m: 0,
        over_15m: 0,
    };
    for d in durations {
        match *d {
            x if x < 30_000 => buckets.under_30s += 1,
            x if x < 120_000 => buckets.s30_to_2m += 1,
            x if x < 300_000 => buckets.m2_to_5m += 1,
            x if x < 900_000 => buckets.m5_to_15m += 1,
            _ => buckets.over_15m += 1,
        }
    }
    buckets
}

fn percentile(values: &[i64], pct: u8) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort();
    let idx = ((sorted.len() - 1) as f64 * (pct as f64 / 100.0)).round() as usize;
    Some(sorted[idx])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::event::{FlowEvent, FlowEventType, FlowSource};
    use crate::flow::focus_macos::CURSOR_BUNDLE_ID;
    use serde_json::json;

    fn turn_start(id: &str, turn: &str, t: i64) -> FlowEvent {
        FlowEvent::new(
            t,
            FlowSource::Cursor,
            FlowEventType::TurnStarted,
            Some(id.to_string()),
            Some(turn.to_string()),
            json!({"project": "p", "title": "t"}),
        )
    }

    fn turn_end(id: &str, turn: &str, t: i64) -> FlowEvent {
        FlowEvent::new(
            t,
            FlowSource::Cursor,
            FlowEventType::TurnFinished,
            Some(id.to_string()),
            Some(turn.to_string()),
            json!({}),
        )
    }

    fn focus(bundle: &str, app: &str, t: i64) -> FlowEvent {
        FlowEvent::new(
            t,
            FlowSource::Macos,
            FlowEventType::AppFocused,
            None,
            None,
            json!({"bundle_id": bundle, "app_name": app}),
        )
    }

    #[test]
    fn single_turn_runtime() {
        let events = vec![turn_start("s1", "s1:1", 0), turn_end("s1", "s1:1", 60_000)];
        let summary = compute_summary(&events, 0, 120_000);
        assert_eq!(summary.agent_runtime_ms, 60_000);
        assert_eq!(summary.wall_clock_agent_ms, 60_000);
        assert_eq!(summary.prompts, 1);
        assert_eq!(summary.turns, 1);
    }

    #[test]
    fn parallel_three_agents_ten_minutes() {
        let events = vec![
            turn_start("a", "a:1", 0),
            turn_start("b", "b:1", 0),
            turn_start("c", "c:1", 0),
            turn_end("a", "a:1", 600_000),
            turn_end("b", "b:1", 600_000),
            turn_end("c", "c:1", 600_000),
        ];
        let summary = compute_summary(&events, 0, 600_000);
        assert_eq!(summary.agent_runtime_ms, 1_800_000);
        assert_eq!(summary.wall_clock_agent_ms, 600_000);
        assert_eq!(summary.max_concurrent_agents, 3);
    }

    #[test]
    fn premature_check_during_run() {
        let events = vec![
            turn_start("s1", "s1:1", 0),
            focus("com.apple.Safari", "Safari", 10_000),
            focus(CURSOR_BUNDLE_ID, "Cursor", 20_000),
            turn_end("s1", "s1:1", 60_000),
        ];
        let summary = compute_summary(&events, 0, 120_000);
        assert_eq!(summary.premature_checks, 1);
        assert!(summary.longest_autonomous_ms.is_none());
    }

    #[test]
    fn return_latency_zero_when_already_on_cursor() {
        let events = vec![
            focus(CURSOR_BUNDLE_ID, "Cursor", 0),
            turn_start("s1", "s1:1", 1000),
            turn_end("s1", "s1:1", 5000),
        ];
        let summary = compute_summary(&events, 0, 10_000);
        assert_eq!(summary.return_latency_median_ms, Some(0));
    }

    #[test]
    fn switch_while_no_agent_not_counted() {
        let events = vec![
            focus("com.apple.Safari", "Safari", 0),
            focus("com.apple.Terminal", "Terminal", 1000),
            turn_start("s1", "s1:1", 5000),
            turn_end("s1", "s1:1", 10_000),
        ];
        let summary = compute_summary(&events, 0, 20_000);
        assert_eq!(summary.context_switches_during_agent, 0);
    }

    #[test]
    fn river_one_chrome_focus_fills_window() {
        let events = vec![focus("com.google.Chrome", "Chrome", 0)];
        let river = compute_river(&events, 0, 60_000);
        assert_eq!(river.lanes.len(), 1);
        assert_eq!(river.lanes[0].app_name, "Chrome");
        assert!(!river.lanes[0].is_cursor);
        assert_eq!(
            river.lanes[0].segments,
            vec![RiverSegment {
                start_ms: 0,
                end_ms: 60_000
            }]
        );
        assert!(river.agent_bands.is_empty());
        assert!(river.markers.is_empty());
    }

    #[test]
    fn river_safari_ends_where_chrome_starts() {
        let events = vec![
            focus("com.apple.Safari", "Safari", 0),
            focus("com.google.Chrome", "Chrome", 30_000),
        ];
        let river = compute_river(&events, 0, 60_000);
        assert_eq!(river.lanes.len(), 2);
        let safari = river.lanes.iter().find(|l| l.app_name == "Safari").unwrap();
        let chrome = river.lanes.iter().find(|l| l.app_name == "Chrome").unwrap();
        assert_eq!(safari.segments[0].end_ms, 30_000);
        assert_eq!(chrome.segments[0].start_ms, 30_000);
    }

    #[test]
    fn river_two_overlapping_turns_are_two_bands() {
        let events = vec![
            turn_start("a", "a:1", 0),
            turn_start("b", "b:1", 0),
            turn_end("a", "a:1", 600_000),
            turn_end("b", "b:1", 600_000),
        ];
        let river = compute_river(&events, 0, 600_000);
        assert_eq!(river.agent_bands.len(), 2);
        assert!(river.agent_bands.iter().all(|b| b.pieces.len() == 1));
        assert!(river.agent_bands.iter().all(|b| !b.open));
    }

    #[test]
    fn river_chrome_away_and_cursor_present_split_pieces() {
        let events = vec![
            turn_start("s1", "s1:1", 0),
            focus("com.google.Chrome", "Chrome", 10_000),
            focus(CURSOR_BUNDLE_ID, "Cursor", 20_000),
        ];
        let river = compute_river(&events, 0, 60_000);
        let band = &river.agent_bands[0];
        assert!(band.open);
        assert!(band.pieces.iter().any(|p| p.autonomous && p.start_ms == 10_000));
        assert!(band
            .pieces
            .iter()
            .any(|p| !p.autonomous && p.start_ms == 20_000));
    }

    #[test]
    fn river_return_while_open_is_premature_only() {
        let events = vec![
            focus(CURSOR_BUNDLE_ID, "Cursor", 0),
            turn_start("s1", "s1:1", 1_000),
            focus("com.google.Chrome", "Chrome", 10_000),
            focus(CURSOR_BUNDLE_ID, "Cursor", 20_000),
        ];
        let river = compute_river(&events, 0, 60_000);
        assert_eq!(
            river
                .markers
                .iter()
                .filter(|m| m.kind == "premature_check")
                .count(),
            1
        );
        assert!(river
            .markers
            .iter()
            .all(|m| m.kind != "return_after_finish"));
    }

    #[test]
    fn river_return_after_finish() {
        let events = vec![
            focus("com.google.Chrome", "Chrome", 0),
            turn_start("s1", "s1:1", 0),
            turn_end("s1", "s1:1", 30_000),
            focus(CURSOR_BUNDLE_ID, "Cursor", 40_000),
        ];
        let river = compute_river(&events, 0, 80_000);
        assert_eq!(river.markers.len(), 1);
        assert_eq!(river.markers[0].kind, "return_after_finish");
        assert_eq!(river.markers[0].timestamp, 40_000);
    }

    #[test]
    fn river_already_on_cursor_has_no_return_marker() {
        let events = vec![
            focus(CURSOR_BUNDLE_ID, "Cursor", 0),
            turn_start("s1", "s1:1", 1_000),
            turn_end("s1", "s1:1", 5_000),
        ];
        let river = compute_river(&events, 0, 10_000);
        assert!(river.markers.is_empty());
    }

    #[test]
    fn river_seven_apps_collapse_shortest_into_other() {
        let mut events = Vec::new();
        let mut t = 0i64;
        for i in 1..=7 {
            events.push(focus(&format!("app.{i}"), &format!("App{i}"), t));
            t += i * 10_000;
        }
        let river = compute_river(&events, 0, t);
        assert_eq!(river.lanes.len(), 6);
        let other = river.lanes.iter().find(|l| l.app_name == "Other").unwrap();
        let other_ms: i64 = other.segments.iter().map(|s| s.end_ms - s.start_ms).sum();
        assert_eq!(other_ms, 10_000 + 20_000);
        assert!(river.lanes.iter().all(|l| l.app_name != "App1" && l.app_name != "App2"));
        assert_eq!(river.lanes.last().unwrap().app_name, "Other");
    }

    #[test]
    fn river_comma_cursor_joins_real_cursor_lane() {
        let events = vec![
            focus("missing value,", ", Cursor", 0),
            focus(CURSOR_BUNDLE_ID, "Cursor", 30_000),
        ];
        let river = compute_river(&events, 0, 60_000);
        assert_eq!(river.lanes.len(), 1);
        assert_eq!(river.lanes[0].app_name, "Cursor");
        assert!(river.lanes[0].is_cursor);
        assert_eq!(river.lanes[0].segments.len(), 2);
    }

    #[test]
    fn river_same_label_turns_share_a_band_when_sequential() {
        let events = vec![
            turn_start("s1", "s1:1", 0),
            turn_end("s1", "s1:1", 10_000),
            turn_start("s1", "s1:2", 20_000),
            turn_end("s1", "s1:2", 30_000),
        ];
        let river = compute_river(&events, 0, 40_000);
        assert_eq!(river.agent_bands.len(), 1);
        assert_eq!(river.agent_bands[0].pieces.len(), 2);
        assert!(!river.agent_bands[0].open);
    }

    #[test]
    fn river_overlapping_same_label_turns_stay_separate() {
        let events = vec![
            turn_start("a", "a:1", 0),
            turn_start("b", "b:1", 5_000),
            turn_end("a", "a:1", 15_000),
            turn_end("b", "b:1", 20_000),
        ];
        let river = compute_river(&events, 0, 30_000);
        assert_eq!(river.agent_bands.len(), 2);
        assert!(river.agent_bands.iter().all(|b| b.label == "p · t"));
    }

    #[test]
    fn river_hidden_is_own_lane_and_autonomous() {
        let events = vec![
            turn_start("s1", "s1:1", 0),
            focus("com.1password.1password", "Hidden", 0),
        ];
        let river = compute_river(&events, 0, 30_000);
        assert_eq!(river.lanes.len(), 1);
        assert_eq!(river.lanes[0].app_name, "Hidden");
        assert!(!river.lanes[0].is_cursor);
        assert!(river.agent_bands[0].pieces.iter().any(|p| p.autonomous));
    }

    #[test]
    fn hidden_app_still_counts_as_switch() {
        let events = vec![
            turn_start("s1", "s1:1", 0),
            focus(CURSOR_BUNDLE_ID, "Cursor", 1000),
            focus("com.1password.1password", "Hidden", 2000),
            focus(CURSOR_BUNDLE_ID, "Cursor", 3000),
            turn_end("s1", "s1:1", 10_000),
        ];
        let summary = compute_summary(&events, 0, 20_000);
        assert!(summary.context_switches_during_agent >= 1);
    }
}
