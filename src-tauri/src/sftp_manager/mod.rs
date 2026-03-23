use crate::commands::AppState;
use crate::config::types::SftpSettings;
use crate::db::DatabaseManager;
use crate::ssh_manager::ssh::SSHClient;
use futures::stream::FuturesUnordered;
use futures::FutureExt;
use futures::StreamExt;
use lazy_static::lazy_static;
use russh_sftp::client::RawSftpSession;
use russh_sftp::extensions::{self, LimitsExtension};
use russh_sftp::protocol::{FileAttributes, OpenFlags, Packet, Status, StatusCode};
use russh_sftp::ser;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::SeekFrom;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

pub mod edit;

#[derive(Serialize, Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: u64,
    pub link_target: Option<String>,
    pub target_is_dir: Option<bool>,
    pub permissions: Option<u32>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DirectoryListResult {
    pub path: String,
    pub files: Vec<FileEntry>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DirectoryListingHandle {
    pub token: String,
    pub total: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct DirectoryListingPage {
    pub files: Vec<FileEntry>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub enum SftpSortType {
    Name,
    Modified,
}

#[derive(Debug, Clone, Copy)]
pub enum SftpSortOrder {
    Asc,
    Desc,
}

#[derive(Serialize, Clone, Debug)]
pub struct TransferProgress {
    pub task_id: String,
    pub type_: String,
    pub session_id: String,
    pub file_name: String,
    pub source: String,
    pub destination: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub speed: f64,
    pub eta: Option<u64>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FileConflict {
    pub task_id: String,
    pub session_id: String,
    pub file_path: String,
    pub local_size: Option<u64>,
    pub remote_size: Option<u64>,
    pub local_modified: Option<u64>,
    pub remote_modified: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum ConflictResolution {
    Overwrite,
    Skip,
    Cancel,
}

#[derive(Debug, Clone, Serialize)]
pub enum TransferType {
    Download,
    Upload,
}

#[derive(Clone)]
pub struct PendingTask {
    pub task_id: String,
    pub transfer_type: TransferType,
    pub session_id: String,
    pub remote_path: String,
    pub local_path: String,
    pub app: Arc<AppHandle>,
    pub db_manager: DatabaseManager,
    pub cancel_token: Arc<AtomicBool>,
}

const COPY_DATA_EXTENSION_NAME: &str = "copy-data";
const COPY_DATA_EXTENSION_VERSION: &str = "1";
const COPY_TRANSFER_TYPE: &str = "copy";
const COPY_DATA_UNSUPPORTED_ERROR: &str = "SFTP_COPY_DATA_UNSUPPORTED";
const SFTP_REQUEST_TIMEOUT_SECS: u64 = 60;
const DOWNLOAD_CHUNK_SIZE: u64 = 256 * 1024;
const MIN_CHUNK_SIZE_BYTES: u64 = 4 * 1024;
const MAX_CHUNK_SIZE_BYTES: u64 = 1024 * 1024;
const UPLOAD_CHUNK_SIZE_SAFE: u64 = 64 * 1024;
const UPLOAD_CHUNK_SIZE_BALANCED: u64 = 128 * 1024;
const UPLOAD_CHUNK_SIZE_FAST: u64 = 256 * 1024;
const UPLOAD_MAX_INFLIGHT_SAFE: usize = 6;
const UPLOAD_MAX_INFLIGHT_BALANCED: usize = 12;
const UPLOAD_MAX_INFLIGHT_FAST: usize = 16;
const UPLOAD_CHUNK_WRITE_TIMEOUT_SECS: u64 = 30;
const UPLOAD_TIMEOUT_DOWNGRADE_THRESHOLD: u32 = 2;
const UPLOAD_MAX_RETRIES_PER_CHUNK: u8 = 2;
const DOWNLOAD_INITIAL_INFLIGHT: usize = 2;
const DOWNLOAD_TIMEOUT_DOWNGRADE_THRESHOLD: u32 = 2;
const DOWNLOAD_FALLBACK_LOCK_TIMEOUT_THRESHOLD: u32 = 4;
const DOWNLOAD_RAMP_UP_SUCCESS_CHUNKS: u32 = 8;
const DOWNLOAD_MAX_RETRIES_PER_CHUNK: u8 = 2;
const MAX_INFLIGHT_LIMIT: usize = 64;
const TRANSFER_DIAG_INTERVAL_SECS: u64 = 2;
const MAX_CONCURRENT_TRANSFERS_PER_SESSION: usize = 2;

#[derive(Clone, Copy, Debug, Default)]
struct SftpServerLimits {
    max_packet_len: Option<u64>,
    max_read_len: Option<u64>,
    max_write_len: Option<u64>,
    max_open_handles: Option<u64>,
}

impl SftpServerLimits {
    fn from_extension(limits: LimitsExtension) -> Self {
        Self {
            max_packet_len: (limits.max_packet_len > 0).then_some(limits.max_packet_len),
            max_read_len: (limits.max_read_len > 0).then_some(limits.max_read_len),
            max_write_len: (limits.max_write_len > 0).then_some(limits.max_write_len),
            max_open_handles: (limits.max_open_handles > 0).then_some(limits.max_open_handles),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TransferProfile {
    Safe,
    Balanced,
    Fast,
}

impl TransferProfile {
    fn from_str(raw: &str) -> Self {
        match raw {
            "safe" => Self::Safe,
            "fast" => Self::Fast,
            _ => Self::Balanced,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Balanced => "balanced",
            Self::Fast => "fast",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TransferRuntimeConfig {
    profile: TransferProfile,
    download_max_inflight: usize,
    upload_max_inflight: usize,
    chunk_size_min: u64,
    chunk_size_max: u64,
}

#[derive(Clone, Copy, Debug)]
struct TransferTuning {
    profile: TransferProfile,
    download_chunk_size: u64,
    upload_chunk_size: u64,
    download_max_inflight: usize,
    upload_max_inflight: usize,
}

#[derive(Clone, Copy, Debug)]
struct SpeedSampler {
    display_speed: f64,
    last_sample_at: Instant,
    last_sample_bytes: u64,
}

impl SpeedSampler {
    fn new(start_at: Instant) -> Self {
        Self {
            display_speed: 0.0,
            last_sample_at: start_at,
            last_sample_bytes: 0,
        }
    }

    fn sample(&mut self, now: Instant, transferred_bytes: u64) -> f64 {
        let sample_secs = now.duration_since(self.last_sample_at).as_secs_f64();
        if sample_secs > 0.0 && transferred_bytes >= self.last_sample_bytes {
            let instant_speed = (transferred_bytes - self.last_sample_bytes) as f64 / sample_secs;
            self.display_speed = if self.display_speed > 0.0 {
                (self.display_speed * 0.6) + (instant_speed * 0.4)
            } else {
                instant_speed
            };
        }
        self.last_sample_at = now;
        self.last_sample_bytes = transferred_bytes;
        self.display_speed
    }
}

#[derive(Clone, Copy, Debug)]
struct TransferDiagnostics {
    started_at: Instant,
    last_logged_at: Instant,
    timeout_count: u32,
    consecutive_timeout_count: u32,
    downgrade_count: u32,
    retry_count: u32,
    rtt_total_ms: f64,
    rtt_samples: u64,
}

impl TransferDiagnostics {
    fn new(now: Instant) -> Self {
        Self {
            started_at: now,
            last_logged_at: now,
            timeout_count: 0,
            consecutive_timeout_count: 0,
            downgrade_count: 0,
            retry_count: 0,
            rtt_total_ms: 0.0,
            rtt_samples: 0,
        }
    }

    fn record_rtt(&mut self, elapsed: Duration) {
        self.rtt_total_ms += elapsed.as_secs_f64() * 1000.0;
        self.rtt_samples += 1;
    }

    fn avg_rtt_ms(&self) -> Option<f64> {
        if self.rtt_samples == 0 {
            None
        } else {
            Some(self.rtt_total_ms / self.rtt_samples as f64)
        }
    }

    fn mark_timeout(&mut self) {
        self.timeout_count += 1;
        self.consecutive_timeout_count += 1;
    }

    fn mark_retry(&mut self) {
        self.retry_count += 1;
    }

    fn mark_success(&mut self) {
        self.consecutive_timeout_count = 0;
    }

    fn mark_downgrade(&mut self) {
        self.downgrade_count += 1;
        self.consecutive_timeout_count = 0;
    }

    fn should_log_progress(&self, now: Instant) -> bool {
        now.duration_since(self.last_logged_at) >= Duration::from_secs(TRANSFER_DIAG_INTERVAL_SECS)
    }

    fn touch_log_time(&mut self, now: Instant) {
        self.last_logged_at = now;
    }
}

#[derive(Serialize)]
struct CopyDataExtension {
    read_from_handle: String,
    read_from_offset: u64,
    read_data_length: u64,
    write_to_handle: String,
    write_to_offset: u64,
}

enum ServerSideCopyError {
    Unsupported,
    Other(String),
}

#[derive(Clone, Copy)]
enum CopyMode {
    ServerSideOnly,
    StreamingOnly,
}

struct CopyProgressContext<'a> {
    app: &'a AppHandle,
    task_id: &'a str,
    session_id: &'a str,
    file_name: &'a str,
    source: &'a str,
    destination: &'a str,
}

#[derive(Clone)]
struct CachedDirectoryListing {
    session_id: String,
    files: Arc<Vec<FileEntry>>,
    created_at: Instant,
}

lazy_static! {
    static ref SFTP_SESSIONS: Mutex<HashMap<String, Arc<RawSftpSession>>> =
        Mutex::new(HashMap::new());
    static ref SFTP_COPY_DATA_SUPPORT: Mutex<HashMap<String, bool>> = Mutex::new(HashMap::new());
    static ref SFTP_SERVER_LIMITS: Mutex<HashMap<String, SftpServerLimits>> =
        Mutex::new(HashMap::new());
    static ref SFTP_DOWNLOAD_FALLBACK_LOCK: Mutex<HashMap<String, bool>> =
        Mutex::new(HashMap::new());
    static ref TRANSFER_TASKS: Mutex<HashMap<String, Arc<AtomicBool>>> = Mutex::new(HashMap::new());
    static ref CONFLICT_RESPONSES: Mutex<HashMap<String, oneshot::Sender<ConflictResolution>>> =
        Mutex::new(HashMap::new());
    static ref TASK_QUEUE: Mutex<VecDeque<PendingTask>> = Mutex::new(VecDeque::new());
    static ref ACTIVE_TASKS: Mutex<HashMap<String, Arc<AtomicBool>>> = Mutex::new(HashMap::new());
    static ref ACTIVE_TASK_SESSIONS: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
    static ref MAX_CONCURRENT_TRANSFERS: Mutex<u32> = Mutex::new(2);
    static ref QUEUE_NOTIFY: Notify = Notify::new();
    static ref SCHEDULER_RUNNING: AtomicBool = AtomicBool::new(false);
    static ref DIRECTORY_LISTING_CACHE: Mutex<HashMap<String, CachedDirectoryListing>> =
        Mutex::new(HashMap::new());
}

const DIRECTORY_LISTING_CACHE_MAX_ENTRIES: usize = 32;
const DIRECTORY_LISTING_PAGE_LIMIT_DEFAULT: usize = 400;
const DIRECTORY_LISTING_PAGE_LIMIT_MAX: usize = 2_000;

pub struct SftpManager;

impl SftpManager {
    fn sort_entries(files: &mut [FileEntry], sort_type: SftpSortType, sort_order: SftpSortOrder) {
        files.sort_by(|a, b| {
            let a_dir_like = a.is_dir || (a.is_symlink && a.target_is_dir.unwrap_or(false));
            let b_dir_like = b.is_dir || (b.is_symlink && b.target_is_dir.unwrap_or(false));

            if a_dir_like != b_dir_like {
                return b_dir_like.cmp(&a_dir_like);
            }

            let mut comparison = match sort_type {
                SftpSortType::Name => a.name.cmp(&b.name),
                SftpSortType::Modified => a
                    .modified
                    .cmp(&b.modified)
                    .then_with(|| a.name.cmp(&b.name)),
            };

            if matches!(sort_order, SftpSortOrder::Desc) {
                comparison = comparison.reverse();
            }

            comparison
        });
    }

    fn clamp_chunk_size(value: u64, min_size: u64, max_size: u64) -> u64 {
        let min_size = min_size.max(MIN_CHUNK_SIZE_BYTES);
        let max_size = max_size.max(min_size).min(MAX_CHUNK_SIZE_BYTES);
        value.clamp(min_size, max_size)
    }

    fn clamp_inflight(value: usize) -> usize {
        value.clamp(1, MAX_INFLIGHT_LIMIT)
    }

    fn default_transfer_runtime_config() -> TransferRuntimeConfig {
        TransferRuntimeConfig {
            profile: TransferProfile::Balanced,
            download_max_inflight: 8,
            upload_max_inflight: UPLOAD_MAX_INFLIGHT_BALANCED,
            chunk_size_min: UPLOAD_CHUNK_SIZE_SAFE,
            chunk_size_max: UPLOAD_CHUNK_SIZE_FAST,
        }
    }

    fn transfer_runtime_config_from_settings(settings: &SftpSettings) -> TransferRuntimeConfig {
        let min_chunk = settings.chunk_size_min.max(MIN_CHUNK_SIZE_BYTES);
        let max_chunk = settings
            .chunk_size_max
            .max(min_chunk)
            .min(MAX_CHUNK_SIZE_BYTES);
        TransferRuntimeConfig {
            profile: TransferProfile::from_str(&settings.transfer_profile),
            download_max_inflight: Self::clamp_inflight(settings.download_max_inflight as usize),
            upload_max_inflight: Self::clamp_inflight(settings.upload_max_inflight as usize),
            chunk_size_min: min_chunk,
            chunk_size_max: max_chunk,
        }
    }

    async fn resolve_transfer_runtime_config(app: &AppHandle) -> TransferRuntimeConfig {
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            let config = state.config.lock().await;
            return Self::transfer_runtime_config_from_settings(&config.general.sftp);
        }
        Self::default_transfer_runtime_config()
    }

    async fn get_server_limits(session_id: &str) -> Option<SftpServerLimits> {
        let limits = SFTP_SERVER_LIMITS.lock().await;
        limits.get(session_id).copied()
    }

    fn calculate_transfer_tuning(
        runtime: TransferRuntimeConfig,
        limits: Option<SftpServerLimits>,
    ) -> TransferTuning {
        let (profile_upload_chunk, profile_upload_inflight) = match runtime.profile {
            TransferProfile::Safe => (UPLOAD_CHUNK_SIZE_SAFE, UPLOAD_MAX_INFLIGHT_SAFE),
            TransferProfile::Balanced => (UPLOAD_CHUNK_SIZE_BALANCED, UPLOAD_MAX_INFLIGHT_BALANCED),
            TransferProfile::Fast => (UPLOAD_CHUNK_SIZE_FAST, UPLOAD_MAX_INFLIGHT_FAST),
        };

        let mut download_chunk = Self::clamp_chunk_size(
            DOWNLOAD_CHUNK_SIZE,
            runtime.chunk_size_min,
            runtime.chunk_size_max,
        );
        let mut upload_chunk = Self::clamp_chunk_size(
            profile_upload_chunk,
            runtime.chunk_size_min,
            runtime.chunk_size_max,
        );

        if let Some(limits) = limits {
            if let Some(max_read_len) = limits.max_read_len {
                download_chunk = download_chunk.min(max_read_len.max(MIN_CHUNK_SIZE_BYTES));
            }
            if let Some(max_write_len) = limits.max_write_len {
                upload_chunk = upload_chunk.min(max_write_len.max(MIN_CHUNK_SIZE_BYTES));
            }
            if let Some(max_packet_len) = limits.max_packet_len {
                download_chunk = download_chunk.min(max_packet_len.max(MIN_CHUNK_SIZE_BYTES));
                upload_chunk = upload_chunk.min(max_packet_len.max(MIN_CHUNK_SIZE_BYTES));
            }
        }

        download_chunk = Self::clamp_chunk_size(
            download_chunk,
            runtime.chunk_size_min,
            runtime.chunk_size_max,
        );
        upload_chunk =
            Self::clamp_chunk_size(upload_chunk, runtime.chunk_size_min, runtime.chunk_size_max);

        let mut download_max_inflight = runtime.download_max_inflight;
        let mut upload_max_inflight = profile_upload_inflight.min(runtime.upload_max_inflight);
        if let Some(max_handles) = limits.and_then(|v| v.max_open_handles) {
            if max_handles > 2 {
                let handle_cap = (max_handles - 2) as usize;
                if handle_cap > 0 {
                    download_max_inflight = download_max_inflight.min(handle_cap);
                    upload_max_inflight = upload_max_inflight.min(handle_cap);
                }
            }
        }

        TransferTuning {
            profile: runtime.profile,
            download_chunk_size: download_chunk,
            upload_chunk_size: upload_chunk,
            download_max_inflight: Self::clamp_inflight(download_max_inflight),
            upload_max_inflight: Self::clamp_inflight(upload_max_inflight),
        }
    }

    async fn resolve_transfer_tuning(app: &AppHandle, session_id: &str) -> TransferTuning {
        let runtime = Self::resolve_transfer_runtime_config(app).await;
        let limits = Self::get_server_limits(session_id).await;
        Self::calculate_transfer_tuning(runtime, limits)
    }

    async fn is_download_fallback_locked(session_id: &str) -> bool {
        let locks = SFTP_DOWNLOAD_FALLBACK_LOCK.lock().await;
        locks.get(session_id).copied().unwrap_or(false)
    }

    async fn set_download_fallback_lock(session_id: &str, locked: bool) {
        let mut locks = SFTP_DOWNLOAD_FALLBACK_LOCK.lock().await;
        locks.insert(session_id.to_string(), locked);
    }

    async fn log_transfer_start(
        task_id: &str,
        session_id: &str,
        transfer_type: &str,
        source: &str,
        destination: &str,
        total_bytes: u64,
        tuning: TransferTuning,
    ) {
        let limits = Self::get_server_limits(session_id).await;
        tracing::info!(
            target: "sftp::transfer",
            task_id = task_id,
            session_id = session_id,
            transfer_type = transfer_type,
            source = source,
            destination = destination,
            total_bytes = total_bytes,
            profile = tuning.profile.as_str(),
            download_chunk_size = tuning.download_chunk_size,
            upload_chunk_size = tuning.upload_chunk_size,
            download_max_inflight = tuning.download_max_inflight,
            upload_max_inflight = tuning.upload_max_inflight,
            server_max_packet_len = ?limits.and_then(|v| v.max_packet_len),
            server_max_read_len = ?limits.and_then(|v| v.max_read_len),
            server_max_write_len = ?limits.and_then(|v| v.max_write_len),
            server_max_open_handles = ?limits.and_then(|v| v.max_open_handles),
            "transfer started"
        );
    }

    fn log_transfer_progress(
        task_id: &str,
        session_id: &str,
        transfer_type: &str,
        total_bytes: u64,
        transferred_bytes: u64,
        speed_bytes_per_sec: f64,
        inflight: usize,
        diagnostics: &TransferDiagnostics,
    ) {
        tracing::debug!(
            target: "sftp::transfer",
            task_id = task_id,
            session_id = session_id,
            transfer_type = transfer_type,
            total_bytes = total_bytes,
            transferred_bytes = transferred_bytes,
            speed_bytes_per_sec = speed_bytes_per_sec,
            inflight = inflight,
            elapsed_ms = diagnostics.started_at.elapsed().as_millis() as u64,
            timeout_count = diagnostics.timeout_count,
            consecutive_timeout_count = diagnostics.consecutive_timeout_count,
            downgrade_count = diagnostics.downgrade_count,
            retry_count = diagnostics.retry_count,
            avg_rtt_ms = ?diagnostics.avg_rtt_ms(),
            "transfer progress"
        );
    }

    fn log_transfer_finish(
        task_id: &str,
        session_id: &str,
        transfer_type: &str,
        status: &str,
        total_bytes: u64,
        transferred_bytes: u64,
        diagnostics: &TransferDiagnostics,
        error: Option<&str>,
    ) {
        tracing::info!(
            target: "sftp::transfer",
            task_id = task_id,
            session_id = session_id,
            transfer_type = transfer_type,
            status = status,
            total_bytes = total_bytes,
            transferred_bytes = transferred_bytes,
            elapsed_ms = diagnostics.started_at.elapsed().as_millis() as u64,
            timeout_count = diagnostics.timeout_count,
            consecutive_timeout_count = diagnostics.consecutive_timeout_count,
            downgrade_count = diagnostics.downgrade_count,
            retry_count = diagnostics.retry_count,
            avg_rtt_ms = ?diagnostics.avg_rtt_ms(),
            error = ?error,
            "transfer finished"
        );
    }

    pub async fn set_max_concurrent_transfers(max: u32) {
        let mut current = MAX_CONCURRENT_TRANSFERS.lock().await;
        *current = max;
        QUEUE_NOTIFY.notify_one();
    }

    async fn process_queue() {
        loop {
            let next_task = {
                let max_concurrent = *MAX_CONCURRENT_TRANSFERS.lock().await;
                let mut queue = TASK_QUEUE.lock().await;
                let mut active = ACTIVE_TASKS.lock().await;
                let mut active_task_sessions = ACTIVE_TASK_SESSIONS.lock().await;

                if active.len() < max_concurrent as usize {
                    let next_index = queue.iter().position(|task| {
                        let active_for_session = active_task_sessions
                            .values()
                            .filter(|sid| sid.as_str() == task.session_id.as_str())
                            .count();
                        active_for_session < MAX_CONCURRENT_TRANSFERS_PER_SESSION
                    });

                    if let Some(index) = next_index {
                        let task = queue.remove(index).expect("queue index must exist");
                        let task_id = task.task_id.clone();
                        let session_id = task.session_id.clone();
                        let cancel_token = task.cancel_token.clone();
                        active.insert(task_id, cancel_token);
                        active_task_sessions.insert(task.task_id.clone(), session_id);
                        Some(task)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some(task) = next_task {
                let transfer_type = task.transfer_type.clone();
                let app = Arc::clone(&task.app);
                let db_manager = task.db_manager.clone();
                let session_id = task.session_id.clone();
                let remote_path = task.remote_path.clone();
                let local_path = task.local_path.clone();
                let task_id = task.task_id.clone();
                let cancel_token = task.cancel_token.clone();

                tokio::spawn(async move {
                    let _ = match transfer_type {
                        TransferType::Download => {
                            Self::_download_file(
                                (*app).clone(),
                                db_manager,
                                session_id,
                                remote_path,
                                local_path,
                                task_id.clone(),
                                cancel_token,
                                None,
                            )
                            .await
                        }
                        TransferType::Upload => {
                            Self::_upload_file(
                                (*app).clone(),
                                db_manager,
                                session_id,
                                local_path,
                                remote_path,
                                task_id.clone(),
                                cancel_token,
                                None,
                            )
                            .await
                        }
                    };

                    let mut active = ACTIVE_TASKS.lock().await;
                    active.remove(&task_id);
                    drop(active);
                    let mut active_task_sessions = ACTIVE_TASK_SESSIONS.lock().await;
                    active_task_sessions.remove(&task_id);
                    QUEUE_NOTIFY.notify_one();
                });

                continue;
            }

            let should_exit = {
                let queue_empty = TASK_QUEUE.lock().await.is_empty();
                let active_empty = ACTIVE_TASKS.lock().await.is_empty();
                queue_empty && active_empty
            };

            if should_exit {
                break;
            }

            QUEUE_NOTIFY.notified().await;
        }
    }

    fn spawn_scheduler() {
        if SCHEDULER_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        tokio::spawn(async move {
            Self::process_queue().await;
            SCHEDULER_RUNNING.store(false, Ordering::SeqCst);

            if !TASK_QUEUE.lock().await.is_empty() {
                Self::spawn_scheduler();
                QUEUE_NOTIFY.notify_one();
            }
        });
    }

    pub async fn queue_download(
        app: AppHandle,
        db_manager: DatabaseManager,
        session_id: String,
        remote_path: String,
        local_path: String,
        task_id: Option<String>,
    ) -> Result<String, String> {
        let task_id = task_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let cancel_token = Arc::new(AtomicBool::new(false));

        {
            let mut tasks = TRANSFER_TASKS.lock().await;
            tasks.insert(task_id.clone(), cancel_token.clone());
        }

        let pending_task = PendingTask {
            task_id: task_id.clone(),
            transfer_type: TransferType::Download,
            session_id: session_id.clone(),
            remote_path: remote_path.clone(),
            local_path: local_path.clone(),
            app: Arc::new(app.clone()),
            db_manager,
            cancel_token,
        };

        // Emit queued status
        let _ = app.emit(
            "transfer-progress",
            TransferProgress {
                task_id: task_id.clone(),
                type_: "download".to_string(),
                session_id: session_id.clone(),
                file_name: std::path::Path::new(&remote_path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                source: remote_path,
                destination: local_path,
                total_bytes: 0,
                transferred_bytes: 0,
                speed: 0.0,
                eta: None,
                status: "queued".to_string(),
                error: None,
            },
        );

        {
            let mut queue = TASK_QUEUE.lock().await;
            queue.push_back(pending_task);
        }

        Self::spawn_scheduler();
        QUEUE_NOTIFY.notify_one();

        Ok(task_id)
    }

    pub async fn queue_upload(
        app: AppHandle,
        db_manager: DatabaseManager,
        session_id: String,
        local_path: String,
        remote_path: String,
        task_id: Option<String>,
    ) -> Result<String, String> {
        let task_id = task_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let cancel_token = Arc::new(AtomicBool::new(false));

        {
            let mut tasks = TRANSFER_TASKS.lock().await;
            tasks.insert(task_id.clone(), cancel_token.clone());
        }

        let pending_task = PendingTask {
            task_id: task_id.clone(),
            transfer_type: TransferType::Upload,
            session_id: session_id.clone(),
            remote_path: remote_path.clone(),
            local_path: local_path.clone(),
            app: Arc::new(app.clone()),
            db_manager,
            cancel_token,
        };

        // Emit queued status
        let _ = app.emit(
            "transfer-progress",
            TransferProgress {
                task_id: task_id.clone(),
                type_: "upload".to_string(),
                session_id: session_id.clone(),
                file_name: std::path::Path::new(&local_path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                source: local_path,
                destination: remote_path,
                total_bytes: 0,
                transferred_bytes: 0,
                speed: 0.0,
                eta: None,
                status: "queued".to_string(),
                error: None,
            },
        );

        {
            let mut queue = TASK_QUEUE.lock().await;
            queue.push_back(pending_task);
        }

        Self::spawn_scheduler();
        QUEUE_NOTIFY.notify_one();

        Ok(task_id)
    }

    pub async fn metadata(
        session_id: &str,
        path: &str,
    ) -> Result<russh_sftp::protocol::Attrs, String> {
        let sftp = Self::get_session(session_id).await?;
        sftp.stat(path).await.map_err(|e| e.to_string())
    }

    async fn check_file_exists(
        sftp: &Arc<RawSftpSession>,
        path: &str,
    ) -> Result<Option<FileAttributes>, String> {
        match sftp.stat(path).await {
            Ok(attrs) => Ok(Some(attrs.attrs)),
            Err(russh_sftp::client::error::Error::Status(status))
                if status.status_code == StatusCode::NoSuchFile =>
            {
                Ok(None)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    async fn wait_for_conflict_resolution(
        app: &AppHandle,
        task_id: &str,
        session_id: &str,
        remote_path: &str,
        local_metadata: &std::fs::Metadata,
        remote_attrs: &FileAttributes,
    ) -> Result<ConflictResolution, String> {
        let (tx, rx) = oneshot::channel();

        {
            let mut responses = CONFLICT_RESPONSES.lock().await;
            responses.insert(task_id.to_string(), tx);
        }

        let conflict = FileConflict {
            task_id: task_id.to_string(),
            session_id: session_id.to_string(),
            file_path: remote_path.to_string(),
            local_size: Some(local_metadata.len()),
            remote_size: remote_attrs.size,
            local_modified: local_metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
            remote_modified: remote_attrs.mtime.map(|m| m as u64),
        };

        app.emit("sftp-file-conflict", conflict)
            .map_err(|e| e.to_string())?;

        match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
            Ok(Ok(resolution)) => Ok(resolution),
            Ok(Err(_)) => Err("Conflict resolution sender dropped".to_string()),
            Err(_) => Err("Conflict resolution timeout (5 minutes)".to_string()),
        }
    }

    pub async fn resolve_conflict(task_id: String, resolution: String) -> Result<(), String> {
        let mut responses = CONFLICT_RESPONSES.lock().await;
        if let Some(tx) = responses.remove(&task_id) {
            let res = match resolution.as_str() {
                "overwrite" => ConflictResolution::Overwrite,
                "skip" => ConflictResolution::Skip,
                "cancel" => ConflictResolution::Cancel,
                _ => return Err("Invalid resolution".to_string()),
            };
            tx.send(res)
                .map_err(|_| "Failed to send resolution".to_string())?;
            Ok(())
        } else {
            Err("Conflict not found".to_string())
        }
    }

    pub async fn get_session(session_id: &str) -> Result<Arc<RawSftpSession>, String> {
        let mut sessions = SFTP_SESSIONS.lock().await;
        if let Some(s) = sessions.get(session_id) {
            let sftp = s.clone();
            drop(sessions);
            sftp.set_timeout(SFTP_REQUEST_TIMEOUT_SECS).await;
            return Ok(sftp);
        }

        let ssh_session = SSHClient::get_session_handle(session_id)
            .await
            .ok_or("SSH session not found")?;

        let channel = ssh_session
            .channel_open_session()
            .await
            .map_err(|e| format!("Failed to open channel: {}", e))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| format!("Failed to request SFTP subsystem: {}", e))?;

        let sftp = RawSftpSession::new(channel.into_stream());
        sftp.set_timeout(SFTP_REQUEST_TIMEOUT_SECS).await;
        let version = sftp
            .init()
            .await
            .map_err(|e| format!("Failed to init SFTP session: {}", e))?;
        let supports_copy_data = version
            .extensions
            .get(COPY_DATA_EXTENSION_NAME)
            .is_some_and(|value| value == COPY_DATA_EXTENSION_VERSION);
        let limits = if version
            .extensions
            .get(extensions::LIMITS)
            .is_some_and(|value| value == "1")
        {
            match sftp.limits().await {
                Ok(limits) => Some(SftpServerLimits::from_extension(limits)),
                Err(error) => {
                    tracing::warn!(
                        target: "sftp::transfer",
                        session_id = session_id,
                        error = %error,
                        "Failed to query limits@openssh.com, fallback to local defaults"
                    );
                    None
                }
            }
        } else {
            None
        };

        let sftp = Arc::new(sftp);
        sessions.insert(session_id.to_string(), sftp.clone());
        drop(sessions);

        let mut copy_data_support = SFTP_COPY_DATA_SUPPORT.lock().await;
        copy_data_support.insert(session_id.to_string(), supports_copy_data);
        drop(copy_data_support);
        if let Some(limits) = limits {
            let mut limits_cache = SFTP_SERVER_LIMITS.lock().await;
            limits_cache.insert(session_id.to_string(), limits);
            tracing::info!(
                target: "sftp::transfer",
                session_id = session_id,
                max_packet_len = ?limits.max_packet_len,
                max_read_len = ?limits.max_read_len,
                max_write_len = ?limits.max_write_len,
                max_open_handles = ?limits.max_open_handles,
                "SFTP server limits cached"
            );
        } else {
            let mut limits_cache = SFTP_SERVER_LIMITS.lock().await;
            limits_cache.remove(session_id);
        }

        Ok(sftp)
    }

    pub async fn remove_session(session_id: &str) {
        let mut sessions = SFTP_SESSIONS.lock().await;
        sessions.remove(session_id);
        drop(sessions);

        let mut copy_data_support = SFTP_COPY_DATA_SUPPORT.lock().await;
        copy_data_support.remove(session_id);
        drop(copy_data_support);
        let mut limits_cache = SFTP_SERVER_LIMITS.lock().await;
        limits_cache.remove(session_id);
        let mut fallback_lock = SFTP_DOWNLOAD_FALLBACK_LOCK.lock().await;
        fallback_lock.remove(session_id);

        let mut cache = DIRECTORY_LISTING_CACHE.lock().await;
        cache.retain(|_, listing| listing.session_id != session_id);
    }

    pub async fn list_dir(session_id: &str, path: &str) -> Result<Vec<FileEntry>, String> {
        Self::list_dir_with_sort(session_id, path, SftpSortType::Name, SftpSortOrder::Asc).await
    }

    pub async fn list_dir_with_sort(
        session_id: &str,
        path: &str,
        sort_type: SftpSortType,
        sort_order: SftpSortOrder,
    ) -> Result<Vec<FileEntry>, String> {
        let sftp = Self::get_session(session_id).await?;
        let path = if path.is_empty() { "." } else { path };

        let handle = sftp.opendir(path).await.map_err(|e| e.to_string())?.handle;

        let mut files = Vec::new();
        loop {
            match sftp.readdir(&handle).await {
                Ok(name) => {
                    for entry in name.files {
                        let file_name = entry.filename;
                        if file_name == "." || file_name == ".." {
                            continue;
                        }

                        let is_dir = entry.attrs.is_dir();
                        let is_symlink = entry.attrs.is_symlink();
                        let size = entry.attrs.size.unwrap_or(0);
                        let permissions = entry.attrs.permissions;
                        let modified = entry.attrs.mtime.unwrap_or(0) as u64;

                        let full_path = if path == "." || path == "/" {
                            format!("/{}", file_name)
                        } else {
                            format!("{}/{}", path.trim_end_matches('/'), file_name)
                        };

                        let mut link_target = None;
                        let mut target_is_dir = None;
                        if is_symlink {
                            if let Ok(target_name) = sftp.readlink(&full_path).await {
                                if let Some(first) = target_name.files.first() {
                                    link_target = Some(first.filename.clone());
                                }
                            }
                            if let Ok(attrs) = sftp.stat(&full_path).await {
                                target_is_dir = Some(attrs.attrs.is_dir());
                            }
                        }

                        files.push(FileEntry {
                            name: file_name,
                            path: full_path,
                            is_dir,
                            is_symlink,
                            size,
                            modified,
                            link_target,
                            target_is_dir,
                            permissions,
                        });
                    }
                }
                Err(russh_sftp::client::error::Error::Status(status))
                    if status.status_code == StatusCode::Eof =>
                {
                    break
                }
                Err(e) => {
                    let _ = sftp.close(handle).await;
                    return Err(e.to_string());
                }
            }
        }

        let _ = sftp.close(handle).await;

        Self::sort_entries(&mut files, sort_type, sort_order);

        Ok(files)
    }

    pub async fn list_dirs_with_sort(
        session_id: &str,
        paths: &[String],
        sort_type: SftpSortType,
        sort_order: SftpSortOrder,
    ) -> Result<Vec<DirectoryListResult>, String> {
        let mut results = Vec::with_capacity(paths.len());
        for path in paths {
            match Self::list_dir_with_sort(session_id, path, sort_type, sort_order).await {
                Ok(files) => results.push(DirectoryListResult {
                    path: path.clone(),
                    files,
                    error: None,
                }),
                Err(error) => results.push(DirectoryListResult {
                    path: path.clone(),
                    files: Vec::new(),
                    error: Some(error),
                }),
            }
        }
        Ok(results)
    }

    pub async fn prepare_dir_listing_with_sort(
        session_id: &str,
        path: &str,
        sort_type: SftpSortType,
        sort_order: SftpSortOrder,
    ) -> Result<DirectoryListingHandle, String> {
        let files = Self::list_dir_with_sort(session_id, path, sort_type, sort_order).await?;
        let token = Uuid::new_v4().to_string();
        let total = files.len();

        let mut cache = DIRECTORY_LISTING_CACHE.lock().await;
        if cache.len() >= DIRECTORY_LISTING_CACHE_MAX_ENTRIES {
            if let Some(oldest_token) = cache
                .iter()
                .min_by_key(|(_, listing)| listing.created_at)
                .map(|(token, _)| token.clone())
            {
                cache.remove(&oldest_token);
            }
        }

        cache.insert(
            token.clone(),
            CachedDirectoryListing {
                session_id: session_id.to_string(),
                files: Arc::new(files),
                created_at: Instant::now(),
            },
        );

        Ok(DirectoryListingHandle { token, total })
    }

    pub async fn get_dir_listing_page(
        token: &str,
        offset: usize,
        limit: usize,
    ) -> Result<DirectoryListingPage, String> {
        let page_limit = if limit == 0 {
            DIRECTORY_LISTING_PAGE_LIMIT_DEFAULT
        } else {
            std::cmp::min(limit, DIRECTORY_LISTING_PAGE_LIMIT_MAX)
        };

        let listing = {
            let cache = DIRECTORY_LISTING_CACHE.lock().await;
            cache
                .get(token)
                .cloned()
                .ok_or_else(|| format!("Directory listing token not found: {}", token))?
        };

        let total = listing.files.len();
        let start = std::cmp::min(offset, total);
        let end = std::cmp::min(start.saturating_add(page_limit), total);
        let files = listing.files[start..end].to_vec();
        let next_offset = if end < total { Some(end) } else { None };

        if next_offset.is_none() {
            let mut cache = DIRECTORY_LISTING_CACHE.lock().await;
            cache.remove(token);
        }

        Ok(DirectoryListingPage {
            files,
            total,
            next_offset,
        })
    }

    pub async fn release_dir_listing(token: &str) {
        let mut cache = DIRECTORY_LISTING_CACHE.lock().await;
        cache.remove(token);
    }

    pub async fn cancel_transfer(task_id: &str) -> Result<(), String> {
        let tasks = TRANSFER_TASKS.lock().await;
        if let Some(token) = tasks.get(task_id) {
            token.store(true, Ordering::SeqCst);
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn download_file(
        app: AppHandle,
        db_manager: DatabaseManager,
        session_id: String,
        remote_path: String,
        local_path: String,
        task_id: Option<String>,
        _ai_session_id: Option<String>,
    ) -> Result<String, String> {
        let sftp = Self::get_session(&session_id).await?;
        let metadata = sftp
            .stat(&remote_path)
            .await
            .map_err(|e| e.to_string())?
            .attrs;

        if metadata.is_dir() {
            let download_files =
                Self::collect_directory_download_files(&sftp, &remote_path, &local_path).await?;

            if download_files.is_empty() {
                tokio::fs::create_dir_all(&local_path)
                    .await
                    .map_err(|e| e.to_string())?;
                return Ok(task_id.unwrap_or_else(|| Uuid::new_v4().to_string()));
            }

            let mut preferred_task_id = task_id;
            let mut first_task_id: Option<String> = None;

            for (file_remote_path, file_local_path) in download_files {
                let queued_task_id = Self::queue_download(
                    app.clone(),
                    db_manager.clone(),
                    session_id.clone(),
                    file_remote_path,
                    file_local_path,
                    preferred_task_id.take(),
                )
                .await?;

                if first_task_id.is_none() {
                    first_task_id = Some(queued_task_id);
                }
            }

            return first_task_id.ok_or_else(|| "No download tasks were queued".to_string());
        }

        Self::queue_download(
            app,
            db_manager,
            session_id,
            remote_path,
            local_path,
            task_id,
        )
        .await
    }

    async fn collect_directory_download_files(
        sftp: &Arc<RawSftpSession>,
        remote_root: &str,
        local_root: &str,
    ) -> Result<Vec<(String, String)>, String> {
        tokio::fs::create_dir_all(local_root)
            .await
            .map_err(|e| e.to_string())?;

        let mut pending_dirs = vec![(remote_root.to_string(), local_root.to_string())];
        let mut files = Vec::new();

        while let Some((remote_dir, local_dir)) = pending_dirs.pop() {
            let dir_handle = sftp
                .opendir(&remote_dir)
                .await
                .map_err(|e| e.to_string())?
                .handle;

            let mut dir_result: Result<(), String> = Ok(());

            loop {
                match sftp.readdir(&dir_handle).await {
                    Ok(name) => {
                        for entry in name.files {
                            let file_name = entry.filename;
                            if file_name == "." || file_name == ".." {
                                continue;
                            }

                            let remote_path =
                                format!("{}/{}", remote_dir.trim_end_matches('/'), file_name);
                            let local_path = Path::new(&local_dir)
                                .join(&file_name)
                                .to_string_lossy()
                                .to_string();

                            if entry.attrs.is_dir() {
                                tokio::fs::create_dir_all(&local_path)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                pending_dirs.push((remote_path, local_path));
                            } else {
                                files.push((remote_path, local_path));
                            }
                        }
                    }
                    Err(russh_sftp::client::error::Error::Status(status))
                        if status.status_code == StatusCode::Eof =>
                    {
                        break
                    }
                    Err(e) => {
                        dir_result = Err(e.to_string());
                        break;
                    }
                }
            }

            let _ = sftp.close(dir_handle).await;
            dir_result?;
        }

        Ok(files)
    }

    async fn _download_file(
        app: AppHandle,
        db_manager: DatabaseManager,
        session_id: String,
        remote_path: String,
        local_path: String,
        task_id: String,
        cancel_token: Arc<AtomicBool>,
        ai_session_id: Option<String>,
    ) -> Result<String, String> {
        let sftp = Self::get_session(&session_id).await?;

        let ai_session_id_clone = ai_session_id.clone();
        let task_id_inner = task_id.clone();
        let session_id_inner = session_id.clone();
        let remote_path_inner = remote_path.clone();
        let local_path_inner = local_path.clone();

        let metadata = match sftp.stat(&remote_path_inner).await {
            Ok(m) => m.attrs,
            Err(e) => {
                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        task_id: task_id_inner,
                        type_: "download".to_string(),
                        session_id: session_id_inner,
                        file_name: std::path::Path::new(&remote_path_inner)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        source: remote_path_inner.clone(),
                        destination: local_path_inner.clone(),
                        total_bytes: 0,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "failed".to_string(),
                        error: Some(e.to_string()),
                    },
                );
                return Ok(task_id);
            }
        };

        let file_name = std::path::Path::new(&remote_path_inner)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let result = async {
            if metadata.is_dir() {
                Self::download_dir_recursive(
                    &app,
                    &sftp,
                    &remote_path_inner,
                    &local_path_inner,
                    &task_id_inner,
                    &session_id_inner,
                    &cancel_token,
                )
                .await
            } else {
                let total_bytes = metadata.size.unwrap_or(0);
                let handle = sftp
                    .open(
                        &remote_path_inner,
                        OpenFlags::READ,
                        FileAttributes::default(),
                    )
                    .await
                    .map_err(|e| e.to_string())?
                    .handle;
                let mut local_file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&local_path_inner)
                    .await
                    .map_err(|e| e.to_string())?;

                let tuning = Self::resolve_transfer_tuning(&app, &session_id_inner).await;
                let mut transferred = 0u64;
                let chunk_size = tuning.download_chunk_size;
                let start_time = Instant::now();
                let mut last_emit = Instant::now();
                let mut speed_sampler = SpeedSampler::new(start_time);
                let mut diagnostics = TransferDiagnostics::new(start_time);

                Self::log_transfer_start(
                    &task_id_inner,
                    &session_id_inner,
                    "download",
                    &remote_path_inner,
                    &local_path_inner,
                    total_bytes,
                    tuning,
                )
                .await;

                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        task_id: task_id_inner.clone(),
                        type_: "download".to_string(),
                        session_id: session_id_inner.clone(),
                        file_name: file_name.clone(),
                        source: remote_path_inner.clone(),
                        destination: local_path_inner.clone(),
                        total_bytes,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "transferring".to_string(),
                        error: None,
                    },
                );

                let transfer_result: Result<(), String> = async {
                    let fallback_locked = Self::is_download_fallback_locked(&session_id_inner).await;
                    let max_inflight_reads = if fallback_locked {
                        1
                    } else {
                        tuning.download_max_inflight.max(1)
                    };
                    if fallback_locked {
                        tracing::info!(
                            target: "sftp::transfer",
                            task_id = &task_id_inner,
                            session_id = &session_id_inner,
                            "download fallback lock active, forcing single-flight mode"
                        );
                    }
                    let mut adaptive_inflight_limit = if max_inflight_reads > 1 {
                        DOWNLOAD_INITIAL_INFLIGHT.min(max_inflight_reads).max(1)
                    } else {
                        1
                    };
                    let mut consecutive_success_chunks = 0u32;
                    let mut next_request_offset = 0u64;
                    let mut next_write_offset = 0u64;
                    let mut inflight_reads = FuturesUnordered::new();
                    let mut pending_chunks: HashMap<u64, Vec<u8>> = HashMap::new();
                    let mut retry_counts: HashMap<u64, u8> = HashMap::new();

                    while inflight_reads.len() < adaptive_inflight_limit
                        && next_request_offset < total_bytes
                    {
                        let request_size = std::cmp::min(
                            chunk_size,
                            total_bytes.saturating_sub(next_request_offset),
                        );
                        let read_size = std::cmp::min(request_size, u32::MAX as u64) as u32;
                        let offset = next_request_offset;
                        let sftp_clone = sftp.clone();
                        let handle_clone = handle.clone();
                        inflight_reads.push(
                            async move {
                                let read_started_at = Instant::now();
                                let result = sftp_clone.read(&handle_clone, offset, read_size).await;
                                (offset, read_size as u64, read_started_at, result)
                            }
                            .boxed(),
                        );
                        next_request_offset = next_request_offset.saturating_add(read_size as u64);
                    }

                    while next_write_offset < total_bytes {
                        if cancel_token.load(Ordering::SeqCst) {
                            return Err("Cancelled".to_string());
                        }

                        let (offset, requested_size, read_started_at, read_result) =
                            match inflight_reads.next().await {
                                Some(v) => v,
                                None => {
                                    return Err(format!(
                                        "Download incomplete: no in-flight reads while waiting for offset {}",
                                        next_write_offset
                                    ));
                                }
                            };

                        let data = match read_result {
                            Ok(data) => data,
                            Err(russh_sftp::client::error::Error::Status(status))
                                if status.status_code == StatusCode::Eof =>
                            {
                                return Err(format!(
                                    "Download incomplete: EOF before full content ({} / {} bytes)",
                                    next_write_offset, total_bytes
                                ));
                            }
                            Err(e) => {
                                let error = e.to_string();
                                if error.to_ascii_lowercase().contains("timeout") {
                                    diagnostics.mark_timeout();

                                    let retry_count = retry_counts.entry(offset).or_insert(0);
                                    if *retry_count < DOWNLOAD_MAX_RETRIES_PER_CHUNK {
                                        *retry_count += 1;
                                        diagnostics.mark_retry();
                                        consecutive_success_chunks = 0;

                                        if diagnostics.consecutive_timeout_count
                                            >= DOWNLOAD_TIMEOUT_DOWNGRADE_THRESHOLD
                                            && adaptive_inflight_limit > 1
                                        {
                                            let previous = adaptive_inflight_limit;
                                            adaptive_inflight_limit =
                                                (adaptive_inflight_limit / 2).max(1);
                                            diagnostics.mark_downgrade();
                                            tracing::warn!(
                                                target: "sftp::transfer",
                                                task_id = &task_id_inner,
                                                session_id = &session_id_inner,
                                                previous_inflight = previous,
                                                downgraded_inflight = adaptive_inflight_limit,
                                                timeout_count = diagnostics.timeout_count,
                                                "download inflight downgraded due to timeout streak"
                                            );
                                        }

                                        if diagnostics.timeout_count
                                            >= DOWNLOAD_FALLBACK_LOCK_TIMEOUT_THRESHOLD
                                        {
                                            Self::set_download_fallback_lock(
                                                &session_id_inner,
                                                true,
                                            )
                                            .await;
                                            adaptive_inflight_limit = 1;
                                            tracing::warn!(
                                                target: "sftp::transfer",
                                                task_id = &task_id_inner,
                                                session_id = &session_id_inner,
                                                timeout_count = diagnostics.timeout_count,
                                                "download session fallback lock enabled"
                                            );
                                        }

                                        let retry_size =
                                            std::cmp::min(requested_size, u32::MAX as u64) as u32;
                                        let sftp_clone = sftp.clone();
                                        let handle_clone = handle.clone();
                                        inflight_reads.push(Box::pin(async move {
                                            let read_started_at = Instant::now();
                                            let result = sftp_clone
                                                .read(&handle_clone, offset, retry_size)
                                                .await;
                                            (offset, retry_size as u64, read_started_at, result)
                                        }));
                                        continue;
                                    }

                                    if diagnostics.timeout_count
                                        >= DOWNLOAD_FALLBACK_LOCK_TIMEOUT_THRESHOLD
                                    {
                                        Self::set_download_fallback_lock(&session_id_inner, true)
                                            .await;
                                        tracing::warn!(
                                            target: "sftp::transfer",
                                            task_id = &task_id_inner,
                                            session_id = &session_id_inner,
                                            timeout_count = diagnostics.timeout_count,
                                            "download session fallback lock enabled after timeout"
                                        );
                                    }
                                }
                                return Err(error);
                            }
                        };
                        diagnostics.record_rtt(read_started_at.elapsed());
                        diagnostics.mark_success();
                        retry_counts.remove(&offset);
                        if adaptive_inflight_limit < max_inflight_reads {
                            consecutive_success_chunks =
                                consecutive_success_chunks.saturating_add(1);
                            if consecutive_success_chunks >= DOWNLOAD_RAMP_UP_SUCCESS_CHUNKS {
                                let previous = adaptive_inflight_limit;
                                adaptive_inflight_limit =
                                    (adaptive_inflight_limit + 1).min(max_inflight_reads);
                                consecutive_success_chunks = 0;
                                tracing::debug!(
                                    target: "sftp::transfer",
                                    task_id = &task_id_inner,
                                    session_id = &session_id_inner,
                                    previous_inflight = previous,
                                    upgraded_inflight = adaptive_inflight_limit,
                                    "download inflight ramped up after stable chunks"
                                );
                            }
                        }

                        if data.data.is_empty() {
                            return Err(format!(
                                "Download incomplete: empty data before full content ({} / {} bytes)",
                                next_write_offset, total_bytes
                            ));
                        }

                        let actual_size = data.data.len() as u64;
                        if actual_size > requested_size {
                            return Err(format!(
                                "Download integrity error: received chunk larger than requested (offset {}, got {}, requested {})",
                                offset, actual_size, requested_size
                            ));
                        }

                        if actual_size < requested_size {
                            diagnostics.mark_retry();
                            let missing_offset = offset.saturating_add(actual_size);
                            let missing_size = requested_size.saturating_sub(actual_size);
                            let missing_read_size = std::cmp::min(missing_size, u32::MAX as u64) as u32;
                            let sftp_clone = sftp.clone();
                            let handle_clone = handle.clone();
                            inflight_reads.push(Box::pin(async move {
                                let read_started_at = Instant::now();
                                let result = sftp_clone
                                    .read(&handle_clone, missing_offset, missing_read_size)
                                    .await;
                                (missing_offset, missing_read_size as u64, read_started_at, result)
                            }));
                        }

                        if pending_chunks.insert(offset, data.data).is_some() {
                            return Err(format!(
                                "Download integrity error: duplicate chunk offset {}",
                                offset
                            ));
                        }

                        while let Some(chunk) = pending_chunks.remove(&next_write_offset) {
                            local_file
                                .write_all(&chunk)
                                .await
                                .map_err(|e| e.to_string())?;
                            next_write_offset = next_write_offset.saturating_add(chunk.len() as u64);
                            transferred = next_write_offset;
                        }

                        while inflight_reads.len() < adaptive_inflight_limit
                            && next_request_offset < total_bytes
                        {
                            let request_size = std::cmp::min(
                                chunk_size,
                                total_bytes.saturating_sub(next_request_offset),
                            );
                            let read_size = std::cmp::min(request_size, u32::MAX as u64) as u32;
                            let offset = next_request_offset;
                            let sftp_clone = sftp.clone();
                            let handle_clone = handle.clone();
                            inflight_reads.push(
                                async move {
                                    let read_started_at = Instant::now();
                                    let result = sftp_clone.read(&handle_clone, offset, read_size).await;
                                    (offset, read_size as u64, read_started_at, result)
                                }
                                .boxed(),
                            );
                            next_request_offset = next_request_offset.saturating_add(read_size as u64);
                        }

                        if last_emit.elapsed().as_millis() > 500 {
                            let now = Instant::now();
                            let display_speed = speed_sampler.sample(now, transferred);

                            let eta = if display_speed > 0.0 {
                                Some(
                                    ((total_bytes.saturating_sub(transferred)) as f64 / display_speed)
                                        as u64,
                                )
                            } else {
                                None
                            };

                            let _ = app.emit(
                                "transfer-progress",
                                TransferProgress {
                                    task_id: task_id_inner.clone(),
                                    type_: "download".to_string(),
                                    session_id: session_id_inner.clone(),
                                    file_name: file_name.clone(),
                                    source: remote_path_inner.clone(),
                                    destination: local_path_inner.clone(),
                                    total_bytes,
                                    transferred_bytes: transferred,
                                    speed: display_speed,
                                    eta,
                                    status: "transferring".to_string(),
                                    error: None,
                                },
                            );
                            if diagnostics.should_log_progress(now) {
                                Self::log_transfer_progress(
                                    &task_id_inner,
                                    &session_id_inner,
                                    "download",
                                    total_bytes,
                                    transferred,
                                    display_speed,
                                    inflight_reads
                                        .len()
                                        .saturating_add(pending_chunks.len())
                                        .max(1),
                                    &diagnostics,
                                );
                                diagnostics.touch_log_time(now);
                            }
                            last_emit = Instant::now();
                        }
                    }

                    if !pending_chunks.is_empty() {
                        return Err(
                            "Download integrity error: unexpected buffered chunks remain"
                                .to_string(),
                        );
                    }

                    if transferred < total_bytes {
                        return Err(format!(
                            "Download incomplete: transferred {} / {} bytes",
                            transferred, total_bytes
                        ));
                    }

                    if let Ok(latest_metadata) = sftp.fstat(&handle).await {
                        if let Some(latest_size) = latest_metadata.attrs.size {
                            if latest_size != total_bytes {
                                return Err(format!(
                                    "Remote file size changed during download ({} -> {}), please retry after file generation completes",
                                    total_bytes, latest_size
                                ));
                            }
                        }
                    }

                    local_file.flush().await.map_err(|e| e.to_string())?;
                    local_file.sync_all().await.map_err(|e| e.to_string())?;
                    Ok(())
                }
                .await;

                let _ = sftp.close(handle).await;
                let finish_status = if transfer_result.is_ok() {
                    "completed"
                } else if transfer_result
                    .as_ref()
                    .err()
                    .is_some_and(|e| e == "Cancelled")
                {
                    "cancelled"
                } else {
                    "failed"
                };
                let finish_error = transfer_result.as_ref().err().map(|e| e.as_str());
                Self::log_transfer_finish(
                    &task_id_inner,
                    &session_id_inner,
                    "download",
                    finish_status,
                    total_bytes,
                    if transfer_result.is_ok() {
                        total_bytes
                    } else {
                        transferred
                    },
                    &diagnostics,
                    finish_error,
                );
                transfer_result
            }
        }
        .await;

        let is_dir = metadata.is_dir();
        let final_total_bytes = if is_dir {
            0
        } else {
            metadata.size.unwrap_or(0)
        };

        let final_status = match &result {
            Ok(_) => "completed",
            Err(e) if e == "Cancelled" => "cancelled",
            Err(_) => "failed",
        };

        let final_error = result.as_ref().err().cloned();

        let _ = app.emit(
            "transfer-progress",
            TransferProgress {
                task_id: task_id_inner,
                type_: "download".to_string(),
                session_id: session_id_inner,
                file_name: file_name.clone(),
                source: remote_path_inner.clone(),
                destination: local_path_inner.clone(),
                total_bytes: final_total_bytes,
                transferred_bytes: if final_status == "completed" {
                    final_total_bytes
                } else {
                    0
                },
                speed: 0.0,
                eta: None,
                status: final_status.to_string(),
                error: final_error.clone(),
            },
        );

        if let Some(ai_sid) = ai_session_id_clone {
            let msg_id = Uuid::new_v4().to_string();
            let content = match &result {
                Ok(_) => format!(
                    "SFTP Download completed successfully: {} -> {}",
                    remote_path_inner, local_path_inner
                ),
                Err(e) => format!("SFTP Download failed: {}. Path: {}", e, remote_path_inner),
            };

            let conn = db_manager.get_connection();
            let conn = conn.lock().unwrap();
            let _ = conn.execute(
                "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![msg_id, ai_sid, "system", content],
            );

            let _ = app.emit(
                &format!("ai-message-batch-{}", ai_sid),
                vec![serde_json::json!({
                    "role": "system",
                    "content": content
                })],
            );
            let _ = app.emit(&format!("ai-done-{}", ai_sid), "DONE");
        }

        let mut tasks = TRANSFER_TASKS.lock().await;
        tasks.remove(&task_id);

        Ok(task_id)
    }

    async fn download_dir_recursive(
        app: &AppHandle,
        sftp: &Arc<RawSftpSession>,
        remote_dir: &str,
        local_dir: &str,
        task_id: &str,
        session_id: &str,
        cancel_token: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        tokio::fs::create_dir_all(local_dir)
            .await
            .map_err(|e| e.to_string())?;

        let dir_handle = sftp
            .opendir(remote_dir)
            .await
            .map_err(|e| e.to_string())?
            .handle;
        loop {
            if cancel_token.load(Ordering::SeqCst) {
                let _ = sftp.close(dir_handle).await;
                return Err("Cancelled".to_string());
            }

            match sftp.readdir(&dir_handle).await {
                Ok(name) => {
                    for entry in name.files {
                        let file_name = entry.filename;
                        if file_name == "." || file_name == ".." {
                            continue;
                        }

                        let remote_path =
                            format!("{}/{}", remote_dir.trim_end_matches('/'), file_name);
                        let local_path = std::path::Path::new(local_dir)
                            .join(&file_name)
                            .to_string_lossy()
                            .to_string();
                        let is_dir = entry.attrs.is_dir();

                        if is_dir {
                            Box::pin(Self::download_dir_recursive(
                                app,
                                sftp,
                                &remote_path,
                                &local_path,
                                task_id,
                                session_id,
                                cancel_token,
                            ))
                            .await?;
                        } else {
                            let metadata = sftp
                                .stat(&remote_path)
                                .await
                                .map_err(|e| e.to_string())?
                                .attrs;
                            let total_bytes = metadata.size.unwrap_or(0);
                            let handle = sftp
                                .open(&remote_path, OpenFlags::READ, FileAttributes::default())
                                .await
                                .map_err(|e| e.to_string())?
                                .handle;
                            let mut local_file = tokio::fs::File::create(&local_path)
                                .await
                                .map_err(|e| e.to_string())?;
                            let mut transferred = 0u64;
                            let start_time = Instant::now();
                            let mut last_emit = Instant::now();

                            let _ = app.emit(
                                "transfer-progress",
                                TransferProgress {
                                    task_id: task_id.to_string(),
                                    type_: "download".to_string(),
                                    session_id: session_id.to_string(),
                                    file_name: file_name.clone(),
                                    source: remote_path.clone(),
                                    destination: local_path.clone(),
                                    total_bytes,
                                    transferred_bytes: 0,
                                    speed: 0.0,
                                    eta: None,
                                    status: "transferring".to_string(),
                                    error: None,
                                },
                            );

                            loop {
                                if cancel_token.load(Ordering::SeqCst) {
                                    let duration = start_time.elapsed().as_secs_f64();
                                    let speed = if duration > 0.0 {
                                        transferred as f64 / duration
                                    } else {
                                        0.0
                                    };

                                    let _ = sftp.close(handle).await;
                                    let _ = sftp.close(dir_handle).await;
                                    let _ = app.emit(
                                        "transfer-progress",
                                        TransferProgress {
                                            task_id: task_id.to_string(),
                                            type_: "download".to_string(),
                                            session_id: session_id.to_string(),
                                            file_name: file_name.clone(),
                                            source: remote_path.clone(),
                                            destination: local_path.clone(),
                                            total_bytes,
                                            transferred_bytes: transferred,
                                            speed,
                                            eta: None,
                                            status: "cancelled".to_string(),
                                            error: None,
                                        },
                                    );
                                    return Err("Cancelled".to_string());
                                }
                                match sftp.read(&handle, transferred, (256 * 1024) as u32).await {
                                    Ok(data) => {
                                        if data.data.is_empty() {
                                            if total_bytes == 0 || transferred >= total_bytes {
                                                break;
                                            }

                                            let duration = start_time.elapsed().as_secs_f64();
                                            let speed = if duration > 0.0 {
                                                transferred as f64 / duration
                                            } else {
                                                0.0
                                            };
                                            let error = format!(
                                                "Download incomplete: empty data before EOF ({} / {} bytes)",
                                                transferred, total_bytes
                                            );

                                            let _ = sftp.close(handle).await;
                                            let _ = app.emit(
                                                "transfer-progress",
                                                TransferProgress {
                                                    task_id: task_id.to_string(),
                                                    type_: "download".to_string(),
                                                    session_id: session_id.to_string(),
                                                    file_name: file_name.clone(),
                                                    source: remote_path.clone(),
                                                    destination: local_path.clone(),
                                                    total_bytes,
                                                    transferred_bytes: transferred,
                                                    speed,
                                                    eta: None,
                                                    status: "failed".to_string(),
                                                    error: Some(error.clone()),
                                                },
                                            );

                                            return Err(error);
                                        }
                                        local_file
                                            .write_all(&data.data)
                                            .await
                                            .map_err(|e| e.to_string())?;
                                        transferred += data.data.len() as u64;

                                        if last_emit.elapsed().as_millis() > 500 {
                                            let duration = start_time.elapsed().as_secs_f64();
                                            let speed = if duration > 0.0 {
                                                transferred as f64 / duration
                                            } else {
                                                0.0
                                            };
                                            let eta = if speed > 0.0 {
                                                Some(
                                                    ((total_bytes.saturating_sub(transferred))
                                                        as f64
                                                        / speed)
                                                        as u64,
                                                )
                                            } else {
                                                None
                                            };

                                            let _ = app.emit(
                                                "transfer-progress",
                                                TransferProgress {
                                                    task_id: task_id.to_string(),
                                                    type_: "download".to_string(),
                                                    session_id: session_id.to_string(),
                                                    file_name: file_name.clone(),
                                                    source: remote_path.clone(),
                                                    destination: local_path.clone(),
                                                    total_bytes,
                                                    transferred_bytes: transferred,
                                                    speed,
                                                    eta,
                                                    status: "transferring".to_string(),
                                                    error: None,
                                                },
                                            );
                                            last_emit = Instant::now();
                                        }
                                    }
                                    Err(russh_sftp::client::error::Error::Status(status))
                                        if status.status_code == StatusCode::Eof =>
                                    {
                                        if total_bytes > 0 && transferred < total_bytes {
                                            let duration = start_time.elapsed().as_secs_f64();
                                            let speed = if duration > 0.0 {
                                                transferred as f64 / duration
                                            } else {
                                                0.0
                                            };
                                            let error = format!(
                                                "Download incomplete: EOF before full content ({} / {} bytes)",
                                                transferred, total_bytes
                                            );

                                            let _ = sftp.close(handle).await;
                                            let _ = app.emit(
                                                "transfer-progress",
                                                TransferProgress {
                                                    task_id: task_id.to_string(),
                                                    type_: "download".to_string(),
                                                    session_id: session_id.to_string(),
                                                    file_name: file_name.clone(),
                                                    source: remote_path.clone(),
                                                    destination: local_path.clone(),
                                                    total_bytes,
                                                    transferred_bytes: transferred,
                                                    speed,
                                                    eta: None,
                                                    status: "failed".to_string(),
                                                    error: Some(error.clone()),
                                                },
                                            );

                                            return Err(error);
                                        }
                                        break;
                                    }
                                    Err(e) => {
                                        let duration = start_time.elapsed().as_secs_f64();
                                        let speed = if duration > 0.0 {
                                            transferred as f64 / duration
                                        } else {
                                            0.0
                                        };

                                        let _ = app.emit(
                                            "transfer-progress",
                                            TransferProgress {
                                                task_id: task_id.to_string(),
                                                type_: "download".to_string(),
                                                session_id: session_id.to_string(),
                                                file_name: file_name.clone(),
                                                source: remote_path.clone(),
                                                destination: local_path.clone(),
                                                total_bytes,
                                                transferred_bytes: transferred,
                                                speed,
                                                eta: None,
                                                status: "failed".to_string(),
                                                error: Some(e.to_string()),
                                            },
                                        );
                                        return Err(e.to_string());
                                    }
                                }
                            }

                            if total_bytes > 0 && transferred < total_bytes {
                                let duration = start_time.elapsed().as_secs_f64();
                                let speed = if duration > 0.0 {
                                    transferred as f64 / duration
                                } else {
                                    0.0
                                };
                                let error = format!(
                                    "Download incomplete: transferred {} / {} bytes",
                                    transferred, total_bytes
                                );

                                let _ = sftp.close(handle).await;
                                let _ = app.emit(
                                    "transfer-progress",
                                    TransferProgress {
                                        task_id: task_id.to_string(),
                                        type_: "download".to_string(),
                                        session_id: session_id.to_string(),
                                        file_name: file_name.clone(),
                                        source: remote_path.clone(),
                                        destination: local_path.clone(),
                                        total_bytes,
                                        transferred_bytes: transferred,
                                        speed,
                                        eta: None,
                                        status: "failed".to_string(),
                                        error: Some(error.clone()),
                                    },
                                );

                                return Err(error);
                            }

                            let duration = start_time.elapsed().as_secs_f64();
                            let speed = if duration > 0.0 {
                                total_bytes as f64 / duration
                            } else {
                                0.0
                            };

                            let _ = app.emit(
                                "transfer-progress",
                                TransferProgress {
                                    task_id: task_id.to_string(),
                                    type_: "download".to_string(),
                                    session_id: session_id.to_string(),
                                    file_name: file_name.clone(),
                                    source: remote_path.clone(),
                                    destination: local_path.clone(),
                                    total_bytes,
                                    transferred_bytes: total_bytes,
                                    speed,
                                    eta: None,
                                    status: "completed".to_string(),
                                    error: None,
                                },
                            );
                            local_file.flush().await.map_err(|e| e.to_string())?;
                            local_file.sync_all().await.map_err(|e| e.to_string())?;
                            let _ = sftp.close(handle).await;
                        }
                    }
                }
                Err(russh_sftp::client::error::Error::Status(status))
                    if status.status_code == StatusCode::Eof =>
                {
                    break
                }
                Err(e) => {
                    let _ = sftp.close(dir_handle).await;
                    return Err(e.to_string());
                }
            }
        }
        let _ = sftp.close(dir_handle).await;
        Ok(())
    }

    pub async fn upload_file(
        app: AppHandle,
        db_manager: DatabaseManager,
        session_id: String,
        local_path: String,
        remote_path: String,
        task_id: Option<String>,
        _ai_session_id: Option<String>,
    ) -> Result<String, String> {
        Self::queue_upload(
            app,
            db_manager,
            session_id,
            local_path,
            remote_path,
            task_id,
        )
        .await
    }

    async fn _upload_file(
        app: AppHandle,
        db_manager: DatabaseManager,
        session_id: String,
        local_path: String,
        remote_path: String,
        task_id: String,
        cancel_token: Arc<AtomicBool>,
        ai_session_id: Option<String>,
    ) -> Result<String, String> {
        let sftp = Self::get_session(&session_id).await?;

        let task_id_inner = task_id.clone();
        let session_id_inner = session_id.clone();
        let remote_path_inner = remote_path.clone();
        let local_path_inner = local_path.clone();
        let ai_session_id_clone = ai_session_id.clone();
        let task_id_clone = task_id.clone();

        let local_metadata = match tokio::fs::metadata(&local_path_inner).await {
            Ok(m) => m,
            Err(e) => {
                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        task_id: task_id_inner,
                        type_: "upload".to_string(),
                        session_id: session_id_inner,
                        file_name: std::path::Path::new(&local_path_inner)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        source: local_path_inner.clone(),
                        destination: remote_path_inner.clone(),
                        total_bytes: 0,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "failed".to_string(),
                        error: Some(e.to_string()),
                    },
                );
                return Ok(task_id);
            }
        };
        let file_name = std::path::Path::new(&local_path_inner)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let result = async {
            if local_metadata.is_dir() {
                Self::upload_dir_recursive(
                    &app,
                    &sftp,
                    &local_path_inner,
                    &remote_path_inner,
                    &task_id_inner,
                    &session_id_inner,
                    &cancel_token,
                )
                .await
            } else {
                if let Some(remote_attrs) =
                    Self::check_file_exists(&sftp, &remote_path_inner).await?
                {
                    let std_metadata =
                        std::fs::metadata(&local_path_inner).map_err(|e| e.to_string())?;
                    let resolution = Self::wait_for_conflict_resolution(
                        &app,
                        &task_id_inner,
                        &session_id_inner,
                        &remote_path_inner,
                        &std_metadata,
                        &remote_attrs,
                    )
                    .await?;

                    match resolution {
                        ConflictResolution::Skip => {
                            return Err("Skipped".to_string());
                        }
                        ConflictResolution::Cancel => {
                            return Err("Cancelled".to_string());
                        }
                        ConflictResolution::Overwrite => {}
                    }
                }

                let total_bytes = local_metadata.len();
                let mut local_file = tokio::fs::File::open(&local_path_inner)
                    .await
                    .map_err(|e| e.to_string())?;
                let handle = sftp
                    .open(
                        &remote_path_inner,
                        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
                        FileAttributes::default(),
                    )
                    .await
                    .map_err(|e| e.to_string())?
                    .handle;

                let tuning = Self::resolve_transfer_tuning(&app, &session_id_inner).await;
                let mut transferred = 0u64;
                let chunk_size = tuning.upload_chunk_size;
                let mut adaptive_max_concurrent_requests = tuning.upload_max_inflight;
                let start_time = Instant::now();
                let mut last_emit = Instant::now();
                let mut speed_sampler = SpeedSampler::new(start_time);
                let mut diagnostics = TransferDiagnostics::new(start_time);

                Self::log_transfer_start(
                    &task_id_inner,
                    &session_id_inner,
                    "upload",
                    &local_path_inner,
                    &remote_path_inner,
                    total_bytes,
                    tuning,
                )
                .await;

                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        task_id: task_id_inner.clone(),
                        type_: "upload".to_string(),
                        session_id: session_id_inner.clone(),
                        file_name: file_name.clone(),
                        source: local_path_inner.clone(),
                        destination: remote_path_inner.clone(),
                        total_bytes,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "transferring".to_string(),
                        error: None,
                    },
                );

                let mut futures = FuturesUnordered::new();
                let mut next_offset = 0u64;
                let mut retry_counts: HashMap<u64, u8> = HashMap::new();

                while futures.len() < adaptive_max_concurrent_requests && next_offset < total_bytes
                {
                    let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
                    let offset = next_offset;

                    let mut buffer = vec![0u8; current_chunk_size as usize];
                    local_file
                        .seek(SeekFrom::Start(offset))
                        .await
                        .map_err(|e| e.to_string())?;
                    local_file
                        .read_exact(&mut buffer)
                        .await
                        .map_err(|e| e.to_string())?;

                    let sftp_clone = sftp.clone();
                    let handle_clone = handle.clone();

                    futures.push(
                        async move {
                            let submitted_at = Instant::now();
                            let res = tokio::time::timeout(
                                std::time::Duration::from_secs(UPLOAD_CHUNK_WRITE_TIMEOUT_SECS),
                                sftp_clone.write(handle_clone, offset, buffer),
                            )
                            .await;
                            let result = match res {
                                Ok(r) => r,
                                Err(_) => Err(russh_sftp::client::error::Error::IO(format!(
                                    "Chunk upload timeout ({}s)",
                                    UPLOAD_CHUNK_WRITE_TIMEOUT_SECS
                                ))),
                            };
                            (offset, current_chunk_size, submitted_at, result)
                        }
                        .boxed(),
                    );
                    next_offset += current_chunk_size;
                }

                while let Some((offset, written_chunk_size, submitted_at, result)) =
                    futures.next().await
                {
                    if cancel_token.load(Ordering::SeqCst) {
                        Self::log_transfer_finish(
                            &task_id_inner,
                            &session_id_inner,
                            "upload",
                            "cancelled",
                            total_bytes,
                            transferred,
                            &diagnostics,
                            Some("Cancelled"),
                        );
                        return Err("Cancelled".to_string());
                    }

                    match result {
                        Ok(_) => {
                            diagnostics.record_rtt(submitted_at.elapsed());
                            diagnostics.mark_success();
                            retry_counts.remove(&offset);
                        }
                        Err(error) => {
                            let error = error.to_string();
                            if error.to_ascii_lowercase().contains("timeout") {
                                diagnostics.mark_timeout();

                                let retry_count = retry_counts.entry(offset).or_insert(0);
                                if *retry_count < UPLOAD_MAX_RETRIES_PER_CHUNK {
                                    *retry_count += 1;
                                    diagnostics.mark_retry();

                                    if diagnostics.consecutive_timeout_count
                                        >= UPLOAD_TIMEOUT_DOWNGRADE_THRESHOLD
                                        && adaptive_max_concurrent_requests > 1
                                    {
                                        let previous = adaptive_max_concurrent_requests;
                                        adaptive_max_concurrent_requests =
                                            (adaptive_max_concurrent_requests / 2).max(1);
                                        diagnostics.mark_downgrade();
                                        tracing::warn!(
                                            target: "sftp::transfer",
                                            task_id = &task_id_inner,
                                            session_id = &session_id_inner,
                                            previous_inflight = previous,
                                            downgraded_inflight = adaptive_max_concurrent_requests,
                                            timeout_count = diagnostics.timeout_count,
                                            "upload inflight downgraded due to timeout streak"
                                        );
                                    }

                                    let mut buffer = vec![0u8; written_chunk_size as usize];
                                    local_file
                                        .seek(SeekFrom::Start(offset))
                                        .await
                                        .map_err(|e| e.to_string())?;
                                    local_file
                                        .read_exact(&mut buffer)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                    let sftp_clone = sftp.clone();
                                    let handle_clone = handle.clone();
                                    futures.push(Box::pin(async move {
                                        let submitted_at = Instant::now();
                                        let res = tokio::time::timeout(
                                            std::time::Duration::from_secs(
                                                UPLOAD_CHUNK_WRITE_TIMEOUT_SECS,
                                            ),
                                            sftp_clone.write(handle_clone, offset, buffer),
                                        )
                                        .await;
                                        let result = match res {
                                            Ok(r) => r,
                                            Err(_) => {
                                                Err(russh_sftp::client::error::Error::IO(format!(
                                                    "Chunk upload timeout ({}s)",
                                                    UPLOAD_CHUNK_WRITE_TIMEOUT_SECS
                                                )))
                                            }
                                        };
                                        (offset, written_chunk_size, submitted_at, result)
                                    }));
                                    continue;
                                }
                            }
                            Self::log_transfer_finish(
                                &task_id_inner,
                                &session_id_inner,
                                "upload",
                                "failed",
                                total_bytes,
                                transferred,
                                &diagnostics,
                                Some(error.as_str()),
                            );
                            return Err(error);
                        }
                    }

                    transferred += written_chunk_size;
                    if transferred > total_bytes {
                        transferred = total_bytes;
                    }

                    if last_emit.elapsed().as_millis() > 500 {
                        let now = Instant::now();
                        let speed = speed_sampler.sample(now, transferred);
                        let eta = if speed > 0.0 {
                            Some(((total_bytes.saturating_sub(transferred)) as f64 / speed) as u64)
                        } else {
                            None
                        };

                        let _ = app.emit(
                            "transfer-progress",
                            TransferProgress {
                                task_id: task_id_inner.clone(),
                                type_: "upload".to_string(),
                                session_id: session_id_inner.clone(),
                                file_name: file_name.clone(),
                                source: local_path_inner.clone(),
                                destination: remote_path_inner.clone(),
                                total_bytes,
                                transferred_bytes: transferred,
                                speed,
                                eta,
                                status: "transferring".to_string(),
                                error: None,
                            },
                        );
                        if diagnostics.should_log_progress(now) {
                            Self::log_transfer_progress(
                                &task_id_inner,
                                &session_id_inner,
                                "upload",
                                total_bytes,
                                transferred,
                                speed,
                                adaptive_max_concurrent_requests,
                                &diagnostics,
                            );
                            diagnostics.touch_log_time(now);
                        }
                        last_emit = Instant::now();
                    }

                    while futures.len() < adaptive_max_concurrent_requests
                        && next_offset < total_bytes
                    {
                        let current_chunk_size =
                            std::cmp::min(chunk_size, total_bytes - next_offset);
                        let offset = next_offset;

                        let mut buffer = vec![0u8; current_chunk_size as usize];
                        local_file
                            .seek(SeekFrom::Start(offset))
                            .await
                            .map_err(|e| e.to_string())?;
                        local_file
                            .read_exact(&mut buffer)
                            .await
                            .map_err(|e| e.to_string())?;

                        let sftp_clone = sftp.clone();
                        let handle_clone = handle.clone();

                        futures.push(Box::pin(async move {
                            let submitted_at = Instant::now();
                            let res = tokio::time::timeout(
                                std::time::Duration::from_secs(UPLOAD_CHUNK_WRITE_TIMEOUT_SECS),
                                sftp_clone.write(handle_clone, offset, buffer),
                            )
                            .await;
                            let result = match res {
                                Ok(r) => r,
                                Err(_) => Err(russh_sftp::client::error::Error::IO(format!(
                                    "Chunk upload timeout ({}s)",
                                    UPLOAD_CHUNK_WRITE_TIMEOUT_SECS
                                ))),
                            };
                            (offset, current_chunk_size, submitted_at, result)
                        }));
                        next_offset += current_chunk_size;
                    }
                }
                let _ = sftp.close(handle).await;
                let finish_status = "completed";
                Self::log_transfer_finish(
                    &task_id_inner,
                    &session_id_inner,
                    "upload",
                    finish_status,
                    total_bytes,
                    transferred,
                    &diagnostics,
                    None,
                );
                Ok(())
            }
        }
        .await;

        let is_dir = local_metadata.is_dir();
        let final_total_bytes = if is_dir { 0 } else { local_metadata.len() };

        let final_status = match &result {
            Ok(_) => "completed",
            Err(e) if e == "Cancelled" => "cancelled",
            Err(e) if e == "Skipped" => "cancelled",
            Err(_) => "failed",
        };

        let final_error = match &result {
            Err(e) if e == "Skipped" => Some("Skipped by user".to_string()),
            Err(e) if e == "Cancelled" => Some("Cancelled by user".to_string()),
            Ok(_) => None,
            Err(e) => Some(e.clone()),
        };

        let _ = app.emit(
            "transfer-progress",
            TransferProgress {
                task_id: task_id_inner,
                type_: "upload".to_string(),
                session_id: session_id_inner,
                file_name: file_name.clone(),
                source: local_path_inner.clone(),
                destination: remote_path_inner.clone(),
                total_bytes: final_total_bytes,
                transferred_bytes: if final_status == "completed" {
                    final_total_bytes
                } else {
                    0
                },
                speed: 0.0,
                eta: None,
                status: final_status.to_string(),
                error: final_error,
            },
        );

        if let Some(ai_sid) = ai_session_id_clone {
            let msg_id = Uuid::new_v4().to_string();
            let content = match &result {
                Ok(_) => format!(
                    "SFTP Upload completed successfully: {} -> {}",
                    local_path_inner, remote_path_inner
                ),
                Err(e) => format!("SFTP Upload failed: {}. Path: {}", e, local_path_inner),
            };

            let conn = db_manager.get_connection();
            let conn = conn.lock().unwrap();
            let _ = conn.execute(
                "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![msg_id, ai_sid, "system", content],
            );

            let _ = app.emit(
                &format!("ai-message-batch-{}", ai_sid),
                vec![serde_json::json!({
                    "role": "system",
                    "content": content
                })],
            );
            let _ = app.emit(&format!("ai-done-{}", ai_sid), "DONE");
        }

        let mut tasks = TRANSFER_TASKS.lock().await;
        tasks.remove(&task_id_clone);

        Ok(task_id)
    }

    async fn upload_dir_recursive(
        app: &AppHandle,
        sftp: &Arc<RawSftpSession>,
        local_dir: &str,
        remote_dir: &str,
        task_id: &str,
        session_id: &str,
        cancel_token: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        sftp.mkdir(remote_dir, FileAttributes::default()).await.ok();

        let mut entries = tokio::fs::read_dir(local_dir)
            .await
            .map_err(|e| e.to_string())?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            if cancel_token.load(Ordering::SeqCst) {
                return Err("Cancelled".to_string());
            }

            let file_name = entry.file_name();
            let local_path = entry.path().to_string_lossy().to_string();
            let remote_path = format!(
                "{}/{}",
                remote_dir.trim_end_matches('/'),
                file_name.to_string_lossy()
            );
            let metadata = entry.metadata().await.map_err(|e| e.to_string())?;

            if metadata.is_dir() {
                Box::pin(Self::upload_dir_recursive(
                    app,
                    sftp,
                    &local_path,
                    &remote_path,
                    task_id,
                    session_id,
                    cancel_token,
                ))
                .await?;
            } else {
                // Check if remote file exists
                if let Some(remote_attrs) = Self::check_file_exists(sftp, &remote_path).await? {
                    let std_metadata = std::fs::metadata(&local_path).map_err(|e| e.to_string())?;
                    let conflict_task_id = format!("{}-{}", task_id, file_name.to_string_lossy());
                    let resolution = Self::wait_for_conflict_resolution(
                        app,
                        &conflict_task_id,
                        session_id,
                        &remote_path,
                        &std_metadata,
                        &remote_attrs,
                    )
                    .await?;

                    match resolution {
                        ConflictResolution::Skip => {
                            // Send cancelled status for skipped file in folder upload
                            let _ = app.emit(
                                "transfer-progress",
                                TransferProgress {
                                    task_id: format!("{}-{}", task_id, file_name.to_string_lossy()),
                                    type_: "upload".to_string(),
                                    session_id: session_id.to_string(),
                                    file_name: file_name.to_string_lossy().to_string(),
                                    source: local_path.clone(),
                                    destination: remote_path.clone(),
                                    total_bytes: std_metadata.len(),
                                    transferred_bytes: 0,
                                    speed: 0.0,
                                    eta: None,
                                    status: "cancelled".to_string(),
                                    error: Some("Skipped by user".to_string()),
                                },
                            );
                            continue;
                        }
                        ConflictResolution::Cancel => {
                            return Err("Cancelled".to_string());
                        }
                        ConflictResolution::Overwrite => {
                            // Continue with upload
                        }
                    }
                }

                let local_metadata = tokio::fs::metadata(&local_path)
                    .await
                    .map_err(|e| e.to_string())?;
                let total_bytes = local_metadata.len();
                let mut local_file = tokio::fs::File::open(&local_path)
                    .await
                    .map_err(|e| e.to_string())?;
                let handle = sftp
                    .open(
                        &remote_path,
                        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
                        FileAttributes::default(),
                    )
                    .await
                    .map_err(|e| e.to_string())?
                    .handle;
                let mut buffer = [0u8; 256 * 1024];
                let mut offset = 0u64;
                let mut transferred = 0u64;
                let mut last_emit = Instant::now();

                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        task_id: task_id.to_string(),
                        type_: "upload".to_string(),
                        session_id: session_id.to_string(),
                        file_name: file_name.to_string_lossy().to_string(),
                        source: local_path.clone(),
                        destination: remote_path.clone(),
                        total_bytes,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "transferring".to_string(),
                        error: None,
                    },
                );

                loop {
                    if cancel_token.load(Ordering::SeqCst) {
                        let _ = sftp.close(handle).await;
                        let _ = app.emit(
                            "transfer-progress",
                            TransferProgress {
                                task_id: task_id.to_string(),
                                type_: "upload".to_string(),
                                session_id: session_id.to_string(),
                                file_name: file_name.to_string_lossy().to_string(),
                                source: local_path.clone(),
                                destination: remote_path.clone(),
                                total_bytes,
                                transferred_bytes: transferred,
                                speed: 0.0,
                                eta: None,
                                status: "cancelled".to_string(),
                                error: Some("Cancelled by user".to_string()),
                            },
                        );
                        return Err("Cancelled".to_string());
                    }
                    let n = local_file
                        .read(&mut buffer)
                        .await
                        .map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    match sftp.write(&handle, offset, buffer[..n].to_vec()).await {
                        Ok(_) => {
                            offset += n as u64;
                            transferred += n as u64;

                            if last_emit.elapsed().as_millis() > 500 {
                                let _ = app.emit(
                                    "transfer-progress",
                                    TransferProgress {
                                        task_id: task_id.to_string(),
                                        type_: "upload".to_string(),
                                        session_id: session_id.to_string(),
                                        file_name: file_name.to_string_lossy().to_string(),
                                        source: local_path.clone(),
                                        destination: remote_path.clone(),
                                        total_bytes,
                                        transferred_bytes: transferred,
                                        speed: 0.0,
                                        eta: None,
                                        status: "transferring".to_string(),
                                        error: None,
                                    },
                                );
                                last_emit = Instant::now();
                            }
                        }
                        Err(e) => {
                            let _ = app.emit(
                                "transfer-progress",
                                TransferProgress {
                                    task_id: task_id.to_string(),
                                    type_: "upload".to_string(),
                                    session_id: session_id.to_string(),
                                    file_name: file_name.to_string_lossy().to_string(),
                                    source: local_path.clone(),
                                    destination: remote_path.clone(),
                                    total_bytes,
                                    transferred_bytes: transferred,
                                    speed: 0.0,
                                    eta: None,
                                    status: "failed".to_string(),
                                    error: Some(e.to_string()),
                                },
                            );
                            return Err(e.to_string());
                        }
                    }
                }
                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        task_id: task_id.to_string(),
                        type_: "upload".to_string(),
                        session_id: session_id.to_string(),
                        file_name: file_name.to_string_lossy().to_string(),
                        source: local_path.clone(),
                        destination: remote_path.clone(),
                        total_bytes,
                        transferred_bytes: total_bytes,
                        speed: 0.0,
                        eta: None,
                        status: "completed".to_string(),
                        error: None,
                    },
                );
                let _ = sftp.close(handle).await;
            }
        }
        Ok(())
    }

    pub async fn create_directory(session_id: &str, path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        sftp.mkdir(path, FileAttributes::default())
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn create_file(session_id: &str, path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        let handle = sftp
            .open(
                path,
                OpenFlags::CREATE | OpenFlags::WRITE,
                FileAttributes::default(),
            )
            .await
            .map_err(|e| e.to_string())?
            .handle;
        let _ = sftp.close(handle).await;
        Ok(())
    }

    pub async fn delete_item(session_id: &str, path: &str, is_dir: bool) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;

        if !is_dir {
            sftp.remove(path).await.map_err(|e| e.to_string())?;
        } else {
            Self::remove_dir_recursive(&sftp, path).await?;
        }

        Ok(())
    }

    async fn remove_dir_recursive(sftp: &Arc<RawSftpSession>, path: &str) -> Result<(), String> {
        let handle = sftp.opendir(path).await.map_err(|e| e.to_string())?.handle;

        loop {
            match sftp.readdir(&handle).await {
                Ok(name) => {
                    for entry in name.files {
                        let file_name = entry.filename;
                        if file_name == "." || file_name == ".." {
                            continue;
                        }

                        let entry_path = format!("{}/{}", path.trim_end_matches('/'), file_name);
                        let is_dir = entry.attrs.is_dir();

                        if is_dir {
                            Box::pin(Self::remove_dir_recursive(sftp, &entry_path)).await?;
                        } else {
                            sftp.remove(&entry_path).await.map_err(|e| e.to_string())?;
                        }
                    }
                }
                Err(russh_sftp::client::error::Error::Status(status))
                    if status.status_code == StatusCode::Eof =>
                {
                    break
                }
                Err(e) => {
                    let _ = sftp.close(handle).await;
                    return Err(e.to_string());
                }
            }
        }

        let _ = sftp.close(handle).await;
        sftp.rmdir(path).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn chmod(session_id: &str, path: &str, mode: u32) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;

        let mut attrs = sftp.stat(path).await.map_err(|e| e.to_string())?.attrs;
        attrs.permissions = Some(mode);

        sftp.setstat(path, attrs).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn rename_item(
        session_id: &str,
        old_path: &str,
        new_path: &str,
    ) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        sftp.rename(old_path, new_path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn copy_item(
        app: AppHandle,
        session_id: &str,
        source_path: &str,
        dest_path: &str,
        task_id: Option<String>,
    ) -> Result<(), String> {
        Self::copy_item_with_mode(
            app,
            session_id,
            source_path,
            dest_path,
            task_id,
            CopyMode::ServerSideOnly,
        )
        .await
    }

    pub async fn copy_item_streaming(
        app: AppHandle,
        session_id: &str,
        source_path: &str,
        dest_path: &str,
        task_id: Option<String>,
    ) -> Result<(), String> {
        Self::copy_item_with_mode(
            app,
            session_id,
            source_path,
            dest_path,
            task_id,
            CopyMode::StreamingOnly,
        )
        .await
    }

    async fn copy_item_with_mode(
        app: AppHandle,
        session_id: &str,
        source_path: &str,
        dest_path: &str,
        task_id: Option<String>,
        copy_mode: CopyMode,
    ) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        let metadata = sftp
            .stat(source_path)
            .await
            .map_err(|e| e.to_string())?
            .attrs;
        let is_dir = metadata.is_dir();
        let total_bytes = if is_dir {
            0
        } else {
            metadata.size.unwrap_or(0)
        };
        if matches!(copy_mode, CopyMode::ServerSideOnly)
            && !Self::session_supports_copy_data(session_id).await
        {
            return Err(COPY_DATA_UNSUPPORTED_ERROR.to_string());
        }

        let copy_task_id = task_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let file_name = Path::new(source_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let progress_context = CopyProgressContext {
            app: &app,
            task_id: &copy_task_id,
            session_id,
            file_name: &file_name,
            source: source_path,
            destination: dest_path,
        };

        let should_emit_progress = matches!(copy_mode, CopyMode::StreamingOnly);
        if should_emit_progress {
            Self::emit_copy_progress(&progress_context, total_bytes, 0, 0.0, None, "queued", None);
            Self::emit_copy_progress(
                &progress_context,
                total_bytes,
                0,
                0.0,
                None,
                "transferring",
                None,
            );
        }

        let result = if is_dir {
            Self::copy_dir_recursive(session_id, &sftp, source_path, dest_path, copy_mode).await
        } else {
            Self::copy_file(
                session_id,
                &sftp,
                source_path,
                dest_path,
                if should_emit_progress {
                    Some(&progress_context)
                } else {
                    None
                },
                copy_mode,
            )
            .await
        };

        match result {
            Ok(()) => {
                if should_emit_progress {
                    Self::emit_copy_progress(
                        &progress_context,
                        total_bytes,
                        total_bytes,
                        0.0,
                        Some(0),
                        "completed",
                        None,
                    );
                }
                Ok(())
            }
            Err(error) => {
                if should_emit_progress {
                    Self::emit_copy_progress(
                        &progress_context,
                        total_bytes,
                        0,
                        0.0,
                        None,
                        "failed",
                        Some(error.clone()),
                    );
                }
                Err(error)
            }
        }
    }

    fn emit_copy_progress(
        context: &CopyProgressContext<'_>,
        total_bytes: u64,
        transferred_bytes: u64,
        speed: f64,
        eta: Option<u64>,
        status: &str,
        error: Option<String>,
    ) {
        let _ = context.app.emit(
            "transfer-progress",
            TransferProgress {
                task_id: context.task_id.to_string(),
                type_: COPY_TRANSFER_TYPE.to_string(),
                session_id: context.session_id.to_string(),
                file_name: context.file_name.to_string(),
                source: context.source.to_string(),
                destination: context.destination.to_string(),
                total_bytes,
                transferred_bytes,
                speed,
                eta,
                status: status.to_string(),
                error,
            },
        );
    }

    fn is_copy_data_unsupported_status(status: &Status) -> bool {
        if status.status_code == StatusCode::OpUnsupported {
            return true;
        }

        if status.status_code != StatusCode::Failure {
            return false;
        }

        let error_message = status.error_message.to_ascii_lowercase();
        error_message.contains("unsupported")
            || error_message.contains("unknown extended request")
            || error_message.contains("copy-data")
    }

    async fn session_supports_copy_data(session_id: &str) -> bool {
        let support = SFTP_COPY_DATA_SUPPORT.lock().await;
        support.get(session_id).copied().unwrap_or(false)
    }

    async fn mark_copy_data_unsupported(session_id: &str) {
        let mut support = SFTP_COPY_DATA_SUPPORT.lock().await;
        support.insert(session_id.to_string(), false);
    }

    async fn copy_file(
        session_id: &str,
        sftp: &Arc<RawSftpSession>,
        source: &str,
        dest: &str,
        progress_context: Option<&CopyProgressContext<'_>>,
        copy_mode: CopyMode,
    ) -> Result<(), String> {
        let supports_copy_data = Self::session_supports_copy_data(session_id).await;
        if matches!(copy_mode, CopyMode::ServerSideOnly) && !supports_copy_data {
            return Err(COPY_DATA_UNSUPPORTED_ERROR.to_string());
        }

        if supports_copy_data && !matches!(copy_mode, CopyMode::StreamingOnly) {
            match Self::copy_file_server_side(sftp, source, dest).await {
                Ok(()) => return Ok(()),
                Err(ServerSideCopyError::Unsupported) => {
                    Self::mark_copy_data_unsupported(session_id).await;
                    if matches!(copy_mode, CopyMode::ServerSideOnly) {
                        return Err(COPY_DATA_UNSUPPORTED_ERROR.to_string());
                    }
                }
                Err(ServerSideCopyError::Other(error)) => return Err(error),
            }
        }

        if matches!(copy_mode, CopyMode::ServerSideOnly) {
            return Err(COPY_DATA_UNSUPPORTED_ERROR.to_string());
        }

        Self::copy_file_streaming(sftp, source, dest, progress_context).await
    }

    async fn copy_file_server_side(
        sftp: &Arc<RawSftpSession>,
        source: &str,
        dest: &str,
    ) -> Result<(), ServerSideCopyError> {
        let handle = sftp
            .open(source, OpenFlags::READ, FileAttributes::default())
            .await
            .map_err(|e| ServerSideCopyError::Other(e.to_string()))?
            .handle;

        let mut attrs = FileAttributes::default();
        if let Ok(source_attrs) = sftp.stat(source).await {
            attrs.permissions = source_attrs.attrs.permissions;
        }

        let dest_handle = match sftp
            .open(
                dest,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
                attrs,
            )
            .await
        {
            Ok(result) => result.handle,
            Err(error) => {
                let _ = sftp.close(handle).await;
                return Err(ServerSideCopyError::Other(error.to_string()));
            }
        };

        let request_data = match ser::to_bytes(&CopyDataExtension {
            read_from_handle: handle.clone(),
            read_from_offset: 0,
            read_data_length: 0,
            write_to_handle: dest_handle.clone(),
            write_to_offset: 0,
        }) {
            Ok(data) => data.to_vec(),
            Err(error) => {
                let _ = sftp.close(handle).await;
                let _ = sftp.close(dest_handle).await;
                return Err(ServerSideCopyError::Other(error.to_string()));
            }
        };

        let result = match sftp.extended(COPY_DATA_EXTENSION_NAME, request_data).await {
            Ok(Packet::Status(status)) if status.status_code == StatusCode::Ok => Ok(()),
            Ok(Packet::Status(status)) if Self::is_copy_data_unsupported_status(&status) => {
                Err(ServerSideCopyError::Unsupported)
            }
            Ok(Packet::Status(status)) => Err(ServerSideCopyError::Other(format!(
                "Server-side copy failed: {}",
                status.error_message
            ))),
            Ok(_) => Err(ServerSideCopyError::Other(
                "Server-side copy failed: unexpected response packet".to_string(),
            )),
            Err(russh_sftp::client::error::Error::Status(status))
                if Self::is_copy_data_unsupported_status(&status) =>
            {
                Err(ServerSideCopyError::Unsupported)
            }
            Err(error) => Err(ServerSideCopyError::Other(error.to_string())),
        };

        let _ = sftp.close(handle).await;
        let _ = sftp.close(dest_handle).await;
        if matches!(result, Err(ServerSideCopyError::Unsupported)) {
            let _ = sftp.remove(dest).await;
        }

        result
    }

    async fn copy_file_streaming(
        sftp: &Arc<RawSftpSession>,
        source: &str,
        dest: &str,
        progress_context: Option<&CopyProgressContext<'_>>,
    ) -> Result<(), String> {
        let handle = sftp
            .open(source, OpenFlags::READ, FileAttributes::default())
            .await
            .map_err(|e| e.to_string())?
            .handle;

        let mut attrs = FileAttributes::default();
        if let Ok(source_attrs) = sftp.stat(source).await {
            attrs.permissions = source_attrs.attrs.permissions;
        }

        let dest_handle = sftp
            .open(
                dest,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
                attrs,
            )
            .await
            .map_err(|e| e.to_string())?
            .handle;

        let total_bytes = sftp
            .fstat(&handle)
            .await
            .map_err(|e| e.to_string())?
            .attrs
            .size
            .unwrap_or(0);
        let chunk_size = 256 * 1024;
        let start_time = Instant::now();
        let mut last_emit = Instant::now();
        let mut transferred_bytes = 0u64;

        let mut futures = FuturesUnordered::new();
        let mut next_offset = 0u64;
        let max_concurrent_requests = 64;

        for _ in 0..max_concurrent_requests {
            if next_offset >= total_bytes {
                break;
            }
            let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
            let offset = next_offset;
            let sftp_clone = sftp.clone();
            let handle_clone = handle.clone();

            futures.push(
                async move {
                    let data = sftp_clone
                        .read(handle_clone, offset, current_chunk_size as u32)
                        .await;
                    (offset, data)
                }
                .boxed(),
            );
            next_offset += current_chunk_size;
        }

        while let Some((offset, result)) = futures.next().await {
            let data = result.map_err(|e| e.to_string())?.data;
            transferred_bytes = transferred_bytes.saturating_add(data.len() as u64);
            sftp.write(&dest_handle, offset, data)
                .await
                .map_err(|e| e.to_string())?;

            if last_emit.elapsed().as_millis() > 500 {
                if let Some(context) = progress_context {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let speed = if elapsed > 0.0 {
                        transferred_bytes as f64 / elapsed
                    } else {
                        0.0
                    };
                    let eta = if speed > 0.0 {
                        Some(
                            ((total_bytes.saturating_sub(transferred_bytes)) as f64 / speed)
                                .max(0.0) as u64,
                        )
                    } else {
                        None
                    };

                    Self::emit_copy_progress(
                        context,
                        total_bytes,
                        transferred_bytes,
                        speed,
                        eta,
                        "transferring",
                        None,
                    );
                }

                last_emit = Instant::now();
            }

            if next_offset < total_bytes {
                let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
                let offset = next_offset;
                let sftp_clone = sftp.clone();
                let handle_clone = handle.clone();

                futures.push(Box::pin(async move {
                    let data = sftp_clone
                        .read(handle_clone, offset, current_chunk_size as u32)
                        .await;
                    (offset, data)
                }));
                next_offset += current_chunk_size;
            }
        }

        let _ = sftp.close(handle).await;
        let _ = sftp.close(dest_handle).await;
        Ok(())
    }

    async fn copy_dir_recursive(
        session_id: &str,
        sftp: &Arc<RawSftpSession>,
        source: &str,
        dest: &str,
        copy_mode: CopyMode,
    ) -> Result<(), String> {
        sftp.mkdir(dest, FileAttributes::default())
            .await
            .map_err(|e| e.to_string())?;

        let handle = sftp
            .opendir(source)
            .await
            .map_err(|e| e.to_string())?
            .handle;

        loop {
            match sftp.readdir(&handle).await {
                Ok(name) => {
                    for entry in name.files {
                        let file_name = entry.filename;
                        if file_name == "." || file_name == ".." {
                            continue;
                        }

                        let source_path = format!("{}/{}", source.trim_end_matches('/'), file_name);
                        let dest_path = format!("{}/{}", dest.trim_end_matches('/'), file_name);
                        let is_dir = entry.attrs.is_dir();

                        if is_dir {
                            Box::pin(Self::copy_dir_recursive(
                                session_id,
                                sftp,
                                &source_path,
                                &dest_path,
                                copy_mode,
                            ))
                            .await?;
                        } else {
                            Box::pin(async {
                                Self::copy_file(
                                    session_id,
                                    sftp,
                                    &source_path,
                                    &dest_path,
                                    None,
                                    copy_mode,
                                )
                                .await
                            })
                            .await?;
                        }
                    }
                }
                Err(russh_sftp::client::error::Error::Status(status))
                    if status.status_code == StatusCode::Eof =>
                {
                    break
                }
                Err(e) => {
                    let _ = sftp.close(handle).await;
                    return Err(e.to_string());
                }
            }
        }

        let _ = sftp.close(handle).await;
        Ok(())
    }
}
