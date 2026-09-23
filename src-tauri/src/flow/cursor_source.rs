use std::collections::{HashMap, HashSet};

use serde_json::json;

use crate::sessions::{AgentSession, AgentStatus, SessionHost};
use crate::state::WORKING_HOLD_MS;

use super::event::{FlowEvent, FlowEventType, FlowSource};

#[derive(Debug, Clone)]
struct SessionTrack {
    status: AgentStatus,
    turn_number: u32,
    open_turn_id: Option<String>,
    working_gap_at: Option<i64>,
}

#[derive(Debug, Default)]
pub struct CursorDiffer {
    sessions: HashMap<String, SessionTrack>,
}

impl CursorDiffer {
    pub fn diff(&mut self, next: &[AgentSession], now_ms: i64) -> Vec<FlowEvent> {
        let mut events = Vec::new();
        let next_ids: HashSet<String> = next.iter().map(|s| s.id.clone()).collect();

        let prev_ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in prev_ids {
            if !next_ids.contains(&id) {
                if let Some(track) = self.sessions.remove(&id) {
                    events.extend(finish_open_turn(&id, &track, now_ms));
                    events.push(FlowEvent::new(
                        now_ms,
                        FlowSource::Cursor,
                        FlowEventType::SessionEnded,
                        Some(id),
                        None,
                        json!({}),
                    ));
                }
            }
        }

        for session in next {
            let discovered = session.status;
            if !self.sessions.contains_key(&session.id) {
                let mut track = SessionTrack {
                    status: discovered,
                    turn_number: 0,
                    open_turn_id: None,
                    working_gap_at: None,
                };
                events.push(session_started_event(session, now_ms));
                if discovered == AgentStatus::Working {
                    let turn_id = bump_turn(&mut track, &session.id);
                    events.push(turn_started_event(session, now_ms, &turn_id));
                }
                self.sessions.insert(session.id.clone(), track);
                continue;
            }

            let track = self.sessions.get_mut(&session.id).unwrap();
            let prev_status = track.status;

            if discovered == AgentStatus::Working && track.working_gap_at.is_some() {
                let gap_at = track.working_gap_at.unwrap();
                if now_ms.saturating_sub(gap_at) <= WORKING_HOLD_MS {
                    track.working_gap_at = None;
                    track.status = AgentStatus::Working;
                    continue;
                }
                if let Some(turn_id) = track.open_turn_id.clone() {
                    events.push(FlowEvent::new(
                        now_ms,
                        FlowSource::Cursor,
                        FlowEventType::TurnFinished,
                        Some(session.id.clone()),
                        Some(turn_id),
                        json!({}),
                    ));
                    track.open_turn_id = None;
                }
                track.working_gap_at = None;
            }

            if prev_status == AgentStatus::Working && discovered != AgentStatus::Working {
                if track.open_turn_id.is_some() {
                    if matches!(
                        discovered,
                        AgentStatus::NeedsAttention | AgentStatus::Completed
                    ) {
                        let turn_id = track.open_turn_id.clone().unwrap();
                        events.push(FlowEvent::new(
                            now_ms,
                            FlowSource::Cursor,
                            FlowEventType::TurnFinished,
                            Some(session.id.clone()),
                            Some(turn_id),
                            json!({}),
                        ));
                        track.open_turn_id = None;
                    } else {
                        track.working_gap_at = Some(now_ms);
                    }
                }
                track.status = discovered;
                continue;
            }

            if prev_status != AgentStatus::Working && discovered == AgentStatus::Working {
                let turn_id = bump_turn(track, &session.id);
                events.push(turn_started_event(session, now_ms, &turn_id));
                track.status = discovered;
                continue;
            }

            if track.working_gap_at.is_some()
                && discovered != AgentStatus::Working
                && now_ms.saturating_sub(track.working_gap_at.unwrap()) > WORKING_HOLD_MS
            {
                if let Some(turn_id) = track.open_turn_id.clone() {
                    events.push(FlowEvent::new(
                        now_ms,
                        FlowSource::Cursor,
                        FlowEventType::TurnFinished,
                        Some(session.id.clone()),
                        Some(turn_id),
                        json!({}),
                    ));
                    track.open_turn_id = None;
                }
                track.working_gap_at = None;
            }

            if prev_status != discovered {
                track.status = discovered;
            }
        }

        events
    }
}

fn finish_open_turn(id: &str, track: &SessionTrack, now_ms: i64) -> Vec<FlowEvent> {
    let mut events = Vec::new();
    if let Some(turn_id) = &track.open_turn_id {
        events.push(FlowEvent::new(
            now_ms,
            FlowSource::Cursor,
            FlowEventType::TurnFinished,
            Some(id.to_string()),
            Some(turn_id.clone()),
            json!({}),
        ));
    }
    events
}

fn bump_turn(track: &mut SessionTrack, session_id: &str) -> String {
    track.turn_number += 1;
    let turn_id = format!("{}:{}", session_id, track.turn_number);
    track.open_turn_id = Some(turn_id.clone());
    turn_id
}

fn session_started_event(session: &AgentSession, now_ms: i64) -> FlowEvent {
    FlowEvent::new(
        now_ms,
        FlowSource::Cursor,
        FlowEventType::SessionStarted,
        Some(session.id.clone()),
        None,
        cursor_payload(session),
    )
}

fn turn_started_event(session: &AgentSession, now_ms: i64, turn_id: &str) -> FlowEvent {
    FlowEvent::new(
        now_ms,
        FlowSource::Cursor,
        FlowEventType::TurnStarted,
        Some(session.id.clone()),
        Some(turn_id.to_string()),
        cursor_payload(session),
    )
}

fn cursor_payload(session: &AgentSession) -> serde_json::Value {
    let host = match &session.host {
        SessionHost::CursorDesktop { workspace_path } => json!({
            "type": "cursor_desktop",
            "workspace": workspace_path
        }),
        SessionHost::CursorCloud { workspace_path } => json!({
            "type": "cursor_cloud",
            "workspace": workspace_path
        }),
    };
    json!({
        "provider": "cursor",
        "project": session.project.name,
        "title": session.title,
        "host": host
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session(id: &str, status: AgentStatus) -> AgentSession {
        AgentSession {
            id: id.to_string(),
            agent: crate::sessions::AgentKind::Cursor,
            title: "Test".to_string(),
            project: crate::sessions::ProjectInfo {
                name: "proj".to_string(),
                path: None,
            },
            status,
            host: SessionHost::CursorDesktop {
                workspace_path: "/tmp".to_string(),
            },
            updated_at: 0,
        }
    }

    #[test]
    fn first_sighting_emits_session_and_turn_when_working() {
        let mut differ = CursorDiffer::default();
        let events = differ.diff(&[sample_session("s1", AgentStatus::Working)], 1000);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, FlowEventType::SessionStarted);
        assert_eq!(events[1].event_type, FlowEventType::TurnStarted);
        assert_eq!(events[1].turn_id.as_deref(), Some("s1:1"));
    }

    #[test]
    fn second_turn_after_needs_attention() {
        let mut differ = CursorDiffer::default();
        differ.diff(&[sample_session("s1", AgentStatus::Working)], 1000);
        let events = differ.diff(&[sample_session("s1", AgentStatus::NeedsAttention)], 2000);
        assert!(events
            .iter()
            .any(|e| e.event_type == FlowEventType::TurnFinished));
        let events = differ.diff(&[sample_session("s1", AgentStatus::Working)], 3000);
        assert!(events.iter().any(|e| e.turn_id.as_deref() == Some("s1:2")));
    }

    #[test]
    fn disappearance_emits_turn_finished_and_session_ended() {
        let mut differ = CursorDiffer::default();
        differ.diff(&[sample_session("s1", AgentStatus::Working)], 1000);
        let events = differ.diff(&[], 2000);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, FlowEventType::TurnFinished);
        assert_eq!(events[1].event_type, FlowEventType::SessionEnded);
    }

    #[test]
    fn poll_gap_within_hold_emits_nothing() {
        let mut differ = CursorDiffer::default();
        differ.diff(&[sample_session("s1", AgentStatus::Working)], 1000);
        let events = differ.diff(&[sample_session("s1", AgentStatus::Waiting)], 1500);
        assert!(events.is_empty());
        let events = differ.diff(&[sample_session("s1", AgentStatus::Working)], 2500);
        assert!(events.is_empty());
    }

    #[test]
    fn parallel_sessions_independent_turn_ids() {
        let mut differ = CursorDiffer::default();
        let events = differ.diff(
            &[
                sample_session("a", AgentStatus::Working),
                sample_session("b", AgentStatus::Working),
            ],
            1000,
        );
        let turns: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == FlowEventType::TurnStarted)
            .map(|e| e.turn_id.clone())
            .collect();
        assert!(turns.contains(&Some("a:1".to_string())));
        assert!(turns.contains(&Some("b:1".to_string())));
    }

    #[test]
    fn unchanged_status_emits_nothing() {
        let mut differ = CursorDiffer::default();
        differ.diff(&[sample_session("s1", AgentStatus::Working)], 1000);
        let events = differ.diff(&[sample_session("s1", AgentStatus::Working)], 2000);
        assert!(events.is_empty());
    }
}
