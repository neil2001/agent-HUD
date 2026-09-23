mod daily;
pub mod flow;
mod flow_window;
mod focus;
mod lifecycle;
pub mod sessions;
pub mod state;
mod window;

use std::sync::Arc;

use daily::{current_usage, start_pr_poller, DailyUsage, OpenPrList, PrCache};
use flow::metrics::{
    compute_attention, compute_summary, compute_timeline, AttentionReport, FlowSummary,
    FlowTimeline,
};
use flow::{default_flow_db_path, FlowPipeline, FlowSettings, FlowStore};
use flow_window::open_flow_window;
use lifecycle::{init_autostart, setup_tray};
use sessions::aggregator;
use state::AppState;
use tauri::Manager;

#[tauri::command]
fn get_sessions(state: tauri::State<Arc<AppState>>) -> Vec<sessions::AgentSession> {
    state.get_sessions()
}

#[tauri::command]
fn focus_session(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    focus::focus_session(state.inner(), &id)?;
    let sessions = state.inner().acknowledge_session(&id);
    aggregator::apply_sessions(&app, state.inner(), sessions);
    Ok(())
}

#[tauri::command]
fn get_flow_summary(
    flow: tauri::State<Arc<FlowPipeline>>,
    start_ms: i64,
    end_ms: i64,
) -> Result<FlowSummary, String> {
    let events = flow
        .store()
        .query_range(start_ms, end_ms)
        .map_err(|e| e.to_string())?;
    Ok(compute_summary(&events, start_ms, end_ms))
}

#[tauri::command]
fn get_attention(
    flow: tauri::State<Arc<FlowPipeline>>,
    start_ms: i64,
    end_ms: i64,
) -> Result<AttentionReport, String> {
    let events = flow
        .store()
        .query_range(start_ms, end_ms)
        .map_err(|e| e.to_string())?;
    Ok(compute_attention(&events, start_ms, end_ms))
}

#[tauri::command]
fn get_flow_timeline(
    flow: tauri::State<Arc<FlowPipeline>>,
    start_ms: i64,
    end_ms: i64,
) -> Result<FlowTimeline, String> {
    let events = flow
        .store()
        .query_range(start_ms, end_ms)
        .map_err(|e| e.to_string())?;
    Ok(compute_timeline(&events, start_ms, end_ms))
}

#[tauri::command]
fn get_flow_settings(flow: tauri::State<Arc<FlowPipeline>>) -> Result<FlowSettings, String> {
    flow.store().get_settings().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_flow_settings(
    flow: tauri::State<Arc<FlowPipeline>>,
    settings: FlowSettings,
) -> Result<(), String> {
    flow.store()
        .set_settings(&settings)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_daily_usage(
    flow: tauri::State<Arc<FlowPipeline>>,
    prs: tauri::State<Arc<PrCache>>,
) -> DailyUsage {
    current_usage(flow.store(), prs.inner())
}

#[tauri::command]
fn list_open_prs(prs: tauri::State<Arc<PrCache>>, days: u32) -> Result<OpenPrList, String> {
    daily::list_open_prs(prs.inner(), days)
}

#[tauri::command]
fn open_flow(app: tauri::AppHandle) -> Result<(), String> {
    open_flow_window(&app)
}

#[tauri::command]
fn dismiss_session(
    app: tauri::AppHandle,
    state: tauri::State<Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    state
        .inner()
        .find_session(&id)
        .ok_or_else(|| "Session not found".to_string())?;
    let sessions = state.inner().dismiss_session(&id);
    aggregator::apply_sessions(&app, state.inner(), sessions);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = Arc::new(AppState::new());

    let builder = {
        let builder = tauri::Builder::default()
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_store::Builder::new().build());
        #[cfg(target_os = "macos")]
        let builder = builder.plugin(tauri_nspanel::init());
        builder
    }
    .manage(app_state)
    .setup(|app| {
        let flow_store = match default_flow_db_path() {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match FlowStore::open(&path) {
                    Ok(store) => Arc::new(store),
                    Err(err) => {
                        eprintln!("flow: failed to open store at {path:?}: {err}");
                        Arc::new(FlowStore::open_in_memory().expect("flow in-memory store"))
                    }
                }
            }
            None => Arc::new(FlowStore::open_in_memory().expect("flow in-memory store")),
        };
        let flow = Arc::new(FlowPipeline::new(flow_store));
        let pr_cache = Arc::new(PrCache::new());
        app.manage(flow.clone());
        app.manage(pr_cache.clone());

        #[cfg(target_os = "macos")]
        if let Err(err) = window::create_hud_window(app.handle()) {
            eprintln!("failed to create HUD window: {err}");
        }

        if let Err(err) = setup_tray(app) {
            eprintln!("failed to create tray: {err}");
        }

        let flow_for_focus = flow.clone();
        flow::focus_macos::start_focus_observer(flow_for_focus);

        aggregator::start(
            app.handle().clone(),
            app.state::<Arc<AppState>>().inner().clone(),
            flow.clone(),
            pr_cache.clone(),
        );
        start_pr_poller(app.handle().clone(), flow, pr_cache);
        Ok(())
    })
    .invoke_handler(tauri::generate_handler![
        get_sessions,
        focus_session,
        dismiss_session,
        get_flow_summary,
        get_flow_timeline,
        get_attention,
        get_flow_settings,
        set_flow_settings,
        get_daily_usage,
        list_open_prs,
        open_flow
    ]);

    init_autostart(builder)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
