# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Cursor CLI sessions (`agent` / `cursor-agent`) are a separate host from Desktop and cloud. Discovery and click routing live in `src-tauri/src/sessions/cli.rs` and `src-tauri/src/focus.rs`. Desktop and cloud clicks stay on the `cursor://` background-agent link.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
