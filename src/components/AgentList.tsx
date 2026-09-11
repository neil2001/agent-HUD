import type { AgentSession } from "../types";
import { AgentRow } from "./AgentRow";

type AgentListProps = {
  sessions: AgentSession[];
};

export function AgentList({ sessions }: AgentListProps) {
  return (
    <ul className="agent-list">
      {sessions.map((session) => (
        <AgentRow key={session.id} session={session} />
      ))}
    </ul>
  );
}
