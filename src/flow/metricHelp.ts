export const METRIC_HELP = {
  turnsPerHour:
    "Completed turns divided by the hours from your first turn today until now.",
  prematureChecks:
    "Times you focused Cursor while an agent was still working after being in another app.",
  notYetReturned:
    "Finished turns where you had not returned to Cursor before the next prompt or end of the day.",
  attentionFocused:
    "Time an app was in front during this window.",
  attentionSessions:
    "Cursor chats that started a turn in this window. A chat under Cursor counts only while Cursor was in front and that chat's turn was open, so overlapping chats can add up to more than the Cursor bar.",
} as const;
