# Agent Session HUD — UX Specification

## 1. UX Philosophy

The HUD is a minimal, ephemeral status surface for active coding agents.

It should answer exactly two questions:

1. **What agents are currently active?**
2. **Where can I click to return to one?**

It should not behave like a dashboard, application window, or agent management interface.

The desired interaction is:

```text
Agent starts
    ↓
HUD appears
    ↓
User sees active agents
    ↓
User clicks one
    ↓
Correct terminal/application is focused
```

When there are no active agent sessions, the HUD should not be visible.

---

# 2. Visibility

The HUD should only exist visibly while there is at least one active agent session.

```text
active_sessions.count > 0
        │
        ├── YES → show HUD
        │
        └── NO  → hide HUD
```

This is a core product requirement.

## Agent starts

When the first active agent session is detected:

```text
No active agents
      ↓
Agent detected
      ↓
HUD becomes visible
```

The HUD should appear automatically without requiring the user to manually open it.

## Agent exits

When the final active agent session disappears:

```text
One active agent
      ↓
Agent exits
      ↓
No active agents
      ↓
HUD disappears
```

The application itself should continue running in the background so that it can detect a new agent later.

The process should not terminate simply because the HUD is hidden.

---

# 3. HUD Contents

The HUD should contain only a list of active agent sessions.

Each row should communicate:

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
- statistics
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

    if (sessions.length === 0) {
        return null;
    }

    return (
        <AgentList sessions={sessions} />
    );
}
```

However, HUD window visibility should preferably be controlled by the native/Tauri layer rather than relying solely on React rendering an empty window.

The desired architecture is:

```text
Rust aggregator
       │
       │ active session count
       ▼
Tauri window manager
       │
       ├── 0 sessions → hide
       │
       └── >0 sessions → show
                         │
                         ▼
                       React
```

This means that when there are no agents, the actual window is hidden rather than merely rendering an empty React application.

---

# 16. Animation

Animations should be minimal.

Optional:

- short fade/scale when HUD appears
- short fade when HUD disappears
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
HUD is completely invisible
```

### First agent starts

```text
Cursor agent starts
        ↓
HUD automatically appears
        ↓
One row is displayed
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
HUD automatically disappears
```

### New agent later

```text
HUD hidden
        ↓
New Cursor agent starts
        ↓
HUD automatically reappears
```

---

# 18. Product Principle

The HUD should be almost invisible when it is not useful.

It should appear when there is agent work happening, provide an instantaneous overview, and disappear when there is nothing to monitor.

The ideal experience is:

```text
                    Agent work
                        │
                        ▼
                 ┌─────────────┐
                 │  HUD appears │
                 └──────┬──────┘
                        │
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
                 ┌─────────────┐
                 │ HUD vanishes │
                 └─────────────┘
```

The HUD should never become another application the user feels responsible for managing.