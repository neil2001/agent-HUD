use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde_json::Value;

use super::sqlite::{
    json_from_row, open_readonly, read_disk_kv_json, read_item_json, row_bool, row_opt_i64,
};
use super::{AgentKind, AgentSession, AgentStatus, ProjectInfo, SessionHost};

const STALE_MS: i64 = 600_000;
const TRANSCRIPT_FRESH_MS: i64 = 600_000;

pub fn cursor_support_paths() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let home = dirs::home_dir()?;
    let global_db = home
        .join("Library/Application Support/Cursor/User/globalStorage/state.vscdb");
    let workspace_root = home.join("Library/Application Support/Cursor/User/workspaceStorage");
    let transcripts_root = home.join(".cursor/projects");
    Some((global_db, workspace_root, transcripts_root))
}

pub fn is_cursor_running() -> bool {
    if pgrep_cursor() {
        return true;
    }

    // `pgrep` is often blocked for GUI binaries launched from Cursor's sandbox.
    // Fall back to a recent write on Cursor's DB.
    let Some((global_db, _, _)) = cursor_support_paths() else {
        return false;
    };
    let wal = global_db.with_extension("vscdb-wal");
    let probe = if wal.exists() { wal } else { global_db };
    fs::metadata(probe)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(system_time_to_ms)
        .map(|modified| now_ms() - modified <= 60_000)
        .unwrap_or(true)
}

fn pgrep_cursor() -> bool {
    Command::new("/usr/bin/pgrep")
        .args(["-f", "/Applications/Cursor.app/Contents/MacOS/Cursor"])
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

pub fn discover_active_sessions() -> Vec<AgentSession> {
    let Some((global_db, workspace_root, transcripts_root)) = cursor_support_paths() else {
        return Vec::new();
    };

    if !global_db.exists() {
        return Vec::new();
    }

    let conn = match open_readonly(&global_db) {
        Ok(conn) => conn,
        Err(_) => return Vec::new(),
    };

    let now_ms = now_ms();
    let transcript_activity = recent_transcript_activity(&transcripts_root, now_ms);
    let mut sessions = HashMap::new();

    collect_from_composer_headers(&conn, now_ms, &transcript_activity, &mut sessions);
    collect_from_cloud_agents(&conn, now_ms, &mut sessions);
    collect_from_workspace_fallback(&workspace_root, now_ms, &transcript_activity, &mut sessions);

    let mut active: Vec<AgentSession> = sessions
        .into_values()
        .filter(|session| session.is_active())
        .collect();

    sort_sessions(&mut active);
    active
}

fn collect_from_composer_headers(
    conn: &Connection,
    now_ms: i64,
    transcript_activity: &HashMap<String, i64>,
    out: &mut HashMap<String, AgentSession>,
) {
    let headers = read_composer_headers(conn);
    for header in headers {
        if header.is_archived || header.is_subagent {
            continue;
        }

        let composer_data = read_disk_kv_json(conn, &format!("composerData:{}", header.id))
            .ok()
            .flatten();

        if is_draft(&header.value, composer_data.as_ref()) {
            continue;
        }

        if !is_agent_session(&header.value, composer_data.as_ref()) {
            continue;
        }

        let data_updated = composer_data.as_ref().and_then(|data| {
            data.get("lastUpdatedAt")
                .or_else(|| data.get("updatedAt"))
                .and_then(|v| v.as_i64())
        });
        let updated_at = [
            header.last_updated_at,
            header.recency,
            header.created_at,
            data_updated,
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(0);

        let transcript_ts = transcript_activity.get(&header.id).copied();
        let status_str = composer_data
            .as_ref()
            .and_then(|data| data.get("status"))
            .and_then(|v| v.as_str());

        let generating = composer_data
            .as_ref()
            .and_then(|data| data.get("generatingBubbleIds"))
            .map(value_is_nonempty)
            .unwrap_or(false);

        let continuation = composer_data
            .as_ref()
            .and_then(|data| data.get("isContinuationInProgress"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let pending = composer_data
            .as_ref()
            .and_then(|data| data.get("hasUnreadMessages"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let (status, active) = map_composer_activity(
            status_str,
            updated_at,
            transcript_ts,
            generating,
            continuation,
            pending,
            now_ms,
        );

        if !active {
            continue;
        }

        let workspace_path = workspace_path_from_value(&header.value);
        let project_name = project_name_from_path(&workspace_path, &header.value);

        out.insert(
            header.id.clone(),
            AgentSession {
                id: header.id.clone(),
                agent: AgentKind::Cursor,
                project: ProjectInfo {
                    name: project_name,
                    path: workspace_path.clone(),
                },
                status,
                host: match workspace_path {
                    Some(path) => SessionHost::CursorDesktop { workspace_path: path },
                    None => SessionHost::CursorCloud { workspace_path: None },
                },
                updated_at,
            },
        );
    }
}

fn collect_from_cloud_agents(conn: &Connection, now_ms: i64, out: &mut HashMap<String, AgentSession>) {
    let keys = cloud_agent_keys(conn);
    for key in keys {
        let Some(agents) = read_item_json(conn, &key).ok().flatten() else {
            continue;
        };
        let Some(agents) = agents.as_array() else {
            continue;
        };

        for agent in agents {
            let Some(id) = agent.get("bcId").and_then(|v| v.as_str()) else {
                continue;
            };

            if agent
                .get("isArchived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                continue;
            }

            if agent.get("isKilled").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }

            let updated_at = agent
                .get("lastMessageActivityAtMs")
                .or_else(|| agent.get("updatedAt"))
                .or_else(|| agent.get("rowUpdatedAtMs"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let pending = agent
                .get("hasPendingInteraction")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let status_code = agent.get("status").and_then(|v| v.as_i64());
            let workflow = agent.get("workflowStatus").and_then(|v| v.as_i64());

            let (status, active) = map_cloud_activity(
                status_code,
                workflow,
                updated_at,
                pending,
                now_ms,
            );

            if !active {
                continue;
            }

            let workspace_path = agent
                .get("workspaceRootPath")
                .and_then(|v| v.as_str())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string());

            let name = agent
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|n| !n.is_empty())
                .map(|n| n.to_string())
                .or_else(|| {
                    workspace_path.as_deref().and_then(|p| {
                        Path::new(p)
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                    })
                })
                .unwrap_or_else(|| "cloud".to_string());

            out.entry(id.to_string()).or_insert(AgentSession {
                id: id.to_string(),
                agent: AgentKind::Cursor,
                project: ProjectInfo {
                    name,
                    path: workspace_path.clone(),
                },
                status,
                host: SessionHost::CursorCloud { workspace_path },
                updated_at,
            });
        }
    }
}

fn collect_from_workspace_fallback(
    workspace_root: &Path,
    now_ms: i64,
    transcript_activity: &HashMap<String, i64>,
    out: &mut HashMap<String, AgentSession>,
) {
    let entries = fs::read_dir(workspace_root).ok();
    let Some(entries) = entries else {
        return;
    };

    for entry in entries.flatten() {
        let db_path = entry.path().join("state.vscdb");
        if !db_path.exists() {
            continue;
        }

        let conn = match open_readonly(&db_path) {
            Ok(conn) => conn,
            Err(_) => continue,
        };

        let composer_data = read_item_json(&conn, "composer.composerData")
            .ok()
            .flatten();
        let Some(composer_data) = composer_data else {
            continue;
        };

        let workspace_path = workspace_json_path(&entry.path().join("workspace.json"));
        let composers = composer_data
            .get("allComposers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        for composer in composers {
            let Some(id) = composer.get("composerId").and_then(|v| v.as_str()) else {
                continue;
            };

            if out.contains_key(id) {
                continue;
            }

            if composer.get("isArchived").and_then(|v| v.as_bool()).unwrap_or(false) {
                continue;
            }

            if is_draft(&composer, None) || !is_agent_session(&composer, None) {
                continue;
            }

            let updated_at = composer
                .get("lastUpdatedAt")
                .or_else(|| composer.get("createdAt"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let transcript_ts = transcript_activity.get(id).copied();
            let (status, active) = map_composer_activity(
                None,
                updated_at,
                transcript_ts,
                false,
                false,
                false,
                now_ms,
            );

            if !active {
                continue;
            }

            let project_name = project_name_from_path(&workspace_path, &composer);
            out.insert(
                id.to_string(),
                AgentSession {
                    id: id.to_string(),
                    agent: AgentKind::Cursor,
                    project: ProjectInfo {
                        name: project_name,
                        path: workspace_path.clone(),
                    },
                    status,
                    host: match workspace_path.clone() {
                        Some(path) => SessionHost::CursorDesktop { workspace_path: path },
                        None => SessionHost::CursorCloud { workspace_path: None },
                    },
                    updated_at,
                },
            );
        }
    }
}

struct HeaderRow {
    id: String,
    created_at: Option<i64>,
    last_updated_at: Option<i64>,
    recency: Option<i64>,
    is_archived: bool,
    is_subagent: bool,
    value: Value,
}

fn read_composer_headers(conn: &Connection) -> Vec<HeaderRow> {
    let mut stmt = match conn.prepare(
        "SELECT composerId, createdAt, lastUpdatedAt, isArchived, isSubagent, recency, value FROM composerHeaders",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };

    let rows = match stmt.query_map([], |row| {
        let value = json_from_row(row.get_ref(6)?)?;
        Ok(HeaderRow {
            id: row.get(0)?,
            created_at: row_opt_i64(row, 1),
            last_updated_at: row_opt_i64(row, 2),
            is_archived: row_bool(row, 3),
            is_subagent: row_bool(row, 4),
            recency: row_opt_i64(row, 5),
            value,
        })
    }) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    rows.filter_map(|row| row.ok()).collect()
}

fn cloud_agent_keys(conn: &Connection) -> Vec<String> {
    let mut stmt = match conn.prepare(
        "SELECT key FROM ItemTable WHERE key LIKE 'cloudAgentRepository.agents%'",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };

    let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
        Ok(rows) => rows,
        Err(_) => return Vec::new(),
    };

    rows.filter_map(|row| row.ok()).collect()
}

fn recent_transcript_activity(root: &Path, now_ms: i64) -> HashMap<String, i64> {
    let mut activity = HashMap::new();
    if !root.exists() {
        return activity;
    }

    let projects = fs::read_dir(root).ok();
    let Some(projects) = projects else {
        return activity;
    };

    for project in projects.flatten() {
        let transcripts_dir = project.path().join("agent-transcripts");
        if !transcripts_dir.is_dir() {
            continue;
        }

        collect_transcript_dir(&transcripts_dir, now_ms, &mut activity);
    }

    activity
}

fn collect_transcript_dir(dir: &Path, now_ms: i64, out: &mut HashMap<String, i64>) {
    let entries = fs::read_dir(dir).ok();
    let Some(entries) = entries else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_transcript_dir(&path, now_ms, out);
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let modified = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(system_time_to_ms);

        let Some(modified) = modified else {
            continue;
        };

        if now_ms - modified > TRANSCRIPT_FRESH_MS {
            continue;
        }

        let session_id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();

        if session_id.is_empty() {
            continue;
        }

        out.insert(session_id, modified);
    }
}

pub fn map_composer_activity(
    status: Option<&str>,
    updated_at: i64,
    transcript_ts: Option<i64>,
    generating: bool,
    continuation: bool,
    pending: bool,
    now_ms: i64,
) -> (AgentStatus, bool) {
    let fresh = updated_at > 0 && now_ms - updated_at <= STALE_MS;
    let transcript_fresh = transcript_ts
        .map(|ts| now_ms - ts <= TRANSCRIPT_FRESH_MS)
        .unwrap_or(false);

    if matches!(status, Some("completed") | Some("aborted") | Some("error")) {
        if transcript_fresh || generating || continuation {
            return (AgentStatus::Working, true);
        }
        if pending && fresh {
            return (AgentStatus::NeedsAttention, true);
        }
        if fresh {
            // Cursor often leaves status=completed while a follow-up turn is still live.
            return (AgentStatus::Waiting, true);
        }
        return (AgentStatus::Working, false);
    }

    if !fresh && !transcript_fresh && !generating && !continuation {
        return (AgentStatus::Working, false);
    }

    if pending {
        return (AgentStatus::NeedsAttention, true);
    }

    if generating || continuation || transcript_fresh {
        return (AgentStatus::Working, true);
    }

    if fresh {
        return (AgentStatus::Waiting, true);
    }

    (AgentStatus::Working, false)
}

pub fn map_cloud_activity(
    status: Option<i64>,
    workflow: Option<i64>,
    updated_at: i64,
    pending: bool,
    now_ms: i64,
) -> (AgentStatus, bool) {
    let fresh = updated_at > 0 && now_ms - updated_at <= STALE_MS;
    let running = matches!(status, Some(1)) || matches!(workflow, Some(1));

    if !fresh && !running {
        return (AgentStatus::Working, false);
    }

    if pending {
        return (AgentStatus::NeedsAttention, true);
    }

    if running || fresh {
        return (AgentStatus::Working, true);
    }

    (AgentStatus::Waiting, true)
}

fn is_agent_session(header: &Value, composer_data: Option<&Value>) -> bool {
    let unified = header
        .get("unifiedMode")
        .or_else(|| composer_data.and_then(|d| d.get("unifiedMode")))
        .and_then(|v| v.as_str());

    let is_agentic = composer_data
        .and_then(|d| d.get("isAgentic"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Cursor's unified UI stores executing Plan sessions as `plan`, not `agent`.
    // Include those; skip only plain chat.
    matches!(unified, Some("agent") | Some("plan")) || is_agentic
}

fn value_is_nonempty(value: &Value) -> bool {
    match value {
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        Value::String(text) => !text.is_empty(),
        Value::Bool(flag) => *flag,
        Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        Value::Null => false,
    }
}

fn is_draft(header: &Value, composer_data: Option<&Value>) -> bool {
    header
        .get("isDraft")
        .or_else(|| composer_data.and_then(|d| d.get("isDraft")))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn workspace_path_from_value(value: &Value) -> Option<String> {
    value
        .get("workspaceIdentifier")
        .and_then(|id| id.get("uri"))
        .and_then(|uri| uri.get("fsPath"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn workspace_json_path(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&content).ok()?;
    json.get("folder")
        .and_then(|v| v.as_str())
        .map(|uri| uri.trim_start_matches("file://").to_string())
}

fn project_name_from_path(workspace_path: &Option<String>, value: &Value) -> String {
    if let Some(path) = workspace_path {
        if let Some(name) = Path::new(path).file_name() {
            return name.to_string_lossy().to_string();
        }
    }

    value
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("agent")
        .to_string()
}

fn sort_sessions(sessions: &mut [AgentSession]) {
    sessions.sort_by(|a, b| {
        status_rank(a.status)
            .cmp(&status_rank(b.status))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn status_rank(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::NeedsAttention => 0,
        AgentStatus::Working => 1,
        AgentStatus::Waiting => 2,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn system_time_to_ms(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

pub fn watch_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some((global_db, workspace_root, transcripts_root)) = cursor_support_paths() {
        paths.push(global_db.clone());
        paths.push(global_db.with_extension("vscdb-wal"));
        paths.push(workspace_root);
        paths.push(transcripts_root);
    }
    paths
}
