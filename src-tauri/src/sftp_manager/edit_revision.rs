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
}
