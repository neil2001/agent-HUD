use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Size};

pub const HUD_WIDTH: f64 = 268.0;
pub const HUD_CORNER_RADIUS: f64 = 12.0;
pub const HUD_HANDLE_HEIGHT: f64 = 5.0;
pub const HUD_METRICS_HEIGHT: f64 = 28.0;
pub const HUD_ROW_HEIGHT: f64 = 22.0;
pub const HUD_PADDING: f64 = 10.0;
pub const HUD_MAX_VISIBLE_ROWS: usize = 8;

pub fn height_for_session_count(count: usize) -> f64 {
    let rows = count.min(HUD_MAX_VISIBLE_ROWS);
    HUD_HANDLE_HEIGHT + HUD_METRICS_HEIGHT + rows as f64 * HUD_ROW_HEIGHT + HUD_PADDING
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use tauri::{WebviewUrl, WebviewWindow};
    use tauri_nspanel::{
        CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt, tauri_panel,
    };

    tauri_panel! {
        panel!(HudPanel {
            config: {
                can_become_key_window: false,
                is_floating_panel: true
            }
        })
    }

    pub fn create_hud_window(app: &AppHandle) -> Result<(), String> {
        let window = tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("agent-HUD")
            .inner_size(HUD_WIDTH, height_for_session_count(0))
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .accept_first_mouse(true)
            .build()
            .map_err(|e| format!("window build failed: {e}"))?;

        if let Err(err) = apply_vibrancy(&window) {
            eprintln!("vibrancy failed: {err}");
        }
        if let Err(err) = position_top_center(&window) {
            eprintln!("position HUD failed: {err}");
        }

        match window.to_panel::<HudPanel>() {
            Ok(panel) => {
                panel.set_level(PanelLevel::Status.value());
                // Borderless so AppKit does not paint a square HUD bezel
                // around the rounded panel.
                panel.set_style_mask(
                    StyleMask::empty()
                        .borderless()
                        .nonactivating_panel()
                        .into(),
                );
                panel.set_collection_behavior(
                    CollectionBehavior::new()
                        .can_join_all_spaces()
                        .full_screen_auxiliary()
                        .stationary()
                        .into(),
                );
                panel.set_floating_panel(true);
                panel.set_hides_on_deactivate(false);
                panel.set_becomes_key_only_if_needed(true);
                let _ = panel;
            }
            Err(err) => eprintln!("panel conversion failed: {err}"),
        }

        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        clip_to_hud_radius(&window);
        sync_visibility(app, 0);

        Ok(())
    }

    fn position_on_monitors(window: &WebviewWindow, x: f64, y: f64) -> bool {
        let Ok(monitors) = window.available_monitors() else {
            return x.is_finite() && y.is_finite();
        };
        monitors.into_iter().any(|monitor| {
            let pos = monitor.position();
            let size = monitor.size();
            let left = pos.x as f64;
            let top = pos.y as f64;
            let right = left + size.width as f64;
            let bottom = top + size.height as f64;
            x >= left - 80.0 && x <= right - 40.0 && y >= top - 80.0 && y <= bottom - 20.0
        })
    }

    fn ensure_on_screen(window: &WebviewWindow) {
        if let Ok(pos) = window.outer_position() {
            if position_on_monitors(window, pos.x as f64, pos.y as f64) {
                return;
            }
        }
        let _ = position_top_center(window);
    }

    fn apply_vibrancy(window: &WebviewWindow) -> Result<(), String> {
        use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
        apply_vibrancy(
            window,
            NSVisualEffectMaterial::HudWindow,
            None,
            Some(HUD_CORNER_RADIUS),
        )
        .map_err(|e| e.to_string())
    }

    fn clip_to_hud_radius(window: &WebviewWindow) {
        let Ok(view_ptr) = window.ns_view() else {
            return;
        };
        unsafe {
            let view: &objc2_app_kit::NSView = &*view_ptr.cast();
            view.setWantsLayer(true);
            if let Some(layer) = view.layer() {
                layer.setCornerRadius(HUD_CORNER_RADIUS);
                layer.setMasksToBounds(true);
            }
        }
    }

    fn position_top_center(window: &WebviewWindow) -> Result<(), String> {
        if let Ok(Some(monitor)) = window.current_monitor() {
            let scale = monitor.scale_factor();
            let size = monitor.size();
            let window_size = window
                .outer_size()
                .unwrap_or_else(|_| tauri::PhysicalSize::new(HUD_WIDTH as u32, 120));
            let logical_w = window_size.width as f64 / scale;
            let screen_w = size.width as f64 / scale;
            let x = (screen_w - logical_w) / 2.0;
            let y = 12.0;
            let _ = window.set_position(LogicalPosition::new(x, y));
        }
        Ok(())
    }

    pub fn sync_visibility(app: &AppHandle, session_count: usize) {
        if let Some(window) = app.get_webview_window("main") {
            let height = height_for_session_count(session_count);
            let _ = window.set_size(Size::Logical(LogicalSize::new(HUD_WIDTH, height)));
            ensure_on_screen(&window);
        }

        // Never call window.show() — Tauri's show activates and steals Cursor's focus.
        // Panel::show is orderFrontRegardless on a nonactivating NSPanel.
        // The panel stays visible even when no agent sessions are active.
        if let Ok(panel) = app.get_webview_panel("main") {
            if !panel.is_visible() {
                panel.show();
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod macos {
    use super::*;
    use tauri::WebviewUrl;

    pub fn create_hud_window(app: &AppHandle) -> Result<(), String> {
        tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("agent-HUD")
            .inner_size(HUD_WIDTH, height_for_session_count(0))
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .visible(false)
            .build()
            .map_err(|e| e.to_string())?;
        sync_visibility(app, 0);
        Ok(())
    }

    pub fn sync_visibility(app: &AppHandle, session_count: usize) {
        if let Some(window) = app.get_webview_window("main") {
            let height = height_for_session_count(session_count);
            let _ = window.set_size(Size::Logical(LogicalSize::new(HUD_WIDTH, height)));
            let _ = window.show();
        }
    }
}

pub use macos::{create_hud_window, sync_visibility};

#[cfg(test)]
mod tests {
    use super::{
        height_for_session_count, HUD_HANDLE_HEIGHT, HUD_MAX_VISIBLE_ROWS, HUD_METRICS_HEIGHT,
        HUD_PADDING, HUD_ROW_HEIGHT,
    };

    #[test]
    fn height_is_metrics_band_when_empty_and_grows_by_one_row() {
        let empty = HUD_HANDLE_HEIGHT + HUD_METRICS_HEIGHT + HUD_PADDING;
        assert_eq!(height_for_session_count(0), empty);
        assert_eq!(height_for_session_count(1), empty + HUD_ROW_HEIGHT);
        assert_eq!(height_for_session_count(3), empty + 3.0 * HUD_ROW_HEIGHT);
        assert_eq!(
            height_for_session_count(100),
            empty + HUD_MAX_VISIBLE_ROWS as f64 * HUD_ROW_HEIGHT
        );
    }
}
