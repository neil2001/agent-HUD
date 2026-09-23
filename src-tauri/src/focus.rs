use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::process_cmd::run_command;
use crate::sessions::cli::normalize_tty;
use crate::sessions::{AgentSession, SessionHost};
use crate::state::AppState;

pub const UNIDENTIFIED_TERMINAL: &str = "Cannot identify the terminal for this CLI session";

const FOCUS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPane {
    pub pane_id: String,
    pub tty: String,
    pub session_id: String,
    pub window_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxClient {
    pub tty: String,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxTarget {
    pub client_tty: String,
    pub session_id: String,
    pub window_id: String,
    pub pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusPlan {
    CursorLink(String),
    Terminal {
        tty: String,
        tmux: Option<TmuxTarget>,
    },
}

pub fn focus_session(state: &AppState, id: &str) -> Result<(), String> {
    let session = state
        .find_session(id)
        .ok_or_else(|| "Session not found".to_string())?;
    let (panes, clients) = current_tmux();
    execute_focus(focus_plan(&session, &panes, &clients)?)
}

pub fn focus_plan(
    session: &AgentSession,
    panes: &[TmuxPane],
    clients: &[TmuxClient],
) -> Result<FocusPlan, String> {
    match &session.host {
        SessionHost::CursorDesktop { .. } | SessionHost::CursorCloud { .. } => Ok(
            FocusPlan::CursorLink(cursor_background_agent_url(&session.id)),
        ),
        SessionHost::CursorCli { tty, .. } => {
            let Some(tty) = tty.as_deref().and_then(normalize_tty) else {
                return Err(UNIDENTIFIED_TERMINAL.to_string());
            };
            let focus = plan_terminal_focus(&tty, panes, clients)?;
            Ok(FocusPlan::Terminal {
                tty: focus.terminal_tty,
                tmux: focus.tmux,
            })
        }
    }
}

pub fn cursor_background_agent_url(id: &str) -> String {
    format!(
        "cursor://anysphere.cursor-deeplink/background-agent?bcId={}",
        id
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalFocus {
    pub terminal_tty: String,
    pub tmux: Option<TmuxTarget>,
}

pub fn plan_terminal_focus(
    agent_tty: &str,
    panes: &[TmuxPane],
    clients: &[TmuxClient],
) -> Result<TerminalFocus, String> {
    let agent_tty = normalize_tty(agent_tty).ok_or_else(|| UNIDENTIFIED_TERMINAL.to_string())?;
    if let Some(pane) = panes
        .iter()
        .find(|pane| normalize_tty(&pane.tty).as_deref() == Some(agent_tty.as_str()))
    {
        let Some(client) = clients
            .iter()
            .find(|client| client.session_id == pane.session_id)
        else {
            return Err(UNIDENTIFIED_TERMINAL.to_string());
        };
        let client_tty =
            normalize_tty(&client.tty).ok_or_else(|| UNIDENTIFIED_TERMINAL.to_string())?;
        return Ok(TerminalFocus {
            terminal_tty: client_tty.clone(),
            tmux: Some(TmuxTarget {
                client_tty,
                session_id: pane.session_id.clone(),
                window_id: pane.window_id.clone(),
                pane_id: pane.pane_id.clone(),
            }),
        });
    }

    Ok(TerminalFocus {
        terminal_tty: agent_tty,
        tmux: None,
    })
}

pub fn parse_tmux_panes(output: &str) -> Vec<TmuxPane> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pane_id = parts.next()?.to_string();
            let tty = parts.next()?.to_string();
            let session_id = parts.next()?.to_string();
            let window_id = parts.next()?.to_string();
            if parts.next().is_some()
                || !valid_pane_id(&pane_id)
                || normalize_tty(&tty).is_none()
                || !valid_session_id(&session_id)
                || !valid_window_id(&window_id)
            {
                return None;
            }
            Some(TmuxPane {
                pane_id,
                tty,
                session_id,
                window_id,
            })
        })
        .collect()
}

pub fn parse_tmux_clients(output: &str) -> Vec<TmuxClient> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let tty = parts.next()?.to_string();
            let session_id = parts.next()?.to_string();
            if parts.next().is_some()
                || normalize_tty(&tty).is_none()
                || !valid_session_id(&session_id)
            {
                return None;
            }
            Some(TmuxClient { tty, session_id })
        })
        .collect()
}

pub fn tmux_focus_args(target: &TmuxTarget) -> Result<Vec<String>, String> {
    if !valid_tty(&target.client_tty)
        || !valid_session_id(&target.session_id)
        || !valid_window_id(&target.window_id)
        || !valid_pane_id(&target.pane_id)
    {
        return Err(UNIDENTIFIED_TERMINAL.to_string());
    }
    Ok(vec![
        "switch-client".into(),
        "-c".into(),
        target.client_tty.clone(),
        "-t".into(),
        target.session_id.clone(),
        ";".into(),
        "select-window".into(),
        "-t".into(),
        target.window_id.clone(),
        ";".into(),
        "select-pane".into(),
        "-t".into(),
        target.pane_id.clone(),
    ])
}

pub fn ghostty_focus_script(working_directory: Option<&str>) -> String {
    match working_directory.map(applescript_string) {
        Some(dir) => format!(
            r#"tell application "Ghostty"
  set hitCount to 0
  set hitIndex to 0
  set i to 0
  repeat with term in terminals
    set i to i + 1
    try
      if working directory of term is {dir} then
        set hitCount to hitCount + 1
        if hitIndex is 0 then set hitIndex to i
      end if
    end try
  end repeat
  if hitCount is 1 then
    set theTerm to terminal hitIndex
    focus theTerm
    return
  end if
  if (count of windows) is 1 then
    set w to window 1
    activate window w
    return
  end if
  error "{UNIDENTIFIED_TERMINAL}"
end tell"#
        ),
        None => format!(
            r#"tell application "Ghostty"
  if (count of windows) is 1 then
    set w to window 1
    activate window w
    return
  end if
  error "{UNIDENTIFIED_TERMINAL}"
end tell"#
        ),
    }
}

pub fn terminal_app_focus_script(tty: &str) -> String {
    let tty = applescript_string(tty);
    format!(
        r#"tell application "Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t is {tty} then
        set index of w to 1
        set selected of t to true
        activate
        return
      end if
    end repeat
  end repeat
  error "{UNIDENTIFIED_TERMINAL}"
end tell"#
    )
}

pub fn iterm_focus_script(tty: &str) -> String {
    let tty = applescript_string(tty);
    format!(
        r#"tell application "iTerm"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is {tty} then
          select w
          select t
          select s
          activate
          return
        end if
      end repeat
    end repeat
  end repeat
  error "{UNIDENTIFIED_TERMINAL}"
end tell"#
    )
}

fn execute_focus(plan: FocusPlan) -> Result<(), String> {
    match plan {
        FocusPlan::CursorLink(url) => open_url(&url),
        FocusPlan::Terminal { tty, tmux } => {
            if let Some(target) = tmux {
                let args = tmux_focus_args(&target)?;
                let bin = tmux_binary().ok_or_else(|| UNIDENTIFIED_TERMINAL.to_string())?;
                let argv: Vec<&str> = args.iter().map(String::as_str).collect();
                match run_command(&bin, &argv, FOCUS_TIMEOUT) {
                    Some(result) if result.success => {}
                    _ => return Err(UNIDENTIFIED_TERMINAL.to_string()),
                }
            }
            raise_terminal(&tty)
        }
    }
}

fn raise_terminal(tty: &str) -> Result<(), String> {
    let Some(kind) = detect_terminal_app(tty) else {
        return Err(UNIDENTIFIED_TERMINAL.to_string());
    };
    match kind {
        TerminalKind::Ghostty => {
            let cwd = shell_cwd_on_tty(tty).filter(|path| !path.chars().any(|c| c.is_control()));
            run_osascript(&ghostty_focus_script(cwd.as_deref()))
        }
        TerminalKind::AppleTerminal => run_osascript(&terminal_app_focus_script(tty)),
        TerminalKind::ITerm => run_osascript(&iterm_focus_script(tty)),
        TerminalKind::Named(name) => activate_app(&name),
    }
}

enum TerminalKind {
    Ghostty,
    AppleTerminal,
    ITerm,
    Named(String),
}

fn detect_terminal_app(tty: &str) -> Option<TerminalKind> {
    let short = tty.strip_prefix("/dev/").unwrap_or(tty);
    let table = process_table()?;
    let on_tty: Vec<u32> = table
        .iter()
        .filter_map(|(pid, row)| (row.tty == short).then_some(*pid))
        .collect();
    for pid in on_tty {
        let mut current = pid;
        for _ in 0..16 {
            let Some(row) = table.get(&current) else {
                break;
            };
            if let Some(kind) = terminal_kind_from_comm(&row.comm) {
                return Some(kind);
            }
            if row.ppid == 0 || row.ppid == current {
                break;
            }
            current = row.ppid;
        }
    }
    None
}

struct ProcRow {
    ppid: u32,
    tty: String,
    comm: String,
}

fn process_table() -> Option<HashMap<u32, ProcRow>> {
    let result = run_command(
        "/bin/ps",
        &["-ax", "-o", "pid=,ppid=,tty=,comm="],
        FOCUS_TIMEOUT,
    )?;
    if !result.success {
        return None;
    }
    let text = String::from_utf8_lossy(&result.stdout);
    let mut table = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(pid) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(ppid) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(tty) = parts.next() else {
            continue;
        };
        let comm = parts.collect::<Vec<_>>().join(" ");
        if comm.is_empty() {
            continue;
        }
        table.insert(
            pid,
            ProcRow {
                ppid,
                tty: tty.to_string(),
                comm,
            },
        );
    }
    Some(table)
}

fn terminal_kind_from_comm(comm: &str) -> Option<TerminalKind> {
    if comm.contains("Ghostty.app") {
        Some(TerminalKind::Ghostty)
    } else if comm.contains("iTerm") {
        Some(TerminalKind::ITerm)
    } else if comm.contains("Terminal.app") {
        Some(TerminalKind::AppleTerminal)
    } else if comm.contains("Warp.app") {
        Some(TerminalKind::Named("Warp".to_string()))
    } else if comm.contains("kitty.app") {
        Some(TerminalKind::Named("kitty".to_string()))
    } else if comm.contains("WezTerm.app") {
        Some(TerminalKind::Named("WezTerm".to_string()))
    } else if comm.contains("Alacritty.app") {
        Some(TerminalKind::Named("Alacritty".to_string()))
    } else {
        None
    }
}

fn shell_cwd_on_tty(tty: &str) -> Option<String> {
    let short = tty.strip_prefix("/dev/").unwrap_or(tty);
    let table = process_table()?;
    let mut shells: Vec<u32> = table
        .iter()
        .filter(|(_, row)| row.tty == short && is_shell(&row.comm))
        .map(|(pid, _)| *pid)
        .collect();
    shells.sort_unstable();
    let pid = *shells.first()?;
    cwd_of_pid(pid)
}

fn is_shell(comm: &str) -> bool {
    matches!(
        Path::new(comm.trim())
            .file_name()
            .and_then(|name| name.to_str()),
        Some("zsh" | "bash" | "fish" | "sh" | "-zsh" | "-bash")
    )
}

fn cwd_of_pid(pid: u32) -> Option<String> {
    let result = run_command(
        "/usr/sbin/lsof",
        &["-n", "-P", "-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"],
        FOCUS_TIMEOUT,
    )?;
    if result.stdout.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&result.stdout);
    let mut cwd_next = false;
    for line in text.lines() {
        if line == "fcwd" {
            cwd_next = true;
            continue;
        }
        if cwd_next {
            return line.strip_prefix('n').map(|path| path.to_string());
        }
    }
    None
}

fn activate_app(name: &str) -> Result<(), String> {
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-')
    {
        return Err(UNIDENTIFIED_TERMINAL.to_string());
    }
    let status = Command::new("/usr/bin/open")
        .args(["-a", name])
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(UNIDENTIFIED_TERMINAL.to_string())
    }
}

fn run_osascript(script: &str) -> Result<(), String> {
    match run_command("/usr/bin/osascript", &["-e", script], FOCUS_TIMEOUT) {
        Some(result) if result.success => Ok(()),
        _ => Err(UNIDENTIFIED_TERMINAL.to_string()),
    }
}

fn current_tmux() -> (Vec<TmuxPane>, Vec<TmuxClient>) {
    let Some(bin) = tmux_binary() else {
        return (Vec::new(), Vec::new());
    };
    let panes = run_command(
        &bin,
        &[
            "list-panes",
            "-a",
            "-F",
            "#{pane_id} #{pane_tty} #{session_id} #{window_id}",
        ],
        FOCUS_TIMEOUT,
    )
    .filter(|result| result.success)
    .map(|result| parse_tmux_panes(&String::from_utf8_lossy(&result.stdout)))
    .unwrap_or_default();
    let clients = run_command(
        &bin,
        &["list-clients", "-F", "#{client_tty} #{session_id}"],
        FOCUS_TIMEOUT,
    )
    .filter(|result| result.success)
    .map(|result| parse_tmux_clients(&String::from_utf8_lossy(&result.stdout)))
    .unwrap_or_default();
    (panes, clients)
}

fn tmux_binary() -> Option<String> {
    for candidate in [
        "/opt/homebrew/bin/tmux",
        "/usr/local/bin/tmux",
        "/usr/bin/tmux",
    ] {
        if Path::new(candidate).is_file() {
            return Some(candidate.to_string());
        }
    }
    None
}

fn open_url(url: &str) -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Failed to open URL: {}", url))
    }
}

fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn valid_tty(tty: &str) -> bool {
    normalize_tty(tty).as_deref() == Some(tty)
}

fn valid_pane_id(id: &str) -> bool {
    id.strip_prefix('%')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn valid_window_id(id: &str) -> bool {
    id.strip_prefix('@')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

fn valid_session_id(id: &str) -> bool {
    id.strip_prefix('$')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}
