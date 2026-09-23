export const METRIC_HELP = {
  turns: "Completed cycles: agent started working, then finished.",
  sessions: "Cursor agent sessions first detected today.",
  medianReturnLatency:
    "Typical time from when an agent finished until you focused Cursor again. Zero if you were already there.",
  p90ReturnLatency:
    "Slow returns: 90% of measured returns were faster than this. Same definition as median return.",
  prematureChecks:
    "Times you focused Cursor while an agent was still working after being in another app.",
  notYetReturned:
    "Finished turns where you had not returned to Cursor before the next prompt or end of the day.",
  maxConcurrent: "Most agents running at the same moment today.",
  concurrencyOne:
    "Share of wall-clock agent time with exactly one agent running.",
  concurrencyTwo:
    "Share of wall-clock agent time with two agents running at once.",
  concurrencyThree:
    "Share of wall-clock agent time with three agents running at once.",
  concurrencyFourPlus:
    "Share of wall-clock agent time with four or more agents running at once.",
  turnDurationSection:
    "How long completed turns lasted, grouped into buckets.",
} as const;
