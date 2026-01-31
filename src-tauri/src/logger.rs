use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::Level;
use tracing_log::LogTracer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, Layer};

/// Global atomic to control debug logging
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn init_logging(app_data_dir: PathBuf, debug_enabled: bool) {
    DEBUG_ENABLED.store(debug_enabled, Ordering::SeqCst);

    // Convert standard log events (from russh, etc.) to tracing events
    // Use try_init to avoid panicking if already set
    let _ = LogTracer::init();

    let log_dir = app_data_dir.join("logs");

    // Ensure log directory exists
    if !log_dir.exists() {
        let _ = std::fs::create_dir_all(&log_dir);
    }

    // Daily rolling file appender: resh.YYYY-MM-DD
    let file_appender = tracing_appender::rolling::daily(log_dir, "resh");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Filter that checks the atomic bool dynamically
    let file_layer = fmt::Layer::default()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            let max_level = if DEBUG_ENABLED.load(Ordering::Relaxed) {
                Level::DEBUG
            } else {
                Level::INFO
            };
            // Suppress verbose debug logs from third-party libraries
            if metadata.level() <= &max_level {
                let target = metadata.target();
                if target.starts_with("russh::cipher")
                    || target.starts_with("hyper::proto::h1::decode")
                {
                    // Only allow WARN and above for these verbose modules
                    metadata.level() <= &Level::WARN
                } else {
                    true
                }
            } else {
                false
            }
        }));

    // Registry without a global EnvFilter to allow layers to decide their own levels
    // Use try_init to avoid panicking
    let _ = tracing_subscriber::registry().with(file_layer).try_init();

    // Note: _guard must be kept alive to ensure logs are flushed.
    // In a Tauri app, we let it leak for the app's lifetime.
    std::mem::forget(_guard);
}

pub fn set_log_level(debug_enabled: bool) {
    DEBUG_ENABLED.store(debug_enabled, Ordering::SeqCst);
}
