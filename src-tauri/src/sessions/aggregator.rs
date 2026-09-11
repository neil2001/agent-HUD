use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

use crate::state::AppState;
use crate::window::sync_visibility;

use super::cursor::{discover_active_sessions, watch_paths};

pub fn start(app: AppHandle, state: Arc<AppState>) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        let (tx, rx) = mpsc::channel();
        let _watcher = setup_watcher(tx);

        loop {
            publish(&app, &state);
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
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

fn publish(app: &AppHandle, state: &Arc<AppState>) {
    let sessions = discover_active_sessions();
    state.set_sessions(sessions.clone());

    let _ = app.emit("sessions-changed", &sessions);

    let handle = app.clone();
    let count = sessions.len();
    let _ = app.run_on_main_thread(move || {
        sync_visibility(&handle, count);
    });
}
