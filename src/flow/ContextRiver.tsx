import { useEffect, useRef, useState, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { formatDuration } from "./format";
import type { FlowRiver, RiverAgentBand, RiverSegment } from "./types";

const PLOT_LEFT = 176;
const PLOT_RIGHT = 952;
const PAD_TOP = 28;
const PAD_BOTTOM = 8;
const APP_STRIDE = 28;
const APP_HEIGHT = 22;
const AGENT_GAP = 14;
const AGENT_STRIDE = 24;
const AGENT_HEIGHT = 18;
const MIN_SPAN_MS = 2 * 60 * 1000;

function xAt(t: number, start: number, end: number): number {
  const span = end - start;
  if (span <= 0) return PLOT_LEFT;
  return PLOT_LEFT + ((t - start) / span) * (PLOT_RIGHT - PLOT_LEFT);
}

function ellipsize(name: string): string {
  if (name.length <= 22) return name;
  return `${name.slice(0, 21)}…`;
}

function tickStep(span: number): number {
  if (span > 6 * 60 * 60 * 1000) return 60 * 60 * 1000;
  if (span > 2 * 60 * 60 * 1000) return 30 * 60 * 1000;
  if (span > 30 * 60 * 1000) return 10 * 60 * 1000;
  if (span > 10 * 60 * 1000) return 60 * 1000;
  return 30 * 1000;
}

function ticksFor(start: number, end: number): number[] {
  const step = tickStep(end - start);
  const first = Math.ceil(start / step) * step;
  const ticks: number[] = [];
  for (let t = first; t < end; t += step) {
    if (t > start) ticks.push(t);
  }
  return ticks;
}

function formatHour(ts: number): string {
  return new Date(ts).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function cursorSegmentText(segment: RiverSegment, bands: RiverAgentBand[]): string {
  const duration = formatDuration(segment.end_ms - segment.start_ms);
  const labels = [
    ...new Set(
      bands
        .filter((band) =>
          band.pieces.some(
            (piece) => piece.start_ms < segment.end_ms && piece.end_ms > segment.start_ms,
          ),
        )
        .map((band) => band.label),
    ),
  ];
  if (labels.length === 0) return `Cursor · ${duration}`;
  return `${labels.join(", ")} · ${duration}`;
}

function showBarTip(
  event: MouseEvent<SVGRectElement>,
  text: string,
  setTip: (tip: { text: string; top: number; left: number } | null) => void,
) {
  const rect = event.currentTarget.getBoundingClientRect();
  setTip({
    text,
    top: rect.bottom + 8,
    left: Math.max(12, Math.min(rect.left, window.innerWidth - 280)),
  });
}

function clipInterval(
  start: number,
  end: number,
  viewStart: number,
  viewEnd: number,
): { start: number; end: number } | null {
  const clippedStart = Math.max(start, viewStart);
  const clippedEnd = Math.min(end, viewEnd);
  if (clippedEnd <= clippedStart) return null;
  return { start: clippedStart, end: clippedEnd };
}

export function ContextRiver({ river }: { river: FlowRiver }) {
  const svgRef = useRef<SVGSVGElement>(null);
  const viewRef = useRef({ start: river.range_start_ms, end: river.range_end_ms });
  const dragRef = useRef<{ x: number; start: number; end: number } | null>(null);
  const [view, setView] = useState({
    start: river.range_start_ms,
    end: river.range_end_ms,
  });
  const [dragging, setDragging] = useState(false);
  const [tip, setTip] = useState<{ text: string; top: number; left: number } | null>(null);

  useEffect(() => {
    const next = { start: river.range_start_ms, end: river.range_end_ms };
    viewRef.current = next;
    setView(next);
  }, [river.range_start_ms, river.range_end_ms]);

  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  const zoomBy = (factor: number) => {
    const current = viewRef.current;
    const span = current.end - current.start;
    const full = river.range_end_ms - river.range_start_ms;
    if (full <= 0 || span <= 0) return;
    const nextSpan = Math.max(MIN_SPAN_MS, Math.min(full, span * factor));
    const anchor = current.start + span / 2;
    let nextStart = anchor - nextSpan / 2;
    let nextEnd = nextStart + nextSpan;
    if (nextStart < river.range_start_ms) {
      nextStart = river.range_start_ms;
      nextEnd = Math.min(river.range_end_ms, nextStart + nextSpan);
    }
    if (nextEnd > river.range_end_ms) {
      nextEnd = river.range_end_ms;
      nextStart = Math.max(river.range_start_ms, nextEnd - nextSpan);
    }
    const next = { start: nextStart, end: nextEnd };
    viewRef.current = next;
    setView(next);
  };

  if (river.lanes.length === 0 && river.agent_bands.length === 0) {
    return null;
  }

  const { start, end } = view;
  const appCount = river.lanes.length;
  const agentCount = river.agent_bands.length;
  const height =
    PAD_TOP + appCount * APP_STRIDE + AGENT_GAP + agentCount * AGENT_STRIDE + PAD_BOTTOM;
  const agentTop = PAD_TOP + appCount * APP_STRIDE + AGENT_GAP;
  const cursorIndex = river.lanes.findIndex((lane) => lane.is_cursor);
  const cursorY =
    cursorIndex >= 0 ? PAD_TOP + cursorIndex * APP_STRIDE + APP_HEIGHT / 2 : null;
  const tickList = ticksFor(start, end);

  const panTo = (clientX: number) => {
    const drag = dragRef.current;
    const svg = svgRef.current;
    if (!drag || !svg) return;
    const rect = svg.getBoundingClientRect();
    const dx = ((clientX - drag.x) / rect.width) * 960;
    const span = drag.end - drag.start;
    const shift = (dx / (PLOT_RIGHT - PLOT_LEFT)) * span;
    let nextStart = drag.start - shift;
    let nextEnd = drag.end - shift;
    if (nextStart < river.range_start_ms) {
      nextStart = river.range_start_ms;
      nextEnd = nextStart + span;
    }
    if (nextEnd > river.range_end_ms) {
      nextEnd = river.range_end_ms;
      nextStart = nextEnd - span;
    }
    const next = { start: nextStart, end: nextEnd };
    viewRef.current = next;
    setView(next);
  };

  return (
    <div className="flow-river-wrap">
      <div className="flow-river-caption-row">
        <p className="flow-river-caption">
          Today · {formatHour(start)}–{formatHour(end)}
        </p>
        <div className="flow-river-zoom">
          <button type="button" aria-label="Zoom out" onClick={() => zoomBy(1.25)}>
            −
          </button>
          <button type="button" aria-label="Zoom in" onClick={() => zoomBy(0.8)}>
            +
          </button>
        </div>
      </div>
      <svg
        ref={svgRef}
        className={dragging ? "flow-river flow-river-dragging" : "flow-river"}
        viewBox={`0 0 960 ${height}`}
        width="100%"
        onDoubleClick={() => {
          const next = { start: river.range_start_ms, end: river.range_end_ms };
          viewRef.current = next;
          setView(next);
        }}
        onPointerDown={(event) => {
          dragRef.current = { x: event.clientX, start, end };
          setDragging(true);
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => panTo(event.clientX)}
        onPointerUp={() => {
          dragRef.current = null;
          setDragging(false);
        }}
      >
        {tickList.map((tick, index) => (
          <text
            key={tick}
            className="flow-river-tick"
            x={xAt(tick, start, end)}
            y={16}
            textAnchor={index === 0 ? "start" : "middle"}
          >
            {formatHour(tick)}
          </text>
        ))}
        {river.lanes.map((lane) => {
          const index = river.lanes.indexOf(lane);
          const top = PAD_TOP + index * APP_STRIDE;
          const segmentClass = lane.is_cursor
            ? "flow-river-segment flow-river-segment-cursor"
            : lane.app_name === "Other"
              ? "flow-river-segment flow-river-segment-other"
              : "flow-river-segment";
          return (
            <g key={lane.app_name}>
              <text className="flow-river-label" x={168} y={top + 15} textAnchor="end">
                <title>{lane.app_name}</title>
                {ellipsize(lane.app_name)}
              </text>
              {lane.segments.map((segment) => {
                const visible = clipInterval(segment.start_ms, segment.end_ms, start, end);
                if (!visible) return null;
                const x = xAt(visible.start, start, end);
                const width = Math.max(1, xAt(visible.end, start, end) - x);
                return (
                  <rect
                    key={`${segment.start_ms}-${segment.end_ms}`}
                    className={segmentClass}
                    x={x}
                    y={top + (APP_HEIGHT - 8) / 2}
                    width={width}
                    height={8}
                    rx={2}
                    onMouseEnter={
                      lane.is_cursor
                        ? (event) =>
                            showBarTip(
                              event,
                              cursorSegmentText(segment, river.agent_bands),
                              setTip,
                            )
                        : undefined
                    }
                    onMouseLeave={lane.is_cursor ? () => setTip(null) : undefined}
                  />
                );
              })}
            </g>
          );
        })}
        {river.agent_bands.map((band, index) => {
          const top = agentTop + index * AGENT_STRIDE;
          const last = band.pieces[band.pieces.length - 1];
          const checkX = last ? xAt(last.end_ms, start, end) + 4 : PLOT_RIGHT;
          const showCheck = !band.open && last && last.end_ms >= start && last.end_ms <= end;
          return (
            <g key={`${band.label}-${index}`}>
              <text className="flow-river-label" x={168} y={top + 13} textAnchor="end">
                <title>{band.label}</title>
                {ellipsize(band.label)}
              </text>
              {band.pieces.map((piece) => {
                const visible = clipInterval(piece.start_ms, piece.end_ms, start, end);
                if (!visible) return null;
                const x = xAt(visible.start, start, end);
                const width = Math.max(1, xAt(visible.end, start, end) - x);
                const bar = piece.autonomous ? 8 : 2;
                return (
                  <rect
                    key={`${piece.start_ms}-${piece.end_ms}-${piece.autonomous}`}
                    className={
                      piece.autonomous
                        ? "flow-river-piece flow-river-piece-away"
                        : "flow-river-piece"
                    }
                    x={x}
                    y={top + (AGENT_HEIGHT - bar) / 2}
                    width={width}
                    height={bar}
                    rx={1}
                    onMouseEnter={(event) =>
                      showBarTip(
                        event,
                        `${band.label} · ${formatDuration(piece.end_ms - piece.start_ms)}${piece.autonomous ? " · away" : ""}`,
                        setTip,
                      )
                    }
                    onMouseLeave={() => setTip(null)}
                  />
                );
              })}
              {showCheck && checkX + 6 <= PLOT_RIGHT && (
                <text className="flow-river-check" x={checkX} y={top + 13}>
                  ✓
                </text>
              )}
            </g>
          );
        })}
        {cursorY != null &&
          river.markers.map((marker) => {
            if (marker.timestamp < start || marker.timestamp > end) return null;
            return (
              <circle
                key={`${marker.kind}-${marker.timestamp}`}
                className={
                  marker.kind === "premature_check"
                    ? "flow-river-marker flow-river-marker-check"
                    : "flow-river-marker flow-river-marker-return"
                }
                cx={xAt(marker.timestamp, start, end)}
                cy={cursorY}
                r={3.5}
              >
                <title>
                  {marker.kind === "premature_check" ? "Premature check" : "Return"} ·{" "}
                  {formatHour(marker.timestamp)}
                </title>
              </circle>
            );
          })}
      </svg>
      {tip &&
        createPortal(
          <div className="flow-tooltip" style={{ top: tip.top, left: tip.left, visibility: "visible", opacity: 1 }} role="tooltip">
            {tip.text}
          </div>,
          document.body,
        )}
    </div>
  );
}
