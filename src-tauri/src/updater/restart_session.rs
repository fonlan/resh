//! Versioned restart session snapshot for restoring tabs after update install.
//!
//! Snapshots live under `{app_data}/updates/restarts/` and are referenced by a
//! one-time CLI token. They must never contain passwords, private keys, or
//! remote file bodies.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::types::Server;
use crate::updater::current_app_version;

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
/// Snapshots older than this are rejected and cleaned up.
pub const SNAPSHOT_TTL_SECS: u64 = 24 * 60 * 60;
/// Orphaned `.part` / unacked snapshots past this age are deleted on cleanup.
pub const SNAPSHOT_STALE_CLEANUP_SECS: u64 = 48 * 60 * 60;

const RESTORE_FLAG: &str = "--restore-update-session";

static PENDING_RESTORE_TOKEN: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSplitLayout {
    pub layout: String,
    pub tab_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SnapshotTab {
    #[serde(rename = "terminal")]
    Terminal {
        id: String,
        label: String,
        server_id: String,
        /// Temporary Quick Connect server (connection structure only; no secrets).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temporary_server: Option<Server>,
    },
    #[serde(rename = "editor")]
    Editor {
        id: String,
        label: String,
        server_id: String,
        remote_path: String,
        language: String,
        /// Terminal tab id this editor was associated with (for rebinding session).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal_tab_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartSessionSnapshot {
    pub schema_version: u32,
    pub token: String,
    /// Version of the app that wrote the snapshot (pre-update).
    pub source_version: String,
    /// Target version expected after update install.
    pub target_version: String,
    /// Unix epoch seconds when the snapshot was written.
    pub created_at: u64,
    /// Unix epoch seconds after which the snapshot is rejected.
    pub expires_at: u64,
    pub tabs: Vec<SnapshotTab>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_view: Option<SnapshotSplitLayout>,
    #[serde(default)]
    pub remembered_split_views: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRestartSession {
    pub token: String,
    pub snapshot: RestartSessionSnapshot,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn restarts_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("updates").join("restarts")
}

fn snapshot_path(app_data_dir: &Path, token: &str) -> PathBuf {
    restarts_dir(app_data_dir).join(format!("{token}.json"))
}

fn part_path(app_data_dir: &Path, token: &str) -> PathBuf {
    restarts_dir(app_data_dir).join(format!("{token}.json.part"))
}

/// Parse `--restore-update-session <token>` from process args. Call once at startup.
pub fn capture_restore_token_from_args<I, S>(args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        if arg == RESTORE_FLAG {
            if let Some(token) = iter.next() {
                let token = token.as_ref().trim();
                if is_valid_token(token) {
                    return Some(token.to_string());
                }
            }
            return None;
        }
        if let Some(rest) = arg.strip_prefix(&format!("{RESTORE_FLAG}=")) {
            let token = rest.trim();
            if is_valid_token(token) {
                return Some(token.to_string());
            }
            return None;
        }
    }
    None
}

pub fn is_valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 128
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Store token for later frontend fetch (setup only records; validation is deferred).
pub fn set_pending_restore_token(token: Option<String>) {
    if let Ok(mut guard) = PENDING_RESTORE_TOKEN.lock() {
        *guard = token;
    }
}

pub fn get_pending_restore_token() -> Option<String> {
    PENDING_RESTORE_TOKEN
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

/// Atomic write: temp file + fsync + rename. Does not modify local.json / sync.json.
pub fn write_snapshot(
    app_data_dir: &Path,
    mut snapshot: RestartSessionSnapshot,
) -> Result<String, String> {
    sanitize_snapshot_in_place(&mut snapshot)?;

    let dir = restarts_dir(app_data_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("create restarts dir: {e}"))?;

    let token = if snapshot.token.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        snapshot.token.clone()
    };
    if !is_valid_token(&token) {
        return Err("invalid snapshot token".to_string());
    }
    snapshot.token = token.clone();
    if snapshot.schema_version == 0 {
        snapshot.schema_version = SNAPSHOT_SCHEMA_VERSION;
    }
    let now = now_unix_secs();
    if snapshot.created_at == 0 {
        snapshot.created_at = now;
    }
    if snapshot.expires_at == 0 {
        snapshot.expires_at = now.saturating_add(SNAPSHOT_TTL_SECS);
    }

    let part = part_path(app_data_dir, &token);
    let final_path = snapshot_path(app_data_dir, &token);

    let json = serde_json::to_vec_pretty(&snapshot)
        .map_err(|e| format!("serialize restart snapshot: {e}"))?;

    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&part)
            .map_err(|e| format!("open snapshot part: {e}"))?;
        file.write_all(&json)
            .map_err(|e| format!("write snapshot part: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("fsync snapshot part: {e}"))?;
    }

    fs::rename(&part, &final_path).map_err(|e| format!("rename snapshot: {e}"))?;

    // Best-effort fsync of directory (may fail on some platforms).
    if let Ok(dir_file) = File::open(&dir) {
        let _ = dir_file.sync_all();
    }

    // Never log snapshot body or the one-time restore token (capability).
    tracing::info!(
        tabs = snapshot.tabs.len(),
        target_version = %snapshot.target_version,
        "wrote restart session snapshot"
    );

    Ok(token)
}

fn sanitize_snapshot_in_place(snapshot: &mut RestartSessionSnapshot) -> Result<(), String> {
    for tab in &mut snapshot.tabs {
        if let SnapshotTab::Terminal {
            temporary_server: Some(server),
            ..
        } = tab
        {
            // Ensure we never embed authentication secrets on temporary servers.
            // Server already has auth_id only; strip additional_prompt noise if huge.
            if server
                .additional_prompt
                .as_ref()
                .map(|s| s.len() > 4096)
                .unwrap_or(false)
            {
                server.additional_prompt = None;
            }
        }
        if let SnapshotTab::Editor {
            remote_path,
            language,
            ..
        } = tab
        {
            if remote_path.trim().is_empty() {
                return Err("editor tab missing remotePath".to_string());
            }
            if language.is_empty() {
                *language = "plaintext".to_string();
            }
        }
    }
    Ok(())
}

pub fn load_snapshot(app_data_dir: &Path, token: &str) -> Result<RestartSessionSnapshot, String> {
    load_snapshot_with_options(app_data_dir, token, true)
}

/// Load snapshot; when `require_target_version_match` is false (post-write verify
/// before install), only schema/token/expiry are checked.
pub fn load_snapshot_with_options(
    app_data_dir: &Path,
    token: &str,
    require_target_version_match: bool,
) -> Result<RestartSessionSnapshot, String> {
    if !is_valid_token(token) {
        return Err("invalid restore token".to_string());
    }
    let path = snapshot_path(app_data_dir, token);
    if !path.exists() {
        return Err("restart session snapshot not found".to_string());
    }
    let mut file = File::open(&path).map_err(|e| format!("open snapshot: {e}"))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|e| format!("read snapshot: {e}"))?;
    let snapshot: RestartSessionSnapshot =
        serde_json::from_slice(&buf).map_err(|e| format!("parse snapshot: {e}"))?;
    validate_snapshot(&snapshot, token, require_target_version_match)?;
    Ok(snapshot)
}

pub fn validate_snapshot(
    snapshot: &RestartSessionSnapshot,
    expected_token: &str,
    require_target_version_match: bool,
) -> Result<(), String> {
    if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported snapshot schema version {}",
            snapshot.schema_version
        ));
    }
    if snapshot.token != expected_token {
        return Err("snapshot token mismatch".to_string());
    }
    if !is_valid_token(&snapshot.token) {
        return Err("invalid snapshot token".to_string());
    }
    let now = now_unix_secs();
    if snapshot.expires_at > 0 && now > snapshot.expires_at {
        return Err("restart session snapshot expired".to_string());
    }
    if require_target_version_match {
        let current = current_app_version();
        if !snapshot.target_version.is_empty()
            && snapshot.target_version != current
            && !versions_loosely_equal(&snapshot.target_version, current)
        {
            return Err(format!(
                "snapshot target version {} does not match running version {}",
                snapshot.target_version, current
            ));
        }
    }
    Ok(())
}

fn versions_loosely_equal(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_start_matches('v').to_string();
    norm(a) == norm(b)
}

/// Load pending session for the token captured at process start.
pub fn get_pending_restart_session(
    app_data_dir: &Path,
) -> Result<Option<PendingRestartSession>, String> {
    let token = match get_pending_restore_token() {
        Some(t) => t,
        None => return Ok(None),
    };
    match load_snapshot(app_data_dir, &token) {
        Ok(snapshot) => Ok(Some(PendingRestartSession { token, snapshot })),
        Err(e) => {
            tracing::warn!(error = %e, "pending restart session unavailable");
            Err(e)
        }
    }
}

/// Acknowledge successful restore and delete the snapshot file.
pub fn ack_restart_session(app_data_dir: &Path, token: &str) -> Result<(), String> {
    if !is_valid_token(token) {
        return Err("invalid restore token".to_string());
    }
    let path = snapshot_path(app_data_dir, token);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("delete snapshot: {e}"))?;
    }
    // Clear pending token if it matches.
    if let Ok(mut guard) = PENDING_RESTORE_TOKEN.lock() {
        if guard.as_deref() == Some(token) {
            *guard = None;
        }
    }
    tracing::info!("acked restart session snapshot");
    Ok(())
}

/// Delete expired snapshots and leftover `.part` files.
pub fn cleanup_stale_snapshots(app_data_dir: &Path) {
    let dir = restarts_dir(app_data_dir);
    if !dir.is_dir() {
        return;
    }
    let now = now_unix_secs();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let age_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| now.saturating_sub(d.as_secs()))
            .unwrap_or(0);

        if name.ends_with(".part") {
            if age_secs > SNAPSHOT_STALE_CLEANUP_SECS {
                let _ = fs::remove_file(&path);
            }
            continue;
        }
        if !name.ends_with(".json") {
            continue;
        }
        // Load lightly to check expires_at; on parse failure and old file, delete.
        match fs::read(&path) {
            Ok(bytes) => {
                if let Ok(snap) = serde_json::from_slice::<RestartSessionSnapshot>(&bytes) {
                    if snap.expires_at > 0 && now > snap.expires_at.saturating_add(60) {
                        let _ = fs::remove_file(&path);
                    } else if age_secs > SNAPSHOT_STALE_CLEANUP_SECS {
                        let _ = fs::remove_file(&path);
                    }
                } else if age_secs > SNAPSHOT_STALE_CLEANUP_SECS {
                    let _ = fs::remove_file(&path);
                }
            }
            Err(_) => {
                if age_secs > Duration::from_secs(SNAPSHOT_STALE_CLEANUP_SECS).as_secs() {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }
}

/// Build a CLI argument list fragment for launching the new process.
pub fn restore_session_cli_args(token: &str) -> Vec<String> {
    vec![RESTORE_FLAG.to_string(), token.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("resh-restart-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_snapshot(token: &str, target: &str) -> RestartSessionSnapshot {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        RestartSessionSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            token: token.to_string(),
            source_version: "1.0.0".to_string(),
            target_version: target.to_string(),
            created_at: now,
            expires_at: now + SNAPSHOT_TTL_SECS,
            tabs: vec![
                SnapshotTab::Terminal {
                    id: "t1".to_string(),
                    label: "box".to_string(),
                    server_id: "s1".to_string(),
                    temporary_server: None,
                },
                SnapshotTab::Editor {
                    id: "e1".to_string(),
                    label: "a.txt".to_string(),
                    server_id: "s1".to_string(),
                    remote_path: "/tmp/a.txt".to_string(),
                    language: "plaintext".to_string(),
                    terminal_tab_id: Some("t1".to_string()),
                },
            ],
            active_tab_id: Some("t1".to_string()),
            split_view: Some(SnapshotSplitLayout {
                layout: "horizontal".to_string(),
                tab_ids: vec!["t1".to_string(), "e1".to_string()],
            }),
            remembered_split_views: serde_json::json!({}),
        }
    }

    #[test]
    fn parse_restore_args() {
        let t = capture_restore_token_from_args([
            "resh",
            "--restore-update-session",
            "abc-123_def",
        ]);
        assert_eq!(t.as_deref(), Some("abc-123_def"));
        let t2 = capture_restore_token_from_args(["resh", "--restore-update-session=tok1"]);
        assert_eq!(t2.as_deref(), Some("tok1"));
        assert!(capture_restore_token_from_args(["resh"]).is_none());
        assert!(
            capture_restore_token_from_args(["resh", "--restore-update-session", "../evil"])
                .is_none()
        );
    }

    #[test]
    fn write_load_ack_roundtrip() {
        let dir = temp_dir();
        let current = current_app_version().to_string();
        let snap = sample_snapshot("", &current);
        let token = write_snapshot(&dir, snap).unwrap();
        let loaded = load_snapshot(&dir, &token).unwrap();
        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.token, token);
        ack_restart_session(&dir, &token).unwrap();
        assert!(load_snapshot(&dir, &token).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_expired() {
        let dir = temp_dir();
        let current = current_app_version().to_string();
        let mut snap = sample_snapshot("expired-token-1", &current);
        snap.expires_at = 1;
        // Bypass write_snapshot expiry rewrite by writing manually then load.
        let path = snapshot_path(&dir, "expired-token-1");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec(&snap).unwrap()).unwrap();
        let err = load_snapshot(&dir, "expired-token-1").unwrap_err();
        assert!(err.contains("expired"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_token_flow() {
        set_pending_restore_token(None);
        assert!(get_pending_restore_token().is_none());
        set_pending_restore_token(Some("tok-ok".to_string()));
        assert_eq!(get_pending_restore_token().as_deref(), Some("tok-ok"));
        set_pending_restore_token(None);
    }
}
