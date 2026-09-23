use std::sync::{Arc, Mutex};

use crate::sessions::AgentSession;

use super::cursor_source::CursorDiffer;
use super::store::FlowStore;

pub struct FlowPipeline {
    store: Arc<FlowStore>,
    differ: Mutex<CursorDiffer>,
}

impl FlowPipeline {
    pub fn new(store: Arc<FlowStore>) -> Self {
        Self {
            store,
            differ: Mutex::new(CursorDiffer::default()),
        }
    }

    pub fn record_cursor_discovery(&self, sessions: &[AgentSession], now_ms: i64) {
        let settings = match self.store.get_settings() {
            Ok(s) => s,
            Err(err) => {
                eprintln!("flow: failed to read settings: {err}");
                return;
            }
        };
        if !settings.recording_enabled {
            return;
        }

        let events = self.differ.lock().unwrap().diff(sessions, now_ms);

        for event in events {
            if let Err(err) = self.store.append(&event) {
                eprintln!("flow: failed to append event: {err}");
            }
        }
    }

    pub fn store(&self) -> &Arc<FlowStore> {
        &self.store
    }
}
