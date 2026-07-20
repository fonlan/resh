use crate::commands::AppState;
use crate::sftp_manager::edit_revision::{
    compare_remote_metadata, metadata_matches, read_remote_revision, read_remote_snapshot,
    sha256_hex, RemoteFileRevision, RemoteRevisionComparison,
};
use crate::sftp_manager::{SftpManager, TransferProgress};
use russh_sftp::protocol::{OpenFlags, StatusCode};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const SFTP_EDIT_DOWNLOAD_CHUNK_SIZE: u32 = 255 * 1024;
const SFTP_TEXT_SAVE_UPLOAD_CHUNK_SIZE: usize = 32 * 1024;
/// Small editor files get a post-write content hash verification; large files
/// retain the mandatory size verification without an extra full download.
const SFTP_TEXT_SAVE_HASH_VERIFY_MAX_BYTES: usize = 1024 * 1024;
const UTF8_BOM: &[u8; 3] = b"\xEF\xBB\xBF";
const UTF16LE_BOM: &[u8; 2] = b"\xFF\xFE";
const UTF16BE_BOM: &[u8; 2] = b"\xFE\xFF";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpOpenTextFileResult {
    pub session_id: String,
    pub remote_path: String,
    pub local_path: String,
    pub content: String,
    pub encoding: String,
    pub language_hint: Option<String>,
    pub revision: RemoteFileRevision,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum SftpSaveTextFileOutcome {
    Saved {
        session_id: String,
        remote_path: String,
        local_path: String,
        bytes_written: u64,
        encoding: String,
        revision: RemoteFileRevision,
    },
    Conflict {
        session_id: String,
        remote_path: String,
        reason: String,
        expected_revision: RemoteFileRevision,
        current_revision: RemoteFileRevision,
        remote_content: Option<String>,
        remote_encoding: Option<String>,
        snapshot_error: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum SftpCheckTextFileOutcome {
    Unchanged {
        revision: RemoteFileRevision,
    },
    Changed {
        reason: String,
        current_revision: RemoteFileRevision,
        remote_content: Option<String>,
        remote_encoding: Option<String>,
        snapshot_error: Option<String>,
    },
}

#[derive(Debug)]
struct DownloadedRemoteFile {
    bytes_written: u64,
    sha256: String,
}

fn infer_language_hint(remote_path: &str) -> Option<String> {
    let path = Path::new(remote_path);
    let file_name = path.file_name()?.to_string_lossy().to_ascii_lowercase();

    let from_name = match file_name.as_str() {
        "dockerfile" => Some("dockerfile"),
        "makefile" => Some("makefile"),
        _ => None,
    };
    if let Some(language) = from_name {
        return Some(language.to_string());
    }

    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())?;
    let language = match extension.as_str() {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "py" => "python",
        "java" => "java",
        "c" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" => "cpp",
        "go" => "go",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sh" | "bash" | "zsh" => "shell",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" | "scss" | "less" => "css",
        "xml" => "xml",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "ini" | "conf" | "cfg" => "ini",
        _ => return None,
    };
    Some(language.to_string())
}

fn looks_like_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    if bytes.contains(&0) {
        return true;
    }

    let mut suspicious = 0usize;
    for &b in bytes {
        let is_common_text =
            matches!(b, b'\n' | b'\r' | b'\t' | 0x0C) || (0x20..=0x7E).contains(&b) || b >= 0x80;
        if !is_common_text {
            suspicious += 1;
        }
    }

    suspicious * 100 > bytes.len() * 30
}

fn decode_utf16_bytes(bytes: &[u8], little_endian: bool) -> Result<String, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("Invalid UTF-16 byte length".to_string());
    }

    let utf16_units = bytes
        .chunks_exact(2)
        .map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<u16>>();

    String::from_utf16(&utf16_units).map_err(|e| format!("Invalid UTF-16 content: {}", e))
}

fn decode_text_bytes(bytes: &[u8]) -> Result<(String, String), String> {
    if bytes.starts_with(UTF8_BOM) {
        let content = std::str::from_utf8(&bytes[UTF8_BOM.len()..])
            .map_err(|e| format!("Invalid UTF-8 content with BOM: {}", e))?
            .to_string();
        return Ok((content, "utf-8-bom".to_string()));
    }

    if bytes.starts_with(UTF16LE_BOM) {
        let content = decode_utf16_bytes(&bytes[UTF16LE_BOM.len()..], true)?;
        return Ok((content, "utf-16le".to_string()));
    }

    if bytes.starts_with(UTF16BE_BOM) {
        let content = decode_utf16_bytes(&bytes[UTF16BE_BOM.len()..], false)?;
        return Ok((content, "utf-16be".to_string()));
    }

    if looks_like_binary(bytes) {
        return Err("File appears to be binary and cannot be opened as text".to_string());
    }

    let content = std::str::from_utf8(bytes)
        .map_err(|e| format!("File is not valid UTF-8 text: {}", e))?
        .to_string();
    Ok((content, "utf-8".to_string()))
}

fn encode_text_content(content: &str, encoding: &str) -> Result<Vec<u8>, String> {
    match encoding.to_ascii_lowercase().as_str() {
        "utf-8" => Ok(content.as_bytes().to_vec()),
        "utf-8-bom" => {
            let mut encoded = UTF8_BOM.to_vec();
            encoded.extend_from_slice(content.as_bytes());
            Ok(encoded)
        }
        "utf-16le" => {
            let mut encoded = UTF16LE_BOM.to_vec();
            for unit in content.encode_utf16() {
                encoded.extend_from_slice(&unit.to_le_bytes());
            }
            Ok(encoded)
        }
        "utf-16be" => {
            let mut encoded = UTF16BE_BOM.to_vec();
            for unit in content.encode_utf16() {
                encoded.extend_from_slice(&unit.to_be_bytes());
            }
            Ok(encoded)
        }
        other => Err(format!("Unsupported text encoding: {}", other)),
    }
}

async fn create_temp_local_path(session_id: &str, remote_path: &str) -> Result<PathBuf, String> {
    let temp_dir = std::env::temp_dir()
        .join("resh_sftp")
        .join(session_id)
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| e.to_string())?;

    let file_name = Path::new(remote_path)
        .file_name()
        .ok_or("Invalid remote path")?
        .to_string_lossy()
        .to_string();
    Ok(temp_dir.join(file_name))
}

async fn download_remote_file_to_local(
    session_id: &str,
    remote_path: &str,
    local_path: &Path,
    expected_size: u64,
) -> Result<DownloadedRemoteFile, String> {
    let sftp = SftpManager::get_session(session_id).await?;
    let handle = sftp
        .open(
            remote_path,
            OpenFlags::READ,
            russh_sftp::protocol::FileAttributes::default(),
        )
        .await
        .map_err(|e| e.to_string())?
        .handle;

    let mut local_file = fs::File::create(local_path)
        .await
        .map_err(|e| e.to_string())?;

    let download_result: Result<DownloadedRemoteFile, String> = async {
        let mut offset = 0u64;
        let mut hasher = Sha256::new();
        loop {
            let read_len = if expected_size > 0 {
                let remaining = expected_size.saturating_sub(offset);
                if remaining == 0 {
                    break;
                }
                std::cmp::min(remaining, SFTP_EDIT_DOWNLOAD_CHUNK_SIZE as u64) as u32
            } else {
                SFTP_EDIT_DOWNLOAD_CHUNK_SIZE
            };

            match sftp.read(&handle, offset, read_len).await {
                Ok(data) => {
                    if data.data.is_empty() {
                        if expected_size == 0 || offset >= expected_size {
                            break;
                        }
                        return Err(format!(
                            "Download incomplete: received empty data before EOF ({} / {} bytes)",
                            offset, expected_size
                        ));
                    }

                    hasher.update(&data.data);
                    local_file
                        .write_all(&data.data)
                        .await
                        .map_err(|e| e.to_string())?;
                    offset += data.data.len() as u64;
                }
                Err(russh_sftp::client::error::Error::Status(status))
                    if status.status_code == StatusCode::Eof =>
                {
                    if expected_size > 0 && offset < expected_size {
                        return Err(format!(
                            "Download incomplete: EOF before full content received ({} / {} bytes)",
                            offset, expected_size
                        ));
                    }
                    break;
                }
                Err(e) => {
                    return Err(e.to_string());
                }
            }
        }

        local_file.flush().await.map_err(|e| e.to_string())?;
        local_file.sync_all().await.map_err(|e| e.to_string())?;
        drop(local_file);
        Ok(DownloadedRemoteFile {
            bytes_written: offset,
            sha256: format!("{:x}", hasher.finalize()),
        })
    }
    .await;

    let _ = sftp.close(handle).await;
    download_result
}

async fn write_local_file_atomically(local_path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = local_path
        .parent()
        .ok_or("Invalid local path: missing parent directory")?;
    fs::create_dir_all(parent)
        .await
        .map_err(|e| e.to_string())?;

    let file_name = local_path
        .file_name()
        .ok_or("Invalid local path: missing file name")?
        .to_string_lossy()
        .to_string();
    let temp_path = parent.join(format!(".{}.{}.tmp", file_name, Uuid::new_v4()));

    let mut temp_file = fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("Failed to create temporary file: {}", e))?;
    temp_file
        .write_all(bytes)
        .await
        .map_err(|e| format!("Failed to write temporary file: {}", e))?;
    temp_file
        .flush()
        .await
        .map_err(|e| format!("Failed to flush temporary file: {}", e))?;
    temp_file
        .sync_all()
        .await
        .map_err(|e| format!("Failed to sync temporary file: {}", e))?;
    drop(temp_file);

    match fs::rename(&temp_path, local_path).await {
        Ok(_) => Ok(()),
        Err(rename_err) => {
            let target_exists = fs::try_exists(local_path).await.map_err(|e| {
                format!(
                    "Failed to check destination path while replacing local file: {}",
                    e
                )
            })?;

            if !target_exists {
                let _ = fs::remove_file(&temp_path).await;
                return Err(format!(
                    "Failed to replace local file with temporary file: {}",
                    rename_err
                ));
            }

            fs::remove_file(local_path).await.map_err(|e| {
                format!("Failed to remove existing local file before replace: {}", e)
            })?;
            fs::rename(&temp_path, local_path)
                .await
                .map_err(|e| format!("Failed to move temporary file into place: {}", e))?;
            Ok(())
        }
    }
}

async fn upload_text_bytes_to_remote(
    session_id: &str,
    remote_path: &str,
    bytes: &[u8],
) -> Result<RemoteFileRevision, String> {
    let expected_sha256 = sha256_hex(bytes);
    let sftp = SftpManager::get_session(session_id).await?;
    let handle = sftp
        .open(
            remote_path,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            russh_sftp::protocol::FileAttributes::default(),
        )
        .await
        .map_err(|error| error.to_string())?
        .handle;

    let upload_result: Result<(), String> = async {
        let mut offset = 0u64;
        for chunk in bytes.chunks(SFTP_TEXT_SAVE_UPLOAD_CHUNK_SIZE) {
            sftp.write(&handle, offset, chunk.to_vec())
                .await
                .map_err(|error| error.to_string())?;
            offset += chunk.len() as u64;
        }
        Ok(())
    }
    .await;

    let _ = sftp.close(handle).await;
    upload_result?;

    let revision = read_remote_revision(session_id, remote_path).await?;
    let expected_size = bytes.len() as u64;
    if !revision.exists || revision.size != Some(expected_size) {
        return Err(format!(
            "Remote save verification failed: expected {} bytes, got {:?}",
            expected_size, revision.size
        ));
    }

    if bytes.len() <= SFTP_TEXT_SAVE_HASH_VERIFY_MAX_BYTES {
        let snapshot = read_remote_snapshot(session_id, remote_path).await?;
        if !snapshot.revision.exists
            || snapshot.revision.sha256.as_deref() != Some(&expected_sha256)
        {
            return Err("Remote save content verification failed after write".to_string());
        }
        return Ok(snapshot.revision);
    }

    Ok(revision.with_sha256(expected_sha256))
}

async fn build_save_conflict_outcome(
    session_id: String,
    remote_path: String,
    expected_revision: RemoteFileRevision,
    reason: &str,
    current_revision: RemoteFileRevision,
) -> SftpSaveTextFileOutcome {
    if !current_revision.exists {
        return SftpSaveTextFileOutcome::Conflict {
            session_id,
            remote_path,
            reason: "deleted".to_string(),
            expected_revision,
            current_revision,
            remote_content: None,
            remote_encoding: None,
            snapshot_error: None,
        };
    }

    match read_remote_snapshot(&session_id, &remote_path).await {
        Ok(snapshot) if !snapshot.revision.exists => SftpSaveTextFileOutcome::Conflict {
            session_id,
            remote_path,
            reason: "deleted".to_string(),
            expected_revision,
            current_revision: snapshot.revision,
            remote_content: None,
            remote_encoding: None,
            snapshot_error: None,
        },
        Ok(snapshot) => match decode_text_bytes(&snapshot.bytes) {
            Ok((content, encoding)) => SftpSaveTextFileOutcome::Conflict {
                session_id,
                remote_path,
                reason: reason.to_string(),
                expected_revision,
                current_revision: snapshot.revision,
                remote_content: Some(content),
                remote_encoding: Some(encoding),
                snapshot_error: None,
            },
            Err(error) => SftpSaveTextFileOutcome::Conflict {
                session_id,
                remote_path,
                reason: reason.to_string(),
                expected_revision,
                current_revision: snapshot.revision,
                remote_content: None,
                remote_encoding: None,
                snapshot_error: Some(error),
            },
        },
        Err(error) => SftpSaveTextFileOutcome::Conflict {
            session_id,
            remote_path,
            reason: reason.to_string(),
            expected_revision,
            current_revision,
            remote_content: None,
            remote_encoding: None,
            snapshot_error: Some(format!(
                "Failed to load remote conflict snapshot: {}",
                error
            )),
        },
    }
}

#[tauri::command]
pub async fn sftp_open_text_file(
    session_id: String,
    remote_path: String,
) -> Result<SftpOpenTextFileResult, String> {
    let metadata = SftpManager::metadata(&session_id, &remote_path).await?;
    if metadata.attrs.is_dir() {
        return Err("Not a file (is a directory)".to_string());
    }
    let total_bytes = metadata.attrs.size.unwrap_or(0);
    let mut revision = RemoteFileRevision::from_attrs(&metadata.attrs);

    let local_path = create_temp_local_path(&session_id, &remote_path).await?;
    let downloaded =
        download_remote_file_to_local(&session_id, &remote_path, &local_path, total_bytes).await?;
    revision.sha256 = Some(downloaded.sha256);

    let file_bytes = fs::read(&local_path)
        .await
        .map_err(|error| format!("Failed to read downloaded local file: {}", error))?;
    let (content, encoding) = decode_text_bytes(&file_bytes)?;
    let language_hint = infer_language_hint(&remote_path);

    Ok(SftpOpenTextFileResult {
        session_id,
        remote_path,
        local_path: local_path.to_string_lossy().to_string(),
        content,
        encoding,
        language_hint,
        revision,
    })
}

#[tauri::command]
pub async fn sftp_check_text_file(
    session_id: String,
    remote_path: String,
    expected_revision: RemoteFileRevision,
) -> Result<SftpCheckTextFileOutcome, String> {
    let current_revision = read_remote_revision(&session_id, &remote_path).await?;
    let reason = match compare_remote_metadata(&expected_revision, &current_revision) {
        RemoteRevisionComparison::MetadataUnchanged => {
            return Ok(SftpCheckTextFileOutcome::Unchanged {
                revision: current_revision,
            });
        }
        RemoteRevisionComparison::MetadataChanged => "metadataChanged",
        RemoteRevisionComparison::Deleted => "deleted",
    };

    if !current_revision.exists {
        return Ok(SftpCheckTextFileOutcome::Changed {
            reason: reason.to_string(),
            current_revision,
            remote_content: None,
            remote_encoding: None,
            snapshot_error: None,
        });
    }

    match read_remote_snapshot(&session_id, &remote_path).await {
        Ok(snapshot) if !snapshot.revision.exists => Ok(SftpCheckTextFileOutcome::Changed {
            reason: "deleted".to_string(),
            current_revision: snapshot.revision,
            remote_content: None,
            remote_encoding: None,
            snapshot_error: None,
        }),
        Ok(snapshot) => match decode_text_bytes(&snapshot.bytes) {
            Ok((content, encoding)) => Ok(SftpCheckTextFileOutcome::Changed {
                reason: reason.to_string(),
                current_revision: snapshot.revision,
                remote_content: Some(content),
                remote_encoding: Some(encoding),
                snapshot_error: None,
            }),
            Err(error) => Ok(SftpCheckTextFileOutcome::Changed {
                reason: reason.to_string(),
                current_revision: snapshot.revision,
                remote_content: None,
                remote_encoding: None,
                snapshot_error: Some(error),
            }),
        },
        Err(error) => Ok(SftpCheckTextFileOutcome::Changed {
            reason: reason.to_string(),
            current_revision,
            remote_content: None,
            remote_encoding: None,
            snapshot_error: Some(format!("Failed to load remote change snapshot: {}", error)),
        }),
    }
}

#[tauri::command]
pub async fn sftp_save_text_file(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    remote_path: String,
    local_path: String,
    content: String,
    encoding: String,
    expected_revision: RemoteFileRevision,
    save_mode: Option<String>,
    conflict_revision: Option<RemoteFileRevision>,
) -> Result<SftpSaveTextFileOutcome, String> {
    use crate::updater::OperationCategory;

    let local_path_buf = PathBuf::from(&local_path);
    if local_path_buf.file_name().is_none() {
        return Err("Invalid local path".to_string());
    }
    let encoded_bytes = encode_text_content(&content, &encoding)?;
    let save_mode = save_mode.unwrap_or_else(|| "safe".to_string());

    let permit = state
        .operation_coordinator
        .try_acquire(OperationCategory::SftpEditUpload)
        .await?;
    let result: Result<SftpSaveTextFileOutcome, String> = async {
        let _path_lock = state
            .sftp_edit_manager
            .acquire_remote_path_lock(&session_id, &remote_path)
            .await;
        let current_revision = read_remote_revision(&session_id, &remote_path).await?;

        let conflict = match save_mode.as_str() {
            "safe" => match compare_remote_metadata(&expected_revision, &current_revision) {
                RemoteRevisionComparison::MetadataUnchanged => None,
                RemoteRevisionComparison::MetadataChanged => {
                    Some(("metadataChanged", current_revision))
                }
                RemoteRevisionComparison::Deleted => Some(("deleted", current_revision)),
            },
            "overwrite" => {
                let confirmed_revision = conflict_revision.ok_or(
                    "Explicit overwrite requires the conflictRevision returned by the server",
                )?;
                if metadata_matches(&confirmed_revision, &current_revision) {
                    None
                } else if current_revision.exists {
                    Some(("metadataChanged", current_revision))
                } else {
                    Some(("deleted", current_revision))
                }
            }
            other => return Err(format!("Unsupported SFTP text save mode: {}", other)),
        };

        if let Some((reason, current_revision)) = conflict {
            return Ok(build_save_conflict_outcome(
                session_id.clone(),
                remote_path.clone(),
                expected_revision,
                reason,
                current_revision,
            )
            .await);
        }

        // Do not update the local editor backing file until the remote revision
        // gate has passed. A conflict must leave the local saved baseline intact.
        write_local_file_atomically(&local_path_buf, &encoded_bytes).await?;
        let revision =
            upload_text_bytes_to_remote(&session_id, &remote_path, &encoded_bytes).await?;

        Ok(SftpSaveTextFileOutcome::Saved {
            session_id,
            remote_path,
            local_path,
            bytes_written: encoded_bytes.len() as u64,
            encoding,
            revision,
        })
    }
    .await;
    permit.release().await;
    result
}

#[tauri::command]
pub async fn sftp_edit_file(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    remote_path: String,
) -> Result<String, String> {
    // 1. Create unique temp local path
    let local_path = create_temp_local_path(&session_id, &remote_path).await?;
    let file_name_str = local_path
        .file_name()
        .ok_or("Invalid local path generated for edit task")?
        .to_string_lossy()
        .to_string();

    // 2. Kill any existing watch for the same remote file in this session.
    state
        .sftp_edit_manager
        .stop_watching_remote(&session_id, &remote_path);

    let task_id = Uuid::new_v4().to_string();

    // 3. Download file
    let metadata = SftpManager::metadata(&session_id, &remote_path).await?;
    if metadata.attrs.is_dir() {
        return Err("Not a file (is a directory)".to_string());
    }
    let total_bytes = metadata.attrs.size.unwrap_or(0);

    // Emit Initial Progress
    let _ = app.emit(
        "transfer-progress",
        TransferProgress {
            task_id: task_id.clone(),
            type_: "download".to_string(),
            session_id: session_id.clone(),
            file_name: file_name_str.clone(),
            source: remote_path.clone(),
            destination: local_path.to_string_lossy().to_string(),
            total_bytes,
            transferred_bytes: 0,
            speed: 0.0,
            eta: None,
            status: "transferring".to_string(),
            error: None,
        },
    );

    let mut baseline_revision = RemoteFileRevision::from_attrs(&metadata.attrs);
    let downloaded =
        match download_remote_file_to_local(&session_id, &remote_path, &local_path, total_bytes)
            .await
        {
            Ok(downloaded) => downloaded,
            Err(error_message) => {
                let _ = app.emit(
                    "transfer-progress",
                    TransferProgress {
                        task_id,
                        type_: "download".to_string(),
                        session_id: session_id.clone(),
                        file_name: file_name_str,
                        source: remote_path.clone(),
                        destination: local_path.to_string_lossy().to_string(),
                        total_bytes,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "failed".to_string(),
                        error: Some(error_message.clone()),
                    },
                );
                return Err(error_message);
            }
        };
    baseline_revision.sha256 = Some(downloaded.sha256);

    let _ = app.emit(
        "transfer-progress",
        TransferProgress {
            task_id,
            type_: "download".to_string(),
            session_id: session_id.clone(),
            file_name: file_name_str,
            source: remote_path.clone(),
            destination: local_path.to_string_lossy().to_string(),
            total_bytes,
            transferred_bytes: downloaded.bytes_written,
            speed: 0.0,
            eta: None,
            status: "completed".to_string(),
            error: None,
        },
    );

    // 4. Register watcher with the same revision contract as the built-in editor.
    let _edit_id = state.sftp_edit_manager.watch_file(
        local_path.clone(),
        session_id,
        remote_path,
        baseline_revision,
    )?;

    Ok(local_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn resolve_sftp_edit_conflict(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    edit_id: String,
    conflict_id: String,
    action: String,
) -> Result<crate::sftp_manager::edit::SftpResolveEditConflictOutcome, String> {
    let state = state.inner().clone();
    let state_for_args = state.clone();
    state
        .sftp_edit_manager
        .resolve_edit_conflict(&app, state_for_args.as_ref(), edit_id, conflict_id, action)
        .await
}

#[tauri::command]
pub async fn open_local_editor(path: String, editor: Option<String>) -> Result<(), String> {
    tracing::info!("Opening local editor for path: {}", path);

    if let Some(editor_cmd) = editor {
        if !editor_cmd.is_empty() {
            tracing::info!("Using custom editor command: {}", editor_cmd);

            #[cfg(target_os = "macos")]
            if editor_cmd.to_ascii_lowercase().ends_with(".app") {
                std::process::Command::new("open")
                    .arg("-a")
                    .arg(&editor_cmd)
                    .arg(&path)
                    .spawn()
                    .map_err(|e| format!("Failed to launch editor app: {}", e))?;
                return Ok(());
            }

            std::process::Command::new(&editor_cmd)
                .arg(&path)
                .spawn()
                .map_err(|e| format!("Failed to launch editor: {}", e))?;
            return Ok(());
        }
    }

    tracing::info!("Using system default editor");
    open::that(&path)
        .map_err(|e| format!("Failed to open file with default application: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn pick_folder() -> Result<Option<String>, String> {
    use rfd::FileDialog;
    let folder = tokio::task::spawn_blocking(move || FileDialog::new().pick_folder())
        .await
        .map_err(|e| e.to_string())?;

    Ok(folder.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn pick_file() -> Result<Option<String>, String> {
    use rfd::FileDialog;
    let file = tokio::task::spawn_blocking(move || FileDialog::new().pick_file())
        .await
        .map_err(|e| e.to_string())?;

    Ok(file.map(|p| p.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_language_hint_from_posix_remote_path() {
        assert_eq!(
            infer_language_hint("/var/www/服务/config.tsx"),
            Some("typescript".to_string())
        );
    }

    #[test]
    fn rejects_binary_content_before_utf8_decoding() {
        let err = decode_text_bytes(b"hello\0world").unwrap_err();

        assert!(err.contains("binary"));
    }

    #[test]
    fn round_trips_utf16le_text() {
        let encoded = encode_text_content("hello 你好", "utf-16le").unwrap();
        let (decoded, encoding) = decode_text_bytes(&encoded).unwrap();

        assert_eq!(decoded, "hello 你好");
        assert_eq!(encoding, "utf-16le");
    }

    #[tokio::test]
    async fn temp_local_path_preserves_remote_unicode_filename() {
        let local_path = create_temp_local_path("session-a", "/tmp/目录/远程 文件😀.rs")
            .await
            .unwrap();

        assert_eq!(
            local_path.file_name().and_then(|name| name.to_str()),
            Some("远程 文件😀.rs")
        );
        assert!(local_path
            .components()
            .any(|component| component.as_os_str() == "session-a"));
    }
}
