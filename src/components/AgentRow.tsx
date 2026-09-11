import { invoke } from "@tauri-apps/api/core";
import type { AgentSession } from "../types";

type AgentRowProps = {
  session: AgentSession;
};

export function AgentRow({ session }: AgentRowProps) {
  return (
    <li className="agent-row">
      <button
        type="button"
        className="agent-row-button"
        onClick={() => {
          invoke("focus_session", { id: session.id }).catch(() => undefined);
        }}
      >
        <span className={`status-glyph status-${session.status}`} aria-hidden />
        <span className="agent-label">Cursor</span>
        <span className="project-label">{session.project.name}</span>
      </button>
    </li>
  );
}
