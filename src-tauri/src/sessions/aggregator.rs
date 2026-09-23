use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

use crate::daily::{self, PrCache};
use crate::flow::FlowPipeline;
use crate::sessions::AgentSession;
use crate::state::AppState;
use crate::window::sync_visibility;

use super::cursor::{discover_active_sessions, watch_paths};

pub fn start(
    app: AppHandle,
    state: Arc<AppState>,
    flow: Arc<FlowPipeline>,
    prs: Arc<PrCache>,
) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        let (tx, rx) = mpsc::channel();
        let _watcher = setup_watcher(tx);

        loop {
            publish(&app, &state, &flow, &prs);
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
    });
}

pub fn apply_sessions(app: &AppHandle, _state: &AppState, sessions: Vec<AgentSession>) {
    let count = sessions.len();
    let _ = app.emit("sessions-changed", &sessions);

    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        sync_visibility(&handle, count);
    });
}

fn setup_watcher(tx: mpsc::Sender<()>) -> Option<RecommendedWatcher> {
    let mut watcher = RecommendedWatcher::new(
        move |_| {
            let _ = tx.send(());
        },
        Config::default(),
    )
    .ok()?;

    for path in watch_paths() {
        let mode = if path.is_dir() {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        let _ = watcher.watch(&path, mode);
    }

    Some(watcher)
}

fn publish(app: &AppHandle, state: &Arc<AppState>, flow: &Arc<FlowPipeline>, prs: &Arc<PrCache>) {
    let live = discover_active_sessions();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    flow.record_cursor_discovery(&live, now_ms);
    let sessions = state.merge_sessions(live);
    apply_sessions(app, state, sessions);
    let usage = daily::current_usage(flow.store(), prs);
    let _ = app.emit("daily-usage-changed", &usage);
}
