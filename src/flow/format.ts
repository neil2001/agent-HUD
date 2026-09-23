export function formatDuration(ms: number): string {
  if (ms < 1000) {
    return `${ms}ms`;
  }
  const totalSec = Math.round(ms / 1000);
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) {
    return `${h}h ${m}m`;
  }
  if (m > 0) {
    return s > 0 ? `${m}m ${s}s` : `${m}m`;
  }
  return `${s}s`;
}

export function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function formatTurnsPerHour(value: number | null): string {
  if (value == null || !Number.isFinite(value)) return "—";
  if (value >= 10) return String(Math.round(value));
  const rounded = Math.round(value * 10) / 10;
  if (rounded === 0) return value > 0 ? "<0.1" : "0";
  return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
}

export function formatViewSpan(ms: number): string {
  const minutes = Math.max(0, Math.round(ms / 60000));
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  const remainder = minutes % 60;
  if (remainder === 0) return `${hours} hr`;
  if (remainder === 30) return `${hours}.5 hr`;
  return `${hours} hr ${remainder} min`;
}

export function formatPercent(value: number): string {
  return `${Math.round(value * 100)}%`;
}

export function formatRelative(ts: number, now = Date.now()): string {
  const minutes = Math.floor(Math.max(0, now - ts) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  return `${months}mo ago`;
}

export type AttentionSpan = "1d" | "7d" | "14d" | "30d";

export const ATTENTION_SPANS: ReadonlyArray<readonly [AttentionSpan, string]> = [
  ["1d", "1 day"],
  ["7d", "7 days"],
  ["14d", "14 days"],
  ["30d", "30 days"],
];

export function spanLengthDays(span: AttentionSpan): number {
  switch (span) {
    case "1d":
      return 1;
    case "7d":
      return 7;
    case "14d":
      return 14;
    case "30d":
      return 30;
  }
}

function startOfLocalDay(now: number): Date {
  const today = new Date(now);
  return new Date(today.getFullYear(), today.getMonth(), today.getDate());
}

function addDays(date: Date, days: number): Date {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}

export function attentionBoundsMs(
  span: AttentionSpan,
  stepsBack = 0,
  now = Date.now(),
): [number, number] {
  const days = spanLengthDays(span);
  const latestStart = addDays(startOfLocalDay(now), -(days - 1));
  const start = addDays(latestStart, -stepsBack * days);
  if (stepsBack === 0) return [start.getTime(), now];
  return [start.getTime(), addDays(start, days).getTime()];
}

export function dayBoundsMs(stepsBack = 0, now = Date.now()): [number, number] {
  const start = addDays(startOfLocalDay(now), -stepsBack);
  const next = addDays(start, 1).getTime();
  const end = stepsBack === 0 ? Math.min(next, now) : next;
  return [start.getTime(), end];
}

function formatMonthDay(date: Date): string {
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}

export function formatDayLabel(startMs: number, now = Date.now()): string {
  const start = startOfLocalDay(now).getTime();
  if (startMs >= start) return "Today";
  return formatMonthDay(new Date(startMs));
}

export function formatAttentionRange(
  startMs: number,
  endMs: number,
  now = Date.now(),
): string {
  const start = new Date(startMs);
  const endDay = new Date(endMs - 1);
  const sameDay =
    start.getFullYear() === endDay.getFullYear() &&
    start.getMonth() === endDay.getMonth() &&
    start.getDate() === endDay.getDate();
  if (sameDay) return formatDayLabel(startMs, now);
  return `${formatMonthDay(start)}–${formatMonthDay(endDay)}`;
}

export function todayBoundsMs(): [number, number] {
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const end = new Date(start);
  end.setDate(end.getDate() + 1);
  return [start.getTime(), end.getTime()];
}
