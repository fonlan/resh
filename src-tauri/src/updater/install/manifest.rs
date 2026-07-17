//! Precise install artifact manifest for post-update ack cleanup.
//!
//! The on-disk manifest is an **untrusted index** only. Cleanup never uses
//! `install_parent` as an authorization boundary. Install-parent deletes are
//! re-derived exclusively from the live parent of `current_exe()` / `Resh.app`
//! plus exact backup/staging basenames (version + nonce). Updates-root deletes
//! must resolve under the real app-data `updates/` tree. Restore token, platform,
//! and prepared id must match process-bound values when present.

use super::updates_root;
use crate::updater::paths::{
    remove_trusted_updates_relative as paths_remove_trusted_updates_relative,
    resolve_trusted_updates_root as paths_resolve_trusted_updates_root,
    updates_root_exists_and_trusted as paths_updates_root_exists_and_trusted,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const INSTALL_MANIFEST_FILE: &str = "install-manifest.json";
const MANIFEST_PART_NAME: &str = "install-manifest.json.part";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallManifest {
    pub prepared_id: String,
    pub platform: String,
    pub target_version: String,
    /// Random short nonce embedded in backup/staging basenames.
    pub install_nonce: String,
    /// Absolute parent directory of the live install (EXE dir or .app parent).
    /// Diagnostic only — ACK cleanup re-derives parent from current_exe().
    pub install_parent: String,
    /// Exact backup basename (e.g. Resh.backup.v1.2.3.abcd1234.exe|.app).
    pub backup_name: String,
    /// Exact staging basename when used (macOS Resh.staging.v….app); empty on Windows.
    #[serde(default)]
    pub staging_name: String,
    /// Absolute paths recorded at spawn for updates-root artifacts (alive marker, helper, DMG).
    pub paths: Vec<String>,
    /// Restore token that authorized this install; required at ACK for binding.
    #[serde(default)]
    pub restore_token: String,
}

pub fn install_manifest_path(app_data_dir: &Path) -> PathBuf {
    updates_root(app_data_dir).join(INSTALL_MANIFEST_FILE)
}

/// Atomically write the install manifest (part + rename). Restrictive mode on Unix.
pub fn write_install_manifest(app_data_dir: &Path, manifest: &InstallManifest) -> Result<(), String> {
    if !manifest_fields_are_sane(manifest) {
        return Err("Install manifest failed internal safety checks".to_string());
    }
    let json =
        serde_json::to_string_pretty(manifest).map_err(|e| format!("serialize manifest: {e}"))?;
    crate::updater::paths::write_trusted_updates_file_atomic(
        app_data_dir,
        Path::new(INSTALL_MANIFEST_FILE),
        json.as_bytes(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = install_manifest_path(app_data_dir);
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&path, perms);
        }
    }
    Ok(())
}

pub fn load_install_manifest(app_data_dir: &Path) -> Option<InstallManifest> {
    let path = install_manifest_path(app_data_dir);
    let raw = fs::read_to_string(&path).ok()?;
    let manifest: InstallManifest = serde_json::from_str(&raw).ok()?;
    if !manifest_fields_are_sane(&manifest) {
        tracing::warn!("ignoring install manifest with unsafe fields");
        return None;
    }
    Some(manifest)
}

pub fn clear_install_manifest(app_data_dir: &Path) {
    // Never follow a symlink-escaped updates root when removing the manifest.
    // Require an *existing* real updates/ directory — a missing path is not
    // treated as trusted for delete (it could be replaced by a symlink before
    // open, and remove_file would then follow intermediate links).
    if !remove_trusted_updates_relative(app_data_dir, Path::new(INSTALL_MANIFEST_FILE)) {
        tracing::warn!("skipping install-manifest clear: untrusted updates directory");
    }
    let _ = remove_trusted_updates_relative(app_data_dir, Path::new(MANIFEST_PART_NAME));
}

/// Remove a relative path under a currently trusted app-data `updates/` tree.
///
/// `relative` may only be a known safe leaf under updates/ (manifest, helpers, …).
/// Returns true if the updates root was trusted and the unlink was attempted
/// (or the leaf was already absent). Returns false when the updates root is
/// missing, a symlink/junction, or escapes app-data — callers must not fall
/// back to an unprotected delete.
pub fn remove_trusted_updates_relative(app_data_dir: &Path, relative: &Path) -> bool {
    paths_remove_trusted_updates_relative(app_data_dir, relative)
}

/// True only when `updates` currently exists as a non-symlink directory whose
/// canonical path is under app-data `updates/`. Missing paths are **not** trusted
/// for delete operations.
fn updates_root_exists_and_trusted(app_data_dir: &Path, _updates: &Path) -> bool {
    paths_updates_root_exists_and_trusted(app_data_dir)
}

/// Optional binding for ack-time cleanup. When provided, paths outside these
/// constraints are skipped even if present in the on-disk manifest.
#[derive(Debug, Clone, Default)]
pub struct CleanupBinding {
    /// Current live install parent (EXE directory or parent of Resh.app).
    /// **Required** for install-parent deletes; never fall back to manifest parent.
    pub live_install_parent: Option<PathBuf>,
    /// Prepared update id from the frontend (if any).
    pub prepared_id: Option<String>,
    /// Expected platform label (`windows` / `macos`).
    pub platform: Option<String>,
    /// Restore token still pending in this process (must match manifest).
    pub restore_token: Option<String>,
}

/// Remove only re-derived safe paths after strict binding checks.
///
/// Security model: the on-disk manifest is an **untrusted index**. Install-parent
/// deletes require a live parent from `current_exe()` and only use the exact
/// backup/staging basenames after field sanity + binding checks. Manifest
/// `install_parent` is never used as a delete root or parent-match oracle.
pub fn cleanup_manifest_paths(
    app_data_dir: &Path,
    manifest: &InstallManifest,
    binding: &CleanupBinding,
) {
    if !manifest_fields_are_sane(manifest) {
        tracing::warn!("skipping install cleanup: unsafe manifest fields");
        clear_install_manifest(app_data_dir);
        return;
    }

    if let Some(ref expected_platform) = binding.platform {
        if !manifest.platform.eq_ignore_ascii_case(expected_platform) {
            tracing::warn!("skipping install cleanup: platform mismatch");
            clear_install_manifest(app_data_dir);
            return;
        }
    }
    if let Some(ref prepared_id) = binding.prepared_id {
        if !prepared_id.is_empty() && prepared_id != &manifest.prepared_id {
            tracing::warn!("skipping install cleanup: prepared_id mismatch");
            clear_install_manifest(app_data_dir);
            return;
        }
    }
    // Restore token is mandatory for cleanup. Empty or mismatched token means the
    // process cannot prove it owns this install attempt.
    if manifest.restore_token.is_empty() {
        tracing::warn!("skipping install cleanup: empty restore_token in manifest");
        clear_install_manifest(app_data_dir);
        return;
    }
    match binding.restore_token.as_deref() {
        Some(t) if t == manifest.restore_token => {}
        Some(_) => {
            tracing::warn!("skipping install cleanup: restore_token mismatch");
            clear_install_manifest(app_data_dir);
            return;
        }
        None => {
            tracing::warn!("skipping install cleanup: missing restore_token binding");
            clear_install_manifest(app_data_dir);
            return;
        }
    }

    let updates = updates_root(app_data_dir);
    let Some(updates_canon) = resolve_trusted_updates_root(app_data_dir, &updates) else {
        tracing::warn!("skipping updates-root cleanup: untrusted or missing updates directory");
        // Still allow install-parent cleanup when live parent is known.
        cleanup_install_parent_only(app_data_dir, manifest, binding);
        return;
    };

    // Install-parent deletes ONLY when live parent is provided by the running binary.
    let live_parent = match &binding.live_install_parent {
        Some(p) if !p.as_os_str().is_empty() => p.clone(),
        _ => {
            tracing::warn!(
                "skipping install-parent cleanup: live install parent required (untrusted manifest)"
            );
            for path in allowed_updates_root_paths(manifest, &updates_canon) {
                // No trusted live parent — only updates-root candidates may be removed.
                remove_path_if_still_allowed(
                    app_data_dir,
                    &path,
                    manifest,
                    Path::new("/__resh_no_live_parent__"),
                );
            }
            clear_install_manifest(app_data_dir);
            return;
        }
    };
    let live_parent_canon = fs::canonicalize(&live_parent).unwrap_or_else(|_| live_parent.clone());

    // 1) Exact re-derived install-parent artifacts from LIVE parent only.
    //    Never join against manifest.install_parent.
    let mut allowed: Vec<PathBuf> = Vec::new();
    allowed.push(live_parent_canon.join(&manifest.backup_name));
    if !manifest.staging_name.is_empty() {
        allowed.push(live_parent_canon.join(&manifest.staging_name));
    }

    // 2) Staged package next to the live install (Windows ready EXE).
    //    Parent must match live parent; names must include prepared_id + version.
    for raw in &manifest.paths {
        let candidate = PathBuf::from(raw);
        if is_allowed_co_located_staged(&candidate, &live_parent_canon, manifest) {
            allowed.push(candidate);
        }
    }

    // 3) Updates-root only: re-root by basename under trusted updates/ (ignore absolute
    //    parents from the untrusted manifest index to defeat path rebinding).
    allowed.extend(allowed_updates_root_paths(manifest, &updates_canon));

    // Dedup while preserving order.
    let mut seen = std::collections::HashSet::new();
    for path in allowed {
        let key = path.to_string_lossy().to_string();
        if !seen.insert(key) {
            continue;
        }
        if !is_final_path_allowed(&path, manifest, &updates_canon, &live_parent_canon) {
            tracing::warn!("skipping unsafe install cleanup path");
            continue;
        }
        // Re-validate immediately before unlink to shrink TOCTOU (symlink/junction swap).
        remove_path_if_still_allowed(app_data_dir, &path, manifest, &live_parent_canon);
    }

    clear_install_manifest(app_data_dir);
}

/// When updates/ cannot be trusted, still clean live-parent backup/staging only.
fn cleanup_install_parent_only(
    app_data_dir: &Path,
    manifest: &InstallManifest,
    binding: &CleanupBinding,
) {
    let Some(live_parent) = binding.live_install_parent.as_ref() else {
        clear_install_manifest(app_data_dir);
        return;
    };
    if live_parent.as_os_str().is_empty() {
        clear_install_manifest(app_data_dir);
        return;
    }
    let live_parent_canon = fs::canonicalize(live_parent).unwrap_or_else(|_| live_parent.clone());
    let dummy_updates = PathBuf::from("/__resh_no_updates__");
    let mut allowed = vec![live_parent_canon.join(&manifest.backup_name)];
    if !manifest.staging_name.is_empty() {
        allowed.push(live_parent_canon.join(&manifest.staging_name));
    }
    for raw in &manifest.paths {
        let candidate = PathBuf::from(raw);
        if is_allowed_co_located_staged(&candidate, &live_parent_canon, manifest) {
            allowed.push(candidate);
        }
    }
    let mut seen = std::collections::HashSet::new();
    for path in allowed {
        let key = path.to_string_lossy().to_string();
        if !seen.insert(key) {
            continue;
        }
        if !is_final_path_allowed(&path, manifest, &dummy_updates, &live_parent_canon) {
            continue;
        }
        remove_path_if_still_allowed(app_data_dir, &path, manifest, &live_parent_canon);
    }
    clear_install_manifest(app_data_dir);
}

/// Public wrapper used by platform cleanup fallbacks (Windows legacy helper path).
///
/// Unlike [`resolve_trusted_updates_root`], this requires the directory to **exist**
/// as a real (non-symlink) directory — a missing path is not trusted for deletes.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn updates_root_is_trusted(app_data_dir: &Path, updates: &Path) -> bool {
    updates_root_exists_and_trusted(app_data_dir, updates)
}

/// Resolve app-data `updates/` only when it currently exists as a real directory
/// under app-data (not a symlink escape hatch to an external tree).
///
/// **Missing directories are never trusted for cleanup.** Returning a logical
/// path for a non-existent `updates/` would re-root manifest candidates under a
/// name that an attacker can later replace with a symlink/junction before the
/// non-atomic check-then-delete window closes.
fn resolve_trusted_updates_root(app_data_dir: &Path, _updates: &Path) -> Option<PathBuf> {
    paths_resolve_trusted_updates_root(app_data_dir, false)
}

/// Re-resolve trust roots and re-check allow-list immediately before delete.
/// Updates-root artifacts are removed via [`remove_trusted_updates_relative`]
/// (Unix: directory FD + unlinkat). Live-parent backup/staging uses
/// [`crate::updater::paths::remove_live_parent_artifact_nofollow`] (Unix:
/// openat/unlinkat recursive, no path-string `read_dir` follow).
fn remove_path_if_still_allowed(
    app_data_dir: &Path,
    path: &Path,
    manifest: &InstallManifest,
    live_parent_canon: &Path,
) {
    // Re-resolve trusted updates root at delete time.
    let updates = updates_root(app_data_dir);
    let updates_ok = resolve_trusted_updates_root(app_data_dir, &updates);
    let Some(updates_canon) = updates_ok else {
        // Updates root no longer trusted — only allow live-parent exact artifacts.
        if path_is_exact_live_artifact(path, manifest, live_parent_canon)
            || is_allowed_co_located_staged(path, live_parent_canon, manifest)
        {
            if let Some(parent) = path.parent() {
                if !parent_matches_live(parent, live_parent_canon) {
                    return;
                }
                if let Ok(meta) = fs::symlink_metadata(parent) {
                    if meta.file_type().is_symlink() {
                        return;
                    }
                }
            }
            let _ = crate::updater::paths::remove_live_parent_artifact_nofollow(
                live_parent_canon,
                path,
            );
        } else {
            tracing::warn!("skipping cleanup after updates-root revalidation failure");
        }
        return;
    };

    // Reject if updates root itself became a symlink since allow-list build.
    if let Ok(meta) = fs::symlink_metadata(&updates) {
        if meta.file_type().is_symlink() {
            tracing::warn!("skipping cleanup: updates root is symlink at delete time");
            return;
        }
    }
    if let Ok(meta) = fs::symlink_metadata(&updates_canon) {
        if meta.file_type().is_symlink() {
            tracing::warn!("skipping cleanup: canonical updates root is symlink");
            return;
        }
    }

    // Reject if any intermediate component under updates is a symlink now.
    if path_has_symlink_component_under_root(path, &updates_canon) {
        tracing::warn!("skipping cleanup path with symlink component under updates");
        return;
    }

    // Live parent: re-canonicalize binding parent and require parent match again.
    let live_now = binding_live_parent_still_matches(path, live_parent_canon);

    if !is_final_path_allowed(path, manifest, &updates_canon, live_parent_canon) {
        tracing::warn!("skipping cleanup path after revalidation");
        return;
    }

    // If this path claims to be under live parent, require the parent still matches
    // after a fresh canonicalize of the parent directory (when it exists).
    if let Some(parent) = path.parent() {
        if parent_matches_live(parent, live_parent_canon) {
            if !live_now {
                // Parent existed as live earlier but no longer matches (swap/redirect).
                if !is_allowed_updates_root_path(path, &updates_canon) {
                    tracing::warn!("skipping live-parent path after parent revalidation");
                    return;
                }
            } else if let Ok(meta) = fs::symlink_metadata(parent) {
                // Never delete through a live-parent that became a symlink.
                if meta.file_type().is_symlink() {
                    tracing::warn!("skipping live-parent path: parent is symlink");
                    return;
                }
            }
        }
    }

    // Prefer allowlisted relative deletes under trusted updates/ (no string-path
    // remove that can re-resolve through a mid-race directory symlink).
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name == "apply-windows-update.ps1" {
            if remove_trusted_updates_relative(
                app_data_dir,
                Path::new("helpers/apply-windows-update.ps1"),
            ) {
                return;
            }
            tracing::warn!("skipping helper cleanup: trusted relative remove refused");
            return;
        }
        if updates_artifact_name_ok(name)
            && (is_allowed_updates_root_path(path, &updates_canon)
                || path
                    .parent()
                    .is_some_and(|p| p == updates_canon || p == updates.as_path()))
        {
            if !updates_root_exists_and_trusted(app_data_dir, &updates) {
                tracing::warn!("skipping updates-root path: updates directory untrusted");
                return;
            }
            if remove_trusted_updates_relative(app_data_dir, Path::new(name)) {
                return;
            }
            tracing::warn!("skipping updates-root cleanup: trusted relative remove refused");
            return;
        }
    }

    // Live-parent backup/staging / Windows co-located staged EXE only.
    if path_is_exact_live_artifact(path, manifest, live_parent_canon)
        || is_allowed_co_located_staged(path, live_parent_canon, manifest)
    {
        if !updates_root_exists_and_trusted(app_data_dir, &updates)
            && !path_is_exact_live_artifact(path, manifest, live_parent_canon)
            && !is_allowed_co_located_staged(path, live_parent_canon, manifest)
        {
            return;
        }
        let _ =
            crate::updater::paths::remove_live_parent_artifact_nofollow(live_parent_canon, path);
        return;
    }

    tracing::warn!("skipping cleanup path: not a trusted updates or live-parent artifact");
}

fn path_is_exact_live_artifact(
    path: &Path,
    manifest: &InstallManifest,
    live_parent_canon: &Path,
) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name != manifest.backup_name && name != manifest.staging_name {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    parent_matches_live(parent, live_parent_canon)
}

fn binding_live_parent_still_matches(path: &Path, live_parent_canon: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if parent_matches_live(parent, live_parent_canon) {
        return true;
    }
    // Fresh canonicalize of parent if present.
    if parent.exists() {
        if let Ok(p) = fs::canonicalize(parent) {
            return p == *live_parent_canon;
        }
    }
    false
}

/// True if any path component from updates root down to the leaf is a symlink.
fn path_has_symlink_component_under_root(path: &Path, updates_canon: &Path) -> bool {
    // Prefer logical updates path when the candidate is under the non-canonical tree.
    let updates = if path.starts_with(updates_canon) {
        updates_canon
    } else {
        // Fall back: only meaningful when path is under some updates tree.
        updates_canon
    };
    crate::updater::paths::path_has_symlink_component_under_root(path, updates)
}

fn allowed_updates_root_paths(manifest: &InstallManifest, updates_canon: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in &manifest.paths {
        let candidate = PathBuf::from(raw);
        let Some(name) = candidate.file_name() else {
            continue;
        };
        let name_str = name.to_string_lossy();
        if name_str.is_empty()
            || name_str.contains('/')
            || name_str.contains('\\')
            || name_str.contains("..")
        {
            continue;
        }

        // Prefer re-rooting under trusted updates/. Nested helpers/ is only used for
        // the known Windows PowerShell helper basename.
        let rerooted = if name_str == "apply-windows-update.ps1" {
            updates_canon.join("helpers").join(name)
        } else {
            updates_canon.join(name)
        };

        if is_allowed_updates_root_path(&rerooted, updates_canon) {
            let key = rerooted.to_string_lossy().to_string();
            if seen.insert(key) {
                out.push(rerooted);
            }
            continue;
        }
        // Fallback for already-correct absolute paths under trusted updates.
        if is_allowed_updates_root_path(&candidate, updates_canon) {
            let key = candidate.to_string_lossy().to_string();
            if seen.insert(key) {
                out.push(candidate);
            }
        }
    }
    out
}

fn parent_matches_live(parent: &Path, live_parent_canon: &Path) -> bool {
    // Never treat a symlinked parent as the live install parent.
    if path_entry_is_symlink(parent) {
        return false;
    }
    if let Ok(p) = fs::canonicalize(parent) {
        // Canonical form must match the binding parent and must not be a symlink node.
        if path_entry_is_symlink(&p) {
            return false;
        }
        if p == *live_parent_canon {
            return true;
        }
    }
    // Equality only after symlink rejection — covers missing paths in unit tests
    // where canonicalize fails but the binding path is still the intended parent.
    if parent == live_parent_canon {
        return true;
    }
    let a = parent.to_string_lossy();
    let b = live_parent_canon.to_string_lossy();
    a == b
        || a.trim_end_matches('/') == b.trim_end_matches('/')
        || a.trim_end_matches('\\') == b.trim_end_matches('\\')
}

fn manifest_fields_are_sane(manifest: &InstallManifest) -> bool {
    if manifest.prepared_id.trim().is_empty()
        || manifest.install_nonce.trim().is_empty()
        || manifest.install_parent.trim().is_empty()
        || manifest.backup_name.trim().is_empty()
        || manifest.restore_token.trim().is_empty()
    {
        return false;
    }
    if manifest.install_parent.contains("..")
        || !Path::new(&manifest.install_parent).is_absolute()
    {
        return false;
    }
    if !is_safe_artifact_basename(&manifest.backup_name, "backup", manifest)
        || (!manifest.staging_name.is_empty()
            && !is_safe_artifact_basename(&manifest.staging_name, "staging", manifest))
    {
        return false;
    }
    match manifest.platform.as_str() {
        "windows" | "macos" => true,
        _ => false,
    }
}

fn is_safe_artifact_basename(name: &str, kind: &str, manifest: &InstallManifest) -> bool {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name == "Resh.app"
        || name.eq_ignore_ascii_case("Resh.exe")
    {
        return false;
    }
    if !name.contains(&manifest.install_nonce) {
        return false;
    }
    // Version fragment must appear (sanitized) after the v marker when possible.
    let ver = manifest.target_version.trim().trim_start_matches('v');
    if !ver.is_empty() && !name.contains(ver) {
        return false;
    }
    match kind {
        "backup" => {
            name.starts_with("Resh.backup.v")
                && (name.ends_with(".app") || name.ends_with(".exe"))
        }
        "staging" => name.starts_with("Resh.staging.v") && name.ends_with(".app"),
        _ => false,
    }
}

fn is_allowed_co_located_staged(
    path: &Path,
    live_parent_canon: &Path,
    manifest: &InstallManifest,
) -> bool {
    if !path.is_absolute() || path.to_string_lossy().contains("..") {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    if !parent_matches_live(parent, live_parent_canon) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name == "Resh.app" || name.eq_ignore_ascii_case("Resh.exe") {
        return false;
    }
    // Windows staged ready package next to the EXE.
    // Name is `{prepared_id}.Resh-…exe` from download staging — bind to prepared_id
    // (already matched via CleanupBinding), never to untrusted install_parent.
    if manifest.platform == "windows" {
        let ver = manifest.target_version.trim().trim_start_matches('v');
        return name.contains(".Resh-")
            && name.ends_with(".exe")
            && name.contains(&manifest.prepared_id)
            && (ver.is_empty() || name.contains(ver));
    }
    // macOS staged DMG is normally under updates/; co-located DMG is unusual.
    // Require prepared_id + version so a forged paths entry cannot delete
    // arbitrary .dmg files next to the live app.
    if manifest.platform == "macos" {
        let ver = manifest.target_version.trim().trim_start_matches('v');
        return name.ends_with(".dmg")
            && name.contains(&manifest.prepared_id)
            && (ver.is_empty() || name.contains(ver));
    }
    false
}

fn is_allowed_updates_root_path(path: &Path, updates_canon: &Path) -> bool {
    if !path.is_absolute() || path.to_string_lossy().contains("..") {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !updates_artifact_name_ok(name) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };

    // Direct child of trusted updates/, or updates/helpers/<known helper>.
    if parent_is_trusted_updates(parent, updates_canon) {
        return true;
    }

    // Nested helpers directory (Windows static PowerShell helper only).
    if name == "apply-windows-update.ps1" {
        if let Some(helpers_name) = parent.file_name().and_then(|n| n.to_str()) {
            if helpers_name == "helpers" {
                if let Some(updates_parent) = parent.parent() {
                    if parent_is_trusted_updates(updates_parent, updates_canon)
                        && !path_entry_is_symlink(parent)
                    {
                        return true;
                    }
                }
            }
        }
    }

    false
}

fn updates_artifact_name_ok(name: &str) -> bool {
    name.ends_with(".dmg")
        || (name.contains(".Resh-") && name.ends_with(".exe"))
        || name.starts_with("install-alive-")
        || name.ends_with(".ps1")
        || name.ends_with(".sh")
        || name.ends_with(".part")
        || name.ends_with(".ready")
}

fn path_entry_is_symlink(path: &Path) -> bool {
    crate::updater::paths::path_entry_is_symlink(path)
}

/// Parent is exactly the trusted updates root (not a symlink, not a sibling prefix).
fn parent_is_trusted_updates(parent: &Path, updates_canon: &Path) -> bool {
    if path_entry_is_symlink(parent) {
        return false;
    }
    if parent == updates_canon {
        return true;
    }
    if let Ok(pc) = fs::canonicalize(parent) {
        return pc == *updates_canon && !path_entry_is_symlink(parent);
    }
    false
}

fn is_final_path_allowed(
    path: &Path,
    manifest: &InstallManifest,
    updates_canon: &Path,
    live_parent_canon: &Path,
) -> bool {
    let s = path.to_string_lossy();
    if s.is_empty() || !path.is_absolute() || s.contains("..") {
        return false;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() || name == "Resh.app" || name.eq_ignore_ascii_case("Resh.exe") {
        return false;
    }

    if let Some(parent) = path.parent() {
        if parent_matches_live(parent, live_parent_canon) {
            if name == manifest.backup_name || name == manifest.staging_name {
                // Basenames were already validated by manifest_fields_are_sane
                // (nonce + version + prefix). Live parent is the only root.
                return true;
            }
            return is_allowed_co_located_staged(path, live_parent_canon, manifest);
        }
    }

    is_allowed_updates_root_path(path, updates_canon)
}

/// Public helper for unit tests and callers that only have path-level checks.
#[cfg(test)]
fn is_safe_cleanup_path(
    path: &Path,
    manifest: &InstallManifest,
    updates_canon: &Path,
    install_parent_canon: &Path,
) -> bool {
    is_final_path_allowed(path, manifest, updates_canon, install_parent_canon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> InstallManifest {
        InstallManifest {
            prepared_id: "abc".into(),
            platform: "macos".into(),
            target_version: "1.2.3".into(),
            install_nonce: "abcd1234".into(),
            install_parent: "/Applications".into(),
            backup_name: "Resh.backup.v1.2.3.abcd1234.app".into(),
            staging_name: "Resh.staging.v1.2.3.abcd1234.app".into(),
            paths: vec![
                "/Applications/Resh.backup.v1.2.3.abcd1234.app".into(),
                "/Applications/Resh.staging.v1.2.3.abcd1234.app".into(),
                "/tmp/updates/install-alive-token.ready".into(),
            ],
            restore_token: "restore-token-1".into(),
        }
    }

    #[test]
    fn write_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let m = sample_manifest();
        write_install_manifest(dir.path(), &m).unwrap();
        let loaded = load_install_manifest(dir.path()).unwrap();
        assert_eq!(loaded.prepared_id, "abc");
        assert_eq!(loaded.install_nonce, "abcd1234");
        assert_eq!(loaded.paths.len(), 3);
        clear_install_manifest(dir.path());
        assert!(load_install_manifest(dir.path()).is_none());
    }

    #[test]
    fn rejects_live_app_name() {
        let m = sample_manifest();
        let updates = PathBuf::from("/tmp/updates");
        let parent = PathBuf::from("/Applications");
        assert!(!is_safe_cleanup_path(
            Path::new("/Applications/Resh.app"),
            &m,
            &updates,
            &parent
        ));
    }

    #[test]
    fn allows_exact_backup_name_only() {
        let m = sample_manifest();
        let updates = PathBuf::from("/tmp/updates");
        let parent = PathBuf::from("/Applications");
        assert!(is_safe_cleanup_path(
            Path::new("/Applications/Resh.backup.v1.2.3.abcd1234.app"),
            &m,
            &updates,
            &parent
        ));
        // Same prefix pattern but wrong nonce — reject.
        assert!(!is_safe_cleanup_path(
            Path::new("/Applications/Resh.backup.v1.2.3.deadbeef.app"),
            &m,
            &updates,
            &parent
        ));
        // User directory with matching-looking name but wrong parent — reject.
        assert!(!is_safe_cleanup_path(
            Path::new("/Users/alice/Resh.backup.v1.2.3.abcd1234.app"),
            &m,
            &updates,
            &parent
        ));
    }

    #[test]
    fn rejects_forged_external_path_even_if_listed() {
        let mut m = sample_manifest();
        m.paths
            .push("/Users/alice/Documents/Resh.backup.v1.2.3.abcd1234.app".into());
        let binding = CleanupBinding {
            live_install_parent: Some(PathBuf::from("/Applications")),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        // Should not panic; forged path must not be deleted (we only assert allow-list).
        let updates = PathBuf::from("/tmp/updates");
        assert!(!is_final_path_allowed(
            Path::new("/Users/alice/Documents/Resh.backup.v1.2.3.abcd1234.app"),
            &m,
            &updates,
            Path::new("/Applications"),
        ));
        let _ = binding;
    }

    #[test]
    fn forged_install_parent_cannot_delete_outside_live_parent() {
        let dir = tempfile::tempdir().unwrap();
        let live_parent = dir.path().join("live");
        let evil_parent = dir.path().join("evil");
        fs::create_dir_all(&live_parent).unwrap();
        fs::create_dir_all(&evil_parent).unwrap();
        let evil_target = evil_parent.join("Resh.backup.v1.2.3.abcd1234.app");
        fs::create_dir_all(&evil_target).unwrap();
        let sentinel = evil_target.join("sentinel.txt");
        fs::write(&sentinel, b"keep").unwrap();

        let mut m = sample_manifest();
        // Forged: claim install_parent is evil dir and list that path.
        m.install_parent = evil_parent.to_string_lossy().to_string();
        m.paths = vec![evil_target.to_string_lossy().to_string()];
        write_install_manifest(dir.path(), &m).unwrap();

        let binding = CleanupBinding {
            // Live process is actually under live_parent.
            live_install_parent: Some(live_parent.clone()),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(dir.path(), &m, &binding);
        // External forged path must survive.
        assert!(sentinel.exists());
        assert!(evil_target.exists());
    }

    /// Even if attacker rewrites install_parent + backup_name + paths together,
    /// only the live parent is used as the delete root.
    #[test]
    fn forged_parent_and_backup_name_still_cannot_delete_external() {
        let dir = tempfile::tempdir().unwrap();
        let live_parent = dir.path().join("live");
        let evil_parent = dir.path().join("evil");
        fs::create_dir_all(&live_parent).unwrap();
        fs::create_dir_all(&evil_parent).unwrap();
        // Attacker-chosen basename that still passes basename rules (nonce+ver).
        let evil_name = "Resh.backup.v1.2.3.abcd1234.app";
        let evil_target = evil_parent.join(evil_name);
        fs::create_dir_all(&evil_target).unwrap();
        let sentinel = evil_target.join("do-not-delete.txt");
        fs::write(&sentinel, b"keep").unwrap();
        // Also place a co-located looking staged file outside live parent.
        let evil_staged = evil_parent.join("abc.Resh-v1.2.3-macos-aarch64.dmg");
        fs::write(&evil_staged, b"dmg").unwrap();

        let mut m = sample_manifest();
        m.install_parent = evil_parent.to_string_lossy().to_string();
        m.backup_name = evil_name.into();
        m.staging_name = "Resh.staging.v1.2.3.abcd1234.app".into();
        m.paths = vec![
            evil_target.to_string_lossy().to_string(),
            evil_staged.to_string_lossy().to_string(),
        ];
        write_install_manifest(dir.path(), &m).unwrap();

        let binding = CleanupBinding {
            live_install_parent: Some(live_parent.clone()),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(dir.path(), &m, &binding);
        assert!(sentinel.exists(), "forged backup under external parent must survive");
        assert!(evil_target.exists());
        assert!(evil_staged.exists(), "forged co-located staged path must survive");
    }

    #[test]
    fn legitimate_live_parent_backup_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let live_parent = dir.path().join("Applications");
        fs::create_dir_all(&live_parent).unwrap();
        let backup = live_parent.join("Resh.backup.v1.2.3.abcd1234.app");
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join("marker"), b"x").unwrap();

        let mut m = sample_manifest();
        // install_parent deliberately wrong — cleanup must still use live parent.
        m.install_parent = dir.path().join("wrong").to_string_lossy().to_string();
        m.paths = vec![];
        write_install_manifest(dir.path(), &m).unwrap();

        let binding = CleanupBinding {
            live_install_parent: Some(live_parent.clone()),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(dir.path(), &m, &binding);
        assert!(!backup.exists(), "exact backup under live parent should be cleaned");
    }

    #[test]
    fn windows_co_located_staged_requires_prepared_id_in_name() {
        let mut m = sample_manifest();
        m.platform = "windows".into();
        m.backup_name = "Resh.backup.v1.2.3.abcd1234.exe".into();
        m.staging_name.clear();
        m.prepared_id = "deadbeef-id".into();
        let live = PathBuf::from("/opt/resh");
        let updates = PathBuf::from("/tmp/updates");
        assert!(is_final_path_allowed(
            Path::new("/opt/resh/deadbeef-id.Resh-v1.2.3-windows-x86_64.exe"),
            &m,
            &updates,
            &live,
        ));
        // Looks like staged package but missing prepared_id fragment.
        assert!(!is_final_path_allowed(
            Path::new("/opt/resh/other.Resh-v1.2.3-windows-x86_64.exe"),
            &m,
            &updates,
            &live,
        ));
        // Correct name under forged external parent.
        assert!(!is_final_path_allowed(
            Path::new("/Users/alice/deadbeef-id.Resh-v1.2.3-windows-x86_64.exe"),
            &m,
            &updates,
            &live,
        ));
    }

    #[test]
    fn prepared_id_mismatch_skips_parent_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("Applications");
        fs::create_dir_all(&parent).unwrap();
        let backup = parent.join("Resh.backup.v1.2.3.abcd1234.app");
        fs::create_dir_all(&backup).unwrap();

        let mut m = sample_manifest();
        m.install_parent = parent.to_string_lossy().to_string();
        m.paths = vec![backup.to_string_lossy().to_string()];
        write_install_manifest(dir.path(), &m).unwrap();

        let binding = CleanupBinding {
            live_install_parent: Some(parent.clone()),
            prepared_id: Some("other-id".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(dir.path(), &m, &binding);
        // prepared_id mismatch → no delete of install-parent artifact.
        assert!(backup.exists());
    }

    #[test]
    fn missing_restore_token_binding_skips_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("Applications");
        fs::create_dir_all(&parent).unwrap();
        let backup = parent.join("Resh.backup.v1.2.3.abcd1234.app");
        fs::create_dir_all(&backup).unwrap();
        let mut m = sample_manifest();
        m.install_parent = parent.to_string_lossy().to_string();
        write_install_manifest(dir.path(), &m).unwrap();
        let binding = CleanupBinding {
            live_install_parent: Some(parent.clone()),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: None,
        };
        cleanup_manifest_paths(dir.path(), &m, &binding);
        assert!(backup.exists());
    }

    #[test]
    fn rejects_manifest_without_nonce_in_backup_name() {
        let mut m = sample_manifest();
        m.backup_name = "Resh.backup.v1.2.3.app".into();
        assert!(!manifest_fields_are_sane(&m));
    }

    #[cfg(unix)]
    #[test]
    fn updates_symlink_escape_does_not_delete_external() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let external = dir.path().join("external");
        fs::create_dir_all(&app_data).unwrap();
        fs::create_dir_all(&external).unwrap();
        let external_file = external.join("payload.dmg");
        fs::write(&external_file, b"keep-me").unwrap();
        // Point app-data/updates -> external
        let updates_link = app_data.join("updates");
        symlink(&external, &updates_link).unwrap();

        let mut m = sample_manifest();
        m.paths = vec![external_file.to_string_lossy().to_string()];
        // Writing through a symlink updates/ must fail closed (no forgeable index).
        assert!(write_install_manifest(&app_data, &m).is_err());

        let live = dir.path().join("Applications");
        fs::create_dir_all(&live).unwrap();
        let binding = CleanupBinding {
            live_install_parent: Some(live),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(&app_data, &m, &binding);
        assert!(
            external_file.exists(),
            "symlink-escaped updates root must not authorize external delete"
        );
        assert!(
            external_file.exists(),
            "external payload must remain after cleanup with symlink updates/"
        );
    }

    /// Review P1: forged install_parent + external .dmg/.exe path must not delete.
    #[test]
    fn forged_install_parent_cannot_delete_external_dmg_or_exe() {
        let dir = tempfile::tempdir().unwrap();
        let live_parent = dir.path().join("Applications");
        let evil_parent = dir.path().join("Documents");
        fs::create_dir_all(&live_parent).unwrap();
        fs::create_dir_all(&evil_parent).unwrap();
        let evil_dmg = evil_parent.join("archive-1.2.3.dmg");
        fs::write(&evil_dmg, b"keep-dmg").unwrap();
        let evil_exe = evil_parent.join("other.Resh-v1.2.3-windows-x86_64.exe");
        fs::write(&evil_exe, b"keep-exe").unwrap();

        // macOS branch: versioned .dmg under forged parent.
        let mut m = sample_manifest();
        m.install_parent = evil_parent.to_string_lossy().to_string();
        m.paths = vec![evil_dmg.to_string_lossy().to_string()];
        write_install_manifest(dir.path(), &m).unwrap();
        let binding = CleanupBinding {
            live_install_parent: Some(live_parent.clone()),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(dir.path(), &m, &binding);
        assert!(evil_dmg.exists(), "forged external .dmg must survive");

        // Windows branch: .Resh-*.exe under forged parent without prepared_id binding trick.
        let mut mw = sample_manifest();
        mw.platform = "windows".into();
        mw.backup_name = "Resh.backup.v1.2.3.abcd1234.exe".into();
        mw.staging_name.clear();
        mw.install_parent = evil_parent.to_string_lossy().to_string();
        mw.paths = vec![evil_exe.to_string_lossy().to_string()];
        write_install_manifest(dir.path(), &mw).unwrap();
        let binding_w = CleanupBinding {
            live_install_parent: Some(live_parent),
            prepared_id: Some("abc".into()),
            platform: Some("windows".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(dir.path(), &mw, &binding_w);
        assert!(evil_exe.exists(), "forged external .Resh-*.exe must survive");
    }

    #[test]
    fn updates_path_prefix_collision_is_not_authorized() {
        // "/tmp/updates" must not authorize "/tmp/updates-evil/x.dmg"
        let updates = PathBuf::from("/tmp/updates");
        let evil = Path::new("/tmp/updates-evil/payload.dmg");
        assert!(!is_allowed_updates_root_path(evil, &updates));
    }

    #[test]
    fn cleanup_without_caller_prepared_id_still_uses_live_parent_only() {
        // When frontend omits prepared_id, restore_token + live parent still bind;
        // forged external co-located paths must not delete.
        let dir = tempfile::tempdir().unwrap();
        let live_parent = dir.path().join("live");
        let evil_parent = dir.path().join("evil");
        fs::create_dir_all(&live_parent).unwrap();
        fs::create_dir_all(&evil_parent).unwrap();
        let live_backup = live_parent.join("Resh.backup.v1.2.3.abcd1234.app");
        fs::create_dir_all(&live_backup).unwrap();
        let evil_dmg = evil_parent.join("abc.Resh-v1.2.3-macos-aarch64.dmg");
        fs::write(&evil_dmg, b"keep").unwrap();

        let mut m = sample_manifest();
        m.install_parent = evil_parent.to_string_lossy().to_string();
        m.paths = vec![evil_dmg.to_string_lossy().to_string()];
        write_install_manifest(dir.path(), &m).unwrap();

        let binding = CleanupBinding {
            live_install_parent: Some(live_parent.clone()),
            prepared_id: None, // no caller-supplied prepared_id
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(dir.path(), &m, &binding);
        assert!(
            !live_backup.exists(),
            "exact backup under live parent should still clean with restore_token only"
        );
        assert!(
            evil_dmg.exists(),
            "external co-located staged path must not clean without live parent match"
        );
    }

    /// Deletion must not follow a directory symlink leaf (TOCTOU / planted link).
    #[cfg(unix)]
    #[test]
    fn does_not_follow_directory_symlink_when_deleting() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let live_parent = dir.path().join("Applications");
        let external = dir.path().join("external-secret");
        fs::create_dir_all(&live_parent).unwrap();
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("secret.txt");
        fs::write(&secret, b"keep").unwrap();

        // Plant a symlink at the exact backup path pointing at external.
        let backup_link = live_parent.join("Resh.backup.v1.2.3.abcd1234.app");
        symlink(&external, &backup_link).unwrap();

        let mut m = sample_manifest();
        m.install_parent = live_parent.to_string_lossy().to_string();
        m.paths = vec![];
        write_install_manifest(dir.path(), &m).unwrap();

        let binding = CleanupBinding {
            live_install_parent: Some(live_parent.clone()),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(dir.path(), &m, &binding);

        // Symlink node may be removed, but external contents must survive.
        assert!(secret.exists(), "must not follow symlink into external dir");
        assert!(external.exists());
    }

    /// If updates/ is a real dir at allow-list time but becomes a symlink before
    /// delete, revalidation must refuse and external payload must survive.
    #[cfg(unix)]
    #[test]
    fn updates_swapped_to_symlink_before_delete_does_not_escape() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let updates = app_data.join("updates");
        let external = dir.path().join("external");
        fs::create_dir_all(&updates).unwrap();
        fs::create_dir_all(&external).unwrap();
        let payload_name = "abc.Resh-v1.2.3-macos-aarch64.dmg";
        let internal = updates.join(payload_name);
        fs::write(&internal, b"internal").unwrap();
        let external_payload = external.join(payload_name);
        fs::write(&external_payload, b"external-keep").unwrap();

        let mut m = sample_manifest();
        m.paths = vec![internal.to_string_lossy().to_string()];
        write_install_manifest(&app_data, &m).unwrap();

        // Swap updates -> external between write and cleanup.
        // Clear residual files (including install-manifest.json) so the real
        // directory can be replaced by a symlink.
        for entry in fs::read_dir(&updates).unwrap() {
            let entry = entry.unwrap();
            let p = entry.path();
            if p.is_dir() {
                let _ = fs::remove_dir_all(&p);
            } else {
                let _ = fs::remove_file(&p);
            }
        }
        fs::remove_dir(&updates).unwrap();
        symlink(&external, &updates).unwrap();

        let live = dir.path().join("Applications");
        fs::create_dir_all(&live).unwrap();
        let binding = CleanupBinding {
            live_install_parent: Some(live),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(&app_data, &m, &binding);
        assert!(
            external_payload.exists(),
            "swapped updates symlink must not authorize deleting external payload"
        );
        // clear_install_manifest must also refuse the escaped root.
        let external_manifest = external.join(INSTALL_MANIFEST_FILE);
        fs::write(&external_manifest, b"{}").unwrap();
        clear_install_manifest(&app_data);
        assert!(
            external_manifest.exists(),
            "clear_install_manifest must not follow updates symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn clear_install_manifest_refuses_symlink_updates_root() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let external = dir.path().join("external");
        fs::create_dir_all(&app_data).unwrap();
        fs::create_dir_all(&external).unwrap();
        let external_manifest = external.join(INSTALL_MANIFEST_FILE);
        fs::write(&external_manifest, b"{}").unwrap();
        let external_part = external.join(MANIFEST_PART_NAME);
        fs::write(&external_part, b"{}").unwrap();
        symlink(&external, app_data.join("updates")).unwrap();
        clear_install_manifest(&app_data);
        assert!(external_manifest.exists());
        assert!(external_part.exists());
    }

    #[cfg(unix)]
    #[test]
    fn clear_install_manifest_refuses_missing_updates_then_symlinked() {
        // Missing updates/ must not authorize a later symlink-followed delete.
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let external = dir.path().join("external");
        fs::create_dir_all(&app_data).unwrap();
        fs::create_dir_all(&external).unwrap();
        let external_manifest = external.join(INSTALL_MANIFEST_FILE);
        fs::write(&external_manifest, b"keep").unwrap();
        // updates does not exist yet
        assert!(!app_data.join("updates").exists());
        // Attacker plants symlink after "would have been trusted if missing"
        symlink(&external, app_data.join("updates")).unwrap();
        clear_install_manifest(&app_data);
        assert!(
            external_manifest.exists(),
            "missing-then-symlink updates must not allow external manifest delete"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remove_trusted_updates_relative_helper_script_refuses_symlink_root() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let external = dir.path().join("external");
        let helpers_ext = external.join("helpers");
        fs::create_dir_all(&app_data).unwrap();
        fs::create_dir_all(&helpers_ext).unwrap();
        let external_script = helpers_ext.join("apply-windows-update.ps1");
        fs::write(&external_script, b"malicious").unwrap();
        symlink(&external, app_data.join("updates")).unwrap();
        let relative = PathBuf::from("helpers").join("apply-windows-update.ps1");
        assert!(!remove_trusted_updates_relative(&app_data, &relative));
        assert!(
            external_script.exists(),
            "legacy helper cleanup must not follow updates symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remove_trusted_updates_relative_helper_script_refuses_helpers_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let updates = app_data.join("updates");
        let external = dir.path().join("external");
        fs::create_dir_all(&updates).unwrap();
        fs::create_dir_all(&external).unwrap();
        let external_script = external.join("apply-windows-update.ps1");
        fs::write(&external_script, b"keep").unwrap();
        symlink(&external, updates.join("helpers")).unwrap();
        let relative = PathBuf::from("helpers").join("apply-windows-update.ps1");
        assert!(!remove_trusted_updates_relative(&app_data, &relative));
        assert!(external_script.exists());
    }

    #[cfg(unix)]
    #[test]
    fn remove_trusted_updates_relative_deletes_legitimate_helper() {
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let helpers = app_data.join("updates").join("helpers");
        fs::create_dir_all(&helpers).unwrap();
        let script = helpers.join("apply-windows-update.ps1");
        fs::write(&script, b"helper").unwrap();
        let relative = PathBuf::from("helpers").join("apply-windows-update.ps1");
        assert!(remove_trusted_updates_relative(&app_data, &relative));
        assert!(!script.exists());
    }

    #[cfg(unix)]
    #[test]
    fn app_data_symlink_root_refuses_updates_cleanup() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("install-manifest.json");
        fs::write(&secret, b"keep").unwrap();
        let logical_app = dir.path().join("logical-app");
        symlink(&external, &logical_app).unwrap();
        clear_install_manifest(&logical_app);
        assert!(secret.exists());
        assert!(!remove_trusted_updates_relative(
            &logical_app,
            Path::new("install-manifest.json")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_manifest_paths_refuses_missing_updates_root() {
        // Missing updates/ must not be treated as a trusted root for re-rooted deletes.
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let live = dir.path().join("Applications");
        let external = dir.path().join("external");
        fs::create_dir_all(&app_data).unwrap();
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&external).unwrap();
        let payload_name = "abc.Resh-v1.2.3-macos-aarch64.dmg";
        let external_payload = external.join(payload_name);
        fs::write(&external_payload, b"keep-external").unwrap();

        let mut m = sample_manifest();
        // Point paths at a logical updates location that does not exist yet.
        m.paths = vec![app_data
            .join("updates")
            .join(payload_name)
            .to_string_lossy()
            .to_string()];
        // Do not create updates/; plant a symlink only after allow-list would have
        // been built if missing roots were trusted.
        assert!(!app_data.join("updates").exists());
        symlink(&external, app_data.join("updates")).unwrap();

        let binding = CleanupBinding {
            live_install_parent: Some(live),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(&app_data, &m, &binding);
        assert!(
            external_payload.exists(),
            "missing-then-symlink updates must not authorize external dmg delete"
        );
    }

    #[cfg(unix)]
    #[test]
    fn legitimate_updates_root_file_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path().join("appdata");
        let updates = app_data.join("updates");
        fs::create_dir_all(&updates).unwrap();
        let alive = updates.join("install-alive-restore-token-1.ready");
        fs::write(&alive, b"alive").unwrap();
        let dmg = updates.join("abc.Resh-v1.2.3-macos-aarch64.dmg");
        fs::write(&dmg, b"dmg").unwrap();

        let mut m = sample_manifest();
        m.paths = vec![
            alive.to_string_lossy().to_string(),
            dmg.to_string_lossy().to_string(),
        ];
        write_install_manifest(&app_data, &m).unwrap();

        let live = dir.path().join("Applications");
        fs::create_dir_all(&live).unwrap();
        let binding = CleanupBinding {
            live_install_parent: Some(live),
            prepared_id: Some("abc".into()),
            platform: Some("macos".into()),
            restore_token: Some("restore-token-1".into()),
        };
        cleanup_manifest_paths(&app_data, &m, &binding);
        assert!(!alive.exists());
        assert!(!dmg.exists());
    }
}
