# agent-HUD

A minimal macOS HUD for active Cursor Desktop, cloud, and CLI (`agent` / `cursor-agent`) sessions.

The HUD appears only while agents are running, lists one row per session, and focuses that session when clicked.

See [docs/ux-spec.md](docs/ux-spec.md) for the product specification.

## Development

Prerequisites: Node.js, Rust, and Xcode command-line tools (macOS).

```bash
npm install
npm run tauri dev
```
