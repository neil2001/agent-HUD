use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::{sort_agent_sessions, AgentKind, AgentSession, AgentStatus, ProjectInfo, SessionHost};
use crate::process_cmd::run_command;

const INSPECT_TIMEOUT: Duration = Duration::from_millis(1_500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRoots {
    pub chats_dir: PathBuf,
    pub projects_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcess {
    pub pid: u32,
    pub cwd: String,
    pub tty: Option<String>,
    pub conversation_id: Option<String>,
}

struct ChatRecord {
    id: String,
    cwd: String,
    title: Option<String>,
    updated_at: i64,
    turn_open: bool,
}

struct Assignment {
    tty: Option<String>,
}

pub fn cli_support_paths() -> Option<CliRoots> {
    let home = dirs::home_dir()?;
    Some(CliRoots {
        chats_dir: home.join(".cursor/chats"),
        projects_dir: home.join(".cursor/projects"),
    })
}

pub fn chats_dir() -> Option<PathBuf> {
    Some(cli_support_paths()?.chats_dir)
}

/// A tty is usable when it names a device we can hand to a terminal.
pub fn normalize_tty(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "??" || raw == "-" {
        return None;
    }
    let short = raw.strip_prefix("/dev/").unwrap_or(raw);
    let digits = if let Some(rest) = short.strip_prefix("ttys") {
        rest
    } else if let Some(rest) = short.strip_prefix("tty") {
        rest
    } else {
        return None;
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("/dev/{short}"))
}

pub fn normalize_cwd(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    let raw = if trimmed.is_empty() { "/" } else { trimmed };
    match Path::new(raw).canonicalize() {
        Ok(canon) => {
            let text = canon.to_string_lossy();
            let stripped = text.trim_end_matches('/');
            if stripped.is_empty() {
                "/".to_string()
            } else {
                stripped.to_string()
            }
        }
        Err(_) => raw.to_string(),
    }
}

pub fn is_cli_agent_command(comm: &str) -> bool {
    Path::new(comm.trim())
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "agent" || name == "cursor-agent")
}

pub fn conversation_ids_from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut ids = HashSet::new();
    for path in paths {
        if let Some(id) = conversation_id_from_store_path(path) {
            ids.insert(id);
        }
    }
    if ids.len() == 1 {
        ids.into_iter().next()
    } else {
        None
    }
}

fn conversation_id_from_store_path(path: &str) -> Option<String> {
    let marker = "/chats/";
    let idx = path.find(marker)?;
    let rest = &path[idx + marker.len()..];
    let mut parts = rest.split('/');
    let _hash = parts.next()?;
    let id = parts.next()?;
    let file = parts.next()?;
    if file != "store.db" || parts.next().is_some() || id.is_empty() || id == "." || id == ".." {
        return None;
    }
    Some(id.to_string())
}

/// A user record opens a turn. `type: turn_ended` closes it for any status
/// (`success`, `error`, or `aborted`). Role records with no closer are in progress.
pub fn transcript_turn_open(text: &str) -> bool {
    let mut open = false;
    for line in text.lines() {
        open = apply_transcript_line(line, open);
    }
    open
}

fn file_turn_open(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let mut open = false;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        open = apply_transcript_line(&line, open);
    }
    open
}

fn apply_transcript_line(line: &str, open: bool) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return open;
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return open;
    };
    if value.get("type").and_then(|v| v.as_str()) == Some("turn_ended") {
        return false;
    }
    if value.get("role").and_then(|v| v.as_str()) == Some("user") {
        return true;
    }
    open
}

pub fn discover_live_cli_sessions() -> Vec<AgentSession> {
    let Some(roots) = cli_support_paths() else {
        return Vec::new();
    };
    discover_cli_sessions(&roots, &list_agent_processes())
}

pub fn discover_cli_sessions(roots: &CliRoots, processes: &[AgentProcess]) -> Vec<AgentSession> {
    let chats = load_chats(roots);
    if chats.is_empty() {
        return Vec::new();
    }

    let mut assigned: HashMap<String, Assignment> = HashMap::new();
    let mut claimed_pids = HashSet::new();

    let mut by_conversation: HashMap<String, Vec<&AgentProcess>> = HashMap::new();
    for process in processes {
        if let Some(id) = &process.conversation_id {
            by_conversation.entry(id.clone()).or_default().push(process);
        }
    }

    for chat in &chats {
        let Some(group) = by_conversation.get(&chat.id) else {
            continue;
        };
        let ttys: HashSet<String> = group
            .iter()
            .filter_map(|process| process_tty(process))
            .collect();
        assigned.insert(
            chat.id.clone(),
            Assignment {
                tty: if ttys.len() == 1 {
                    ttys.into_iter().next()
                } else {
                    None
                },
            },
        );
        for process in group {
            claimed_pids.insert(process.pid);
        }
    }

    let mut unbound_by_cwd: HashMap<String, Vec<&AgentProcess>> = HashMap::new();
    for process in processes {
        if claimed_pids.contains(&process.pid) || process.conversation_id.is_some() {
            continue;
        }
        if process.cwd.is_empty() {
            continue;
        }
        unbound_by_cwd
            .entry(normalize_cwd(&process.cwd))
            .or_default()
            .push(process);
    }

    let mut chats_by_cwd: HashMap<String, Vec<&ChatRecord>> = HashMap::new();
    for chat in &chats {
        if assigned.contains_key(&chat.id) {
            continue;
        }
        chats_by_cwd
            .entry(normalize_cwd(&chat.cwd))
            .or_default()
            .push(chat);
    }

    for (cwd, procs) in &unbound_by_cwd {
        let Some(candidates) = chats_by_cwd.get(cwd) else {
            continue;
        };
        // A single unbound process can be tied to the only chat in that
        // workspace, or to the only chat whose turn is still open. Several
        // closed chats stay hidden: guessing among them would focus a terminal
        // the conversation cannot be tied to.
        if procs.len() != 1 {
            continue;
        }
        let open: Vec<&&ChatRecord> = candidates.iter().filter(|chat| chat.turn_open).collect();
        let chosen = if candidates.len() == 1 {
            Some(candidates[0])
        } else if open.len() == 1 {
            Some(*open[0])
        } else {
            None
        };
        if let Some(chat) = chosen {
            assigned.insert(
                chat.id.clone(),
                Assignment {
                    tty: process_tty(procs[0]),
                },
            );
        }
    }

    let mut sessions = Vec::new();
    for chat in &chats {
        if let Some(assignment) = assigned.get(&chat.id) {
            sessions.push(session_from_chat(chat, assignment.tty.clone()));
            continue;
        }
        if chat.turn_open {
            sessions.push(session_from_chat(chat, None));
        }
    }

    sort_agent_sessions(&mut sessions);
    sessions
}

pub fn list_agent_processes() -> Vec<AgentProcess> {
    let Some(result) = run_command(
        "/bin/ps",
        &["-ax", "-o", "pid=,ppid=,tty=,comm="],
        INSPECT_TIMEOUT,
    ) else {
        return Vec::new();
    };
    if !result.success {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&result.stdout);
    let mut processes = Vec::new();
    for line in text.lines() {
        let Some((pid, tty, comm)) = parse_ps_row(line) else {
            continue;
        };
        if !is_cli_agent_command(&comm) {
            continue;
        }
        let inspection = inspect_process(pid);
        if inspection.cwd.is_none() && inspection.conversation_id.is_none() {
            continue;
        }
        processes.push(AgentProcess {
            pid,
            cwd: inspection.cwd.unwrap_or_default(),
            tty: normalize_tty(&tty),
            conversation_id: inspection.conversation_id,
        });
    }
    processes
}

pub fn parse_ps_row(line: &str) -> Option<(u32, String, String)> {
    let mut parts = line.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let _ppid = parts.next()?;
    let tty = parts.next()?.to_string();
    let comm = parts.collect::<Vec<_>>().join(" ");
    if comm.is_empty() {
        return None;
    }
    Some((pid, tty, comm))
}

struct Inspection {
    cwd: Option<String>,
    conversation_id: Option<String>,
}

fn inspect_process(pid: u32) -> Inspection {
    let Some(result) = run_command(
        "/usr/sbin/lsof",
        &["-n", "-P", "-p", &pid.to_string(), "-Fn"],
        INSPECT_TIMEOUT,
    ) else {
        return Inspection {
            cwd: None,
            conversation_id: None,
        };
    };
    if result.stdout.is_empty() {
        return Inspection {
            cwd: None,
            conversation_id: None,
        };
    }
    let text = String::from_utf8_lossy(&result.stdout);
    inspect_lsof_text(&text)
}

pub fn inspect_lsof_fn(text: &str) -> (Option<String>, Option<String>) {
    let inspection = inspect_lsof_text(text);
    (inspection.cwd, inspection.conversation_id)
}

fn inspect_lsof_text(text: &str) -> Inspection {
    let mut cwd = None;
    let mut names = Vec::new();
    let mut cwd_next = false;
    for line in text.lines() {
        if line == "fcwd" {
            cwd_next = true;
            continue;
        }
        let Some(name) = line.strip_prefix('n') else {
            cwd_next = false;
            continue;
        };
        if cwd_next {
            cwd = Some(normalize_cwd(name));
            cwd_next = false;
        }
        names.push(name);
    }
    Inspection {
        cwd,
        conversation_id: conversation_ids_from_paths(names),
    }
}

fn load_chats(roots: &CliRoots) -> Vec<ChatRecord> {
    let Ok(hashes) = std::fs::read_dir(&roots.chats_dir) else {
        return Vec::new();
    };
    let mut chats = Vec::new();
    for hash in hashes.flatten() {
        if !hash.path().is_dir() {
            continue;
        }
        let Ok(conversations) = std::fs::read_dir(hash.path()) else {
            continue;
        };
        for conversation in conversations.flatten() {
            let dir = conversation.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(id) = dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if id.starts_with('.') {
                continue;
            }
            let Some(meta) = read_meta(&dir.join("meta.json"), id) else {
                continue;
            };
            let turn_open = transcript_path(&roots.projects_dir, id)
                .map(|path| file_turn_open(&path))
                .unwrap_or(false);
            chats.push(ChatRecord {
                id: id.to_string(),
                cwd: meta.cwd,
                title: meta.title,
                updated_at: meta.updated_at,
                turn_open,
            });
        }
    }
    chats
}

struct MetaFields {
    cwd: String,
    title: Option<String>,
    updated_at: i64,
}

fn read_meta(path: &Path, _id: &str) -> Option<MetaFields> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let cwd = value.get("cwd").and_then(|v| v.as_str())?.trim();
    if cwd.is_empty() {
        return None;
    }
    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(|title| title.to_string());
    let updated_at = value
        .get("updatedAtMs")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    Some(MetaFields {
        cwd: cwd.to_string(),
        title,
        updated_at,
    })
}

fn transcript_path(projects: &Path, conversation_id: &str) -> Option<PathBuf> {
    let file_name = format!("{conversation_id}.jsonl");
    let entries = std::fs::read_dir(projects).ok()?;
    for entry in entries.flatten() {
        let candidate = entry
            .path()
            .join("agent-transcripts")
            .join(conversation_id)
            .join(&file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn session_from_chat(chat: &ChatRecord, tty: Option<String>) -> AgentSession {
    let project_name = project_basename(&chat.cwd);
    let title = chat.title.clone().unwrap_or_else(|| project_name.clone());
    AgentSession {
        id: chat.id.clone(),
        agent: AgentKind::Cursor,
        title,
        project: ProjectInfo {
            name: project_name,
            path: Some(chat.cwd.clone()),
        },
        status: AgentStatus::Working,
        host: SessionHost::CursorCli {
            workspace_path: chat.cwd.clone(),
            tty,
        },
        updated_at: chat.updated_at,
    }
}

fn process_tty(process: &AgentProcess) -> Option<String> {
    process.tty.as_deref().and_then(normalize_tty)
}

fn project_basename(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "agent".to_string())
}
