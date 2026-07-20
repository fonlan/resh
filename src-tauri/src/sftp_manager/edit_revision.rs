use crate::sftp_manager::SftpManager;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const REMOTE_EDIT_READ_CHUNK_SIZE: u32 = 255 * 1024;

/// The best available revision signal for a remote SFTP file.
///
/// Standard SFTP does not expose a portable conditional-write primitive, so
/// `size` and `mtime` are the fast optimistic-concurrency gate. `sha256` is
/// calculated while content is already being read and is used to describe a
/// resolved conflict, not to force a download before every ordinary save.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileRevision {
    pub exists: bool,
    pub size: Option<u64>,
    pub mtime: Option<u64>,
    pub sha256: Option<String>,
}

impl RemoteFileRevision {
    pub fn missing() -> Self {
        Self {
            exists: false,
            size: None,
            mtime: None,
            sha256: None,
        }
    }

    pub fn from_attrs(attrs: &FileAttributes) -> Self {
        Self {
            exists: true,
            size: attrs.size,
            mtime: attrs.mtime.map(u64::from),
            sha256: None,
        }
    }

    pub fn with_sha256(mut self, sha256: String) -> Self {
        self.sha256 = Some(sha256);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteRevisionComparison {
    MetadataUnchanged,
    MetadataChanged,
    Deleted,
}

/// Compares only the metadata available from a normal SFTP stat request.
/// Hashes deliberately do not participate: a regular save must not download
/// the file merely to refresh a hash.
pub fn compare_remote_metadata(
    expected: &RemoteFileRevision,
    current: &RemoteFileRevision,
) -> RemoteRevisionComparison {
    if !current.exists {
        return RemoteRevisionComparison::Deleted;
    }

    if expected.exists && expected.size == current.size && expected.mtime == current.mtime {
        RemoteRevisionComparison::MetadataUnchanged
    } else {
        RemoteRevisionComparison::MetadataChanged
    }
}

pub fn metadata_matches(left: &RemoteFileRevision, right: &RemoteFileRevision) -> bool {
    left.exists == right.exists && left.size == right.size && left.mtime == right.mtime
}

/// Pure gate for built-in editor save / external auto-upload.
///
/// A regular save never downloads remote content merely to refresh a hash.
/// Full-file reads happen only after this gate reports a conflict (or for the
/// separate post-write verification path on small files).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveGateDecision {
    /// size+mtime match; caller may write without reading remote body.
    ProceedWithoutSnapshot,
    /// Remote diverged; caller must not write. Content snapshot is needed only
    /// when the file still exists (diff UI / confirm revision).
    Conflict {
        reason: &'static str,
        needs_content_snapshot: bool,
    },
    /// overwrite mode requires the conflictRevision the user just reviewed.
    MissingConflictRevision,
    UnsupportedMode,
}

/// Decide whether a save/upload may proceed from metadata alone.
///
/// Known fast-mode ceiling (not a bug to "fix" here): when an external writer
/// changes bytes but keeps the same size and the same mtime precision window,
/// this gate returns [`SaveGateDecision::ProceedWithoutSnapshot`]. Callers must
/// not claim that case is reliably detected without a future opt-in strong check.
pub fn evaluate_save_gate(
    save_mode: &str,
    expected: &RemoteFileRevision,
    current: &RemoteFileRevision,
    conflict_revision: Option<&RemoteFileRevision>,
) -> SaveGateDecision {
    match save_mode {
        "safe" => match compare_remote_metadata(expected, current) {
            RemoteRevisionComparison::MetadataUnchanged => SaveGateDecision::ProceedWithoutSnapshot,
            RemoteRevisionComparison::MetadataChanged => SaveGateDecision::Conflict {
                reason: "metadataChanged",
                needs_content_snapshot: true,
            },
            RemoteRevisionComparison::Deleted => SaveGateDecision::Conflict {
                reason: "deleted",
                needs_content_snapshot: false,
            },
        },
        "overwrite" => {
            let Some(confirmed) = conflict_revision else {
                return SaveGateDecision::MissingConflictRevision;
            };
            if metadata_matches(confirmed, current) {
                SaveGateDecision::ProceedWithoutSnapshot
            } else if current.exists {
                SaveGateDecision::Conflict {
                    reason: "metadataChanged",
                    needs_content_snapshot: true,
                }
            } else {
                SaveGateDecision::Conflict {
                    reason: "deleted",
                    needs_content_snapshot: false,
                }
            }
        }
        _ => SaveGateDecision::UnsupportedMode,
    }
}

/// Lightweight activation check for a clean built-in editor tab.
/// Unchanged metadata must not download remote body content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckGateDecision {
    Unchanged,
    ChangedNeedsSnapshot { reason: &'static str },
    Deleted,
}

pub fn evaluate_check_gate(
    expected: &RemoteFileRevision,
    current: &RemoteFileRevision,
) -> CheckGateDecision {
    match compare_remote_metadata(expected, current) {
        RemoteRevisionComparison::MetadataUnchanged => CheckGateDecision::Unchanged,
        RemoteRevisionComparison::MetadataChanged => CheckGateDecision::ChangedNeedsSnapshot {
            reason: "metadataChanged",
        },
        RemoteRevisionComparison::Deleted => CheckGateDecision::Deleted,
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub async fn read_remote_revision(
    session_id: &str,
    remote_path: &str,
) -> Result<RemoteFileRevision, String> {
    let sftp = SftpManager::get_session(session_id).await?;
    match sftp.stat(remote_path).await {
        Ok(attrs) => Ok(RemoteFileRevision::from_attrs(&attrs.attrs)),
        Err(russh_sftp::client::error::Error::Status(status))
            if status.status_code == StatusCode::NoSuchFile =>
        {
            Ok(RemoteFileRevision::missing())
        }
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug)]
pub struct RemoteFileSnapshot {
    pub revision: RemoteFileRevision,
    pub bytes: Vec<u8>,
}

/// Downloads a remote file only when the caller already needs a conflict
/// snapshot. Hashing happens incrementally in the same read stream.
pub async fn read_remote_snapshot(
    session_id: &str,
    remote_path: &str,
) -> Result<RemoteFileSnapshot, String> {
    let mut revision = read_remote_revision(session_id, remote_path).await?;
    if !revision.exists {
        return Ok(RemoteFileSnapshot {
            revision,
            bytes: Vec::new(),
        });
    }

    let expected_size = revision.size.unwrap_or(0);
    let sftp = SftpManager::get_session(session_id).await?;
    let handle = sftp
        .open(remote_path, OpenFlags::READ, FileAttributes::default())
        .await
        .map_err(|error| error.to_string())?
        .handle;

    let read_result: Result<Vec<u8>, String> = async {
        let mut bytes = Vec::with_capacity(expected_size.try_into().unwrap_or(0));
        let mut hasher = Sha256::new();
        let mut offset = 0u64;

        loop {
            let read_len = if expected_size > 0 {
                let remaining = expected_size.saturating_sub(offset);
                if remaining == 0 {
                    break;
                }
                remaining.min(REMOTE_EDIT_READ_CHUNK_SIZE as u64) as u32
            } else {
                REMOTE_EDIT_READ_CHUNK_SIZE
            };

            match sftp.read(&handle, offset, read_len).await {
                Ok(data) => {
                    if data.data.is_empty() {
                        if expected_size == 0 || offset >= expected_size {
                            break;
                        }
                        return Err(format!(
                            "Remote snapshot incomplete: received empty data before EOF ({} / {} bytes)",
                            offset, expected_size
                        ));
                    }
                    hasher.update(&data.data);
                    offset += data.data.len() as u64;
                    bytes.extend_from_slice(&data.data);
                }
                Err(russh_sftp::client::error::Error::Status(status))
                    if status.status_code == StatusCode::Eof =>
                {
                    if expected_size > 0 && offset < expected_size {
                        return Err(format!(
                            "Remote snapshot incomplete: EOF before full content received ({} / {} bytes)",
                            offset, expected_size
                        ));
                    }
                    break;
                }
                Err(error) => return Err(error.to_string()),
            }
        }

        revision.sha256 = Some(format!("{:x}", hasher.finalize()));
        Ok(bytes)
    }
    .await;

    let _ = sftp.close(handle).await;
    let bytes = read_result?;
    Ok(RemoteFileSnapshot { revision, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision(size: u64, mtime: u64) -> RemoteFileRevision {
        RemoteFileRevision {
            exists: true,
            size: Some(size),
            mtime: Some(mtime),
            sha256: Some("baseline".to_string()),
        }
    }

    fn revision_with_hash(size: u64, mtime: u64, hash: &str) -> RemoteFileRevision {
        RemoteFileRevision {
            exists: true,
            size: Some(size),
            mtime: Some(mtime),
            sha256: Some(hash.to_string()),
        }
    }

    #[test]
    fn metadata_comparison_uses_size_and_mtime_without_hash_download() {
        let expected = revision(4, 10);
        let same_metadata_different_hash = RemoteFileRevision {
            sha256: Some("different".to_string()),
            ..expected.clone()
        };

        assert_eq!(
            compare_remote_metadata(&expected, &same_metadata_different_hash),
            RemoteRevisionComparison::MetadataUnchanged
        );
        assert_eq!(
            compare_remote_metadata(&expected, &revision(5, 10)),
            RemoteRevisionComparison::MetadataChanged
        );
        assert_eq!(
            compare_remote_metadata(&expected, &RemoteFileRevision::missing()),
            RemoteRevisionComparison::Deleted
        );
    }

    #[test]
    fn metadata_match_ignores_conflict_snapshot_hash() {
        let expected = revision(4, 10);
        let current = RemoteFileRevision {
            sha256: Some("new-hash".to_string()),
            ..expected.clone()
        };

        assert!(metadata_matches(&expected, &current));
    }

    #[test]
    fn safe_save_unchanged_metadata_proceeds_without_content_snapshot() {
        let expected = revision(100, 42);
        let current = revision_with_hash(100, 42, "irrelevant-for-gate");

        assert_eq!(
            evaluate_save_gate("safe", &expected, &current, None),
            SaveGateDecision::ProceedWithoutSnapshot
        );
        // Same decision is the contract that ordinary saves must not call
        // read_remote_snapshot / full-body hash download before writing.
        assert_eq!(
            evaluate_check_gate(&expected, &current),
            CheckGateDecision::Unchanged
        );
    }

    #[test]
    fn safe_save_metadata_changed_requires_snapshot_and_does_not_write() {
        let expected = revision(100, 42);
        let current = revision(101, 42);

        assert_eq!(
            evaluate_save_gate("safe", &expected, &current, None),
            SaveGateDecision::Conflict {
                reason: "metadataChanged",
                needs_content_snapshot: true,
            }
        );
        assert_eq!(
            evaluate_check_gate(&expected, &current),
            CheckGateDecision::ChangedNeedsSnapshot {
                reason: "metadataChanged",
            }
        );
    }

    #[test]
    fn safe_save_remote_deleted_returns_deleted_without_body_read() {
        let expected = revision(100, 42);
        let current = RemoteFileRevision::missing();

        assert_eq!(
            evaluate_save_gate("safe", &expected, &current, None),
            SaveGateDecision::Conflict {
                reason: "deleted",
                needs_content_snapshot: false,
            }
        );
        assert_eq!(
            evaluate_check_gate(&expected, &current),
            CheckGateDecision::Deleted
        );
    }

    #[test]
    fn metadata_changed_even_when_content_hash_would_match() {
        // mtime/size moved but bytes could still be identical after snapshot.
        // Gate still reports conflict so the UI can show Diff / adopt remote;
        // auto-merge of identical content is intentionally out of scope.
        let expected = revision_with_hash(100, 42, "same-bytes");
        let current = revision_with_hash(100, 99, "same-bytes");

        assert_eq!(
            compare_remote_metadata(&expected, &current),
            RemoteRevisionComparison::MetadataChanged
        );
        assert_eq!(
            evaluate_save_gate("safe", &expected, &current, None),
            SaveGateDecision::Conflict {
                reason: "metadataChanged",
                needs_content_snapshot: true,
            }
        );
    }

    #[test]
    fn overwrite_requires_conflict_revision_and_rejects_stale_confirmation() {
        let expected = revision(100, 42);
        let dialog_revision = revision(110, 50);
        let current = revision(110, 50);
        let moved_again = revision(120, 60);

        assert_eq!(
            evaluate_save_gate("overwrite", &expected, &current, None),
            SaveGateDecision::MissingConflictRevision
        );
        assert_eq!(
            evaluate_save_gate("overwrite", &expected, &current, Some(&dialog_revision)),
            SaveGateDecision::ProceedWithoutSnapshot
        );
        assert_eq!(
            evaluate_save_gate("overwrite", &expected, &moved_again, Some(&dialog_revision)),
            SaveGateDecision::Conflict {
                reason: "metadataChanged",
                needs_content_snapshot: true,
            }
        );
        assert_eq!(
            evaluate_save_gate(
                "overwrite",
                &expected,
                &RemoteFileRevision::missing(),
                Some(&dialog_revision)
            ),
            SaveGateDecision::Conflict {
                reason: "deleted",
                needs_content_snapshot: false,
            }
        );
    }

    #[test]
    fn successful_save_baseline_updates_to_post_write_revision() {
        // After a write, callers replace the editor/watcher baseline with the
        // verified remote revision (size always; sha256 for small files or from
        // the uploaded stream). This pure assertion documents that contract.
        let previous = revision(10, 1);
        let written_bytes = b"hello world";
        let next = RemoteFileRevision {
            exists: true,
            size: Some(written_bytes.len() as u64),
            mtime: Some(2),
            sha256: Some(sha256_hex(written_bytes)),
        };

        assert_ne!(previous, next);
        assert_eq!(next.size, Some(11));
        let expected_sha256 = sha256_hex(written_bytes);
        assert_eq!(next.sha256.as_deref(), Some(expected_sha256.as_str()));
        assert_eq!(
            evaluate_save_gate("safe", &next, &next, None),
            SaveGateDecision::ProceedWithoutSnapshot
        );
    }

    #[test]
    fn fast_mode_known_ceiling_same_size_same_mtime_is_not_detected() {
        // Documented non-goal of the default fast path: content can change while
        // size and mtime stay identical within server precision. Do not "fix" this
        // as a detection success; only record that the gate treats it as unchanged.
        let expected = revision_with_hash(64, 1_700_000_000, "content-a");
        let stealth_edit = revision_with_hash(64, 1_700_000_000, "content-b");

        assert_eq!(
            compare_remote_metadata(&expected, &stealth_edit),
            RemoteRevisionComparison::MetadataUnchanged
        );
        assert_eq!(
            evaluate_save_gate("safe", &expected, &stealth_edit, None),
            SaveGateDecision::ProceedWithoutSnapshot
        );
    }

    #[test]
    fn unsupported_save_mode_is_rejected() {
        let expected = revision(1, 1);
        assert_eq!(
            evaluate_save_gate("force", &expected, &expected, None),
            SaveGateDecision::UnsupportedMode
        );
    }
}
