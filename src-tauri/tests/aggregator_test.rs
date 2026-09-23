use agent_hud_lib::sessions::cursor::{
    bubble_is_live_tool, classify, classify_cloud, composer_session_status,
    discover_from_support_paths, is_agent_session, is_waiting_on_user, last_turn_kind,
    LastTurnKind, UNFINISHED_PROMPT_RECENCY_MS,
};
use agent_hud_lib::sessions::{AgentKind, AgentSession, AgentStatus, ProjectInfo, SessionHost};
use agent_hud_lib::state::{AppState, WORKING_HOLD_MS};
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

fn insert_composer(
    conn: &Connection,
    id: &str,
    header: serde_json::Value,
    data: serde_json::Value,
    now: i64,
) {
    conn.execute(
        "INSERT INTO composerHeaders (composerId, workspaceId, createdAt, lastUpdatedAt, isArchived, isSubagent, recency, value)
         VALUES (?1, ?2, ?3, ?4, 0, 0, ?3, ?5)",
        rusqlite::params![id, "ws-1", now, now, header.to_string()],
    )
    .expect("insert header");
    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
        rusqlite::params![format!("composerData:{id}"), data.to_string()],
    )
    .expect("insert composer data");
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

fn last_header(kind: LastTurnKind) -> serde_json::Value {
    match kind {
        LastTurnKind::User => serde_json::json!({ "type": 1 }),
        LastTurnKind::Tool => serde_json::json!({
            "type": 2,
            "grouping": { "capabilityType": 15, "toolFormerStatus": "completed" }
        }),
        LastTurnKind::Thinking => serde_json::json!({
            "type": 2,
            "grouping": { "hasThinking": true }
        }),
        LastTurnKind::AssistantText => serde_json::json!({
            "type": 2,
            "grouping": { "hasText": true, "textPreview": "Need your input" }
        }),
        LastTurnKind::Unknown => serde_json::json!({ "type": 2, "grouping": {} }),
    }
}

fn composer(
    status: &str,
    unfinished: Option<i64>,
    last: Option<LastTurnKind>,
    extra: serde_json::Value,
) -> (serde_json::Value, serde_json::Value) {
    let mut data = extra;
    if let serde_json::Value::Object(map) = &mut data {
        map.insert("status".into(), serde_json::json!(status));
        if let Some(kind) = last {
            map.insert(
                "fullConversationHeadersOnly".into(),
                serde_json::json!([last_header(kind)]),
            );
        }
    }
    let header = match unfinished {
        Some(ts) => serde_json::json!({ "unfinishedRunAt": ts }),
        None => serde_json::json!({}),
    };
    (data, header)
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
fn ask_mode_chat_is_an_agent_session() {
    let header = serde_json::json!({ "unifiedMode": "chat", "isDraft": false });
    let data = serde_json::json!({
        "unifiedMode": "chat",
        "isAgentic": false,
        "status": "aborted"
    });
    assert!(is_agent_session(&header, Some(&data)));
    assert!(is_agent_session(
        &serde_json::json!({ "unifiedMode": "agent" }),
        Some(&serde_json::json!({ "isAgentic": true }))
    ));
    assert!(is_agent_session(
        &serde_json::json!({ "unifiedMode": "plan" }),
        None
    ));
    assert!(!is_agent_session(
        &serde_json::json!({ "unifiedMode": "editor" }),
        Some(&serde_json::json!({ "isAgentic": false }))
    ));
}

#[test]
fn ask_mode_unfinished_tool_run_is_working() {
    let now = 1_700_000_010_000;
    let (data, header) = composer(
        "aborted",
        Some(now - 1_000),
        Some(LastTurnKind::Tool),
        serde_json::json!({
            "unifiedMode": "chat",
            "isAgentic": false
        }),
    );
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::Working)
    );
}

#[test]
fn discover_includes_ask_mode_and_skips_idle_chat() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("state.vscdb");
    let workspace_root = dir.path().join("workspaceStorage");
    let transcripts_root = dir.path().join("agent-transcripts");
    std::fs::create_dir_all(&workspace_root).expect("workspace dir");
    std::fs::create_dir_all(&transcripts_root).expect("transcripts dir");

    let conn = Connection::open(&db_path).expect("open fixture db");
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

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .expect("now");
    insert_composer(
        &conn,
        "ask-live",
        serde_json::json!({
            "composerId": "ask-live",
            "unifiedMode": "chat",
            "isDraft": false,
            "name": "Ask live",
            "unfinishedRunAt": now,
            "workspaceIdentifier": {"uri": {"fsPath": "/Users/test/Projects/ask"}}
        }),
        serde_json::json!({
            "composerId": "ask-live",
            "status": "aborted",
            "unifiedMode": "chat",
            "isAgentic": false,
            "isDraft": false,
            "unfinishedRunAt": now,
            "generatingBubbleIds": [],
            "fullConversationHeadersOnly": [last_header(LastTurnKind::Tool)]
        }),
        now,
    );
    insert_composer(
        &conn,
        "chat-idle",
        serde_json::json!({
            "composerId": "chat-idle",
            "unifiedMode": "chat",
            "isDraft": false,
            "name": "Idle chat",
            "workspaceIdentifier": {"uri": {"fsPath": "/Users/test/Projects/chat"}}
        }),
        serde_json::json!({
            "composerId": "chat-idle",
            "status": "none",
            "unifiedMode": "chat",
            "isAgentic": false,
            "isDraft": false,
            "generatingBubbleIds": []
        }),
        now,
    );
    drop(conn);

    let sessions = discover_from_support_paths(&db_path, &workspace_root, &transcripts_root);
    let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["ask-live"]);
    assert_eq!(sessions[0].status, AgentStatus::Working);
    assert_eq!(sessions[0].title, "Ask live");
}

#[test]
fn classify_live_work_wins() {
    assert_eq!(classify(true, false), Some(AgentStatus::Working));
    assert_eq!(classify(true, true), Some(AgentStatus::Working));
    assert_eq!(classify(false, true), Some(AgentStatus::NeedsAttention));
    assert_eq!(classify(false, false), None);
}

#[test]
fn generating_agent_is_working() {
    let now = 1_700_000_010_000;
    let data = serde_json::json!({ "status": "generating" });
    let header = serde_json::json!({});
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::Working)
    );
}

#[test]
fn live_tool_marks_session_working() {
    let now = 1_700_000_010_000;
    let data = serde_json::json!({ "status": "none" });
    let header = serde_json::json!({});
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, true, now),
        Some(AgentStatus::Working)
    );
}

#[test]
fn last_turn_tool_with_unfinished_is_working() {
    let now = 1_700_000_010_000;
    let (data, header) = composer(
        "aborted",
        Some(now - 60_000),
        Some(LastTurnKind::Tool),
        serde_json::json!({}),
    );
    assert_eq!(last_turn_kind(Some(&data)), LastTurnKind::Tool);
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::Working)
    );
}

#[test]
fn last_turn_user_with_unfinished_is_working() {
    let now = 1_700_000_010_000;
    let (data, header) = composer(
        "aborted",
        Some(now - 1_000),
        Some(LastTurnKind::User),
        serde_json::json!({}),
    );
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::Working)
    );
}

#[test]
fn last_assistant_text_with_unfinished_needs_attention() {
    let now = 1_700_000_010_000;
    let (data, header) = composer(
        "aborted",
        Some(now - 60_000),
        Some(LastTurnKind::AssistantText),
        serde_json::json!({}),
    );
    assert_eq!(last_turn_kind(Some(&data)), LastTurnKind::AssistantText);
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::NeedsAttention)
    );
}

#[test]
fn pending_does_not_beat_generating() {
    let now = 1_700_000_010_000;
    let data = serde_json::json!({
        "status": "generating",
        "hasBlockingPendingActions": true
    });
    let header = serde_json::json!({ "hasBlockingPendingActions": true });
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::Working)
    );
}

#[test]
fn blocking_pending_is_awaiting_user() {
    let now = 1_700_000_010_000;
    let data = serde_json::json!({
        "status": "none",
        "hasBlockingPendingActions": true
    });
    let header = serde_json::json!({});
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::NeedsAttention)
    );
}

#[test]
fn generating_beats_unfinished_assistant_text() {
    let now = 1_700_000_010_000;
    let (data, header) = composer(
        "generating",
        Some(now),
        Some(LastTurnKind::AssistantText),
        serde_json::json!({}),
    );
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::Working)
    );
}

#[test]
fn completed_with_unfinished_and_last_tool_is_working() {
    let now = 1_700_000_010_000;
    let (data, header) = composer(
        "completed",
        Some(now),
        Some(LastTurnKind::Tool),
        serde_json::json!({}),
    );
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        Some(AgentStatus::Working)
    );
}

#[test]
fn completed_without_live_work_is_hidden() {
    let now = 1_700_000_010_000;
    let data = serde_json::json!({ "status": "completed" });
    let header = serde_json::json!({});
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        None
    );
}

#[test]
fn completed_with_assistant_text_is_not_waiting() {
    let now = 1_700_000_010_000;
    let (data, header) = composer(
        "completed",
        Some(now),
        Some(LastTurnKind::AssistantText),
        serde_json::json!({}),
    );
    assert!(!is_waiting_on_user(
        Some("completed"),
        &header,
        Some(&data),
        now,
        None,
        LastTurnKind::AssistantText,
        now,
    ));
    assert_eq!(
        composer_session_status(Some(&data), &header, now, None, false, now),
        None
    );
}

#[test]
fn stale_unfinished_aborted_is_inactive() {
    let now = 1_700_000_010_000;
    let stale = now - UNFINISHED_PROMPT_RECENCY_MS - 1;
    let (data, header) = composer(
        "aborted",
        Some(stale),
        Some(LastTurnKind::AssistantText),
        serde_json::json!({}),
    );
    assert_eq!(
        composer_session_status(Some(&data), &header, stale, None, false, now),
        None
    );
}

#[test]
fn idle_none_status_is_hidden() {
    let now = 1_700_000_010_000;
    let data = serde_json::json!({ "status": "none" });
    let header = serde_json::json!({});
    assert_eq!(
        composer_session_status(Some(&data), &header, now, Some(now), false, now),
        None
    );
}

#[test]
fn last_turn_kind_reads_conversation_headers() {
    let tool = serde_json::json!({
        "fullConversationHeadersOnly": [last_header(LastTurnKind::Tool)]
    });
    assert_eq!(last_turn_kind(Some(&tool)), LastTurnKind::Tool);

    let text = serde_json::json!({
        "fullConversationHeadersOnly": [last_header(LastTurnKind::AssistantText)]
    });
    assert_eq!(last_turn_kind(Some(&text)), LastTurnKind::AssistantText);

    let thinking = serde_json::json!({
        "fullConversationHeadersOnly": [last_header(LastTurnKind::Thinking)]
    });
    assert_eq!(last_turn_kind(Some(&thinking)), LastTurnKind::Thinking);
}

#[test]
fn recent_loading_tool_bubble_is_live() {
    let now = 1_700_000_010_000;
    let bubble = serde_json::json!({
        "toolFormerData": { "status": "loading", "name": "run_terminal_command_v2" },
        "startedAtMs": now - 5_000
    });
    assert!(bubble_is_live_tool(&bubble, now, Some("generating")));
}

#[test]
fn long_running_tool_bubble_stays_live() {
    let now = 1_700_000_010_000;
    let bubble = serde_json::json!({
        "toolFormerData": { "status": "running", "name": "run_terminal_command_v2" },
        "startedAtMs": now - 120_000
    });
    assert!(bubble_is_live_tool(&bubble, now, Some("generating")));
}

#[test]
fn completed_or_stale_loading_tool_bubble_is_not_live() {
    let now = 1_700_000_010_000;
    let completed = serde_json::json!({
        "toolFormerData": { "status": "loading" },
        "startedAtMs": now - 1_000,
        "completedAtMs": now
    });
    assert!(!bubble_is_live_tool(&completed, now, None));

    let stale = serde_json::json!({
        "toolFormerData": { "status": "loading" },
        "startedAtMs": now - 35_000
    });
    assert!(!bubble_is_live_tool(&stale, now, Some("generating")));
}

#[test]
fn loading_tool_ignored_when_composer_completed() {
    let now = 1_700_000_010_000;
    let bubble = serde_json::json!({
        "toolFormerData": { "status": "loading", "name": "read_file_v2" },
        "startedAtMs": now - 5_000
    });
    assert!(bubble_is_live_tool(&bubble, now, Some("aborted")));
    assert!(bubble_is_live_tool(&bubble, now, Some("generating")));
    assert!(!bubble_is_live_tool(&bubble, now, Some("completed")));
}

#[test]
fn cloud_idle_is_hidden() {
    assert_eq!(classify_cloud(Some(0), Some(0), false), None);
}

#[test]
fn cloud_running_is_working() {
    assert_eq!(
        classify_cloud(Some(1), Some(1), false),
        Some(AgentStatus::Working)
    );
}

#[test]
fn cloud_pending_is_awaiting_user() {
    assert_eq!(
        classify_cloud(Some(0), Some(0), true),
        Some(AgentStatus::NeedsAttention)
    );
}

#[test]
fn cloud_running_beats_pending() {
    assert_eq!(
        classify_cloud(Some(1), Some(1), true),
        Some(AgentStatus::Working)
    );
}

#[test]
fn acknowledge_completed_dismisses_but_working_stays() {
    let state = AppState::new();
    let t0 = 1_700_000_010_000;
    state.merge_sessions_at(vec![sample_session("agent-1", AgentStatus::Working)], t0);

    let sessions = state.acknowledge_session("agent-1");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, AgentStatus::Working);

    state.merge_sessions_at(Vec::new(), t0 + WORKING_HOLD_MS + 1);
    let sessions = state.acknowledge_session("agent-1");
    assert!(sessions.is_empty());
}

#[test]
fn acknowledge_needs_attention_stays_visible() {
    let state = AppState::new();
    state.merge_sessions(vec![sample_session("agent-1", AgentStatus::NeedsAttention)]);

    let sessions = state.acknowledge_session("agent-1");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, AgentStatus::NeedsAttention);
}

#[test]
fn merge_holds_working_through_brief_idle_then_completes() {
    let state = AppState::new();
    let t0 = 1_700_000_010_000;
    let merged = state.merge_sessions_at(vec![sample_session("agent-1", AgentStatus::Working)], t0);
    assert_eq!(merged[0].status, AgentStatus::Working);

    let merged = state.merge_sessions_at(Vec::new(), t0 + 1_000);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::Working);

    let merged = state.merge_sessions_at(Vec::new(), t0 + WORKING_HOLD_MS + 1);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::Completed);
}

#[test]
fn merge_keeps_recently_live_session_as_completed() {
    let state = AppState::new();
    let t0 = 1_700_000_010_000;
    let live = vec![sample_session("agent-1", AgentStatus::Working)];
    let merged = state.merge_sessions_at(live, t0);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::Working);

    let merged = state.merge_sessions_at(Vec::new(), t0 + WORKING_HOLD_MS + 1);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::Completed);
}

#[test]
fn merge_debounces_working_to_needs_attention() {
    let state = AppState::new();
    let t0 = 1_700_000_010_000;
    state.merge_sessions_at(vec![sample_session("agent-1", AgentStatus::Working)], t0);

    let merged = state.merge_sessions_at(
        vec![sample_session("agent-1", AgentStatus::NeedsAttention)],
        t0 + 1_000,
    );
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::Working);

    let merged = state.merge_sessions_at(
        vec![sample_session("agent-1", AgentStatus::NeedsAttention)],
        t0 + WORKING_HOLD_MS + 1,
    );
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::NeedsAttention);
}

#[test]
fn dismiss_removes_completed_session_until_live_again() {
    let state = AppState::new();
    let t0 = 1_700_000_010_000;
    state.merge_sessions_at(vec![sample_session("agent-1", AgentStatus::Working)], t0);
    state.merge_sessions_at(Vec::new(), t0 + WORKING_HOLD_MS + 1);

    let dismissed = state.dismiss_session("agent-1");
    assert!(dismissed.is_empty());

    let merged = state.merge_sessions_at(Vec::new(), t0 + WORKING_HOLD_MS + 2);
    assert!(merged.is_empty());

    let merged = state.merge_sessions_at(
        vec![sample_session("agent-1", AgentStatus::NeedsAttention)],
        t0 + WORKING_HOLD_MS + 3,
    );
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::NeedsAttention);
}

#[test]
fn dismiss_awaiting_stays_hidden_until_status_changes() {
    let state = AppState::new();
    state.merge_sessions(vec![sample_session("agent-1", AgentStatus::NeedsAttention)]);

    let dismissed = state.dismiss_session("agent-1");
    assert!(dismissed.is_empty());

    let merged = state.merge_sessions(vec![sample_session("agent-1", AgentStatus::NeedsAttention)]);
    assert!(merged.is_empty());

    let merged = state.merge_sessions(vec![sample_session("agent-1", AgentStatus::Working)]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].status, AgentStatus::Working);
}
