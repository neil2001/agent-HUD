use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::flow::event::{FlowEvent, FlowEventType, FlowSource};
use crate::flow::pipeline::FlowPipeline;
use crate::flow::privacy::redact_focus_payload;

pub const CURSOR_BUNDLE_ID: &str = "com.todesktop.230313mzl4w4u92";

pub fn is_agent_host_app(bundle_id: &str, app_name: &str) -> bool {
    bundle_id == CURSOR_BUNDLE_ID || app_name == "Cursor"
}

/// Emit only when bundle id changes from the last recorded focus.
pub fn focus_changed(previous_bundle_id: Option<&str>, bundle_id: &str) -> bool {
    previous_bundle_id != Some(bundle_id)
}

pub fn build_focus_event(
    timestamp: i64,
    bundle_id: &str,
    app_name: &str,
    excluded_bundle_ids: &[String],
) -> FlowEvent {
    let payload = redact_focus_payload(bundle_id, app_name, excluded_bundle_ids);
    FlowEvent::new(
        timestamp,
        FlowSource::Macos,
        FlowEventType::AppFocused,
        None,
        None,
        payload,
    )
}

pub fn parse_frontmost_output(text: &str) -> Option<(String, String)> {
    let (bundle_raw, name_raw) = text.split_once('\t')?;
    let app_name = name_raw
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if app_name.is_empty() {
        return None;
    }
    let bundle = bundle_raw.trim();
    let bundle_id = if bundle.is_empty() || bundle.contains("missing value") || bundle.contains(',')
    {
        String::new()
    } else {
        bundle.to_string()
    };
    Some((bundle_id, app_name))
}

fn focus_identity(bundle_id: &str, app_name: &str) -> String {
    if bundle_id.is_empty() {
        format!("name:{app_name}")
    } else {
        bundle_id.to_string()
    }
}

pub fn start_focus_observer(flow: Arc<FlowPipeline>) {
    thread::spawn(move || {
        let mut last_bundle: Option<String> = None;

        let mut record = |bundle_id: String, app_name: String| {
            let settings = flow.store().get_settings().unwrap_or_default();
            if !settings.recording_enabled {
                return;
            }
            let identity = focus_identity(&bundle_id, &app_name);
            if !focus_changed(last_bundle.as_deref(), &identity) {
                return;
            }
            last_bundle = Some(identity);
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let event = build_focus_event(
                now_ms,
                &bundle_id,
                &app_name,
                &settings.excluded_bundle_ids,
            );
            if let Err(err) = flow.store().append(&event) {
                eprintln!("flow: focus append failed: {err}");
            }
        };

        #[cfg(target_os = "macos")]
        {
            if let Some((bundle, name)) = macos::current_frontmost_app() {
                record(bundle, name);
            }

            loop {
                if let Some((bundle, name)) = macos::current_frontmost_app() {
                    record(bundle, name);
                }
                thread::sleep(Duration::from_millis(500));
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
    });
}

#[cfg(target_os = "macos")]
mod macos {
    pub fn current_frontmost_app() -> Option<(String, String)> {
        use std::process::Command;

        let script = r#"
            tell application "System Events"
                set p to first application process whose frontmost is true
                set appName to name of p
                try
                    set bundleId to bundle identifier of p
                on error
                    set bundleId to ""
                end try
                if bundleId is missing value then set bundleId to ""
                return bundleId & tab & appName
            end tell
        "#;

        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        super::parse_frontmost_output(&String::from_utf8_lossy(&output.stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_changed_only_on_bundle_change() {
        assert!(!focus_changed(Some("com.apple.Safari"), "com.apple.Safari"));
        assert!(focus_changed(Some("com.apple.Safari"), "com.google.Chrome"));
        assert!(focus_changed(None, "com.apple.Safari"));
    }

    #[test]
    fn agent_host_detection() {
        assert!(is_agent_host_app(CURSOR_BUNDLE_ID, "Cursor"));
        assert!(is_agent_host_app("other", "Cursor"));
        assert!(!is_agent_host_app("com.apple.Safari", "Safari"));
    }

    #[test]
    fn parse_frontmost_splits_on_tab_and_drops_missing_bundle() {
        let parsed = parse_frontmost_output("com.google.Chrome\tGoogle Chrome\n").unwrap();
        assert_eq!(parsed.0, "com.google.Chrome");
        assert_eq!(parsed.1, "Google Chrome");

        let missing = parse_frontmost_output("missing value,\t, Cursor\n").unwrap();
        assert_eq!(missing.0, "");
        assert_eq!(missing.1, ", Cursor");
        assert!(parse_frontmost_output("com.apple.Safari\t\n").is_none());
    }

    #[test]
    fn redacted_focus_event_payload() {
        let excluded = vec!["com.1password.1password".to_string()];
        let event = build_focus_event(1000, "com.1password.1password", "1Password", &excluded);
        assert_eq!(event.payload["app_name"], "Hidden");
        assert_eq!(event.payload["excluded"], true);
    }
}
