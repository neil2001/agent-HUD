# Architecture

agent-HUD is one Tauri process with two webview windows. Both load `index.html`. [`src/main.tsx`](../src/main.tsx) picks the React tree from the window label: `flow` renders Agent Flow, anything else renders the HUD.

```text
Cursor state.vscdb + transcripts          macOS frontmost app
            │                                      │
            ▼                                      ▼
   sessions::cursor::discover            flow::focus_macos
            │                                      │
            ▼                                      ▼
   sessions::aggregator ──record──► FlowPipeline ──append──► FlowStore
            │                                      sqlite
            │ emit sessions-changed                     │
            ▼                                           │ invoke
        HUD window                              Agent Flow window
```

## Process

[`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs) builds the app:

1. Open the flow store, or an in-memory store if the data directory is unavailable.
2. Create the HUD panel and the tray menu.
3. Start the macOS focus observer.
4. Start the session aggregator loop and the pull-request poller.

The tray menu opens Agent Flow, toggles launch-at-login, and quits. The process stays running when no agent is active. On macOS the activation policy is accessory while only the HUD is up, and regular while the Agent Flow window exists.

## HUD sessions

Active sessions come from Cursor's local state, not from a network API.

[`sessions/cursor.rs`](../src-tauri/src/sessions/cursor.rs) reads:

- `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`
- `~/Library/Application Support/Cursor/User/workspaceStorage`
- `~/.cursor/projects` transcripts

A session is shown only while it still looks live: a recent transcript, a running tool, or an unfinished prompt inside the recency window. Desktop and cloud hosts are both `SessionHost` variants. The only agent kind today is Cursor.

[`sessions/aggregator.rs`](../src-tauri/src/sessions/aggregator.rs) polls about once a second and also wakes on file changes under those paths. Each pass:

1. Discovers the live session list.
2. Asks `FlowPipeline` to diff that list into recorder events.
3. Merges the list into `AppState` (including a short hold so a session that just went idle does not vanish immediately).
4. Emits `sessions-changed` and resizes the HUD.
5. Emits `daily-usage-changed`.

Clicking a HUD row calls `focus_session`, which opens `cursor://anysphere.cursor-deeplink/background-agent?bcId=<id>`.

## Flight recorder

`FlowPipeline` owns a `CursorDiffer` and the store. The differ compares the previous discovery snapshot with the next one and emits:

| Event | When |
| --- | --- |
| `session_started` / `session_ended` | A session id appears or disappears |
| `turn_started` / `turn_finished` | The session enters or leaves `working`, after the working-hold gap |

Turn ids are `<session id>:<n>` inside one process lifetime. The payload carries `project` and `title`.

Separately, [`flow/focus_macos.rs`](../src-tauri/src/flow/focus_macos.rs) samples the frontmost app. An app change becomes `app_focused`. While Google Chrome is frontmost, a tab change becomes `chrome_page_focused`. Password-manager bundle ids are stored as app name `Hidden`. Recording can be turned off, and retention defaults to 90 days. Both are settings in the store.

Events are appended to sqlite at:

```text
~/Library/Application Support/com.neilxu.agent-hud/flow.sqlite
```

Tables are `events` (id, timestamp, source, type, session id, turn id, payload) and `settings`.

## Metrics

Agent Flow does not render the event log directly. `get_flow_timeline` and `get_flow_summary` load the day's events and [`flow/metrics.rs`](../src-tauri/src/flow/metrics.rs) folds them.

A **session** is one Cursor chat: every `turn_started` with that `session_id`. A **turn** is one working interval inside that chat. Summary counts for the local day:

- Sessions: distinct session ids that started a turn
- Turns: finished turns
- Prompts: turn starts

The river is the same fold, drawn rather than counted. See [layout.md](layout.md).

`get_attention` uses the same events over 7, 14, or 30 days. It returns frontmost time ranked by app, with Cursor chats and Chrome tabs nested under those apps, plus the session, turn, and runtime totals for that window.

Return latency is the time from a finished turn until focus comes back to Cursor, unless another turn in that session starts first. Coming back while a turn is still open is a premature check, not a return.

Pull-request opened and merged counts on the HUD come from a separate poller ([`daily.rs`](../src-tauri/src/daily.rs)), cached in memory, and are not stored in the flow database.

## Commands

The webview talks to Rust only through Tauri commands:

| Command | Used by |
| --- | --- |
| `get_sessions` | HUD list |
| `focus_session` | HUD row click |
| `dismiss_session` | HUD dismiss |
| `get_daily_usage` | HUD strip |
| `open_flow` | HUD overflow button and tray |
| `get_flow_summary` | Agent Flow overview |
| `get_flow_timeline` | Agent Flow river, timeline, focus, agents |
| `get_attention` | Agent Flow overview, attention chart |
| `get_flow_settings` / `set_flow_settings` | Privacy tab |

The HUD listens for `sessions-changed` and `daily-usage-changed`. Agent Flow refetches on an interval from [`src/flow/useFlowData.ts`](../src/flow/useFlowData.ts).

## Layout of the repo

```text
src/                  React UI
  App.tsx             HUD
  components/         daily strip and session rows
  flow/               Agent Flow window
src-tauri/src/
  lib.rs              commands and startup
  window.rs           HUD panel size and position
  flow_window.rs      Agent Flow window
  lifecycle.rs        tray and launch-at-login
  sessions/           Cursor discovery and the aggregator
  flow/               recorder, focus observer, metrics, sqlite
  daily.rs            HUD usage counts and PR cache
  focus.rs            click-to-focus
```
