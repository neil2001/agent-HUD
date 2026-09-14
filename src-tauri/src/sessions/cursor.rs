use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde_json::Value;

use super::sqlite::{
    json_from_row, open_readonly, read_disk_kv_json, read_item_json, row_bool, row_opt_i64,
};
use super::{sort_agent_sessions, AgentKind, AgentSession, AgentStatus, ProjectInfo, SessionHost};

const TRANSCRIPT_FRESH_MS: i64 = 600_000;
const TOOL_LIVE_MS: i64 = 30_000;
/// Running tools (e.g. long shell commands) may not refresh timestamps for minutes.
const TOOL_RUNNING_LIVE_MS: i64 = 1_800_000;
/// Limits HUD rows to current open waits; leftover unfinished timestamps do not resurrect old chats.
pub const UNFINISHED_PROMPT_RECENCY_MS: i64 = 45 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastTurnKind {
    User,
    Tool,
    Thinking,
    AssistantText,
    Unknown,
}

pub fn cursor_support_paths() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let home = dirs::home_dir()?;
    let global_db = home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb");
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
    collect_from_cloud_agents(&conn, &mut sessions);
    collect_from_workspace_fallback(&workspace_root, now_ms, &transcript_activity, &mut sessions);

    let mut active: Vec<AgentSession> = sessions
        .into_values()
        .filter(|session| session.is_active())
        .collect();

    sort_agent_sessions(&mut active);
    active
}

fn collect_from_composer_headers(
    conn: &Connection,
    now_ms: i64,
    transcript_activity: &HashMap<String, i64>,
    out: &mut HashMap<String, AgentSession>,
) {
    let headers = read_composer_headers(conn);
    let composer_data_by_id = read_composer_data_map(conn, &headers);
    let live_tools = live_tool_composer_ids(conn, &headers, &composer_data_by_id, now_ms);
    for header in headers {
        if header.is_archived || header.is_subagent {
            continue;
        }

        let composer_data = composer_data_by_id.get(&header.id);

        if is_draft(&header.value, composer_data) {
            continue;
        }

        if !is_agent_session(&header.value, composer_data) {
            continue;
        }

        let data_updated = composer_data.and_then(|data| {
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
        let Some(status) = composer_session_status(
            composer_data,
            &header.value,
            updated_at,
            transcript_ts,
            live_tools.contains(&header.id),
            now_ms,
        ) else {
            continue;
        };

        let workspace_path = workspace_path_from_value(&header.value);
        let project_name = project_basename(&workspace_path);
        let title = session_title_from_value(&header.value, composer_data, &project_name);

        out.insert(
            header.id.clone(),
            AgentSession {
                id: header.id.clone(),
                agent: AgentKind::Cursor,
                title,
                project: ProjectInfo {
                    name: project_name,
                    path: workspace_path.clone(),
                },
                status,
                host: match workspace_path {
                    Some(path) => SessionHost::CursorDesktop {
                        workspace_path: path,
                    },
                    None => SessionHost::CursorCloud {
                        workspace_path: None,
                    },
                },
                updated_at,
            },
        );
    }
}

fn collect_from_cloud_agents(conn: &Connection, out: &mut HashMap<String, AgentSession>) {
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

            if agent
                .get("isKilled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
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
            let Some(status) = classify_cloud(status_code, workflow, pending) else {
                continue;
            };

            let workspace_path = agent
                .get("workspaceRootPath")
                .and_then(|v| v.as_str())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string());

            let project_name = project_basename(&workspace_path);
            let title = agent
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(|name| name.to_string())
                .unwrap_or_else(|| project_name.clone());

            out.entry(id.to_string()).or_insert(AgentSession {
                id: id.to_string(),
                agent: AgentKind::Cursor,
                title,
                project: ProjectInfo {
                    name: project_name,
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

            if composer
                .get("isArchived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
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
            let Some(status) = composer_session_status(
                Some(&composer),
                &composer,
                updated_at,
                transcript_ts,
                false,
                now_ms,
            ) else {
                continue;
            };

            let project_name = project_basename(&workspace_path);
            let title = session_title_from_value(&composer, None, &project_name);
            out.insert(
                id.to_string(),
                AgentSession {
                    id: id.to_string(),
                    agent: AgentKind::Cursor,
                    title,
                    project: ProjectInfo {
                        name: project_name,
                        path: workspace_path.clone(),
                    },
                    status,
                    host: match workspace_path.clone() {
                        Some(path) => SessionHost::CursorDesktop {
                            workspace_path: path,
                        },
                        None => SessionHost::CursorCloud {
                            workspace_path: None,
                        },
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
    let mut stmt = match conn
        .prepare("SELECT key FROM ItemTable WHERE key LIKE 'cloudAgentRepository.agents%'")
    {
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

pub fn classify(live_work: bool, awaiting_user: bool) -> Option<AgentStatus> {
    if live_work {
        Some(AgentStatus::Working)
    } else if awaiting_user {
        Some(AgentStatus::NeedsAttention)
    } else {
        None
    }
}

pub fn classify_cloud(
    status: Option<i64>,
    workflow: Option<i64>,
    pending: bool,
) -> Option<AgentStatus> {
    let running = matches!(status, Some(1)) || matches!(workflow, Some(1));
    classify(running, pending)
}

pub fn composer_session_status(
    composer_data: Option<&Value>,
    header: &Value,
    updated_at: i64,
    transcript_ts: Option<i64>,
    has_live_tool: bool,
    now_ms: i64,
) -> Option<AgentStatus> {
    let status_str = composer_data
        .and_then(|data| data.get("status"))
        .and_then(|v| v.as_str())
        .or_else(|| header.get("status").and_then(|v| v.as_str()));
    let continuation = composer_data
        .and_then(|data| data.get("isContinuationInProgress"))
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| {
            header
                .get("isContinuationInProgress")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        });
    let last_turn = last_turn_kind(composer_data.or(Some(header)));
    let recent_unfinished =
        is_recent_unfinished(header, composer_data, updated_at, transcript_ts, now_ms);
    let open_run_working = recent_unfinished
        && matches!(
            last_turn,
            LastTurnKind::Tool
                | LastTurnKind::Thinking
                | LastTurnKind::User
                | LastTurnKind::Unknown
        );
    let live_work = is_generating(composer_data.or(Some(header)), status_str)
        || has_live_tool
        || continuation
        || open_run_working;
    let awaiting_user = needs_prompt(header, composer_data)
        || is_waiting_on_user(
            status_str,
            header,
            composer_data,
            updated_at,
            transcript_ts,
            last_turn,
            now_ms,
        );
    classify(live_work, awaiting_user)
}

fn read_composer_data_map(conn: &Connection, headers: &[HeaderRow]) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    for header in headers {
        if header.is_archived || header.is_subagent {
            continue;
        }
        if let Some(data) = read_disk_kv_json(conn, &format!("composerData:{}", header.id))
            .ok()
            .flatten()
        {
            map.insert(header.id.clone(), data);
        }
    }
    map
}

fn is_generating(composer_data: Option<&Value>, status: Option<&str>) -> bool {
    if matches!(status, Some("generating")) {
        return true;
    }

    composer_data
        .and_then(|data| data.get("generatingBubbleIds"))
        .map(value_is_nonempty)
        .unwrap_or(false)
}

fn live_tool_composer_ids(
    conn: &Connection,
    headers: &[HeaderRow],
    composer_data_by_id: &HashMap<String, Value>,
    now_ms: i64,
) -> HashSet<String> {
    let mut live = HashSet::new();
    let mut stmt = match conn.prepare(
        "SELECT value FROM cursorDiskKV WHERE key LIKE ?1 AND (value LIKE '%\"status\":\"loading\"%' OR value LIKE '%\"status\":\"pending\"%' OR value LIKE '%\"status\":\"running\"%')",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return live,
    };

    for header in headers {
        if header.is_archived || header.is_subagent {
            continue;
        }
        let composer_status = composer_data_by_id
            .get(&header.id)
            .and_then(|data| data.get("status"))
            .and_then(|v| v.as_str());
        let pattern = format!("bubbleId:{}:%", header.id);
        let rows = match stmt.query_map([&pattern], |row| json_from_row(row.get_ref(0)?)) {
            Ok(rows) => rows,
            Err(_) => continue,
        };
        if rows
            .filter_map(|row| row.ok())
            .any(|bubble| bubble_is_live_tool(&bubble, now_ms, composer_status))
        {
            live.insert(header.id.clone());
        }
    }

    live
}

pub fn bubble_is_live_tool(bubble: &Value, now_ms: i64, composer_status: Option<&str>) -> bool {
    if bubble
        .get("completedAtMs")
        .and_then(|v| v.as_i64())
        .is_some()
    {
        return false;
    }

    let status = bubble
        .get("toolFormerData")
        .and_then(|data| data.get("status"))
        .and_then(|v| v.as_str());
    if !matches!(status, Some("loading") | Some("pending") | Some("running")) {
        return false;
    }

    if matches!(status, Some("loading") | Some("pending"))
        && matches!(composer_status, Some("completed"))
    {
        return false;
    }

    let started = json_timestamp_ms(bubble.get("startedAtMs"))
        .or_else(|| json_timestamp_ms(bubble.get("createdAt")));
    let max_age = if matches!(status, Some("running")) {
        TOOL_RUNNING_LIVE_MS
    } else {
        TOOL_LIVE_MS
    };
    match started {
        Some(ts) => now_ms.saturating_sub(ts) <= max_age,
        None => false,
    }
}

fn json_timestamp_ms(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(ms) = value.as_i64() {
        return Some(ms);
    }
    if let Some(ms) = value.as_f64() {
        return Some(ms as i64);
    }
    iso_to_ms(value.as_str()?)
}

fn iso_to_ms(raw: &str) -> Option<i64> {
    let trimmed = raw.trim().trim_end_matches('Z');
    let (date, time) = trimmed.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i32 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    let (hms, frac) = match time.split_once('.') {
        Some((hms, frac)) => (hms, frac),
        None => (time, "0"),
    };
    let mut time_parts = hms.split(':');
    let hour: u32 = time_parts.next()?.parse().ok()?;
    let minute: u32 = time_parts.next()?.parse().ok()?;
    let second: u32 = time_parts.next()?.parse().ok()?;
    let millis: u32 = frac
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(3)
        .collect::<String>()
        .parse()
        .unwrap_or(0);

    let days = days_from_civil(year, month, day)?;
    Some(
        days * 86_400_000
            + hour as i64 * 3_600_000
            + minute as i64 * 60_000
            + second as i64 * 1_000
            + millis as i64,
    )
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u32;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe as i64 * 365 + (yoe as i64 / 4) - (yoe as i64 / 100) + doy as i64;
    Some((era as i64) * 146_097 + doe - 719_468)
}

fn unfinished_run_at_ms(header: &Value, composer_data: Option<&Value>) -> Option<i64> {
    json_timestamp_ms(header.get("unfinishedRunAt"))
        .or_else(|| composer_data.and_then(|data| json_timestamp_ms(data.get("unfinishedRunAt"))))
}

fn is_recent_unfinished(
    header: &Value,
    composer_data: Option<&Value>,
    updated_at: i64,
    transcript_ts: Option<i64>,
    now_ms: i64,
) -> bool {
    let Some(unfinished_at) = unfinished_run_at_ms(header, composer_data) else {
        return false;
    };
    let activity = updated_at
        .max(unfinished_at)
        .max(transcript_ts.unwrap_or(0));
    activity > 0 && now_ms.saturating_sub(activity) <= UNFINISHED_PROMPT_RECENCY_MS
}

pub fn is_waiting_on_user(
    status: Option<&str>,
    header: &Value,
    composer_data: Option<&Value>,
    updated_at: i64,
    transcript_ts: Option<i64>,
    last_turn: LastTurnKind,
    now_ms: i64,
) -> bool {
    if !matches!(status, Some("aborted") | Some("none")) {
        return false;
    }
    if last_turn != LastTurnKind::AssistantText {
        return false;
    }
    is_recent_unfinished(header, composer_data, updated_at, transcript_ts, now_ms)
}

pub fn last_turn_kind(composer_data: Option<&Value>) -> LastTurnKind {
    let Some(headers) = composer_data
        .and_then(|data| data.get("fullConversationHeadersOnly"))
        .and_then(|v| v.as_array())
    else {
        return LastTurnKind::Unknown;
    };

    for entry in headers.iter().rev() {
        let kind = last_turn_from_header(entry);
        if kind != LastTurnKind::Unknown {
            return kind;
        }
    }
    LastTurnKind::Unknown
}

fn last_turn_from_header(entry: &Value) -> LastTurnKind {
    if entry.get("type").and_then(|v| v.as_i64()) == Some(1) {
        return LastTurnKind::User;
    }

    let grouping = entry.get("grouping");
    let capability = grouping
        .and_then(|g| g.get("capabilityType"))
        .and_then(|v| v.as_i64());
    let tool_status = grouping
        .and_then(|g| g.get("toolFormerStatus"))
        .and_then(|v| v.as_str());
    if capability == Some(15) || tool_status.is_some() || entry.get("toolFormerData").is_some() {
        return LastTurnKind::Tool;
    }

    let thinking = grouping
        .and_then(|g| g.get("hasThinking"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if thinking {
        return LastTurnKind::Thinking;
    }

    let has_text = grouping
        .and_then(|g| g.get("hasText"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let preview = grouping
        .and_then(|g| g.get("textPreview"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if has_text || !preview.is_empty() {
        return LastTurnKind::AssistantText;
    }

    LastTurnKind::Unknown
}

fn needs_prompt(header: &Value, composer_data: Option<&Value>) -> bool {
    json_flag(header, "hasBlockingPendingActions")
        || composer_data
            .map(|data| json_flag(data, "hasBlockingPendingActions"))
            .unwrap_or(false)
}

fn json_flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
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

fn project_basename(workspace_path: &Option<String>) -> String {
    workspace_path
        .as_ref()
        .and_then(|path| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "agent".to_string())
}

fn session_title_from_value(
    value: &Value,
    composer_data: Option<&Value>,
    project_name: &str,
) -> String {
    let candidates = [
        value.get("name").and_then(|v| v.as_str()),
        value.get("subtitle").and_then(|v| v.as_str()),
        composer_data
            .and_then(|data| data.get("name"))
            .and_then(|v| v.as_str()),
        composer_data
            .and_then(|data| data.get("subtitle"))
            .and_then(|v| v.as_str()),
    ];

    for candidate in candidates {
        if let Some(name) = candidate {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    project_name.to_string()
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
