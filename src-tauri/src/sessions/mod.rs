pub mod aggregator;
pub mod cli;
pub mod cursor;
pub mod sqlite;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Working,
    Waiting,
    NeedsAttention,
    Completed,
}

pub fn sort_agent_sessions(sessions: &mut [AgentSession]) {
    sessions.sort_by(|a, b| {
        status_rank(a.status)
            .cmp(&status_rank(b.status))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn status_rank(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::NeedsAttention => 0,
        AgentStatus::Working => 1,
        AgentStatus::Completed => 2,
        AgentStatus::Waiting => 3,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Cursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionHost {
    CursorDesktop {
        workspace_path: String,
    },
    CursorCloud {
        workspace_path: Option<String>,
    },
    CursorCli {
        workspace_path: String,
        tty: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub agent: AgentKind,
    pub title: String,
    pub project: ProjectInfo,
    pub status: AgentStatus,
    pub host: SessionHost,
    pub updated_at: i64,
}

impl AgentSession {
    pub fn is_active(&self) -> bool {
        true
    }
}
