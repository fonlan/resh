use serde::Serialize;
use tauri::AppHandle;

pub(super) const COPY_DATA_EXTENSION_NAME: &str = "copy-data";
pub(super) const COPY_DATA_EXTENSION_VERSION: &str = "1";
pub(super) const COPY_TRANSFER_TYPE: &str = "copy";
pub(super) const COPY_DATA_UNSUPPORTED_ERROR: &str = "SFTP_COPY_DATA_UNSUPPORTED";

#[derive(Serialize)]
pub(super) struct CopyDataExtension {
    pub(super) read_from_handle: String,
    pub(super) read_from_offset: u64,
    pub(super) read_data_length: u64,
    pub(super) write_to_handle: String,
    pub(super) write_to_offset: u64,
}

pub(super) enum ServerSideCopyError {
    Unsupported,
    Other(String),
}

#[derive(Clone, Copy)]
pub(super) enum CopyMode {
    ServerSideOnly,
    StreamingOnly,
}

pub(super) struct CopyProgressContext<'a> {
    pub(super) app: &'a AppHandle,
    pub(super) task_id: &'a str,
    pub(super) session_id: &'a str,
    pub(super) file_name: &'a str,
    pub(super) source: &'a str,
    pub(super) destination: &'a str,
}
