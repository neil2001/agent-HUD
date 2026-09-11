export type AgentStatus = "working" | "waiting" | "needs_attention";

export type AgentKind = "cursor";

export type SessionHost =
  | { type: "cursor_desktop"; workspace_path: string }
  | { type: "cursor_cloud"; workspace_path?: string };

export type AgentSession = {
  id: string;
  agent: AgentKind;
  project: {
    name: string;
    path?: string;
  };
  status: AgentStatus;
  host: SessionHost;
  updated_at: number;
};
