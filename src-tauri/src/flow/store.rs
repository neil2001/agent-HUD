use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::event::{FlowEvent, FlowEventType, FlowSource};
use super::privacy::default_excluded_bundle_ids;

pub const SETTING_RECORDING_ENABLED: &str = "recording_enabled";
pub const SETTING_RETENTION_DAYS: &str = "retention_days";
pub const SETTING_EXCLUDED_BUNDLE_IDS: &str = "excluded_bundle_ids";

const DEFAULT_RETENTION_DAYS: i64 = 90;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowSettings {
    pub recording_enabled: bool,
    pub retention_days: i64,
    pub excluded_bundle_ids: Vec<String>,
}

impl Default for FlowSettings {
    fn default() -> Self {
        Self {
            recording_enabled: true,
            retention_days: DEFAULT_RETENTION_DAYS,
            excluded_bundle_ids: default_excluded_bundle_ids(),
        }
    }
}

pub struct FlowStore {
    conn: Mutex<Connection>,
}

impl FlowStore {
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        store.apply_retention_on_startup()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS events (
              id TEXT PRIMARY KEY,
              timestamp INTEGER NOT NULL,
              source TEXT NOT NULL,
              type TEXT NOT NULL,
              session_id TEXT,
              turn_id TEXT,
              payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
            CREATE TABLE IF NOT EXISTS settings (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    pub fn append(&self, event: &FlowEvent) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let source = source_to_str(event.source);
        let event_type = event_type_to_str(event.event_type);
        let payload = serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_string());
        conn.execute(
            "INSERT INTO events (id, timestamp, source, type, session_id, turn_id, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id,
                event.timestamp,
                source,
                event_type,
                event.session_id,
                event.turn_id,
                payload
            ],
        )?;
        Ok(())
    }

    pub fn append_many(&self, events: &[FlowEvent]) -> Result<(), rusqlite::Error> {
        for event in events {
            self.append(event)?;
        }
        Ok(())
    }

    pub fn query_range(&self, start_ms: i64, end_ms: i64) -> Result<Vec<FlowEvent>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, source, type, session_id, turn_id, payload
             FROM events
             WHERE timestamp >= ?1 AND timestamp < ?2
             ORDER BY timestamp ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![start_ms, end_ms], |row| {
            let source_str: String = row.get(2)?;
            let type_str: String = row.get(3)?;
            let payload_str: String = row.get(6)?;
            let payload: JsonValue =
                serde_json::from_str(&payload_str).unwrap_or(JsonValue::Object(Default::default()));
            Ok(FlowEvent {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                source: str_to_source(&source_str),
                event_type: str_to_event_type(&type_str),
                session_id: row.get(4)?,
                turn_id: row.get(5)?,
                payload,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_settings(&self) -> Result<FlowSettings, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut settings = FlowSettings::default();

        if let Ok(value) = self.read_setting(&conn, SETTING_RECORDING_ENABLED) {
            settings.recording_enabled = value == "true";
        }
        if let Ok(value) = self.read_setting(&conn, SETTING_RETENTION_DAYS) {
            if let Ok(days) = value.parse::<i64>() {
                settings.retention_days = days.max(1);
            }
        }
        if let Ok(value) = self.read_setting(&conn, SETTING_EXCLUDED_BUNDLE_IDS) {
            if let Ok(ids) = serde_json::from_str::<Vec<String>>(&value) {
                settings.excluded_bundle_ids = ids;
            }
        }

        Ok(settings)
    }

    pub fn set_settings(&self, settings: &FlowSettings) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        self.write_setting(
            &conn,
            SETTING_RECORDING_ENABLED,
            if settings.recording_enabled {
                "true".to_string()
            } else {
                "false".to_string()
            },
        )?;
        self.write_setting(
            &conn,
            SETTING_RETENTION_DAYS,
            settings.retention_days.to_string(),
        )?;
        let ids_json = serde_json::to_string(&settings.excluded_bundle_ids)
            .unwrap_or_else(|_| "[]".to_string());
        self.write_setting(&conn, SETTING_EXCLUDED_BUNDLE_IDS, ids_json)?;
        Ok(())
    }

    pub fn delete_older_than(&self, cutoff_ms: i64) -> Result<u64, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let deleted = conn.execute(
            "DELETE FROM events WHERE timestamp < ?1",
            params![cutoff_ms],
        )?;
        Ok(deleted as u64)
    }

    fn apply_retention_on_startup(&self) -> Result<(), rusqlite::Error> {
        let settings = self.get_settings()?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cutoff = now_ms - settings.retention_days * 24 * 60 * 60 * 1000;
        self.delete_older_than(cutoff)?;
        Ok(())
    }

    fn read_setting(&self, conn: &Connection, key: &str) -> Result<String, rusqlite::Error> {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
    }

    fn write_setting(
        &self,
        conn: &Connection,
        key: &str,
        value: String,
    ) -> Result<(), rusqlite::Error> {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn source_to_str(source: FlowSource) -> &'static str {
    match source {
        FlowSource::Cursor => "cursor",
        FlowSource::Macos => "macos",
    }
}

fn event_type_to_str(event_type: FlowEventType) -> &'static str {
    match event_type {
        FlowEventType::SessionStarted => "session_started",
        FlowEventType::SessionEnded => "session_ended",
        FlowEventType::TurnStarted => "turn_started",
        FlowEventType::TurnFinished => "turn_finished",
        FlowEventType::AppFocused => "app_focused",
    }
}

fn str_to_source(s: &str) -> FlowSource {
    match s {
        "macos" => FlowSource::Macos,
        _ => FlowSource::Cursor,
    }
}

fn str_to_event_type(s: &str) -> FlowEventType {
    match s {
        "session_ended" => FlowEventType::SessionEnded,
        "turn_started" => FlowEventType::TurnStarted,
        "turn_finished" => FlowEventType::TurnFinished,
        "app_focused" => FlowEventType::AppFocused,
        _ => FlowEventType::SessionStarted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::event::FlowEvent;
    use crate::flow::privacy::redact_focus_payload;
    use serde_json::json;

    fn sample_event(ts: i64, id_suffix: &str) -> FlowEvent {
        FlowEvent {
            id: format!("id-{}", id_suffix),
            timestamp: ts,
            source: FlowSource::Cursor,
            event_type: FlowEventType::TurnStarted,
            session_id: Some("s1".to_string()),
            turn_id: Some("s1:1".to_string()),
            payload: json!({"provider": "cursor"}),
        }
    }

    #[test]
    fn append_and_query_range_preserves_order() {
        let store = FlowStore::open_in_memory().unwrap();
        store.append(&sample_event(1000, "a")).unwrap();
        store.append(&sample_event(2000, "b")).unwrap();
        store.append(&sample_event(3000, "c")).unwrap();

        let events = store.query_range(0, 5000).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].timestamp, 1000);
        assert_eq!(events[2].timestamp, 3000);
    }

    #[test]
    fn query_range_is_exclusive_end() {
        let store = FlowStore::open_in_memory().unwrap();
        store.append(&sample_event(1000, "a")).unwrap();
        store.append(&sample_event(2000, "b")).unwrap();

        let events = store.query_range(0, 2000).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn retention_deletes_old_rows() {
        let store = FlowStore::open_in_memory().unwrap();
        store.append(&sample_event(1000, "old")).unwrap();
        store.append(&sample_event(1_000_000, "new")).unwrap();

        let deleted = store.delete_older_than(500_000).unwrap();
        assert_eq!(deleted, 1);

        let events = store.query_range(0, 2_000_000).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "id-new");
    }

    #[test]
    fn settings_round_trip() {
        let store = FlowStore::open_in_memory().unwrap();
        let settings = FlowSettings {
            recording_enabled: false,
            retention_days: 30,
            excluded_bundle_ids: vec!["com.example.app".to_string()],
        };
        store.set_settings(&settings).unwrap();
        let loaded = store.get_settings().unwrap();
        assert_eq!(loaded, settings);
    }

    #[test]
    fn excluded_app_payload_redaction() {
        let excluded = default_excluded_bundle_ids();
        let payload = redact_focus_payload(
            "com.1password.1password",
            "1Password",
            &excluded,
        );
        assert_eq!(payload["app_name"], "Hidden");
        assert_eq!(payload["excluded"], true);

        let normal = redact_focus_payload("com.apple.Safari", "Safari", &excluded);
        assert_eq!(normal["app_name"], "Safari");
        assert!(normal.get("excluded").is_none());
    }
}
