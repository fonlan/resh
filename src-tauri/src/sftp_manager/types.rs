use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::Serialize;
use tauri::AppHandle;

use crate::db::DatabaseManager;

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
