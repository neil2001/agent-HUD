use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowSource {
    Cursor,
    Macos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowEventType {
    SessionStarted,
    SessionEnded,
    TurnStarted,
    TurnFinished,
    AppFocused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowEvent {
    pub id: String,
    pub timestamp: i64,
    pub source: FlowSource,
    #[serde(rename = "type")]
    pub event_type: FlowEventType,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub payload: JsonValue,
}

impl FlowEvent {
    pub fn new(
        timestamp: i64,
        source: FlowSource,
        event_type: FlowEventType,
        session_id: Option<String>,
        turn_id: Option<String>,
        payload: JsonValue,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp,
            source,
            event_type,
            session_id,
            turn_id,
            payload,
        }
    }
}
