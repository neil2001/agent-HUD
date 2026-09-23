import { invoke } from "@tauri-apps/api/core";
import type { AgentSession } from "../types";

type AgentRowProps = {
  session: AgentSession;
};

export function AgentRow({ session }: AgentRowProps) {
  const showProject = session.project.name !== session.title;

  return (
    <li className={`agent-row agent-row-${session.status}`}>
      <button
        type="button"
        className="agent-row-button"
        onClick={() => {
          invoke("focus_session", { id: session.id }).catch(() => undefined);
        }}
      >
        <span className={`status-glyph status-${session.status}`} aria-hidden />
        <span className="session-title">{session.title}</span>
        {showProject ? (
          <span className="project-label">{session.project.name}</span>
        ) : (
          <span className="project-label project-label-spacer" aria-hidden />
        )}
      </button>
      <button
        type="button"
        className="dismiss-button"
        onClick={() => {
          invoke("dismiss_session", { id: session.id }).catch(() => undefined);
        }}
      >
        Dismiss
      </button>
    </li>
  );
}
