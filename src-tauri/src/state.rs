use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::sessions::{sort_agent_sessions, AgentSession, AgentStatus};

pub const WORKING_HOLD_MS: i64 = 5_000;

pub struct AppState {
    pub sessions: Mutex<Vec<AgentSession>>,
    dismissed: Mutex<HashMap<String, AgentStatus>>,
    last_live_at: Mutex<HashMap<String, i64>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
            dismissed: Mutex::new(HashMap::new()),
            last_live_at: Mutex::new(HashMap::new()),
        }
    }

    pub fn merge_sessions(&self, live: Vec<AgentSession>) -> Vec<AgentSession> {
        self.merge_sessions_at(live, now_ms())
    }

    pub fn merge_sessions_at(&self, live: Vec<AgentSession>, now_ms: i64) -> Vec<AgentSession> {
        let mut dismissed = self.dismissed.lock().unwrap();
        let mut previous = self.sessions.lock().unwrap();
        let mut last_live_at = self.last_live_at.lock().unwrap();

        let live_ids: HashSet<String> = live.iter().map(|session| session.id.clone()).collect();
        let mut merged = Vec::new();

        for session in live {
            last_live_at.insert(session.id.clone(), now_ms);
            match dismissed.get(&session.id) {
                Some(status) if *status == session.status => continue,
                Some(_) => {
                    dismissed.remove(&session.id);
                    merged.push(session);
                }
                None => merged.push(session),
            }
        }

        for prev in previous.iter() {
            if live_ids.contains(&prev.id) || dismissed.contains_key(&prev.id) {
                continue;
            }

            if prev.status == AgentStatus::Working {
                let last = last_live_at.get(&prev.id).copied().unwrap_or(0);
                if now_ms.saturating_sub(last) <= WORKING_HOLD_MS {
                    merged.push(prev.clone());
                    continue;
                }
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
        let mut dismissed = self.dismissed.lock().unwrap();
        let mut sessions = self.sessions.lock().unwrap();
        self.last_live_at.lock().unwrap().remove(id);
        if let Some(session) = sessions.iter().find(|session| session.id == id) {
            dismissed.insert(id.to_string(), session.status);
        }
        sessions.retain(|session| session.id != id);
        sessions.clone()
    }

    /// Row-click ack: remove from HUD only when the session is already completed.
    pub fn acknowledge_session(&self, id: &str) -> Vec<AgentSession> {
        let should_dismiss = self
            .find_session(id)
            .map(|session| session.status == AgentStatus::Completed)
            .unwrap_or(false);
        if should_dismiss {
            self.dismiss_session(id)
        } else {
            self.get_sessions()
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
