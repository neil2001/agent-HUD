use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Days, Local, NaiveDate, Offset, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::flow::{FlowEventType, FlowPipeline, FlowStore};

const PR_POLL_INTERVAL: Duration = Duration::from_secs(180);
const GH_CANDIDATES: &[&str] = &["/opt/homebrew/bin/gh", "/usr/local/bin/gh"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyUsage {
    pub sessions: u64,
    pub turns: u64,
    pub prs_opened: Option<u64>,
    pub prs_merged: Option<u64>,
}

#[derive(Debug, Default)]
struct PrCacheInner {
    day_start_ms: Option<i64>,
    opened: Option<u64>,
    merged: Option<u64>,
    login: Option<String>,
    logged_failure: bool,
}

#[derive(Debug, Default)]
pub struct PrCache {
    inner: Mutex<PrCacheInner>,
}

impl PrCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn counts_for_day(&self, day_start_ms: i64) -> (Option<u64>, Option<u64>) {
        let guard = self.inner.lock().unwrap();
        if guard.day_start_ms == Some(day_start_ms) {
            (guard.opened, guard.merged)
        } else {
            (None, None)
        }
    }

    fn cached_login(&self) -> Option<String> {
        self.inner.lock().unwrap().login.clone()
    }

    fn set_login(&self, login: String) {
        self.inner.lock().unwrap().login = Some(login);
    }

    fn clear_login(&self) {
        self.inner.lock().unwrap().login = None;
    }

    fn store_success(&self, day_start_ms: i64, opened: u64, merged: u64) {
        let mut guard = self.inner.lock().unwrap();
        guard.day_start_ms = Some(day_start_ms);
        guard.opened = Some(opened);
        guard.merged = Some(merged);
        guard.logged_failure = false;
    }

    /// Keep the last success for this local day. After midnight, drop it until the next success.
    /// Returns true when this failure should be logged.
    fn note_failure(&self, day_start_ms: i64) -> bool {
        let mut guard = self.inner.lock().unwrap();
        if guard.day_start_ms != Some(day_start_ms) {
            guard.day_start_ms = Some(day_start_ms);
            guard.opened = None;
            guard.merged = None;
            guard.logged_failure = false;
        }
        if guard.logged_failure {
            return false;
        }
        guard.logged_failure = true;
        true
    }
}

pub fn current_usage(store: &FlowStore, prs: &PrCache) -> DailyUsage {
    let (start_ms, end_ms) = local_day_bounds_ms();
    let sessions = store
        .count_prompted_sessions(start_ms, end_ms)
        .unwrap_or(0);
    // Completed turns since local midnight, exclusive of the next midnight.
    // Same definition as FlowSummary.turns (turn_finished events).
    let turns = store
        .count_event_type_in_range(FlowEventType::TurnFinished, start_ms, end_ms)
        .unwrap_or(0);
    let (prs_opened, prs_merged) = prs.counts_for_day(start_ms);
    DailyUsage {
        sessions,
        turns,
        prs_opened,
        prs_merged,
    }
}

pub fn start_pr_poller(app: AppHandle, flow: Arc<FlowPipeline>, prs: Arc<PrCache>) {
    thread::spawn(move || loop {
        refresh_prs(&prs);
        let usage = current_usage(flow.store(), &prs);
        let _ = app.emit("daily-usage-changed", &usage);
        thread::sleep(PR_POLL_INTERVAL);
    });
}

fn refresh_prs(cache: &PrCache) {
    let now = Local::now();
    let (start, end) = day_bounds(now);
    let start_ms = start.timestamp_millis();
    let start_label = format_github_timestamp(&start);
    let end_label = format_github_timestamp(&end);

    let Some(bin) = resolve_gh() else {
        if cache.note_failure(start_ms) {
            eprintln!("daily: gh not found");
        }
        return;
    };

    let login = match cache.cached_login() {
        Some(login) => login,
        None => match fetch_login(&bin) {
            Ok(login) => {
                cache.set_login(login.clone());
                login
            }
            Err(err) => {
                if cache.note_failure(start_ms) {
                    eprintln!("daily: gh login failed: {}", brief(&err));
                }
                return;
            }
        },
    };

    let opened_query = prs_opened_query(&login, &start_label, &end_label);
    let merged_query = prs_merged_query(&login, &start_label, &end_label);
    match (
        search_total(&bin, &opened_query),
        search_total(&bin, &merged_query),
    ) {
        (Ok(opened), Ok(merged)) => cache.store_success(start_ms, opened, merged),
        (Err(err), _) | (_, Err(err)) => {
            cache.clear_login();
            if cache.note_failure(start_ms) {
                eprintln!("daily: gh search failed: {}", brief(&err));
            }
        }
    }
}

fn local_day_bounds_ms() -> (i64, i64) {
    let (start, end) = day_bounds(Local::now());
    (start.timestamp_millis(), end.timestamp_millis())
}

fn day_bounds<Tz: TimeZone>(now: DateTime<Tz>) -> (DateTime<Tz>, DateTime<Tz>) {
    let start_date = now.date_naive();
    let end_date = start_date
        .checked_add_days(Days::new(1))
        .unwrap_or(start_date);
    let start = midnight_in_zone(&now.timezone(), start_date);
    let end = midnight_in_zone(&now.timezone(), end_date);
    (start, end)
}

fn midnight_in_zone<Tz: TimeZone>(zone: &Tz, date: NaiveDate) -> DateTime<Tz> {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight");
    zone.from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| zone.from_utc_datetime(&naive))
}

fn format_github_timestamp<Tz: TimeZone>(dt: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let secs = dt.offset().fix().local_minus_utc();
    let sign = if secs >= 0 { '+' } else { '-' };
    let abs = secs.abs();
    let hours = abs / 3600;
    let mins = (abs % 3600) / 60;
    format!(
        "{date}{sign}{hours:02}:{mins:02}",
        date = dt.format("%Y-%m-%dT%H:%M:%S")
    )
}

fn prs_opened_query(login: &str, start: &str, end: &str) -> String {
    format!("author:{login} is:pr created:{start}..{end}")
}

fn prs_merged_query(login: &str, start: &str, end: &str) -> String {
    format!("author:{login} is:pr merged:{start}..{end}")
}

fn resolve_gh() -> Option<PathBuf> {
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("gh");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for candidate in GH_CANDIDATES {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn fetch_login(bin: &Path) -> Result<String, String> {
    let login = run_gh(bin, &["api", "user", "--jq", ".login"])?;
    if login.is_empty() || login.chars().any(char::is_whitespace) {
        return Err(format!("unexpected login: {login}"));
    }
    Ok(login)
}

fn search_total(bin: &Path, query: &str) -> Result<u64, String> {
    let field = format!("q={query}");
    let raw = run_gh(
        bin,
        &[
            "api",
            "-X",
            "GET",
            "search/issues",
            "-f",
            &field,
            "-F",
            "per_page=1",
            "--jq",
            ".total_count",
        ],
    )?;
    raw.parse::<u64>()
        .map_err(|_| format!("unexpected total_count: {raw}"))
}

fn run_gh(bin: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("{} {}", stderr.trim(), stdout.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn brief(err: &str) -> String {
    let line = err.lines().next().unwrap_or(err).trim();
    line.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::{FlowEvent, FlowSource};
    use chrono::{FixedOffset, Utc};
    use serde_json::json;

    fn event(ts: i64, id: &str, event_type: FlowEventType, session_id: &str) -> FlowEvent {
        FlowEvent {
            id: id.to_string(),
            timestamp: ts,
            source: FlowSource::Cursor,
            event_type,
            session_id: Some(session_id.to_string()),
            turn_id: Some(format!("{session_id}:{id}")),
            payload: json!({}),
        }
    }

    #[test]
    fn current_usage_counts_finished_turns_and_prompted_sessions() {
        let store = FlowStore::open_in_memory().unwrap();
        let (start_ms, end_ms) = local_day_bounds_ms();
        let prs = PrCache::new();

        store
            .append(&event(
                start_ms + 10,
                "open",
                FlowEventType::TurnStarted,
                "s-open",
            ))
            .unwrap();
        store
            .append(&event(
                start_ms + 20,
                "done-start",
                FlowEventType::TurnStarted,
                "s-done",
            ))
            .unwrap();
        store
            .append(&event(
                start_ms + 30,
                "done-finish",
                FlowEventType::TurnFinished,
                "s-done",
            ))
            .unwrap();
        store
            .append(&event(
                start_ms + 40,
                "done-start-2",
                FlowEventType::TurnStarted,
                "s-done",
            ))
            .unwrap();
        store
            .append(&event(
                start_ms + 50,
                "done-finish-2",
                FlowEventType::TurnFinished,
                "s-done",
            ))
            .unwrap();
        store
            .append(&event(
                start_ms - 5,
                "yesterday-start",
                FlowEventType::TurnStarted,
                "s-yesterday",
            ))
            .unwrap();
        store
            .append(&event(
                start_ms + 60,
                "yesterday-finish",
                FlowEventType::TurnFinished,
                "s-yesterday",
            ))
            .unwrap();
        store
            .append(&event(
                end_ms,
                "next-finish",
                FlowEventType::TurnFinished,
                "s-boundary",
            ))
            .unwrap();

        let usage = current_usage(&store, &prs);
        assert_eq!(usage.sessions, 2);
        assert_eq!(usage.turns, 3);
    }

    #[test]
    fn github_query_uses_local_midnight_not_utc_date() {
        let tz = FixedOffset::west_opt(7 * 3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 9, 23, 15, 4, 0).unwrap();
        let (start, end) = day_bounds(now);
        let opened = prs_opened_query(
            "neil",
            &format_github_timestamp(&start),
            &format_github_timestamp(&end),
        );
        let merged = prs_merged_query(
            "neil",
            &format_github_timestamp(&start),
            &format_github_timestamp(&end),
        );
        assert_eq!(
            opened,
            "author:neil is:pr created:2026-09-23T00:00:00-07:00..2026-09-24T00:00:00-07:00"
        );
        assert_eq!(
            merged,
            "author:neil is:pr merged:2026-09-23T00:00:00-07:00..2026-09-24T00:00:00-07:00"
        );

        // 06:30Z on Sep 24 is still Sep 23 in Pacific, so it belongs in today's range
        // even though the UTC calendar date has already rolled.
        let still_today = Utc.with_ymd_and_hms(2026, 9, 24, 6, 30, 0).unwrap();
        assert!(still_today >= start.with_timezone(&Utc));
        assert!(still_today < end.with_timezone(&Utc));

        // 06:30Z on Sep 23 is still Sep 22 locally.
        let previous_local_day = Utc.with_ymd_and_hms(2026, 9, 23, 6, 30, 0).unwrap();
        assert!(previous_local_day < start.with_timezone(&Utc));
    }

    #[test]
    fn pr_cache_keeps_same_day_counts_and_clears_after_midnight() {
        let cache = PrCache::new();
        cache.store_success(100, 2, 1);
        assert_eq!(cache.counts_for_day(100), (Some(2), Some(1)));

        assert!(cache.note_failure(100));
        assert_eq!(cache.counts_for_day(100), (Some(2), Some(1)));
        assert!(!cache.note_failure(100));

        assert!(cache.note_failure(200));
        assert_eq!(cache.counts_for_day(200), (None, None));
        assert_eq!(cache.counts_for_day(100), (None, None));
    }
}
