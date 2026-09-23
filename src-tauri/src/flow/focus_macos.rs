use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value as JsonValue};

use crate::flow::event::{FlowEvent, FlowEventType, FlowSource};
use crate::flow::pipeline::FlowPipeline;
use crate::flow::privacy::redact_focus_payload;

pub const CURSOR_BUNDLE_ID: &str = "com.todesktop.230313mzl4w4u92";
pub const CHROME_BUNDLE_ID: &str = "com.google.Chrome";

const PAGE_FIELD_SEP: char = '\u{1e}';

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontmostSample {
    pub bundle_id: String,
    pub app_name: String,
    pub page: Option<ObservedPage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedPage {
    pub page_key: String,
    pub page_title: String,
}

/// Host plus path. Query string and hash are dropped.
pub fn page_key_from_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let without_hash = url.split_once('#').map(|(head, _)| head).unwrap_or(url);
    let without_query = without_hash
        .split_once('?')
        .map(|(head, _)| head)
        .unwrap_or(without_hash);
    let rest = without_query.split_once("://")?.1;
    if rest.is_empty() {
        return None;
    }
    let rest = rest.split_once('@').map(|(_, host)| host).unwrap_or(rest);
    let (host_port, path) = match rest.split_once('/') {
        Some((host, path)) => (host, path),
        None => (rest, ""),
    };
    let host = host_port
        .split_once(':')
        .map(|(host, _)| host)
        .unwrap_or(host_port)
        .trim();
    if host.is_empty() || host.contains(' ') {
        return None;
    }
    let path = path.trim().trim_matches('/');
    if path.is_empty() {
        Some(host.to_string())
    } else {
        Some(format!("{host}/{path}"))
    }
}

pub fn chrome_page_payload(page_key: &str, page_title: &str) -> JsonValue {
    json!({
        "page_key": page_key,
        "page_title": page_title,
    })
}

pub fn parse_frontmost_output(text: &str) -> Option<FrontmostSample> {
    let text = text.trim_end_matches(['\n', '\r']);
    let (bundle_raw, rest) = text.split_once('\t')?;
    let mut parts = rest.split(PAGE_FIELD_SEP);
    let name_raw = parts.next().unwrap_or("");
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
    let page = match (parts.next(), parts.next(), parts.next()) {
        (Some(title), Some(url), Some(flag)) => observed_page(title, url, flag),
        _ => None,
    };
    Some(FrontmostSample {
        bundle_id,
        app_name,
        page,
    })
}

fn observed_page(title: &str, url: &str, flag: &str) -> Option<ObservedPage> {
    let flag = flag.trim();
    if flag == "1" {
        return Some(ObservedPage {
            page_key: "incognito".to_string(),
            page_title: "Incognito".to_string(),
        });
    }
    let page_key = page_key_from_url(url)?;
    Some(ObservedPage {
        page_key,
        page_title: title.trim().to_string(),
    })
}

fn build_chrome_page_event(timestamp: i64, page_key: &str, page_title: &str) -> FlowEvent {
    FlowEvent::new(
        timestamp,
        FlowSource::Macos,
        FlowEventType::ChromePageFocused,
        None,
        None,
        chrome_page_payload(page_key, page_title),
    )
}

struct OpenPage {
    page_key: String,
    page_title: String,
    event_id: String,
}

enum PageDecision {
    None,
    Append(FlowEvent),
    Retitle { id: String, payload: JsonValue },
}

struct PageSession {
    open: Option<OpenPage>,
}

impl PageSession {
    fn observe(
        &mut self,
        timestamp: i64,
        bundle_id: &str,
        app_changed: bool,
        page: Option<&ObservedPage>,
    ) -> PageDecision {
        if bundle_id != CHROME_BUNDLE_ID {
            if app_changed {
                self.open = None;
            }
            return PageDecision::None;
        }
        let Some(page) = page else {
            return PageDecision::None;
        };
        if let Some(open) = &mut self.open {
            if open.page_key == page.page_key && !app_changed {
                if open.page_title != page.page_title {
                    open.page_title = page.page_title.clone();
                    let id = open.event_id.clone();
                    return PageDecision::Retitle {
                        id,
                        payload: chrome_page_payload(&page.page_key, &page.page_title),
                    };
                }
                return PageDecision::None;
            }
        }
        let event = build_chrome_page_event(timestamp, &page.page_key, &page.page_title);
        self.open = Some(OpenPage {
            page_key: page.page_key.clone(),
            page_title: page.page_title.clone(),
            event_id: event.id.clone(),
        });
        PageDecision::Append(event)
    }
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
        let mut pages = PageSession { open: None };

        let mut record = |sample: FrontmostSample| {
            let settings = flow.store().get_settings().unwrap_or_default();
            if !settings.recording_enabled {
                return;
            }
            let identity = focus_identity(&sample.bundle_id, &sample.app_name);
            let app_changed = focus_changed(last_bundle.as_deref(), &identity);
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if app_changed {
                last_bundle = Some(identity);
                let event = build_focus_event(
                    now_ms,
                    &sample.bundle_id,
                    &sample.app_name,
                    &settings.excluded_bundle_ids,
                );
                if let Err(err) = flow.store().append(&event) {
                    eprintln!("flow: focus append failed: {err}");
                }
            }
            match pages.observe(now_ms, &sample.bundle_id, app_changed, sample.page.as_ref()) {
                PageDecision::None => {}
                PageDecision::Append(event) => {
                    if let Err(err) = flow.store().append(&event) {
                        eprintln!("flow: chrome page append failed: {err}");
                    }
                }
                PageDecision::Retitle { id, payload } => {
                    if let Err(err) = flow.store().update_payload(&id, &payload) {
                        eprintln!("flow: chrome page retitle failed: {err}");
                    }
                }
            }
        };

        #[cfg(target_os = "macos")]
        {
            if let Some(sample) = macos::current_frontmost_app() {
                record(sample);
            }

            loop {
                if let Some(sample) = macos::current_frontmost_app() {
                    record(sample);
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
    pub fn current_frontmost_app() -> Option<super::FrontmostSample> {
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
            end tell
            set pagePart to ""
            if bundleId is "com.google.Chrome" then
                try
                    tell application "Google Chrome"
                        if (count of windows) > 0 then
                            set w to front window
                            set t to active tab of w
                            set pageTitle to title of t
                            set pageUrl to URL of t
                            set isIncog to "0"
                            try
                                if mode of w is "incognito" then set isIncog to "1"
                            end try
                            set rs to ASCII character 30
                            set pagePart to rs & pageTitle & rs & pageUrl & rs & isIncog
                        end if
                    end tell
                end try
            end if
            return bundleId & tab & appName & pagePart
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
    use crate::flow::store::FlowStore;

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
        assert_eq!(parsed.bundle_id, "com.google.Chrome");
        assert_eq!(parsed.app_name, "Google Chrome");
        assert!(parsed.page.is_none());

        let missing = parse_frontmost_output("missing value,\t, Cursor\n").unwrap();
        assert_eq!(missing.bundle_id, "");
        assert_eq!(missing.app_name, ", Cursor");
        assert!(parse_frontmost_output("com.apple.Safari\t\n").is_none());
    }

    #[test]
    fn parse_chrome_page_keeps_tab_inside_title() {
        let raw = "com.google.Chrome\tGoogle Chrome\u{1e}Hello\tWorld\u{1e}https://github.com/foo/bar?x=1#h\u{1e}0\n";
        let parsed = parse_frontmost_output(raw).unwrap();
        let page = parsed.page.unwrap();
        assert_eq!(page.page_title, "Hello\tWorld");
        assert_eq!(page.page_key, "github.com/foo/bar");
    }

    #[test]
    fn parse_non_chrome_has_no_page() {
        let parsed = parse_frontmost_output("com.apple.Safari\tSafari\n").unwrap();
        assert!(parsed.page.is_none());
    }

    #[test]
    fn page_key_from_url_drops_query_and_hash() {
        assert_eq!(
            page_key_from_url("https://github.com/foo/bar?x=1#section"),
            Some("github.com/foo/bar".to_string())
        );
        assert_eq!(
            page_key_from_url("https://example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(page_key_from_url(""), None);
        assert_eq!(page_key_from_url("not a url"), None);
    }

    #[test]
    fn incognito_payload_stores_no_url() {
        let raw = "com.google.Chrome\tGoogle Chrome\u{1e}Secret Title\u{1e}https://secret.example/path?token=1\u{1e}1\n";
        let page = parse_frontmost_output(raw).unwrap().page.unwrap();
        assert_eq!(page.page_key, "incognito");
        assert_eq!(page.page_title, "Incognito");
        let payload = chrome_page_payload(&page.page_key, &page.page_title);
        assert_eq!(payload["page_key"], "incognito");
        assert_eq!(payload["page_title"], "Incognito");
        assert!(payload.get("url").is_none());
        assert!(!payload.to_string().contains("secret"));
    }

    #[test]
    fn title_change_updates_one_event() {
        let store = FlowStore::open_in_memory().unwrap();
        let mut session = PageSession { open: None };
        let first = ObservedPage {
            page_key: "github.com/foo".to_string(),
            page_title: "GitHub".to_string(),
        };
        let decision = session.observe(1_000, CHROME_BUNDLE_ID, true, Some(&first));
        let PageDecision::Append(event) = decision else {
            panic!("expected append");
        };
        store.append(&event).unwrap();

        let renamed = ObservedPage {
            page_key: "github.com/foo".to_string(),
            page_title: "GitHub - PR #123".to_string(),
        };
        let decision = session.observe(2_000, CHROME_BUNDLE_ID, false, Some(&renamed));
        let PageDecision::Retitle { id, payload } = decision else {
            panic!("expected retitle");
        };
        store.update_payload(&id, &payload).unwrap();

        let events = store.query_range(0, 5_000).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, FlowEventType::ChromePageFocused);
        assert_eq!(events[0].payload["page_title"], "GitHub - PR #123");
        assert_eq!(events[0].payload["page_key"], "github.com/foo");
    }

    #[test]
    fn chrome_without_page_still_builds_app_focus() {
        let sample = parse_frontmost_output("com.google.Chrome\tGoogle Chrome\n").unwrap();
        assert!(sample.page.is_none());
        let event = build_focus_event(0, &sample.bundle_id, &sample.app_name, &[]);
        assert_eq!(event.event_type, FlowEventType::AppFocused);
        assert_eq!(event.payload["bundle_id"], "com.google.Chrome");
        let mut session = PageSession { open: None };
        let decision = session.observe(0, &sample.bundle_id, true, sample.page.as_ref());
        assert!(matches!(decision, PageDecision::None));
    }

    #[test]
    fn redacted_focus_event_payload() {
        let excluded = vec!["com.1password.1password".to_string()];
        let event = build_focus_event(1000, "com.1password.1password", "1Password", &excluded);
        assert_eq!(event.payload["app_name"], "Hidden");
        assert_eq!(event.payload["excluded"], true);
    }
}
