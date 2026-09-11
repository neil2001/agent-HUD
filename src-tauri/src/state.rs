use std::collections::HashSet;
use std::sync::Mutex;

use crate::sessions::{sort_agent_sessions, AgentSession, AgentStatus};

pub struct AppState {
    pub sessions: Mutex<Vec<AgentSession>>,
    dismissed: Mutex<HashSet<String>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
            dismissed: Mutex::new(HashSet::new()),
        }
    }

    pub fn merge_sessions(&self, live: Vec<AgentSession>) -> Vec<AgentSession> {
        let mut dismissed = self.dismissed.lock().unwrap();
        let mut previous = self.sessions.lock().unwrap();

        for session in &live {
            dismissed.remove(&session.id);
        }

        let live_ids: HashSet<String> = live.iter().map(|session| session.id.clone()).collect();
        let mut merged = live;

        for prev in previous.iter() {
            if live_ids.contains(&prev.id) || dismissed.contains(&prev.id) {
                continue;
            }

            if matches!(
                prev.status,
                AgentStatus::Working | AgentStatus::NeedsAttention | AgentStatus::Completed
            ) {
                merged.push(AgentSession {
                    status: AgentStatus::Completed,
                    ..prev.clone()
                });
            }
        }

        sort_agent_sessions(&mut merged);
        *previous = merged.clone();
        merged
    }

    pub fn dismiss_session(&self, id: &str) -> Vec<AgentSession> {
        self.dismissed.lock().unwrap().insert(id.to_string());
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|session| session.id != id);
        sessions.clone()
    }

    pub fn get_sessions(&self) -> Vec<AgentSession> {
        self.sessions
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn find_session(&self, id: &str) -> Option<AgentSession> {
        self.sessions
            .lock()
            .ok()
            .and_then(|guard| guard.iter().find(|s| s.id == id).cloned())
    }
}
