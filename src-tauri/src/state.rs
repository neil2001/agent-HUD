use std::sync::Mutex;

use crate::sessions::AgentSession;

pub struct AppState {
    pub sessions: Mutex<Vec<AgentSession>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
        }
    }

    pub fn set_sessions(&self, sessions: Vec<AgentSession>) {
        if let Ok(mut guard) = self.sessions.lock() {
            *guard = sessions;
        }
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
