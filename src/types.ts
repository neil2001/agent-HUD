export type AgentStatus =
  | "working"
  | "waiting"
  | "needs_attention"
  | "completed";

export type AgentKind = "cursor";

export type SessionHost =
  | { type: "cursor_desktop"; workspace_path: string }
  | { type: "cursor_cloud"; workspace_path?: string }
  | { type: "cursor_cli"; workspace_path: string; tty: string | null };

export type AgentSession = {
  id: string;
  agent: AgentKind;
  title: string;
  project: {
    name: string;
    path?: string;
  };
  status: AgentStatus;
  host: SessionHost;
  updated_at: number;
};
