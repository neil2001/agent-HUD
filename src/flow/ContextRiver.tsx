import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { formatDuration, formatViewSpan } from "./format";
import type { FlowRiver, RiverAgentBand, RiverLane, RiverPage, RiverSegment } from "./types";

const PLOT_LEFT = 176;
const PLOT_RIGHT = 952;
const LABEL_X = 168;
const NEST_X = 172;
const PAD_TOP = 28;
const PAD_BOTTOM = 8;
const APP_STRIDE = 28;
const APP_HEIGHT = 22;
const SECTION_STRIDE = 15;
const CHILD_STRIDE = 18;
const GROUP_PAD = 6;
const ORPHAN_GAP = 10;
const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
const MIN_SPAN_MS = 2 * MINUTE_MS;
const SPAN_STEPS_MS = [
  2 * MINUTE_MS,
  5 * MINUTE_MS,
  10 * MINUTE_MS,
  15 * MINUTE_MS,
  30 * MINUTE_MS,
  45 * MINUTE_MS,
  HOUR_MS,
  1.5 * HOUR_MS,
  2 * HOUR_MS,
  3 * HOUR_MS,
  4 * HOUR_MS,
  6 * HOUR_MS,
  8 * HOUR_MS,
  12 * HOUR_MS,
];

function defaultRiverView(rangeStart: number, rangeEnd: number) {
  const full = Math.max(0, rangeEnd - rangeStart);
  const span = Math.min(HOUR_MS, full);
  return { start: rangeEnd - span, end: rangeEnd };
}

function stepSpan(current: number, zoomIn: boolean, full: number): number {
  if (full <= 0) return current;
  const span = Math.min(current, full);
  if (zoomIn) {
    for (let index = SPAN_STEPS_MS.length - 1; index >= 0; index -= 1) {
      if (SPAN_STEPS_MS[index] < span - 500) return SPAN_STEPS_MS[index];
    }
    return Math.min(span, MIN_SPAN_MS);
  }
  for (const step of SPAN_STEPS_MS) {
    if (step > span + 500 && step < full - 500) return step;
  }
  return full;
}
const SECOND_PRECISION_SPAN_MS = 10 * 60 * 1000;
const TIP_PAD = 8;
const TIP_GAP = 6;

type BarTip = {
  startMs: number;
  durationMs: number;
  detail?: string;
  clientX: number;
  anchorTop: number;
  anchorBottom: number;
};

type RiverLayout = {
  laneTops: number[];
  pageTops: number[][];
  tabsHeaderTops: Array<number | null>;
  sessionsHeaderTop: number | null;
  bandTops: number[];
  visibleBands: RiverAgentBand[];
  visiblePages: RiverPage[][];
  cursorY: number | null;
  height: number;
};

function xAt(t: number, start: number, end: number): number {
  const span = end - start;
  if (span <= 0) return PLOT_LEFT;
  return PLOT_LEFT + ((t - start) / span) * (PLOT_RIGHT - PLOT_LEFT);
}

function ellipsize(name: string, max = 22): string {
  if (name.length <= max) return name;
  return `${name.slice(0, max - 1)}…`;
}

function overlapsView(start: number, end: number, viewStart: number, viewEnd: number): boolean {
  return end > viewStart && start < viewEnd;
}

function childName(label: string): string {
  const mark = " · ";
  const split = label.indexOf(mark);
  const name = split >= 0 ? label.slice(split + mark.length).trim() : label;
  return ellipsize(name || label, 28);
}

const TICK_STEPS_MS = [
  30 * 1000,
  60 * 1000,
  2 * 60 * 1000,
  5 * 60 * 1000,
  10 * 60 * 1000,
  15 * 60 * 1000,
  30 * 60 * 1000,
  60 * 60 * 1000,
  2 * 60 * 60 * 1000,
  3 * 60 * 60 * 1000,
  6 * 60 * 60 * 1000,
];
const MAX_AXIS_TICKS = 6;

function tickStep(span: number): number {
  for (const step of TICK_STEPS_MS) {
    if (span / step <= MAX_AXIS_TICKS) return step;
  }
  return TICK_STEPS_MS[TICK_STEPS_MS.length - 1];
}

function formatTick(ts: number, step: number): string {
  const options: Intl.DateTimeFormatOptions =
    step < 60 * 1000
      ? { hour: "2-digit", minute: "2-digit", second: "2-digit" }
      : { hour: "2-digit", minute: "2-digit" };
  return new Date(ts).toLocaleTimeString([], options);
}

function ticksFor(start: number, end: number): number[] {
  const span = end - start;
  if (span <= 0) return [];
  const step = tickStep(span);
  const labelWidth = formatTick(start, step).length * 6.6;
  const first = Math.ceil(start / step) * step;
  const ticks: number[] = [];
  let previousRight = -Infinity;
  for (let t = first; t < end; t += step) {
    if (t <= start) continue;
    const x = xAt(t, start, end);
    const left = x - labelWidth / 2;
    const right = x + labelWidth / 2;
    if (left < PLOT_LEFT - 2 || right > PLOT_RIGHT + 8) continue;
    if (left < previousRight + 12) continue;
    ticks.push(t);
    previousRight = right;
  }
  return ticks;
}

function formatHour(ts: number): string {
  return new Date(ts).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatBarStart(ts: number, viewSpan: number): string {
  const options: Intl.DateTimeFormatOptions =
    viewSpan <= SECOND_PRECISION_SPAN_MS
      ? { hour: "2-digit", minute: "2-digit", second: "2-digit" }
      : { hour: "2-digit", minute: "2-digit" };
  return new Date(ts).toLocaleTimeString([], options);
}

function sessionLabelsDuring(segment: RiverSegment, bands: RiverAgentBand[]): string | undefined {
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
  if (labels.length === 0) return undefined;
  return labels.join(", ");
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

function visibleBands(bands: RiverAgentBand[], viewStart: number, viewEnd: number): RiverAgentBand[] {
  return bands.filter((band) =>
    band.pieces.some((piece) => overlapsView(piece.start_ms, piece.end_ms, viewStart, viewEnd)),
  );
}

function visiblePages(pages: RiverPage[], viewStart: number, viewEnd: number): RiverPage[] {
  return pages.filter((page) =>
    page.segments.some((segment) => overlapsView(segment.start_ms, segment.end_ms, viewStart, viewEnd)),
  );
}

function layoutRiver(
  lanes: RiverLane[],
  bands: RiverAgentBand[],
  viewStart: number,
  viewEnd: number,
): RiverLayout {
  const shownBands = visibleBands(bands, viewStart, viewEnd);
  const shownPages = lanes.map((lane) => visiblePages(lane.pages ?? [], viewStart, viewEnd));
  const laneTops: number[] = [];
  const pageTops: number[][] = [];
  const tabsHeaderTops: Array<number | null> = [];
  const bandTops: number[] = [];
  let sessionsHeaderTop: number | null = null;
  let cursorY: number | null = null;
  let y = PAD_TOP;
  const cursorIndex = lanes.findIndex((lane) => lane.is_cursor);
  const nestBands = cursorIndex >= 0 && shownBands.length > 0;

  const placeBands = () => {
    sessionsHeaderTop = y;
    y += SECTION_STRIDE;
    for (const _band of shownBands) {
      bandTops.push(y);
      y += CHILD_STRIDE;
    }
    y += GROUP_PAD;
  };

  lanes.forEach((lane, index) => {
    laneTops.push(y);
    if (lane.is_cursor && cursorY == null) cursorY = y + APP_HEIGHT / 2;
    y += APP_STRIDE;

    if (nestBands && index === cursorIndex) placeBands();

    const pages = shownPages[index];
    if (pages.length > 0) {
      tabsHeaderTops.push(y);
      y += SECTION_STRIDE;
      const tops: number[] = [];
      for (const _page of pages) {
        tops.push(y);
        y += CHILD_STRIDE;
      }
      pageTops.push(tops);
      y += GROUP_PAD;
    } else {
      tabsHeaderTops.push(null);
      pageTops.push([]);
    }
  });

  if (!nestBands && shownBands.length > 0) {
    y += ORPHAN_GAP;
    placeBands();
  }

  return {
    laneTops,
    pageTops,
    tabsHeaderTops,
    sessionsHeaderTop,
    bandTops,
    visibleBands: shownBands,
    visiblePages: shownPages,
    cursorY,
    height: y + PAD_BOTTOM,
  };
}

function NestCaption({
  label,
  headerTop,
  lastTop,
}: {
  label: string;
  headerTop: number;
  lastTop: number;
}) {
  return (
    <>
      <line
        className="flow-river-nest"
        x1={NEST_X}
        y1={headerTop + 2}
        x2={NEST_X}
        y2={lastTop + 11}
      />
      <text
        className="flow-river-label flow-river-section-label"
        x={LABEL_X}
        y={headerTop + 10}
        textAnchor="end"
      >
        {label}
      </text>
    </>
  );
}

function AgentBands({
  bands,
  tops,
  start,
  end,
  showBarTip,
  dismissTip,
}: {
  bands: RiverAgentBand[];
  tops: number[];
  start: number;
  end: number;
  showBarTip: (
    event: MouseEvent<SVGRectElement>,
    startMs: number,
    endMs: number,
    detail?: string,
  ) => void;
  dismissTip: () => void;
}) {
  return (
    <>
      {bands.map((band, index) => {
        const top = tops[index];
        const last = band.pieces[band.pieces.length - 1];
        const checkX = last ? xAt(last.end_ms, start, end) + 4 : PLOT_RIGHT;
        const showCheck = !band.open && last && last.end_ms >= start && last.end_ms <= end;
        return (
          <g key={`${band.label}-${index}`}>
            <text
              className="flow-river-label flow-river-child-label"
              x={LABEL_X}
              y={top + 12}
              textAnchor="end"
            >
              <title>{band.label}</title>
              {childName(band.label)}
            </text>
            {band.pieces.map((piece) => {
              const visible = clipInterval(piece.start_ms, piece.end_ms, start, end);
              if (!visible) return null;
              const x = xAt(visible.start, start, end);
              const width = Math.max(1, xAt(visible.end, start, end) - x);
                const bar = piece.autonomous ? 6 : 2;
                return (
                  <rect
                    key={`${piece.start_ms}-${piece.end_ms}-${piece.autonomous}`}
                    className={
                      piece.autonomous
                        ? "flow-river-piece flow-river-piece-away"
                        : "flow-river-piece"
                    }
                    x={x}
                    y={top + (CHILD_STRIDE - bar) / 2 - 4}
                  width={width}
                  height={bar}
                  rx={1}
                  onMouseEnter={(event) =>
                    showBarTip(
                      event,
                      piece.start_ms,
                      piece.end_ms,
                      piece.autonomous ? "Away" : undefined,
                    )
                  }
                  onMouseLeave={dismissTip}
                />
              );
            })}
            {showCheck && checkX + 6 <= PLOT_RIGHT && (
              <text className="flow-river-check" x={checkX} y={top + 12}>
                ✓
              </text>
            )}
          </g>
        );
      })}
    </>
  );
}

function placeTip(tip: BarTip, width: number, height: number): { top: number; left: number } {
  let left = Math.min(tip.clientX, window.innerWidth - width - TIP_PAD);
  left = Math.max(TIP_PAD, left);
  let top = tip.anchorBottom + TIP_GAP;
  if (top + height > window.innerHeight - TIP_PAD) {
    top = tip.anchorTop - height - TIP_GAP;
  }
  top = Math.max(TIP_PAD, Math.min(top, window.innerHeight - height - TIP_PAD));
  return { top, left };
}

export function ContextRiver({ river }: { river: FlowRiver }) {
  const svgRef = useRef<SVGSVGElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef(defaultRiverView(river.range_start_ms, river.range_end_ms));
  const dragRef = useRef<{ x: number; start: number; end: number } | null>(null);
  const [view, setView] = useState(() =>
    defaultRiverView(river.range_start_ms, river.range_end_ms),
  );
  const [dragging, setDragging] = useState(false);
  const [tip, setTip] = useState<BarTip | null>(null);
  const [tipPos, setTipPos] = useState<{ top: number; left: number } | null>(null);

  useEffect(() => {
    const next = defaultRiverView(river.range_start_ms, river.range_end_ms);
    viewRef.current = next;
    setView(next);
  }, [river.range_start_ms, river.range_end_ms]);

  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  useLayoutEffect(() => {
    const panel = tipRef.current;
    if (!tip || !panel) return;
    const bounds = panel.getBoundingClientRect();
    setTipPos(placeTip(tip, bounds.width, bounds.height));
  }, [tip]);

  const dismissTip = () => {
    setTip(null);
    setTipPos(null);
  };

  const showBarTip = (
    event: MouseEvent<SVGRectElement>,
    startMs: number,
    endMs: number,
    detail?: string,
  ) => {
    const rect = event.currentTarget.getBoundingClientRect();
    setTipPos(null);
    setTip({
      startMs,
      durationMs: endMs - startMs,
      detail,
      clientX: event.clientX,
      anchorTop: rect.top,
      anchorBottom: rect.bottom,
    });
  };

  const zoomBy = (zoomIn: boolean) => {
    const current = viewRef.current;
    const span = current.end - current.start;
    const full = river.range_end_ms - river.range_start_ms;
    if (full <= 0 || span <= 0) return;
    const nextSpan = stepSpan(span, zoomIn, full);
    if (Math.abs(nextSpan - span) < 500) return;
    let nextEnd = current.end;
    let nextStart = nextEnd - nextSpan;
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
  const layout = layoutRiver(river.lanes, river.agent_bands, start, end);
  const tickList = ticksFor(start, end);
  const viewSpan = end - start;
  const cursorIndex = river.lanes.findIndex((lane) => lane.is_cursor);
  const nestBands = cursorIndex >= 0 && layout.visibleBands.length > 0;

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
          <button type="button" aria-label="Zoom out" onClick={() => zoomBy(false)}>
            −
          </button>
          <span className="flow-river-span">{formatViewSpan(viewSpan)}</span>
          <button type="button" aria-label="Zoom in" onClick={() => zoomBy(true)}>
            +
          </button>
        </div>
      </div>
      <svg
        ref={svgRef}
        className={dragging ? "flow-river flow-river-dragging" : "flow-river"}
        viewBox={`0 0 960 ${layout.height}`}
        width="100%"
        onDoubleClick={() => {
          const next = defaultRiverView(river.range_start_ms, river.range_end_ms);
          viewRef.current = next;
          setView(next);
        }}
        onPointerDown={(event) => {
          dismissTip();
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
        {tickList.map((tick) => (
          <text
            key={tick}
            className="flow-river-tick"
            x={xAt(tick, start, end)}
            y={16}
            textAnchor="middle"
          >
            {formatTick(tick, tickStep(viewSpan))}
          </text>
        ))}
        {river.lanes.map((lane, index) => {
          const top = layout.laneTops[index];
          const segmentClass = lane.is_cursor
            ? "flow-river-segment flow-river-segment-cursor"
            : lane.app_name === "Other"
              ? "flow-river-segment flow-river-segment-other"
              : "flow-river-segment";
          const tabsHeaderTop = layout.tabsHeaderTops[index];
          return (
            <g key={lane.app_name}>
              <text className="flow-river-label" x={LABEL_X} y={top + 15} textAnchor="end">
                <title>{lane.app_name}</title>
                {ellipsize(lane.app_name)}
              </text>
              {lane.segments.map((segment) => {
                const visible = clipInterval(segment.start_ms, segment.end_ms, start, end);
                if (!visible) return null;
                const x = xAt(visible.start, start, end);
                const width = Math.max(1, xAt(visible.end, start, end) - x);
                const detail = lane.is_cursor
                  ? sessionLabelsDuring(segment, river.agent_bands)
                  : undefined;
                return (
                  <rect
                    key={`${segment.start_ms}-${segment.end_ms}`}
                    className={segmentClass}
                    x={x}
                    y={top + (APP_HEIGHT - 8) / 2}
                    width={width}
                    height={8}
                    rx={2}
                    onMouseEnter={(event) =>
                      showBarTip(event, segment.start_ms, segment.end_ms, detail)
                    }
                    onMouseLeave={dismissTip}
                  />
                );
              })}
              {nestBands && index === cursorIndex && layout.sessionsHeaderTop != null && (
                <>
                  <NestCaption
                    label="Sessions"
                    headerTop={layout.sessionsHeaderTop}
                    lastTop={layout.bandTops[layout.bandTops.length - 1] ?? layout.sessionsHeaderTop}
                  />
                  <AgentBands
                    bands={layout.visibleBands}
                    tops={layout.bandTops}
                    start={start}
                    end={end}
                    showBarTip={showBarTip}
                    dismissTip={dismissTip}
                  />
                </>
              )}
              {tabsHeaderTop != null && (
                <NestCaption
                  label="Tabs"
                  headerTop={tabsHeaderTop}
                  lastTop={
                    layout.pageTops[index][layout.pageTops[index].length - 1] ?? tabsHeaderTop
                  }
                />
              )}
              {layout.visiblePages[index].map((page, pageIndex) => {
                const pageTop = layout.pageTops[index][pageIndex];
                const pageClass =
                  page.label === "Other tabs"
                    ? "flow-river-segment flow-river-segment-other"
                    : "flow-river-segment flow-river-segment-page";
                return (
                  <g key={`${page.label}-${pageIndex}`}>
                    <text
                      className="flow-river-label flow-river-child-label"
                      x={LABEL_X}
                      y={pageTop + 12}
                      textAnchor="end"
                    >
                      <title>{page.label}</title>
                      {ellipsize(page.label, 28)}
                    </text>
                    {page.segments.map((segment) => {
                      const visible = clipInterval(segment.start_ms, segment.end_ms, start, end);
                      if (!visible) return null;
                      const x = xAt(visible.start, start, end);
                      const width = Math.max(1, xAt(visible.end, start, end) - x);
                      return (
                        <rect
                          key={`${segment.start_ms}-${segment.end_ms}`}
                          className={pageClass}
                          x={x}
                          y={pageTop + 6}
                          width={width}
                          height={6}
                          rx={2}
                          onMouseEnter={(event) =>
                            showBarTip(event, segment.start_ms, segment.end_ms)
                          }
                          onMouseLeave={dismissTip}
                        />
                      );
                    })}
                  </g>
                );
              })}
            </g>
          );
        })}
        {!nestBands && layout.sessionsHeaderTop != null && (
          <>
            <NestCaption
              label="Sessions"
              headerTop={layout.sessionsHeaderTop}
              lastTop={layout.bandTops[layout.bandTops.length - 1] ?? layout.sessionsHeaderTop}
            />
            <AgentBands
              bands={layout.visibleBands}
              tops={layout.bandTops}
              start={start}
              end={end}
              showBarTip={showBarTip}
              dismissTip={dismissTip}
            />
          </>
        )}
        {layout.cursorY != null &&
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
                cy={layout.cursorY ?? 0}
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
          <div
            ref={tipRef}
            className="flow-river-tip"
            role="tooltip"
            style={
              tipPos
                ? { top: tipPos.top, left: tipPos.left, visibility: "visible" }
                : { visibility: "hidden" }
            }
          >
            <div className="flow-river-tip-time">{formatBarStart(tip.startMs, viewSpan)}</div>
            <div className="flow-river-tip-meta">{formatDuration(tip.durationMs)}</div>
            {tip.detail ? <div className="flow-river-tip-detail">{tip.detail}</div> : null}
          </div>,
          document.body,
        )}
    </div>
  );
}
