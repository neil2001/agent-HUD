use agent_hud_lib::sessions::cursor::{map_cloud_activity, map_composer_activity};
use agent_hud_lib::sessions::AgentStatus;
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
            .map(|s| format!("{}:{:?}", s.project.name, s.status))
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
    assert_eq!(status, AgentStatus::Working);
}

#[test]
fn completed_status_with_fresh_header_is_waiting() {
    let now = 1_700_000_010_000;
    let (status, active) = map_composer_activity(
        Some("completed"),
        now,
        None,
        false,
        false,
        false,
        now,
    );
    assert!(active);
    assert_eq!(status, AgentStatus::Waiting);
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
fn cloud_running_status_is_active() {
    let now = 1_700_000_010_000;
    let (status, active) = map_cloud_activity(Some(1), Some(1), now, false, now);
    assert!(active);
    assert_eq!(status, AgentStatus::Working);
}

#[test]
fn cloud_pending_interaction_needs_attention() {
    let now = 1_700_000_010_000;
    let (status, active) = map_cloud_activity(Some(1), Some(1), now, true, now);
    assert!(active);
    assert_eq!(status, AgentStatus::NeedsAttention);
}
