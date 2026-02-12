#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use resh::commands;
use resh::config::ConfigManager;
use resh::db::DatabaseManager;
use resh::logger;
use resh::master_password::MasterPasswordManager;
use resh::sftp_manager::edit::SftpEditManager;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::image::Image;
use tauri::Listener;
use tauri::Manager;
use tokio::sync::Mutex;

static APP_DATA_DIR_SET: AtomicBool = AtomicBool::new(false);
static mut APP_DATA_DIR: Option<std::path::PathBuf> = None;

fn get_panic_log_path() -> std::path::PathBuf {
    // 优先使用已设置的 app_data_dir
    unsafe {
        if let Some(ref dir) = APP_DATA_DIR {
            return dir.join("logs").join("panic.log");
        }
    }
    // 回退到临时目录
    std::env::temp_dir().join("resh_panic.log")
}

#[tokio::main]
async fn main() {
    // 设置 panic hook 捕获所有 panic 并记录
    std::panic::set_hook(Box::new(|info| {
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        let location = if let Some(loc) = info.location() {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        } else {
            "unknown location".to_string()
        };

        let error_msg = format!("[PANIC] {} at {}\n", message, location);

        // 打印到 stderr
        eprintln!("{}", error_msg);

        // 尝试写入 panic 日志文件
        let log_path = get_panic_log_path();
        // 确保日志目录存在
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(file, "[{}] {}", timestamp, error_msg);
        }
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                if window.is_minimized().unwrap_or(false) {
                    let _ = window.unminimize();
                }
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
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

            // Get the default app data dir (e.g., %AppData%/com.fonlan.resh)
            let default_app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法获取应用数据目录: {}", e))?;

            let app_data_dir = default_app_data_dir
                .parent()
                .map(|p| p.join("Resh"))
                .unwrap_or_else(|| default_app_data_dir.join("Resh"));

            // 设置全局 app_data_dir 供 panic hook 使用
            unsafe {
                APP_DATA_DIR = Some(app_data_dir.clone());
                APP_DATA_DIR_SET.store(true, Ordering::SeqCst);
            }

            let config_manager = ConfigManager::new(app_data_dir.clone());
            let master_password_manager = MasterPasswordManager::new(app_data_dir.clone());
            let db_manager = DatabaseManager::new(app_data_dir.clone())
                .map_err(|e| format!("数据库初始化失败: {}", e))?;

            // Load initial config
            let local_config = config_manager
                .load_local_config()
                .unwrap_or_else(|_| resh::config::Config::empty());
            let debug_enabled = local_config.general.debug_enabled;

            // Initialize logging
            logger::init_logging(app_data_dir.clone(), debug_enabled);
            tracing::info!("Logging initialized. Debug mode: {}", debug_enabled);

            let state = Arc::new(commands::AppState {
                config_manager: config_manager.clone(),
                password_manager: master_password_manager,
                db_manager,
                config: Mutex::new(local_config.clone()),
                ai_cancellation_tokens: dashmap::DashMap::new(),
                ai_manager: resh::ai::manager::AiManager::new(),
                sftp_edit_manager: SftpEditManager::new(app.handle().clone()),
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
            }

            // Show window only after frontend reports ready to reduce white-screen time
            let app_handle = app.handle().clone();
            app.listen("resh-app-ready", move |_event| {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            });

            // Fallback: ensure window is shown even if ready event is missed
            let app_handle_for_fallback = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                if let Some(window) = app_handle_for_fallback.get_webview_window("main") {
                    if !window.is_visible().unwrap_or(false) {
                        let _ = window.show();
                    }
                }
            });

            app.manage(state.clone());

            // Listen for window events to save state
            if let Some(window) = app.get_webview_window("main") {
                let state_for_event = state.clone();
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Resized(_)
                    | tauri::WindowEvent::Moved(_)
                    | tauri::WindowEvent::CloseRequested { .. } = event
                    {
                        let window = &window_clone;

                        // Don't save state if minimized
                        if window.is_minimized().unwrap_or(false) {
                            return;
                        }

                        let is_maximized = window.is_maximized().unwrap_or(false);
                        let size = window
                            .inner_size()
                            .unwrap_or_default()
                            .to_logical(window.scale_factor().unwrap_or(1.0));
                        let position = window
                            .outer_position()
                            .unwrap_or_default()
                            .to_logical(window.scale_factor().unwrap_or(1.0));

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
            commands::connection::reconnect_session,
            commands::connection::export_terminal_log,
            commands::connection::select_save_path,
            commands::connection::update_terminal_selection,
            commands::master_password::get_master_password_status,
            commands::master_password::set_master_password,
            commands::master_password::verify_master_password,
            commands::ai::create_ai_session,
            commands::ai::get_ai_sessions,
            commands::ai::get_ai_messages,
            commands::ai::send_chat_message,
            commands::ai::cancel_ai_chat,
            commands::ai::get_terminal_output,
            commands::ai::run_in_terminal,
            commands::ai::send_interrupt,
            commands::ai::send_terminal_input,
            commands::ai::execute_agent_tools,
            commands::ai::generate_session_title,
            commands::ai::delete_ai_session,
            commands::ai::delete_all_ai_sessions,
            commands::ai::start_copilot_auth,
            commands::ai::poll_copilot_auth,
            commands::ai::open_url,
            commands::ai::fetch_ai_models,
            commands::sftp::sftp_list_dir,
            commands::sftp::sftp_download,
            commands::sftp::sftp_upload,
            commands::sftp::sftp_set_max_concurrent,
            commands::sftp::sftp_cancel_transfer,
            commands::sftp::sftp_resolve_conflict,
            commands::sftp::pick_files,
            commands::sftp::sftp_delete,
            commands::sftp::sftp_create_folder,
            commands::sftp::sftp_create_file,
            commands::sftp::sftp_chmod,
            commands::sftp::sftp_rename,
            commands::sftp::sftp_copy,
            commands::sftp_edit::sftp_edit_file,
            commands::sftp_edit::open_local_editor,
            commands::sftp_edit::pick_folder,
            commands::sftp_edit::pick_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
