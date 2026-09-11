use agent_hud_lib::sessions::cursor::{map_cloud_activity, map_composer_activity};
use agent_hud_lib::sessions::{AgentKind, AgentSession, AgentStatus, ProjectInfo, SessionHost};
use agent_hud_lib::state::AppState;
use rusqlite::Connection;
use tempfile::tempdir;

fn create_fixture_db(path: &std::path::Path) {
    let conn = Connection::open(path).expect("open fixture db");
    conn.execute_batch(
        "
        CREATE TABLE composerHeaders (
            composerId TEXT PRIMARY KEY,
            workspaceId TEXT,
            createdAt INTEGER,
            lastUpdatedAt INTEGER,
            isArchived INTEGER,
            isSubagent INTEGER,
            recency INTEGER,
            checkpointAt INTEGER,
            subagentTypeName TEXT,
            value TEXT
        );
        CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value TEXT);
        CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT);
        ",
    )
    .expect("create tables");

    let now = 1_700_000_000_000i64;
    let header = r#"{
        "type":"head",
        "composerId":"agent-1",
        "unifiedMode":"agent",
        "isDraft":false,
        "name":"api",
        "workspaceIdentifier":{"uri":{"fsPath":"/Users/test/Projects/api"}}
    }"#;

    conn.execute(
        "INSERT INTO composerHeaders (composerId, workspaceId, createdAt, lastUpdatedAt, isArchived, isSubagent, recency, value)
         VALUES (?1, ?2, ?3, ?4, 0, 0, ?3, ?5)",
        rusqlite::params!["agent-1", "ws-1", now, now, header],
    )
    .expect("insert header");

    let composer_data = r#"{
        "composerId":"agent-1",
        "status":"none",
        "unifiedMode":"agent",
        "isAgentic":true,
        "isDraft":false,
        "generatingBubbleIds":[],
        "isContinuationInProgress":false,
        "hasUnreadMessages":false
    }"#;

    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
        rusqlite::params!["composerData:agent-1", composer_data],
    )
    .expect("insert composer data");
}

#[test]
fn live_cursor_process_is_detected_on_this_machine() {
    assert!(
        agent_hud_lib::sessions::cursor::is_cursor_running(),
        "Cursor.app should be running during local DoD"
    );
}

#[test]
fn live_discover_prints_session_count() {
    let sessions = agent_hud_lib::sessions::cursor::discover_active_sessions();
    eprintln!(
        "active_count={} projects={:?}",
        sessions.len(),
        sessions
            .iter()
            .map(|s| format!("{}|{}:{:?}", s.title, s.project.name, s.status))
            .collect::<Vec<_>>()
    );
}

#[test]
fn fixture_db_has_expected_tables() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("state.vscdb");
    create_fixture_db(&db_path);

    let conn = Connection::open(&db_path).expect("reopen fixture");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM composerHeaders", [], |row| row.get(0))
        .expect("count headers");

    assert_eq!(count, 1);
}

#[test]
fn completed_status_without_fresh_activity_is_inactive() {
    let now = 1_700_000_600_001;
    let (status, active) = map_composer_activity(
        Some("completed"),
        1_700_000_000_000,
        None,
        false,
        false,
        false,
        now,
    );
    assert!(!active);
    assert_eq!(status, AgentStatus::Waiting);
}

#[test]
fn completed_status_with_fresh_header_is_inactive() {
    let now = 1_700_000_010_000;
    let (_, active) = map_composer_activity(
        Some("completed"),
        now,
        None,
        false,
        false,
        false,
        now,
    );
    assert!(!active);
}

#[test]
fn completed_status_with_fresh_transcript_is_inactive() {
    let now = 1_700_000_010_000;
    let (_, active) = map_composer_activity(
        Some("completed"),
        1_700_000_000_000,
        Some(now),
        false,
        false,
        false,
        now,
    );
    assert!(!active);
}

#[test]
fn generating_agent_is_working() {
    let now = 1_700_000_010_000;
    let (status, active) = map_composer_activity(
        Some("none"),
        now,
        Some(now),
        true,
        false,
        false,
        now,
    );
    assert!(active);
    assert_eq!(status, AgentStatus::Working);
}

#[test]
fn pending_interaction_needs_attention() {
    let now = 1_700_000_010_000;
    let (status, active) =
        map_composer_activity(Some("none"), now, None, false, false, true, now);
    assert!(active);
    assert_eq!(status, AgentStatus::NeedsAttention);
}

#[test]
fn stale_none_status_is_inactive() {
    let now = 1_700_000_600_001;
    let (_, active) = map_composer_activity(
        Some("none"),
        1_700_000_000_000,
        None,
        false,
        false,
        false,
        now,
    );
    assert!(!active);
}

#[test]
fn cloud_idle_even_if_recent_is_inactive() {
    let now = 1_700_000_010_000;
    let (_, active) = map_cloud_activity(Some(0), Some(0), now, false, now);
    assert!(!active);
}

#[test]
fn cloud_running_status_is_active() {
    let now = 1_700_000_010_000;
    let (status, active) = map_cloud_activity(Some(1), Some(1), now, false, now);
    assert!(active);
    assert_eq!(status, AgentStatus::Working);
}

fn sample_session(id: &str, status: AgentStatus) -> AgentSession {
    AgentSession {
        id: id.to_string(),
        agent: AgentKind::Cursor,
        title: "Test session".to_string(),
        project: ProjectInfo {
            name: "agent-HUD".to_string(),
            path: Some("/Users/test/Projects/agent-HUD".to_string()),
        },
        status,
        host: SessionHost::CursorDesktop {
            workspace_path: "/Users/test/Projects/agent-HUD".to_string(),
        },
        updated_at: 1_700_000_010_000,
    }
}

#[test]
fn merge_keeps_recently_live_session_as_completed() {
    let state = AppState::new();
    let live = vec![sample_session("agent-1", AgentStatus::Working)];
    let merged = state.merge_sessions(live);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::Working);

    let merged = state.merge_sessions(Vec::new());
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::Completed);
}

#[test]
fn dismiss_removes_completed_session_until_live_again() {
    let state = AppState::new();
    state.merge_sessions(vec![sample_session("agent-1", AgentStatus::Working)]);
    state.merge_sessions(Vec::new());

    let dismissed = state.dismiss_session("agent-1");
    assert!(dismissed.is_empty());

    let merged = state.merge_sessions(Vec::new());
    assert!(merged.is_empty());

    let merged = state.merge_sessions(vec![sample_session("agent-1", AgentStatus::NeedsAttention)]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::NeedsAttention);
}

#[test]
fn cloud_pending_interaction_needs_attention() {
    let now = 1_700_000_010_000;
    let (status, active) = map_cloud_activity(Some(1), Some(1), now, true, now);
    assert!(active);
    assert_eq!(status, AgentStatus::NeedsAttention);
}
