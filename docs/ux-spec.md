# Agent Session HUD — UX Specification

## 1. UX Philosophy

The HUD is a minimal status surface that stays on screen.

It should answer three questions:

1. **How has agent work gone today?** Agent tabs prompted, completed turns, and pull requests opened or merged.
2. **What agents are currently active?**
3. **Where can I click to return to one?**

It should not behave like a dashboard, application window, or agent management interface. The daily usage strip is one compact row, not a statistics page.

The desired interaction is:

```text
HUD is already visible
    ↓
Agent starts
    ↓
A session row appears
    ↓
User clicks one
    ↓
Correct terminal/application is focused
```

When there are no active agent sessions, the session list is empty and the daily strip remains.

---

# 2. Visibility

The HUD window stays visible, including when no agent session is active.

```text
HUD process running
        │
        └── window visible
              ├── daily usage strip (always)
              └── session rows while agents are active
```

This is a core product requirement.

## Agent starts

When an agent session is detected, a session row appears under the daily strip. The window is already visible; the user does not open it.

## Agent exits

When a session ends, its row leaves the list. The daily strip remains.

The application itself should continue running in the background so that it can keep recording usage and detect a new agent later.

The process should not terminate when there are no active sessions.

---

# 3. HUD Contents

The HUD contains a daily usage strip and, while agents are active, a list of those sessions.

The strip shows, for the local calendar day:

- Sessions: agent tabs prompted today, each tab once (same count as Agent Flow)
- Turns: completed cycles today (same count as Agent Flow)
- Pull requests opened
- Pull requests merged
- An open icon that opens Agent Flow

Each session row should communicate:

- Agent type
- Project/session identifier sufficient to distinguish sessions
- Current status

Example:

```text
┌──────────────────────┐
│ ● Cursor   api       │
│ ◐ Cursor   frontend  │
│ ● Cursor   tests     │
└──────────────────────┘
```

The exact visual design should be refined by the implementation agent, but it should remain extremely minimal.

There should be no requirement for:

- navigation
- sidebar
- tabs
- dashboard cards
- detailed session information
- configuration controls
- agent controls
- embedded terminal
- chat UI

---

# 4. Session Rows

Each active session is represented by a single clickable row.

Conceptually:

```text
┌─────────────────────────┐
│ ● Cursor    frontend    │
└─────────────────────────┘
```

The entire row should be clickable.

Clicking anywhere on the row should focus the underlying session.

There should not be a separate "Focus" button.

---

# 5. Status Display

Status should be represented compactly.

For example:

```text
● Working
◐ Waiting
! Needs attention
✓ Done
× Error
```

However, V1 should generally display only **active sessions**, so terminal states such as `Done` may cause the session to disappear once the session is no longer considered active.

The implementation plan should determine the exact definition of "active."

The important UX distinction is:

```text
Active agent
    → visible in HUD

Inactive/completed agent
    → not visible in HUD
```

The HUD is not intended to be a session history.

---

# 6. What Counts as Active

The aggregator should expose a normalized concept of whether a session is active.

The frontend should not determine this itself.

Conceptually:

```rust
pub struct AgentSession {
    pub id: String,
    pub agent: AgentKind,
    pub project: ProjectInfo,
    pub status: AgentStatus,
    pub host: SessionHost,
}
```

The aggregator determines:

```rust
session.is_active()
```

or equivalent.

The frontend simply receives the active-session list.

Potential active states:

```text
Working
Waiting
NeedsAttention
```

Potential inactive states:

```text
Done
Error
Exited
Unknown/stale
```

The exact mapping must be based on the actual Cursor session lifecycle and should be validated during implementation.

The implementation should avoid keeping stale sessions visible simply because historical session metadata still exists.

---

# 7. Dynamic HUD Size

The HUD should automatically size itself based on the number of active sessions.

For example:

```text
1 agent

┌───────────────────┐
│ ● Cursor  api     │
└───────────────────┘
```

Two agents:

```text
┌───────────────────┐
│ ● Cursor  api     │
│ ● Cursor  web     │
└───────────────────┘
```

Many agents:

```text
┌───────────────────┐
│ ● Cursor  api     │
│ ● Cursor  web     │
│ ◐ Cursor  tests   │
│ ● Cursor  infra   │
└───────────────────┘
```

There should be a reasonable maximum height so an unusually large number of sessions does not create an enormous HUD.

If necessary, the list can scroll.

This should be implemented only if the number of sessions makes it necessary.

---

# 8. Appearance

The HUD should feel like a native macOS utility rather than a conventional application window.

Desired characteristics:

- Small
- Compact
- Minimal
- Translucent or subtly opaque
- Rounded corners
- No traditional title bar
- No unnecessary chrome
- Always above normal windows
- Visible across Spaces
- Easy to drag
- Low visual distraction

The exact visual language should be determined during implementation.

The implementation agent should research modern macOS floating utility/HUD patterns and choose an appropriate approach.

Do not over-design the interface.

---

# 9. Interaction

There is exactly one primary interaction:

> Click an agent → focus that agent.

Example:

```text
HUD
 │
 ├── Cursor / api
 │       ↓ click
 │
 └── focus exact Ghostty terminal
```

For Cursor Desktop:

```text
HUD
 │
 ├── Cursor / frontend
 │       ↓ click
 │
 └── activate appropriate Cursor workspace/window
```

No secondary interaction is required for V1.

---

# 10. Hover State

A subtle hover state is desirable so the user can tell which row will be activated.

It should remain visually restrained.

Example:

```text
normal:
● Cursor   api

hover:
▸ Cursor   api
```

Avoid large animations or visual effects.

---

# 11. Active Session Ordering

The implementation should choose a sensible deterministic ordering.

Possible ordering:

1. Working agents
2. Agents needing attention
3. Waiting agents

Within the same state:

- most recently active first, or
- most recently started first

The exact ordering should be decided during implementation planning.

The ordering should remain stable enough that rows do not constantly jump around while agents are running.

---

# 12. HUD Lifecycle

The application lifecycle and HUD lifecycle are separate.

```text
Application process
───────────────────────────────────────
        running continuously
                  │
                  ▼

HUD lifecycle
───────────────────────────────────────

No agents       → hidden

Agent appears   → visible

Agents active   → visible

Last agent exits
                → hidden

New agent       → visible again
```

The Rust backend/session aggregator continues running even when the HUD is hidden.

---

# 13. No Persistent Dashboard

Do not implement an always-visible empty-state window.

When there are no agents:

```text
[ nothing ]
```

not:

```text
┌───────────────────┐
│ AGENTS            │
│                   │
│ No active agents  │
└───────────────────┘
```

The correct behavior is simply that the HUD is absent.

---

# 14. No Manual Open Requirement

The user should not have to:

- launch the HUD every time an agent starts
- click an icon to reveal the HUD
- open a menu bar dashboard
- manually refresh the session list

The background application should automatically detect active sessions and control HUD visibility.

A manual global shortcut may be useful later, but it is not required for V1.

---

# 15. Frontend Responsibilities

React should be almost entirely presentational.

The frontend receives:

```typescript
AgentSession[]
```

and renders them.

Conceptually:

```typescript
function App() {
    const sessions = useSessions();
    const usage = useDailyUsage();

    return (
        <>
            <DailyStrip usage={usage} />
            {sessions.length > 0 ? <AgentList sessions={sessions} /> : null}
        </>
    );
}
```

Window visibility is controlled by the native/Tauri layer. The window stays shown. Its height tracks the session count, with the daily strip always included.

```text
Rust aggregator
       │
       │ active session count + daily usage
       ▼
Tauri window manager
       │
       └── always show
             │
             ▼
           React
             ├── daily strip
             └── session rows when count > 0
```

---

# 16. Animation

Animations should be minimal.

Optional:

- short fade/scale when a session row appears
- subtle row transition when status changes

Do not let animations delay interaction.

The user should be able to click a newly appeared agent immediately.

---

# 17. V1 UX Definition of Done

The UX is complete when:

### No agents

```text
No active Cursor agents
        ↓
HUD stays visible
        ↓
Daily strip shows today's counts
        ↓
Session list is empty
```

### First agent starts

```text
Cursor agent starts
        ↓
One session row appears under the daily strip
```

### Multiple agents

```text
Multiple Cursor agents
        ↓
HUD displays one row per active session
```

### Agent status changes

```text
Agent status changes
        ↓
Row updates automatically
```

### User clicks

```text
User clicks row
        ↓
Correct underlying session is focused
```

### Final agent exits

```text
Last active agent exits
        ↓
Session list clears
        ↓
Daily strip remains
```

### New agent later

```text
HUD still visible
        ↓
New Cursor agent starts
        ↓
Session row appears
```

---

# 18. Product Principle

The HUD should stay out of the way without going away.

It should remain visible with today's usage, grow a session row when agent work is happening, and drop that row when the session ends.

The ideal experience is:

```text
                 ┌─────────────┐
                 │ HUD stays up │
                 └──────┬──────┘
                        │
                 daily usage strip
                        │
                        ▼
                    Agent work
                        │
                        ▼
                 see active agents
                        │
                        ▼
                   click one
                        │
                        ▼
                 return to work
                        │
                        ▼
                 agents finish
                        │
                        ▼
                 session rows clear
```

The HUD should never become another application the user feels responsible for managing.