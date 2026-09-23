use std::fs;
use std::path::Path;

use agent_hud_lib::focus::{
    cursor_background_agent_url, focus_plan, ghostty_focus_script, iterm_focus_script,
    parse_tmux_clients, parse_tmux_panes, plan_terminal_focus, terminal_app_focus_script,
    tmux_focus_args, FocusPlan, TmuxClient, TmuxPane, TmuxTarget, UNIDENTIFIED_TERMINAL,
};
use agent_hud_lib::sessions::cli::{
    conversation_ids_from_paths, discover_cli_sessions, inspect_lsof_fn, is_cli_agent_command,
    normalize_tty, parse_ps_row, transcript_turn_open, AgentProcess, CliRoots,
};
use agent_hud_lib::sessions::cursor::discover_all;
use agent_hud_lib::sessions::{AgentKind, AgentSession, AgentStatus, ProjectInfo, SessionHost};
use agent_hud_lib::state::{AppState, WORKING_HOLD_MS};
use rusqlite::Connection;
use tempfile::tempdir;

fn roots(dir: &Path) -> CliRoots {
    CliRoots {
        chats_dir: dir.join("chats"),
        projects_dir: dir.join("projects"),
    }
}

fn write_chat(
    dir: &Path,
    id: &str,
    cwd: &str,
    title: Option<&str>,
    updated: i64,
    transcript: Option<&str>,
) {
    let chat_dir = dir.join("chats").join("hash").join(id);
    fs::create_dir_all(&chat_dir).expect("chat dir");
    let mut meta = serde_json::json!({
        "schemaVersion": 1,
        "createdAtMs": updated,
        "hasConversation": transcript.is_some(),
        "updatedAtMs": updated,
        "cwd": cwd,
    });
    if let Some(title) = title {
        meta["title"] = serde_json::json!(title);
    }
    fs::write(chat_dir.join("meta.json"), meta.to_string()).expect("meta");
    if let Some(body) = transcript {
        let transcript_dir = dir
            .join("projects")
            .join("slug")
            .join("agent-transcripts")
            .join(id);
        fs::create_dir_all(&transcript_dir).expect("transcript dir");
        fs::write(transcript_dir.join(format!("{id}.jsonl")), body).expect("transcript");
    }
}

fn process(pid: u32, cwd: &str, tty: &str, conversation_id: Option<&str>) -> AgentProcess {
    AgentProcess {
        pid,
        cwd: cwd.to_string(),
        tty: Some(tty.to_string()),
        conversation_id: conversation_id.map(|id| id.to_string()),
    }
}

fn cli_tty(session: &AgentSession) -> Option<String> {
    match &session.host {
        SessionHost::CursorCli { tty, .. } => tty.clone(),
        other => panic!("expected cli host, got {other:?}"),
    }
}

fn user_line() -> &'static str {
    r#"{"role":"user","message":{"content":[{"type":"text","text":"ping"}]}}"#
}

fn assistant_line() -> &'static str {
    r#"{"role":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}"#
}

fn ended(status: &str) -> String {
    format!(r#"{{"type":"turn_ended","status":"{status}"}}"#)
}

#[test]
fn installed_turn_shape_classifies_open_and_closed_turns() {
    assert!(transcript_turn_open(&format!(
        "{}\n{}",
        user_line(),
        assistant_line()
    )));
    assert!(!transcript_turn_open(&format!(
        "{}\n{}\n{}",
        user_line(),
        assistant_line(),
        ended("success")
    )));
    assert!(!transcript_turn_open(&format!(
        "{}\n{}",
        user_line(),
        ended("error")
    )));
    assert!(!transcript_turn_open(&format!(
        "{}\n{}",
        user_line(),
        ended("aborted")
    )));
    assert!(transcript_turn_open(&format!(
        "{}\n{}\n{}",
        user_line(),
        ended("success"),
        user_line()
    )));
    assert!(!transcript_turn_open(assistant_line()));
    assert!(!transcript_turn_open(""));
}

#[test]
fn agent_command_and_store_path_parsers_ignore_unrelated_processes() {
    assert!(is_cli_agent_command("/Users/me/.local/bin/agent"));
    assert!(is_cli_agent_command("cursor-agent"));
    assert!(!is_cli_agent_command(
        "/Applications/Cursor.app/Contents/MacOS/Cursor"
    ));
    assert!(!is_cli_agent_command("node"));
    assert!(!is_cli_agent_command("agent-hud"));

    assert_eq!(
        parse_ps_row("  305 99199 ttys011  /Users/me/.local/bin/cursor-agent"),
        Some((
            305,
            "ttys011".to_string(),
            "/Users/me/.local/bin/cursor-agent".to_string()
        ))
    );

    let store = "/Users/me/.cursor/chats/abc/conv-1/store.db";
    assert_eq!(
        conversation_ids_from_paths([store, "/tmp/other"]),
        Some("conv-1".to_string())
    );
    assert_eq!(
        conversation_ids_from_paths([store, "/Users/me/.cursor/chats/abc/conv-2/store.db",]),
        None
    );
    assert_eq!(
        conversation_ids_from_paths(["/Users/me/.cursor/chats/abc/conv-1/store.db-wal"]),
        None
    );

    let lsof = "\
p305
fcwd
n/Users/test/Projects/api
f8
n/Users/me/.cursor/chats/hash/conv-9/store.db
";
    assert_eq!(
        inspect_lsof_fn(lsof),
        (
            Some("/Users/test/Projects/api".to_string()),
            Some("conv-9".to_string())
        )
    );
    assert_eq!(normalize_tty("ttys010"), Some("/dev/ttys010".to_string()));
    assert_eq!(
        normalize_tty("/dev/ttys010"),
        Some("/dev/ttys010".to_string())
    );
    assert_eq!(normalize_tty("??"), None);
}

#[test]
fn historical_chat_without_a_live_process_or_open_turn_is_hidden() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(
        dir.path(),
        "old",
        cwd,
        Some("Old chat"),
        10,
        Some(&format!("{}\n{}", user_line(), ended("success"))),
    );

    let sessions = discover_cli_sessions(&roots(dir.path()), &[]);
    assert!(sessions.is_empty());
}

#[test]
fn open_turn_is_listed_even_without_a_process() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(
        dir.path(),
        "live",
        cwd,
        None,
        20,
        Some(&format!("{}\n{}", user_line(), assistant_line())),
    );

    let sessions = discover_cli_sessions(&roots(dir.path()), &[]);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "live");
    assert_eq!(sessions[0].title, "api");
    assert_eq!(sessions[0].project.name, "api");
    assert_eq!(sessions[0].status, AgentStatus::Working);
    assert_eq!(cli_tty(&sessions[0]), None);
    assert_eq!(
        sessions[0].host,
        SessionHost::CursorCli {
            workspace_path: cwd.to_string(),
            tty: None,
        }
    );
}

#[test]
fn title_is_used_when_present_and_process_tty_is_kept() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(
        dir.path(),
        "conv-1",
        cwd,
        Some("Fix the parser"),
        30,
        Some(&format!("{}\n{}", user_line(), ended("success"))),
    );

    let sessions = discover_cli_sessions(
        &roots(dir.path()),
        &[process(7, cwd, "ttys004", Some("conv-1"))],
    );
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title, "Fix the parser");
    assert_eq!(sessions[0].project.name, "api");
    assert_eq!(cli_tty(&sessions[0]), Some("/dev/ttys004".to_string()));
}

#[test]
fn several_agents_in_one_workspace_show_only_an_open_turn_without_a_tty() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(dir.path(), "open", cwd, Some("Open"), 40, Some(user_line()));
    write_chat(
        dir.path(),
        "closed",
        cwd,
        Some("Closed"),
        50,
        Some(&format!("{}\n{}", user_line(), ended("success"))),
    );

    let sessions = discover_cli_sessions(
        &roots(dir.path()),
        &[
            process(1, cwd, "ttys001", None),
            process(2, cwd, "ttys002", None),
        ],
    );
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "open");
    assert_eq!(cli_tty(&sessions[0]), None);
}

#[test]
fn store_binding_ties_each_conversation_to_its_process() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(
        dir.path(),
        "conv-a",
        cwd,
        Some("A"),
        10,
        Some(&ended("success")),
    );
    write_chat(
        dir.path(),
        "conv-b",
        cwd,
        Some("B"),
        20,
        Some(&ended("aborted")),
    );
    write_chat(
        dir.path(),
        "conv-c",
        cwd,
        Some("Historical"),
        30,
        Some(&ended("error")),
    );

    let sessions = discover_cli_sessions(
        &roots(dir.path()),
        &[
            process(1, cwd, "ttys001", Some("conv-a")),
            process(2, cwd, "ttys002", Some("conv-b")),
        ],
    );
    let mut found: Vec<(String, Option<String>)> = sessions
        .iter()
        .map(|session| (session.id.clone(), cli_tty(session)))
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            ("conv-a".to_string(), Some("/dev/ttys001".to_string())),
            ("conv-b".to_string(), Some("/dev/ttys002".to_string())),
        ]
    );
}

#[test]
fn open_turn_does_not_borrow_another_conversations_terminal() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(
        dir.path(),
        "conv-a",
        cwd,
        Some("A"),
        10,
        Some(&ended("success")),
    );
    write_chat(dir.path(), "conv-b", cwd, Some("B"), 20, Some(user_line()));

    let sessions = discover_cli_sessions(
        &roots(dir.path()),
        &[process(1, cwd, "ttys001", Some("conv-a"))],
    );
    let b = sessions
        .iter()
        .find(|session| session.id == "conv-b")
        .unwrap();
    assert_eq!(cli_tty(b), None);
    let a = sessions
        .iter()
        .find(|session| session.id == "conv-a")
        .unwrap();
    assert_eq!(cli_tty(a), Some("/dev/ttys001".to_string()));
}

#[test]
fn one_unbound_process_does_not_guess_among_closed_chats() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(
        dir.path(),
        "older",
        cwd,
        Some("Older"),
        10,
        Some(&ended("success")),
    );
    write_chat(
        dir.path(),
        "newer",
        cwd,
        Some("Newer"),
        20,
        Some(&ended("success")),
    );
    let sessions = discover_cli_sessions(&roots(dir.path()), &[process(4, cwd, "ttys009", None)]);
    assert!(sessions.is_empty());
}

#[test]
fn one_unbound_process_ties_the_only_open_turn_in_the_workspace() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(
        dir.path(),
        "closed",
        cwd,
        Some("Closed"),
        10,
        Some(&ended("success")),
    );
    write_chat(dir.path(), "open", cwd, Some("Open"), 20, Some(user_line()));
    let sessions = discover_cli_sessions(&roots(dir.path()), &[process(4, cwd, "ttys009", None)]);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "open");
    assert_eq!(cli_tty(&sessions[0]), Some("/dev/ttys009".to_string()));
}

#[test]
fn single_unbound_process_matches_the_only_chat_in_its_workspace() {
    let dir = tempdir().expect("tempdir");
    let cwd = "/Users/test/Projects/api";
    write_chat(dir.path(), "only", cwd, None, 5, Some(&ended("success")));
    let sessions = discover_cli_sessions(&roots(dir.path()), &[process(4, cwd, "ttys009", None)]);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "only");
    assert_eq!(sessions[0].title, "api");
    assert_eq!(cli_tty(&sessions[0]), Some("/dev/ttys009".to_string()));
}

#[test]
fn desktop_discovery_stays_alongside_cli_sessions() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("state.vscdb");
    let workspace_root = dir.path().join("workspaceStorage");
    let transcripts_root = dir.path().join("desktop-transcripts");
    fs::create_dir_all(&workspace_root).unwrap();
    fs::create_dir_all(&transcripts_root).unwrap();

    let conn = Connection::open(&db_path).unwrap();
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
    .unwrap();
    let now = 1_700_000_000_000i64;
    let header = r#"{"composerId":"desktop-1","unifiedMode":"agent","isDraft":false,"name":"Desktop","workspaceIdentifier":{"uri":{"fsPath":"/Users/test/Projects/desktop"}}}"#;
    let data = r#"{"composerId":"desktop-1","status":"generating","unifiedMode":"agent","isAgentic":true,"isDraft":false,"generatingBubbleIds":["b1"]}"#;
    conn.execute(
        "INSERT INTO composerHeaders (composerId, workspaceId, createdAt, lastUpdatedAt, isArchived, isSubagent, recency, value)
         VALUES (?1, 'ws', ?2, ?2, 0, 0, ?2, ?3)",
        rusqlite::params!["desktop-1", now, header],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cursorDiskKV (key, value) VALUES ('composerData:desktop-1', ?1)",
        rusqlite::params![data],
    )
    .unwrap();
    drop(conn);

    let cwd = "/Users/test/Projects/api";
    write_chat(
        dir.path(),
        "desktop-1",
        cwd,
        Some("CLI"),
        40,
        Some(user_line()),
    );
    write_chat(
        dir.path(),
        "cli-2",
        cwd,
        Some("Other"),
        50,
        Some(user_line()),
    );

    let sessions = discover_all(
        &db_path,
        &workspace_root,
        &transcripts_root,
        &roots(dir.path()),
        &[process(3, cwd, "ttys003", Some("cli-2"))],
    );
    let desktop = sessions
        .iter()
        .find(|session| session.id == "desktop-1")
        .unwrap();
    assert!(matches!(desktop.host, SessionHost::CursorDesktop { .. }));
    assert_eq!(desktop.title, "Desktop");
    let collided = sessions
        .iter()
        .find(|session| session.id == "cli:desktop-1")
        .unwrap();
    assert!(matches!(collided.host, SessionHost::CursorCli { .. }));
    let cli = sessions
        .iter()
        .find(|session| session.id == "cli-2")
        .unwrap();
    assert_eq!(cli_tty(cli), Some("/dev/ttys003".to_string()));
    assert!(sessions.iter().all(|session| {
        !matches!(session.host, SessionHost::CursorCli { .. }) || session.id != "desktop-1"
    }));
}

#[test]
fn cli_session_uses_the_existing_working_hold() {
    let state = AppState::new();
    let t0 = 1_700_000_010_000;
    let session = AgentSession {
        id: "conv-1".to_string(),
        agent: AgentKind::Cursor,
        title: "Fix the parser".to_string(),
        project: ProjectInfo {
            name: "api".to_string(),
            path: Some("/Users/test/Projects/api".to_string()),
        },
        status: AgentStatus::Working,
        host: SessionHost::CursorCli {
            workspace_path: "/Users/test/Projects/api".to_string(),
            tty: Some("/dev/ttys004".to_string()),
        },
        updated_at: t0,
    };
    state.merge_sessions_at(vec![session], t0);
    let held = state.merge_sessions_at(Vec::new(), t0 + 1_000);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].status, AgentStatus::Working);
    let finished = state.merge_sessions_at(Vec::new(), t0 + WORKING_HOLD_MS + 1);
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].status, AgentStatus::Completed);
}

#[test]
fn click_routing_keeps_desktop_and_cloud_on_the_background_agent_link() {
    let desktop = AgentSession {
        id: "desktop-1".to_string(),
        agent: AgentKind::Cursor,
        title: "Desktop".to_string(),
        project: ProjectInfo {
            name: "desktop".to_string(),
            path: Some("/Users/test/Projects/desktop".to_string()),
        },
        status: AgentStatus::Working,
        host: SessionHost::CursorDesktop {
            workspace_path: "/Users/test/Projects/desktop".to_string(),
        },
        updated_at: 1,
    };
    let cloud = AgentSession {
        host: SessionHost::CursorCloud {
            workspace_path: None,
        },
        id: "bc-9".to_string(),
        title: "Cloud".to_string(),
        project: ProjectInfo {
            name: "cloud".to_string(),
            path: None,
        },
        ..desktop.clone()
    };
    assert_eq!(
        focus_plan(&desktop, &[], &[]).unwrap(),
        FocusPlan::CursorLink(cursor_background_agent_url("desktop-1"))
    );
    assert_eq!(
        focus_plan(&cloud, &[], &[]).unwrap(),
        FocusPlan::CursorLink(cursor_background_agent_url("bc-9"))
    );
    assert!(cursor_background_agent_url("desktop-1").starts_with("cursor://"));
}

#[test]
fn cli_click_focuses_the_tty_and_tmux_client_when_the_agent_is_inside_tmux() {
    let direct = AgentSession {
        id: "conv-1".to_string(),
        agent: AgentKind::Cursor,
        title: "CLI".to_string(),
        project: ProjectInfo {
            name: "api".to_string(),
            path: Some("/Users/test/Projects/api".to_string()),
        },
        status: AgentStatus::Working,
        host: SessionHost::CursorCli {
            workspace_path: "/Users/test/Projects/api".to_string(),
            tty: Some("/dev/ttys004".to_string()),
        },
        updated_at: 1,
    };
    match focus_plan(&direct, &[], &[]).unwrap() {
        FocusPlan::Terminal { tty, tmux } => {
            assert_eq!(tty, "/dev/ttys004");
            assert!(tmux.is_none());
            assert!(!tty.contains("cursor://"));
        }
        other => panic!("unexpected plan {other:?}"),
    }

    let inside = AgentSession {
        host: SessionHost::CursorCli {
            workspace_path: "/Users/test/Projects/api".to_string(),
            tty: Some("ttys011".to_string()),
        },
        ..direct.clone()
    };
    let panes = vec![TmuxPane {
        pane_id: "%2".to_string(),
        tty: "/dev/ttys011".to_string(),
        session_id: "$0".to_string(),
        window_id: "@2".to_string(),
    }];
    let clients = vec![TmuxClient {
        tty: "/dev/ttys000".to_string(),
        session_id: "$0".to_string(),
    }];
    match focus_plan(&inside, &panes, &clients).unwrap() {
        FocusPlan::Terminal { tty, tmux } => {
            assert_eq!(tty, "/dev/ttys000");
            let tmux = tmux.expect("tmux target");
            assert_eq!(tmux.pane_id, "%2");
            assert_eq!(tmux.client_tty, "/dev/ttys000");
            assert!(!format!("{tmux:?}").contains("cursor://"));
        }
        other => panic!("unexpected plan {other:?}"),
    }

    let detached = plan_terminal_focus("ttys011", &panes, &[]);
    assert_eq!(detached.unwrap_err(), UNIDENTIFIED_TERMINAL);

    let unknown = AgentSession {
        host: SessionHost::CursorCli {
            workspace_path: "/Users/test/Projects/api".to_string(),
            tty: None,
        },
        ..direct
    };
    assert_eq!(
        focus_plan(&unknown, &panes, &clients).unwrap_err(),
        UNIDENTIFIED_TERMINAL
    );
}

#[test]
fn tmux_listing_and_focus_args_reject_unidentified_targets() {
    let panes = parse_tmux_panes("%2 /dev/ttys011 $0 @2\nbogus line\n%nope /dev/ttys011 $0 @2\n");
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].pane_id, "%2");
    let clients = parse_tmux_clients("/dev/ttys000 $0\nnot-a-tty $0\n");
    assert_eq!(clients.len(), 1);

    let args = tmux_focus_args(&TmuxTarget {
        client_tty: "/dev/ttys000".to_string(),
        session_id: "$0".to_string(),
        window_id: "@2".to_string(),
        pane_id: "%2".to_string(),
    })
    .unwrap();
    assert_eq!(
        args,
        vec![
            "switch-client",
            "-c",
            "/dev/ttys000",
            "-t",
            "$0",
            ";",
            "select-window",
            "-t",
            "@2",
            ";",
            "select-pane",
            "-t",
            "%2",
        ]
    );
    assert!(tmux_focus_args(&TmuxTarget {
        client_tty: "/dev/ttys000;touch".to_string(),
        session_id: "$0".to_string(),
        window_id: "@2".to_string(),
        pane_id: "%2".to_string(),
    })
    .is_err());
}

#[test]
fn terminal_scripts_target_the_tty_and_not_the_desktop_link() {
    let ghostty = ghostty_focus_script(Some("/Users/test/Projects/api"));
    assert!(ghostty.contains("/Users/test/Projects/api"));
    assert!(ghostty.contains("focus theTerm"));
    assert!(!ghostty.contains("cursor://"));

    let terminal = terminal_app_focus_script("/dev/ttys000");
    assert!(terminal.contains("/dev/ttys000"));
    assert!(terminal.contains("tty of t"));
    assert!(!terminal.contains("cursor://"));

    let iterm = iterm_focus_script("/dev/ttys000");
    assert!(iterm.contains("/dev/ttys000"));
    assert!(iterm.contains("tty of s"));
    assert!(!iterm.contains("cursor://"));

    let single = ghostty_focus_script(None);
    assert!(single.contains("count of windows"));
    assert!(!single.contains("cursor://"));
}
