#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use resh::commands;
use resh::config::ConfigManager;
use resh::master_password::MasterPasswordManager;
use resh::db::DatabaseManager;
use resh::logger;
use std::sync::Arc;
use tauri::Manager;
use tauri::image::Image;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Set window icon
            if let Some(window) = app.get_webview_window("main") {
                let icon_bytes = include_bytes!("../icons/icon.png");
                if let Ok(img) = image::load_from_memory(icon_bytes) {
                    let rgba = img.to_rgba8();
                    let (width, height) = rgba.dimensions();
                    let icon = Image::new_owned(rgba.into_raw(), width, height);
                    let _ = window.set_icon(icon);
                }
            }

            // Get the default app data dir (e.g., %AppData%/com.resh.ssh)
            let default_app_data_dir = app.path()
                .app_data_dir()
                .expect("failed to resolve app data dir");

            // We want %AppData%/Resh directly, so we go up one level and join "Resh"
            let app_data_dir = default_app_data_dir
                .parent()
                .map(|p| p.join("Resh"))
                .unwrap_or_else(|| default_app_data_dir.join("Resh"));

            let config_manager = ConfigManager::new(app_data_dir.clone());
            let master_password_manager = MasterPasswordManager::new(app_data_dir.clone());
            let db_manager = DatabaseManager::new(app_data_dir.clone()).expect("Failed to initialize database");

            // Load initial config
            let local_config = config_manager.load_local_config().unwrap_or_else(|_| resh::config::Config::empty());
            let debug_enabled = local_config.general.debug_enabled;

            // Initialize logging
            logger::init_logging(app_data_dir.clone(), debug_enabled);
            tracing::info!("Logging initialized. Debug mode: {}", debug_enabled);

            let state = Arc::new(commands::AppState {
                config_manager: config_manager.clone(),
                password_manager: master_password_manager,
                db_manager,
                config: Mutex::new(local_config.clone()),
            });

            // Apply window state
            if let Some(window) = app.get_webview_window("main") {
                let ws = &local_config.general.window_state;
                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                    width: ws.width,
                    height: ws.height,
                }));
                let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition {
                    x: ws.x,
                    y: ws.y,
                }));
                if ws.is_maximized {
                    let _ = window.maximize();
                }
                let _ = window.show();
            }

            app.manage(state.clone());

            // Listen for window events to save state
            if let Some(window) = app.get_webview_window("main") {
                let state_for_event = state.clone();
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) | tauri::WindowEvent::CloseRequested { .. } = event {
                        let window = &window_clone;
                        
                        // Don't save state if minimized
                        if window.is_minimized().unwrap_or(false) {
                            return;
                        }

                        let is_maximized = window.is_maximized().unwrap_or(false);
                        let size = window.inner_size().unwrap_or_default().to_logical(window.scale_factor().unwrap_or(1.0));
                        let position = window.outer_position().unwrap_or_default().to_logical(window.scale_factor().unwrap_or(1.0));

                        let state = state_for_event.clone();
                        tokio::spawn(async move {
                            let mut config_guard = state.config.lock().await;
                            
                            // Only update if not maximized, to preserve the "restored" size
                            if !is_maximized {
                                config_guard.general.window_state.width = size.width;
                                config_guard.general.window_state.height = size.height;
                                config_guard.general.window_state.x = position.x;
                                config_guard.general.window_state.y = position.y;
                            }
                            config_guard.general.window_state.is_maximized = is_maximized;

                            // Save to disk
                            let _ = state.config_manager.save_local_config(&config_guard);
                        });
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::get_config,
            commands::config::save_config,
            commands::config::trigger_sync,
            commands::config::get_app_data_dir,
            commands::config::log_event,
            commands::connection::connect_to_server,
            commands::connection::start_recording,
            commands::connection::stop_recording,
            commands::connection::send_command,
            commands::connection::resize_terminal,
            commands::connection::close_session,
            commands::connection::export_terminal_log,
            commands::connection::select_save_path,
            commands::master_password::get_master_password_status,
            commands::master_password::set_master_password,
            commands::master_password::verify_master_password,
            commands::ai::create_ai_session,
            commands::ai::get_ai_sessions,
            commands::ai::get_ai_messages,
            commands::ai::send_chat_message,
            commands::ai::get_terminal_text,
            commands::ai::run_in_terminal,
            commands::ai::execute_agent_tools,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
