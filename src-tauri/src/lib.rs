pub mod focus;
mod lifecycle;
mod process_cmd;
pub mod sessions;
pub mod state;
mod window;

use std::sync::Arc;

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
            #[cfg(target_os = "macos")]
            if let Err(err) = window::create_hud_window(app.handle()) {
                eprintln!("failed to create HUD window: {err}");
            }

            if let Err(err) = setup_tray(app) {
                eprintln!("failed to create tray: {err}");
            }

            aggregator::start(
                app.handle().clone(),
                app.state::<Arc<AppState>>().inner().clone(),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_sessions,
            focus_session,
            dismiss_session
        ]);

    init_autostart(builder)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
