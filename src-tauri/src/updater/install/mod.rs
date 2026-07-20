//! Cross-platform prepared-update installation (Windows portable EXE + macOS DMG).
//!
//! Platform helpers run **after** the main process exits. This module only:
//! 1. Validates the prepared package and current install shape
//! 2. Writes a static helper script + parameters (no remote code)
//! 3. Spawns the helper (hidden on Windows)
//! 4. Returns so the frontend can exit once snapshot + barrier are ready
//!
//! On failure after the old process has exited, helpers write a result marker
//! and attempt rollback + relaunch of the previous version.

mod manifest;
mod result;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use super::current_app_version;
use super::download::get_prepared_update;
use super::restart_session::{is_valid_token, load_snapshot_with_options};
use super::types::PreparedUpdate;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub use result::{
    clear_install_alive_marker, clear_last_install_failure, install_failure_path,
    install_result_path, load_last_install_failure, read_install_result,
    write_install_alive_marker,
};

/// Opaque request accepted by `install_prepared_update`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallPreparedUpdateRequest {
    /// Prepared update id from a verified download.
    pub prepared_id: String,
    /// One-time restore token already written via `save_restart_session_snapshot`.
    pub snapshot_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallPreparedUpdateResponse {
    /// True when the platform helper process was spawned successfully.
    pub helper_started: bool,
    pub target_version: String,
    /// Human-readable phase for UI (no sensitive paths).
    pub message: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedInstallContext {
    pub prepared: PreparedUpdate,
    pub staged_path: PathBuf,
    pub restore_token: String,
    pub app_data_dir: PathBuf,
    pub failure_path: PathBuf,
}

/// Whether automatic install is supported on this build target.
pub fn platform_supports_install() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

pub fn updates_root(app_data_dir: &Path) -> PathBuf {
    super::paths::updates_root(app_data_dir)
}

/// Preflight + spawn install helper for a verified prepared update.
///
/// Does **not** exit the process. Caller must exit only after this returns Ok
/// and barrier/snapshot gates already passed.
pub async fn install_prepared_update(
    app: &AppHandle,
    request: InstallPreparedUpdateRequest,
) -> Result<InstallPreparedUpdateResponse, String> {
    if !platform_supports_install() {
        return Err("Automatic update installation is not supported on this platform".to_string());
    }
    if !is_valid_token(&request.snapshot_token) {
        return Err("Invalid restore token".to_string());
    }

    let app_data_dir = resolve_app_data_dir(app)?;
    // Snapshot must exist on disk (pre-install: do not require target == current).
    let snapshot = load_snapshot_with_options(&app_data_dir, &request.snapshot_token, false)?;

    let (prepared, staged_path) =
        get_prepared_update(&request.prepared_id)
            .await
            .ok_or_else(|| {
                "Unknown or expired prepared update. Download the update again.".to_string()
            })?;

    if !staged_path.is_file() {
        return Err("Prepared update file is missing; download again".to_string());
    }

    // Prepared package version must match the snapshot target.
    if !versions_match(&prepared.version, &snapshot.target_version) {
        return Err(format!(
            "Prepared update version {} does not match restart snapshot target {}",
            prepared.version, snapshot.target_version
        ));
    }

    // Refuse installing an equal package over the running build.
    if versions_match(&prepared.version, current_app_version()) {
        return Err(format!(
            "Prepared update version {} matches the running app; nothing to install",
            prepared.version
        ));
    }

    let failure_path = install_failure_path(&app_data_dir);
    // Clear stale failure so a new attempt is not confused with a previous one.
    clear_last_install_failure(&app_data_dir);

    let ctx = PreparedInstallContext {
        prepared: prepared.clone(),
        staged_path,
        restore_token: request.snapshot_token.clone(),
        app_data_dir,
        failure_path,
    };

    #[cfg(windows)]
    {
        windows::preflight_and_spawn(&ctx)?;
    }
    #[cfg(target_os = "macos")]
    {
        macos::preflight_and_spawn(&ctx)?;
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = ctx;
        return Err("Automatic update installation is not supported on this platform".to_string());
    }

    Ok(InstallPreparedUpdateResponse {
        helper_started: true,
        target_version: prepared.version,
        message: "Update helper started; application will exit to complete installation"
            .to_string(),
    })
}

/// After a successful session restore on the new version: remove staging leftovers
/// and any leftover helper scripts / backups that the helper left for ack.
///
/// Cleanup treats the on-disk install-manifest as an untrusted index and re-binds:
/// - live install parent from `current_exe()` / `Resh.app` (never manifest.install_parent)
/// - pending restore token (must match manifest.restore_token; required)
/// - exact backup/staging basenames (version + nonce) under the live parent only
pub async fn ack_update_install(
    app: &AppHandle,
    prepared_id: Option<String>,
) -> Result<(), String> {
    let app_data_dir = resolve_app_data_dir(app)?;
    // Clear success result markers (failures are consumed at startup).
    // Never follow a symlink-escaped updates root.
    let result = install_result_path(&app_data_dir);
    if super::paths::updates_root_exists_and_trusted(&app_data_dir) && result.is_file() {
        if let Ok(parsed) = read_install_result(&result) {
            if parsed.ok {
                let _ = super::paths::remove_trusted_updates_relative(
                    &app_data_dir,
                    Path::new(result::INSTALL_RESULT_FILE),
                );
            }
        }
    }

    // Cleanup authorization never trusts prepared_id from the on-disk manifest.
    // Only a non-empty caller-provided id is used for CleanupBinding; restore_token
    // remains the mandatory process-bound gate. Manifest prepared_id is only used
    // as a best-effort key for forgetting the in-memory prepared registry entry.
    let manifest = manifest::load_install_manifest(&app_data_dir);
    let trusted_prepared_id = prepared_id
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let forget_id = trusted_prepared_id
        .clone()
        .or_else(|| manifest.as_ref().map(|m| m.prepared_id.clone()));

    if let Some(ref id) = forget_id {
        super::download::forget_prepared_update(id, true).await;
    }

    // Capture restore token BEFORE any other side effects clear it.
    let restore_token = super::get_pending_restore_token();

    // Drop the alive handshake marker for this restore token if known.
    if let Some(ref token) = restore_token {
        clear_install_alive_marker(&app_data_dir, token);
    }

    let live_parent = resolve_live_install_parent();
    let prepared_ref = trusted_prepared_id.as_deref();

    #[cfg(windows)]
    {
        let _ = restore_token;
        windows::cleanup_after_ack(&app_data_dir, prepared_ref, live_parent);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = restore_token;
        macos::cleanup_after_ack(&app_data_dir, prepared_ref, live_parent);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        if let Some(m) = manifest {
            use manifest::CleanupBinding;
            manifest::cleanup_manifest_paths(
                &app_data_dir,
                &m,
                &CleanupBinding {
                    live_install_parent: live_parent,
                    prepared_id: trusted_prepared_id,
                    platform: None,
                    restore_token,
                },
            );
        }
    }

    Ok(())
}

/// Resolve the parent directory of the currently running install for ack binding.
fn resolve_live_install_parent() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let current_exe = fs::canonicalize(&current_exe).unwrap_or(current_exe);
    #[cfg(target_os = "macos")]
    {
        if let Some(bundle) = macos::current_macos_app_bundle_from_exe(&current_exe) {
            let bundle = fs::canonicalize(&bundle).unwrap_or(bundle);
            return bundle.parent().map(|p| p.to_path_buf());
        }
    }
    current_exe.parent().map(|p| p.to_path_buf())
}

/// Schedule process exit shortly after helper spawn so the helper can wait on our PID.
pub fn schedule_exit_after_helper_started(app: AppHandle) {
    std::thread::Builder::new()
        .name("resh-update-exit".to_string())
        .spawn(move || {
            // Give the helper a moment to attach to our PID.
            std::thread::sleep(std::time::Duration::from_millis(400));
            app.exit(0);
            // Hard fallback if Tauri exit is delayed by other handlers.
            std::thread::sleep(std::time::Duration::from_secs(8));
            std::process::exit(0);
        })
        .ok();
}

fn resolve_app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let default_app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;
    Ok(crate::app_paths::resolve_app_data_dir_from_default(
        &default_app_data_dir,
    ))
}

pub(crate) fn versions_match(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_start_matches('v').to_ascii_lowercase();
    !a.trim().is_empty() && norm(a) == norm(b)
}

pub(crate) fn sanitize_path_component(value: &str, label: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(format!("Invalid {label} for update path"));
    }
    Ok(trimmed.to_string())
}

/// Best-effort PE magic + machine check without external crates.
#[cfg(windows)]
pub(crate) fn looks_like_windows_pe_x64(path: &Path) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| format!("open staged file: {e}"))?;
    let mut mz = [0u8; 0x40];
    f.read_exact(&mut mz)
        .map_err(|e| format!("read staged file: {e}"))?;
    if &mz[0..2] != b"MZ" {
        return Err("Staged file is not a Windows PE executable".to_string());
    }
    let pe_off = u32::from_le_bytes([mz[0x3c], mz[0x3d], mz[0x3e], mz[0x3f]]) as u64;
    f.seek(SeekFrom::Start(pe_off))
        .map_err(|e| format!("seek PE header: {e}"))?;
    let mut pe = [0u8; 6];
    f.read_exact(&mut pe)
        .map_err(|_| "Staged PE header is incomplete".to_string())?;
    if &pe[0..4] != b"PE\0\0" {
        return Err("Staged file is not a valid PE".to_string());
    }
    let machine = u16::from_le_bytes([pe[4], pe[5]]);
    // IMAGE_FILE_MACHINE_AMD64 = 0x8664
    if machine != 0x8664 {
        return Err(format!(
            "Staged PE machine 0x{machine:04x} is not x86_64 (0x8664)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_match_trims_v_prefix() {
        assert!(versions_match("1.2.3", "v1.2.3"));
        assert!(versions_match("v1.2.3", "1.2.3"));
        assert!(!versions_match("1.2.3", "1.2.4"));
    }

    #[test]
    fn sanitize_rejects_path_traversal() {
        assert!(sanitize_path_component("../evil", "version").is_err());
        assert!(sanitize_path_component("a/b", "version").is_err());
        assert!(sanitize_path_component("1.2.3", "version").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn pe_magic_rejects_non_pe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not.exe");
        std::fs::write(&path, b"not a pe").unwrap();
        assert!(looks_like_windows_pe_x64(&path).is_err());
    }
}
