# agent-HUD

A macOS menu-bar app for Cursor agent work. It keeps a small always-on HUD of what is running, and a separate Agent Flow window that records how the day actually went.

The HUD stays on screen. It shows today's session, turn, and pull-request counts, lists one row per active Cursor session, and focuses that session when the row is clicked. The overflow menu on the strip opens Agent Flow.

Agent Flow is the flight recorder. It keeps a local timeline of agent turns and which app was in front, then draws that day as a river: applications on top, each Cursor chat nested under Cursor, and Chrome tabs nested under Chrome.

See [docs/architecture.md](docs/architecture.md) for how the process is built, and [docs/layout.md](docs/layout.md) for the two windows. [docs/ux-spec.md](docs/ux-spec.md) is the HUD product spec.

## Development

Prerequisites: Node.js, Rust, and Xcode command-line tools (macOS).

```bash
npm install
npm run tauri dev
```

`npm run tauri dev` starts the Vite dev server and the Tauri app. The HUD appears at the top of the screen. Open Agent Flow from the HUD overflow button or the tray menu.

Recorded events live in `~/Library/Application Support/com.neilxu.agent-hud/flow.sqlite`.
