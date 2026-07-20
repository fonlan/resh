#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use resh::commands;
use resh::config::ConfigManager;
use resh::db::DatabaseManager;
use resh::logger;
use resh::sftp_manager::edit::SftpEditManager;
use resh::ssh_manager::ssh::SSHClient;
use std::sync::Arc;
use std::sync::OnceLock;
use tauri::image::Image;
#[cfg(target_os = "macos")]
use tauri::Emitter;
use tauri::Listener;
use tauri::Manager;
use tokio::sync::Mutex;

#[cfg(target_os = "macos")]
use tauri::menu::{Menu, MenuItem, MenuItemKind};

#[cfg(target_os = "macos")]
const SETTINGS_MENU_ID: &str = "resh-settings";

static APP_DATA_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

fn get_panic_log_path() -> std::path::PathBuf {
    // 优先使用已设置的 app_data_dir，否则回退到临时目录
    APP_DATA_DIR
        .get()
        .map(|dir| dir.join("logs").join("panic.log"))
        .unwrap_or_else(|| std::env::temp_dir().join("resh_panic.log"))
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(target_os = "macos")]
fn build_macos_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    let settings = MenuItem::with_id(
        app,
        SETTINGS_MENU_ID,
        "Settings…",
        true,
        Some("CmdOrCtrl+,"),
    )?;

    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        // The default application menu starts with About followed by a
        // separator. Insert Settings between them to match macOS conventions.
        app_menu.insert(&settings, 1)?;
    }

    Ok(menu)
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

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_clipboard_manager::init());

    #[cfg(target_os = "macos")]
    let builder = builder.menu(build_macos_menu).on_menu_event(|app, event| {
        if event.id() == SETTINGS_MENU_ID {
            show_main_window(app);
            let _ = app.emit("resh-open-settings", ());
        }
    });

    let app = builder
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

            let app_data_dir =
                resh::app_paths::resolve_app_data_dir_from_default(&default_app_data_dir);

            // 设置全局 app_data_dir 供 panic hook 使用
            let _ = APP_DATA_DIR.set(app_data_dir.clone());

            let config_manager = ConfigManager::new(app_data_dir.clone());
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
            SSHClient::set_app_handle(app.handle().clone());

            let state = Arc::new(commands::AppState {
                config_manager: config_manager.clone(),
                db_manager,
                config: Mutex::new(local_config.clone()),
                config_sync_gate: Mutex::new(()),
                config_sync_generation: std::sync::atomic::AtomicU64::new(0),
                ai_cancellation_tokens: commands::AiRunRegistry::new(),
                ai_manager: resh::ai::manager::AiManager::new(),
                sftp_edit_manager: SftpEditManager::new(app.handle().clone()),
                operation_coordinator: std::sync::Arc::new(
                    resh::updater::OperationCoordinator::new(),
                ),
            });
            app.manage(state.clone());

            // Capture optional post-update restore token (validated later when loading snapshot).
            {
                let args: Vec<String> = std::env::args().collect();
                let token =
                    resh::updater::capture_restore_token_from_args(args.iter().map(|s| s.as_str()));
                if let Some(ref t) = token {
                    // Signal the install helper ASAP that the new binary is alive
                    // with a valid restore token (before full UI restore completes).
                    resh::updater::write_install_alive_marker(&app_data_dir, t);
                }
                resh::updater::set_pending_restore_token(token);
                resh::updater::cleanup_stale_snapshots(&app_data_dir);
            }

            // Apply window state
            if let Some(window) = app.get_webview_window("main") {
                let ws = &local_config.general.window_state;
                resh::window_state::restore_window(&window, ws);
            }

            // Show main window only after frontend reports ready to avoid white-flash period.
            let app_handle = app.handle().clone();
            app.listen("resh-app-ready", move |_event| {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            });

            // Fallback: avoid permanently hidden window if ready event is missed.
            let app_handle_for_fallback = app.handle().clone();
            let fallback_delay_ms = if cfg!(debug_assertions) { 30000 } else { 8000 };
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(fallback_delay_ms)).await;
                if let Some(window) = app_handle_for_fallback.get_webview_window("main") {
                    if !window.is_visible().unwrap_or(false) {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            });

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

                        #[cfg(target_os = "macos")]
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            // Keep the only window alive so Dock reopen and a second
                            // launch can restore it without rebuilding application state.
                            api.prevent_close();
                            let _ = window.hide();
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::get_config,
            commands::config::backend_smoke_check,
            commands::config::save_config,
            commands::config::record_server_connection,
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
            commands::ai::create_ai_session,
            commands::ai::get_ai_sessions,
            commands::ai::get_ai_messages,
            commands::ai::send_chat_message,
            commands::ai::regenerate_ai_response,
            commands::ai::cancel_ai_chat,
            commands::ai::get_terminal_output,
            commands::ai::run_in_terminal,
            commands::ai::run_in_background,
            commands::ai::send_interrupt,
            commands::ai::send_terminal_input,
            commands::ai::execute_agent_tools,
            commands::ai::get_ai_run_snapshot,
            commands::ai::get_pending_tool_approvals,
            commands::ai::generate_session_title,
            commands::ai::delete_ai_session,
            commands::ai::delete_all_ai_sessions,
            commands::ai::copilot::start_copilot_auth,
            commands::ai::copilot::poll_copilot_auth,
            commands::ai::copilot::open_url,
            commands::ai::fetch_ai_models,
            commands::sftp::sftp_list_dir,
            commands::sftp::sftp_list_dir_sorted,
            commands::sftp::sftp_list_dirs_sorted,
            commands::sftp::sftp_prepare_dir_listing_sorted,
            commands::sftp::sftp_get_dir_listing_page,
            commands::sftp::sftp_release_dir_listing,
            commands::sftp::sftp_download,
            commands::sftp::sftp_upload,
            commands::sftp::sftp_set_max_concurrent,
            commands::sftp::sftp_set_max_concurrent_per_session,
            commands::sftp::sftp_cancel_transfer,
            commands::sftp::sftp_resolve_conflict,
            commands::sftp::pick_files,
            commands::sftp::sftp_delete,
            commands::sftp::sftp_create_folder,
            commands::sftp::sftp_create_file,
            commands::sftp::sftp_chmod,
            commands::sftp::sftp_rename,
            commands::sftp::sftp_copy,
            commands::sftp::sftp_copy_streaming,
            commands::sftp_edit::sftp_open_text_file,
            commands::sftp_edit::sftp_save_text_file,
            commands::sftp_edit::sftp_edit_file,
            commands::sftp_edit::open_local_editor,
            commands::sftp_edit::pick_folder,
            commands::sftp_edit::pick_file,
            commands::updater::check_for_update_cmd,
            commands::updater::get_app_version_cmd,
            commands::updater::download_update_cmd,
            commands::updater::cancel_update_download_cmd,
            commands::updater::get_operation_snapshot_cmd,
            commands::updater::begin_restart_draining_cmd,
            commands::updater::cancel_restart_draining_cmd,
            commands::updater::wait_until_operations_idle_cmd,
            commands::updater::save_restart_session_snapshot_cmd,
            commands::updater::get_pending_restart_session_cmd,
            commands::updater::ack_restart_session_cmd,
            commands::updater::verify_ready_for_restart_cmd,
            commands::updater::install_prepared_update_cmd,
            commands::updater::get_last_install_failure_cmd,
            commands::updater::ack_update_install_cmd,
            commands::updater::platform_supports_install_cmd,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, _event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            show_main_window(_app_handle);
        }
    });
}
