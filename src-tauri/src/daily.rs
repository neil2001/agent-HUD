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
const OPEN_PR_CACHE_TTL_MS: i64 = 60_000;
const OPEN_PR_PAGE_SIZE: usize = 50;
const OPEN_PR_WINDOWS: [u32; 3] = [30, 60, 90];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyUsage {
    pub sessions: u64,
    pub turns: u64,
    pub prs_opened: Option<u64>,
    pub prs_merged: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenPr {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub updated_at_ms: i64,
    pub is_draft: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenPrList {
    pub total: u64,
    pub prs: Vec<OpenPr>,
}

#[derive(Debug, Default)]
struct PrCacheInner {
    day_start_ms: Option<i64>,
    opened: Option<u64>,
    merged: Option<u64>,
    login: Option<String>,
    logged_failure: bool,
    open_prs: Option<(i64, u32, OpenPrList)>,
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

    fn fresh_open_prs(&self, now_ms: i64, days: u32) -> Option<OpenPrList> {
        let guard = self.inner.lock().unwrap();
        let (fetched_at, cached_days, list) = guard.open_prs.as_ref()?;
        if *cached_days == days && now_ms.saturating_sub(*fetched_at) < OPEN_PR_CACHE_TTL_MS {
            Some(list.clone())
        } else {
            None
        }
    }

    fn store_open_prs(&self, now_ms: i64, days: u32, list: OpenPrList) {
        self.inner.lock().unwrap().open_prs = Some((now_ms, days, list));
    }
}

pub fn list_open_prs(cache: &PrCache, days: u32) -> Result<OpenPrList, String> {
    let days = normalize_open_pr_days(days)?;
    let now_ms = unix_now_ms();
    if let Some(hit) = cache.fresh_open_prs(now_ms, days) {
        return Ok(hit);
    }

    let Some(bin) = resolve_gh() else {
        return Err("gh not found".to_string());
    };

    let login = match cache.cached_login() {
        Some(login) => login,
        None => match fetch_login(&bin) {
            Ok(login) => {
                cache.set_login(login.clone());
                login
            }
            Err(err) => return Err(brief(&err)),
        },
    };

    let since = format_github_timestamp(&open_pr_since(Local::now(), days));
    match search_open_prs(&bin, &open_prs_query(&login, &since)) {
        Ok(list) => {
            cache.store_open_prs(unix_now_ms(), days, list.clone());
            Ok(list)
        }
        Err(err) => {
            cache.clear_login();
            Err(brief(&err))
        }
    }
}

fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn current_usage(store: &FlowStore, prs: &PrCache) -> DailyUsage {
    let (start_ms, end_ms) = local_day_bounds_ms();
    let sessions = store.count_prompted_sessions(start_ms, end_ms).unwrap_or(0);
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

fn normalize_open_pr_days(days: u32) -> Result<u32, String> {
    if OPEN_PR_WINDOWS.contains(&days) {
        Ok(days)
    } else {
        Err("days must be 30, 60, or 90".to_string())
    }
}

fn open_pr_since<Tz: TimeZone>(now: DateTime<Tz>, days: u32) -> DateTime<Tz> {
    now.clone()
        .checked_sub_days(Days::new(u64::from(days)))
        .unwrap_or(now)
}

fn open_prs_query(login: &str, updated_since: &str) -> String {
    format!("author:{login} is:pr is:open updated:>={updated_since}")
}

fn search_open_prs(bin: &Path, query: &str) -> Result<OpenPrList, String> {
    let field = format!("q={query}");
    let per_page = format!("per_page={OPEN_PR_PAGE_SIZE}");
    let raw = run_gh(
        bin,
        &[
            "api",
            "-X",
            "GET",
            "search/issues",
            "-f",
            &field,
            "-f",
            "sort=updated",
            "-f",
            "order=desc",
            "-F",
            &per_page,
        ],
    )?;
    parse_open_prs(&raw)
}

fn parse_open_prs(raw: &str) -> Result<OpenPrList, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| format!("unexpected search response: {err}"))?;
    let total = value
        .get("total_count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing total_count".to_string())?;
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "missing items".to_string())?;

    let mut prs = Vec::new();
    for item in items {
        if let Some(pr) = parse_open_pr(item) {
            prs.push(pr);
        }
    }
    prs.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    prs.truncate(OPEN_PR_PAGE_SIZE);
    Ok(OpenPrList { total, prs })
}

fn parse_open_pr(item: &serde_json::Value) -> Option<OpenPr> {
    let number = item.get("number")?.as_u64()?;
    let title = item.get("title")?.as_str()?.to_string();
    let url = item.get("html_url")?.as_str()?.to_string();
    let updated_at = item.get("updated_at")?.as_str()?;
    let updated_at_ms = parse_github_time_ms(updated_at)?;
    let repo = repo_from_repository_url(item.get("repository_url")?.as_str()?)?;
    let is_draft = item.get("draft").and_then(|v| v.as_bool()).unwrap_or(false);
    Some(OpenPr {
        number,
        title,
        url,
        repo,
        updated_at_ms,
        is_draft,
    })
}

fn repo_from_repository_url(url: &str) -> Option<String> {
    let rest = url.split("/repos/").nth(1)?;
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let owner = parts.next()?;
    let name = parts.next()?;
    Some(format!("{owner}/{name}"))
}

fn parse_github_time_ms(raw: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp_millis())
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

    #[test]
    fn open_prs_query_limits_to_recent_updates() {
        let tz = FixedOffset::west_opt(7 * 3600).unwrap();
        let now = tz.with_ymd_and_hms(2026, 9, 23, 16, 14, 0).unwrap();
        let since = open_pr_since(now, 30);
        assert_eq!(
            format_github_timestamp(&since),
            "2026-08-24T16:14:00-07:00"
        );
        assert_eq!(
            open_prs_query("neil", &format_github_timestamp(&since)),
            "author:neil is:pr is:open updated:>=2026-08-24T16:14:00-07:00"
        );
        assert!(normalize_open_pr_days(30).is_ok());
        assert!(normalize_open_pr_days(60).is_ok());
        assert!(normalize_open_pr_days(90).is_ok());
        assert!(normalize_open_pr_days(7).is_err());
    }

    #[test]
    fn parse_open_prs_sorts_by_updated_and_defaults_draft() {
        let raw = r#"{
          "total_count": 3,
          "items": [
            {
              "number": 1,
              "title": "Older",
              "html_url": "https://github.com/acme/app/pull/1",
              "updated_at": "2026-09-20T00:00:00Z",
              "repository_url": "https://api.github.com/repos/acme/app"
            },
            {
              "number": 2,
              "title": "Newer",
              "html_url": "https://github.com/acme/app/pull/2",
              "updated_at": "2026-09-23T12:00:00Z",
              "draft": true,
              "repository_url": "https://api.github.com/repos/acme/app"
            }
          ]
        }"#;
        let list = parse_open_prs(raw).unwrap();
        assert_eq!(list.total, 3);
        assert_eq!(list.prs.len(), 2);
        assert_eq!(list.prs[0].number, 2);
        assert!(list.prs[0].is_draft);
        assert_eq!(list.prs[0].repo, "acme/app");
        assert_eq!(list.prs[0].url, "https://github.com/acme/app/pull/2");
        assert!(!list.prs[1].is_draft);
        assert!(list.prs[0].updated_at_ms > list.prs[1].updated_at_ms);
    }

    #[test]
    fn parse_open_prs_caps_at_fifty() {
        let items: Vec<_> = (0..60)
            .map(|i| {
                json!({
                    "number": i,
                    "title": format!("PR {i}"),
                    "html_url": format!("https://github.com/acme/app/pull/{i}"),
                    "updated_at": "2026-09-23T12:00:00Z",
                    "repository_url": "https://api.github.com/repos/acme/app",
                })
            })
            .collect();
        let raw = json!({ "total_count": 80, "items": items }).to_string();
        let list = parse_open_prs(&raw).unwrap();
        assert_eq!(list.total, 80);
        assert_eq!(list.prs.len(), 50);
    }

    #[test]
    fn open_pr_cache_expires_after_ttl() {
        let cache = PrCache::new();
        let list = OpenPrList {
            total: 1,
            prs: vec![OpenPr {
                number: 7,
                title: "Ship it".to_string(),
                url: "https://github.com/acme/app/pull/7".to_string(),
                repo: "acme/app".to_string(),
                updated_at_ms: 1_700_000_000_000,
                is_draft: false,
            }],
        };
        cache.store_open_prs(1_000, 30, list.clone());
        assert_eq!(
            cache.fresh_open_prs(1_000 + OPEN_PR_CACHE_TTL_MS - 1, 30),
            Some(list.clone())
        );
        assert_eq!(cache.fresh_open_prs(1_000 + OPEN_PR_CACHE_TTL_MS, 30), None);
        assert_eq!(cache.fresh_open_prs(1_000, 60), None);
    }
}
