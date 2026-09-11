use std::process::Command;

use crate::state::AppState;

pub fn focus_session(state: &AppState, id: &str) -> Result<(), String> {
    let _session = state
        .find_session(id)
        .ok_or_else(|| "Session not found".to_string())?;

    let url = format!(
        "cursor://anysphere.cursor-deeplink/background-agent?bcId={}",
        id
    );
    open_url(&url)
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
