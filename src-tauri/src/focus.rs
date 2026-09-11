use std::process::Command;

use crate::sessions::SessionHost;
use crate::state::AppState;

pub fn focus_session(state: &AppState, id: &str) -> Result<(), String> {
    let session = state
        .find_session(id)
        .ok_or_else(|| "Session not found".to_string())?;

    match session.host {
        SessionHost::CursorDesktop { workspace_path } => {
            focus_workspace(&workspace_path)?;
        }
        SessionHost::CursorCloud { workspace_path } => {
            if let Some(path) = workspace_path {
                focus_workspace(&path)?;
            } else {
                activate_cursor()?;
            }
        }
    }

    Ok(())
}

fn focus_workspace(workspace_path: &str) -> Result<(), String> {
    let opened = Command::new("/usr/bin/open")
        .args(["-a", "Cursor", workspace_path])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if opened {
        return Ok(());
    }

    let cursor_bin = cursor_cli_path();
    if Command::new(&cursor_bin)
        .args(["--reuse-window", workspace_path])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
    {
        return Ok(());
    }

    activate_cursor()
}

fn activate_cursor() -> Result<(), String> {
    let opened = Command::new("/usr/bin/open")
        .args(["-a", "Cursor"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if opened {
        return Ok(());
    }
    run_osascript(r#"tell application "Cursor" to activate"#)
}

fn cursor_cli_path() -> String {
    let home = dirs::home_dir();
    let candidates = [
        home.as_ref()
            .map(|h| h.join(".local/bin/cursor"))
            .unwrap_or_default(),
        std::path::PathBuf::from("/usr/local/bin/cursor"),
        std::path::PathBuf::from("/opt/homebrew/bin/cursor"),
    ];

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "cursor".to_string())
}

fn run_osascript(script: &str) -> Result<(), String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
