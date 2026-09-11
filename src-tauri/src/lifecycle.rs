use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    App,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

pub fn init_autostart(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.plugin(
        tauri_plugin_autostart::Builder::new()
            .app_name("agent-HUD")
            .macos_launcher(MacosLauncher::LaunchAgent)
            .build(),
    )
}

pub fn setup_tray(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart_label = if autostart_enabled {
        "Disable Open at Login"
    } else {
        "Enable Open at Login"
    };

    let autostart = MenuItem::with_id(app, "autostart", autostart_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit agent-HUD", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&autostart, &separator, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("agent-HUD")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => app.exit(0),
            "autostart" => toggle_autostart(app),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    let _tray = tray.build(app)?;

    Ok(())
}

fn toggle_autostart(app: &tauri::AppHandle) {
    let autolaunch = app.autolaunch();
    if autolaunch.is_enabled().unwrap_or(false) {
        let _ = autolaunch.disable();
    } else {
        let _ = autolaunch.enable();
    }
}
