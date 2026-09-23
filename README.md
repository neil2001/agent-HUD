# agent-HUD

A minimal macOS HUD for active Cursor Desktop agent sessions.

The HUD stays on screen. It shows today's sessions, prompts, and pull requests, lists one row per running session, and focuses the matching Cursor window when a row is clicked.

See [docs/ux-spec.md](docs/ux-spec.md) for the product specification.

## Development

Prerequisites: Node.js, Rust, and Xcode command-line tools (macOS).

```bash
npm install
npm run tauri dev
```
