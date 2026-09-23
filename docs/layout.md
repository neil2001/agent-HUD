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

Agent Flow is a normal resizable window, 1280×800, titled "Agent Flow". It cannot shrink below 1100×680. Opening it switches the app to a regular activation policy so it can sit in the foreground. Closing it returns to accessory.

The title, subtitle, and tabs sit in a sidebar about 200px wide. It is visible when the window opens. The rest of the window is the active tab, scrolling on its own.

```text
┌────────────────────┬──────────────────────────────┐
│ Agent Flow         │                              │
│ Flight recorder    │   tab body, scrolling        │
│ for human–agent    │                              │
│ work               │                              │
│                    │                              │
│ Overview           │                              │
│ Timeline           │                              │
│ Focus              │                              │
│ Agents             │                              │
│ PRs                │                              │
│ Privacy            │                              │
└────────────────────┴──────────────────────────────┘
```

| Tab | What it shows |
| --- | --- |
| Overview | Attention for one window, then the river for one day, then time between turns, the return loop, concurrency, and turn duration |
| Timeline | Recorded events, newest first |
| Focus | Apps that were in front, and how much of that time overlapped an agent |
| Agents | Per-project rollup of sessions, turns, and runtime |
| PRs | Open pull requests |
| Privacy | Recording on/off, retention, and excluded apps |

Empty days say so, except Overview, Privacy, and PRs, which are always available. Timeline, Focus, and Agents still use today's empty state.

### Attention, top of Overview

A control selects 1 day, 7 days, 14 days, or 30 days. One window is visible. The default is 1 day, from local midnight through now. Previous and next move by that same length, without overlap. Forward stops at the window that ends now. Changing the length returns to that latest window. The caption names the range.

The large figure is total time an app was in front. Under it: sessions prompted, turns finished, agent runtime, and turns per hour.

The bars share one scale, longest app at full width, ordered by focused time. Apps are the rows. Cursor chats nest under Cursor and count only while Cursor was in front and that chat's turn was open. Chrome tabs nest under Chrome. Anything under a minute folds into Other, Other sessions, or Other tabs.

### Overview, under the river

These figures follow the river's day, not the attention window. Tooltips are rare. Premature checks and still out explain themselves only on the label text. Section headings, histogram buckets, and the concurrency rows do not.

The loop is how long you stayed away after a turn finished. Buckets run from under a minute to 30 minutes and beyond. Premature checks are returns to Cursor while a turn was still open. Still out means a finished turn with no return yet.

Concurrency is the share of agent wall-clock spent at 1, 2, 3, or 4+ overlapping turns. Turn duration is the distribution of finished turns.

## The river

The river sits under attention. It is an SVG of one calendar day, from the first recorded mark to the last. Previous and next step one day. Forward stops on today. The caption names that day, then the visible hours. A new day reloads the plot at the full day. Drag pans, the buttons zoom, and double-click resets to the full day. A day with no marks says so, and the sections under the river stay hidden.

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

**Applications are the only top-level rows.** Cursor, Chrome, System Settings, and the rest come from frontmost-app intervals. A row appears only when that app was in front for more than 30 seconds inside the visible range. Past six lanes, the shortest non-Cursor apps collapse into Other; Cursor is always kept, and up to five other apps stay named. Cursor stays first. Other stays last.

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
