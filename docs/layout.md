# Layout

Two windows share one frontend bundle. The window label decides which tree mounts.

## HUD

The HUD is a borderless, always-on-top panel, 268px wide, anchored at the top center of the screen. It does not take keyboard focus. Height grows with the session list, up to eight visible rows ([`window.rs`](../src-tauri/src/window.rs)).

```text
┌ drag handle ─────────────┐
│ sessions  turns  opened  │  daily strip, plus the Agent Flow button
│ merged              ···  │
│ ● Cursor    api          │
│ ◐ Cursor    frontend     │  one row per active session
└──────────────────────────┘
```

The strip is today's counts: sessions prompted, turns finished, pull requests opened, pull requests merged. The button calls `open_flow`.

Each row is the whole hit target. It shows status, the Cursor mark, and enough of the project or title to tell sessions apart. Status order is needs-attention, working, completed, then waiting. Clicking focuses that Cursor session. The panel hides its list when nothing is active; the strip stays.

There is no navigation, settings, or chat in this window. That surface is specified in [ux-spec.md](ux-spec.md).

## Agent Flow

Agent Flow is a normal resizable window, 960×640, titled "Agent Flow". Opening it switches the app to a regular activation policy so it can sit in the foreground. Closing it returns to accessory.

The window is a single column:

```text
Agent Flow
Flight recorder for human–agent work · today

Overview   Timeline   Focus   Agents   Privacy
──────────────────────────────────────────────
  tab body, scrolling
```

| Tab | What it shows |
| --- | --- |
| Overview | The river, then turns per hour, the return loop, concurrency, and turn duration |
| Timeline | Recorded events, newest first |
| Focus | Apps that were in front, and how much of that time overlapped an agent |
| Agents | Per-project rollup of sessions, turns, and runtime |
| Privacy | Recording on/off, retention, and excluded apps |

Empty days say so, except Privacy, which is always available.

### Overview, under the river

Figures and section headings that define a metric use the shared tooltip (`MetricTooltip`): a short explanation, anchored to the label, kept inside the window.

The loop is how long you stayed away after a turn finished. Buckets run from under a minute to 30 minutes and beyond. Premature checks are returns to Cursor while a turn was still open. Still out means a finished turn with no return yet.

Concurrency is the share of agent wall-clock spent at 1, 2, 3, or 4+ overlapping turns. Turn duration is the distribution of finished turns.

## The river

The river is the top of Overview. It is an SVG of today, from the first recorded mark to the last. The caption shows the visible range. Drag pans, the buttons zoom, and double-click resets to the full day.

Time runs left to right. Names sit in a column on the left; bars sit on the plot.

```text
        9:00          10:00          11:00
Cursor        ████████████░░░░████████
  Sessions
  Tokens            ██        ████
  River UI              ███
Chrome              ████
  Tabs
  GitHub PR           ███
Settings                    █
```

**Applications are the only top-level rows.** Cursor, Chrome, System Settings, and the rest come from frontmost-app intervals. Past six lanes, the shortest non-Cursor apps collapse into Other; Cursor is always kept, and up to five other apps stay named. Cursor stays first. Other stays last.

**Sessions nest under Cursor.** One row per Cursor chat, not per turn. Turns in that chat share the row, and the label is the latest title. The visible name is the part after ` · `, so a row reads "Tokens" rather than "agent-HUD · Tokens". The full label is on the row's native tooltip. A check mark after the last bar means the chat's last turn has finished. Dimmer pieces are time the turn was running while Cursor was not the front app.

**Tabs nest under Chrome.** One row per page title recorded while Chrome was frontmost, capped at four plus an Other tabs row. Page changes are not app switches.

A Sessions or Tabs group is a short caption, a hairline, and the child rows, all sharing the same right edge as the app names. A group is omitted when it has no bar inside the current zoom. If agents ran but Cursor itself never appears in the lanes, Sessions is drawn after the app list instead of under a Cursor row.

Markers on the Cursor app row are premature checks and returns after a turn finished.

### Bar tooltip

Hovering a bar opens a small tip, separate from the metric tooltips. It shows:

1. The start time of that bar. Minutes, unless the visible range is 10 minutes or less, in which case seconds.
2. The duration.
3. A third line only when the row does not already say it: the chats overlapping a Cursor app bar, or "Away" on a dim session piece.

The tip is placed at the pointer and shifted so it stays fully on screen.
