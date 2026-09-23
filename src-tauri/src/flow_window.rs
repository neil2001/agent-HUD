use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const FLOW_WIDTH: f64 = 960.0;
const FLOW_HEIGHT: f64 = 640.0;

pub fn open_flow_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("flow") {
        let _ = window.show();
        let _ = window.set_focus();
        set_regular_activation(app);
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(app, "flow", WebviewUrl::App("index.html".into()))
        .title("Agent Flow")
        .inner_size(FLOW_WIDTH, FLOW_HEIGHT)
        .resizable(true)
        .decorations(true)
        .always_on_top(false)
        .focused(true)
        .visible(true)
        .build()
        .map_err(|e| format!("flow window build failed: {e}"))?;

    set_regular_activation(app);

    let handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            if handle.get_webview_window("flow").is_none() {
                set_accessory_activation(&handle);
            }
        }
    });

    Ok(())
}

fn set_regular_activation(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }
}

fn set_accessory_activation(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}
