export const METRIC_HELP = {
  turnsPerHour:
    "Completed turns divided by the hours from your first turn today until now.",
  meanTurnGap: "Average time from the start of one turn to the start of the next.",
  returnLatency:
    "How long after an agent finished until you focused Cursor again. Already being there counts as under a minute.",
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
