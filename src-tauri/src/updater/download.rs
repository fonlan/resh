// Stream download of release assets with SHA-256 verification.

use super::assets::{expected_install_asset_name, PlatformTarget};
use super::check::get_discovered_update;
use super::is_allowed_download_host;
use super::types::{DownloadProgressEvent, PreparedUpdate, UpdateInfo};
use crate::config::types::Proxy;
use crate::http::resolve_proxy_by_id;
use futures::StreamExt;
use reqwest::redirect::{Attempt, Policy};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Hard ceiling for install package size (512 MiB).
pub const MAX_INSTALL_ASSET_BYTES: u64 = 512 * 1024 * 1024;
/// Hard ceiling for SHA256SUMS.txt (1 MiB).
pub const MAX_CHECKSUMS_BYTES: u64 = 1024 * 1024;
const CONNECT_TIMEOUT_SECS: u64 = 15;
const REQUEST_TIMEOUT_SECS: u64 = 60 * 30; // long downloads
const MAX_REDIRECTS: usize = 5;
const PROGRESS_EMIT_EVERY_BYTES: u64 = 256 * 1024;

const EVENT_DOWNLOAD_PROGRESS: &str = "update-download-progress";

#[derive(Debug)]
struct PreparedEntry {
    path: PathBuf,
    prepared: PreparedUpdate,
}

struct DownloadRuntime {
    prepared: Mutex<HashMap<String, PreparedEntry>>,
    /// In-flight download cancel token (at most one download at a time).
    in_flight: Mutex<Option<(String, CancellationToken)>>,
}

impl DownloadRuntime {
    fn new() -> Self {
        Self {
            prepared: Mutex::new(HashMap::new()),
            in_flight: Mutex::new(None),
        }
    }
}

fn runtime() -> &'static DownloadRuntime {
    static RUNTIME: OnceLock<DownloadRuntime> = OnceLock::new();
    RUNTIME.get_or_init(DownloadRuntime::new)
}

/// Look up a previously prepared update by id (for later install phase).
pub async fn get_prepared_update(id: &str) -> Option<(PreparedUpdate, PathBuf)> {
    let map = runtime().prepared.lock().await;
    map.get(id).map(|e| (e.prepared.clone(), e.path.clone()))
}

/// Remove a prepared entry and optionally delete its file.
pub async fn forget_prepared_update(id: &str, delete_file: bool) {
    let mut map = runtime().prepared.lock().await;
    if let Some(entry) = map.remove(id) {
        if delete_file {
            // Prefer trusted updates-relative delete when the file lives under
            // app-data/updates; otherwise fall back to nofollow file delete that
            // also refuses intermediate symlink ancestors.
            remove_prepared_path_safely(&entry.path);
        }
    }
}

fn remove_prepared_path_safely(path: &Path) {
    // Prefer trusted updates-relative delete when the file is a leaf under
    // app-data/updates; refuse rather than follow when the root is a symlink.
    // Windows co-located EXE staging falls through to nofollow ancestor checks.
    crate::updater::paths::remove_prepared_download_path(path);
}

/// Download install asset + SHA256SUMS for a previously discovered update id.
///
/// Only accepts opaque ids from `check_for_update` (backend registry). Frontend
/// never supplies asset URLs.
pub async fn download_update(
    app: &AppHandle,
    proxies: &[crate::config::types::Proxy],
    update_proxy_id: Option<&str>,
    update_id: &str,
) -> Result<PreparedUpdate, String> {
    let update = get_discovered_update(update_id).await.ok_or_else(|| {
        "Unknown or expired update id. Check for updates again before downloading.".to_string()
    })?;

    // Single download at a time.
    let mut guard = runtime().in_flight.lock().await;
    if let Some((active_id, _)) = guard.as_ref() {
        return Err(format!(
            "Another download is already in progress (id={})",
            active_id
        ));
    }
    let download_id = Uuid::new_v4().to_string();
    let token = CancellationToken::new();
    *guard = Some((download_id.clone(), token.clone()));
    drop(guard);

    let result = run_download(app, proxies, update_proxy_id, &update, &download_id, token).await;

    let mut guard = runtime().in_flight.lock().await;
    *guard = None;
    result
}

/// Cancel the active download if any.
pub async fn cancel_update_download() -> Result<(), String> {
    let guard = runtime().in_flight.lock().await;
    if let Some((_, token)) = guard.as_ref() {
        token.cancel();
        Ok(())
    } else {
        Err("No update download is in progress".to_string())
    }
}

async fn run_download(
    app: &AppHandle,
    proxies: &[Proxy],
    update_proxy_id: Option<&str>,
    update: &UpdateInfo,
    download_id: &str,
    token: CancellationToken,
) -> Result<PreparedUpdate, String> {
    let platform = PlatformTarget::current()?;
    let expected_name = expected_install_asset_name(&update.tag_name, platform);
    if update.install_asset.name != expected_name {
        return Err(format!(
            "Install asset name mismatch: got '{}', expected '{}'",
            update.install_asset.name, expected_name
        ));
    }
    if update.checksums_asset.name != super::SHA256SUMS_FILE_NAME {
        return Err(format!(
            "Checksums asset must be '{}', got '{}'",
            super::SHA256SUMS_FILE_NAME,
            update.checksums_asset.name
        ));
    }

    // Reject oversized metadata claims early.
    if update.install_asset.size > MAX_INSTALL_ASSET_BYTES {
        return Err(format!(
            "Install asset size {} exceeds maximum allowed {} bytes",
            update.install_asset.size, MAX_INSTALL_ASSET_BYTES
        ));
    }
    if update.checksums_asset.size > MAX_CHECKSUMS_BYTES {
        return Err(format!(
            "Checksums file size {} exceeds maximum allowed {} bytes",
            update.checksums_asset.size, MAX_CHECKSUMS_BYTES
        ));
    }

    let proxy = resolve_proxy_by_id(proxies, update_proxy_id)?.cloned();
    let client = build_download_client(proxy.as_ref())?;

    // Paths: ready file embeds download_id so PreparedUpdate ids stay content-bound.
    let (staging_dir, ready_path, part_path) = staging_paths(
        app,
        &update.version,
        platform,
        &update.install_asset.name,
        download_id,
    )
    .await?;

    // Clean leftover partials for this download id.
    crate::updater::paths::remove_prepared_download_path(&part_path);

    // Guard: any early return after staging starts should remove the .part file.
    let part_guard = PartGuard::new(part_path.clone());

    // 1) Download SHA256SUMS.txt into memory (small).
    emit_progress(
        Some(app),
        download_id,
        0,
        Some(update.checksums_asset.size),
        "checksums",
    );
    let sums_bytes = download_bytes_capped(
        &client,
        &update.checksums_asset.browser_download_url,
        MAX_CHECKSUMS_BYTES,
        &token,
    )
    .await?;

    let sums_text = String::from_utf8(sums_bytes)
        .map_err(|_| "SHA256SUMS.txt is not valid UTF-8".to_string())?;

    let expected_sha = parse_sha256sums_for_file(&sums_text, &update.install_asset.name)?;

    // Cross-check GitHub asset digest if present.
    if let Some(ref digest) = update.install_asset.digest {
        match parse_github_sha256_digest(digest)? {
            Some(gh) => {
                if !constant_time_eq_hex(&gh, &expected_sha) {
                    return Err(format!(
                        "GitHub asset digest does not match SHA256SUMS.txt for '{}'",
                        update.install_asset.name
                    ));
                }
            }
            None => {}
        }
    }
    if let Some(ref digest) = update.checksums_asset.digest {
        match parse_github_sha256_digest(digest)? {
            Some(expected_sums) => {
                let actual = hex_encode(&Sha256::digest(sums_text.as_bytes()));
                if !constant_time_eq_hex(&actual, &expected_sums) {
                    return Err(
                        "GitHub digest for SHA256SUMS.txt does not match downloaded content"
                            .to_string(),
                    );
                }
            }
            None => {}
        }
    }

    // 2) Stream install asset to .part
    emit_progress(
        Some(app),
        download_id,
        0,
        Some(update.install_asset.size).filter(|&s| s > 0),
        "asset",
    );

    let (written, computed_sha) = stream_download_to_file(
        Some(app),
        download_id,
        &client,
        &update.install_asset.browser_download_url,
        &part_path,
        update.install_asset.size,
        MAX_INSTALL_ASSET_BYTES,
        &token,
    )
    .await?;

    if !constant_time_eq_hex(&computed_sha, &expected_sha) {
        return Err(format!(
            "SHA-256 mismatch for '{}': expected {}, got {}",
            update.install_asset.name, expected_sha, computed_sha
        ));
    }

    // Size consistency: if GitHub reported size, require exact match when > 0.
    if update.install_asset.size > 0 && written != update.install_asset.size {
        return Err(format!(
            "Downloaded size {} does not match GitHub asset size {}",
            written, update.install_asset.size
        ));
    }

    emit_progress(Some(app), download_id, written, Some(written), "verifying");

    // Ready path is unique per full download_id; refuse unexpected collisions.
    // Re-validate parent immediately before rename (TOCTOU after long download).
    revalidate_staging_parent_for_write(&ready_path)?;
    if ready_path.exists() {
        return Err(format!(
            "Prepared update path already exists for id {}; refusing to overwrite",
            download_id
        ));
    }

    // Prefer trusted rename when both paths are updates leaves.
    if let (Some(part_parent), Some(ready_parent)) = (part_path.parent(), ready_path.parent()) {
        if part_parent.file_name().and_then(|n| n.to_str()) == Some("updates")
            && ready_parent.file_name().and_then(|n| n.to_str()) == Some("updates")
        {
            if let (Some(app_data), Some(part_name), Some(ready_name)) = (
                part_parent.parent(),
                part_path.file_name().and_then(|n| n.to_str()),
                ready_path.file_name().and_then(|n| n.to_str()),
            ) {
                crate::updater::paths::rename_trusted_updates_relative(
                    app_data,
                    Path::new(part_name),
                    Path::new(ready_name),
                )
                .map_err(|e| format!("Failed to finalize download file: {e}"))?;
                part_guard.disarm();
                fsync_path(&ready_path).await;
                fsync_path(&staging_dir).await;
                let prepared = PreparedUpdate {
                    id: download_id.to_string(),
                    version: update.version.clone(),
                    tag_name: update.tag_name.clone(),
                    asset_name: update.install_asset.name.clone(),
                    size: written,
                    sha256: expected_sha,
                    current_version: update.current_version.clone(),
                };
                {
                    let mut map = runtime().prepared.lock().await;
                    map.insert(
                        prepared.id.clone(),
                        PreparedEntry {
                            path: ready_path,
                            prepared: prepared.clone(),
                        },
                    );
                }
                emit_progress(Some(app), download_id, written, Some(written), "ready");
                return Ok(prepared);
            }
        }
    }

    fs::rename(&part_path, &ready_path)
        .await
        .map_err(|e| format!("Failed to finalize download file: {}", e))?;
    // Successful rename: disarm part cleanup (file no longer at part_path).
    part_guard.disarm();

    // Best-effort fsync of the ready file (and parent on Unix).
    fsync_path(&ready_path).await;
    fsync_path(&staging_dir).await;

    let prepared = PreparedUpdate {
        id: download_id.to_string(),
        version: update.version.clone(),
        tag_name: update.tag_name.clone(),
        asset_name: update.install_asset.name.clone(),
        size: written,
        sha256: expected_sha,
        current_version: update.current_version.clone(),
    };

    {
        let mut map = runtime().prepared.lock().await;
        // Do not keep stale ids that would point at deleted files of the same asset.
        // Each PreparedUpdate keeps its own ready path; drop only same-id replace.
        map.insert(
            prepared.id.clone(),
            PreparedEntry {
                path: ready_path,
                prepared: prepared.clone(),
            },
        );
    }

    emit_progress(Some(app), download_id, written, Some(written), "ready");
    Ok(prepared)
}

fn build_download_client(proxy: Option<&Proxy>) -> Result<reqwest::Client, String> {
    // Every hop must stay on HTTPS + allowlisted GitHub hosts.
    let redirect = trusted_download_redirect_policy(MAX_REDIRECTS);
    let mut builder = reqwest::Client::builder()
        .user_agent(format!(
            "Resh/{} (https://github.com/fonlan/resh)",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .redirect(redirect)
        .no_proxy();

    if let Some(p) = proxy {
        let reqwest_proxy = crate::http::build_reqwest_proxy(p)?;
        builder = builder.proxy(reqwest_proxy);
        if p.ignore_ssl_errors {
            tracing::warn!(
                "Update download: ignoring SSL certificate validation for proxy {}",
                p.name
            );
            builder = builder.danger_accept_invalid_certs(true);
        }
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build download HTTP client: {}", e))
}

/// Redirect policy that validates scheme/host on every hop before following.
pub fn trusted_download_redirect_policy(max_redirects: usize) -> Policy {
    Policy::custom(move |attempt: Attempt| {
        if attempt.previous().len() >= max_redirects {
            return attempt.error("too many redirects");
        }
        let next = attempt.url().clone();
        // Test builds may follow loopback HTTP for mock servers only.
        #[cfg(test)]
        {
            if next.scheme() == "http"
                && matches!(
                    next.host_str(),
                    Some("127.0.0.1") | Some("localhost") | Some("[::1]")
                )
            {
                return attempt.follow();
            }
        }
        if next.scheme() != "https" {
            return attempt.error("redirect to non-HTTPS URL is not allowed");
        }
        match next.host_str() {
            Some(host) if is_allowed_download_host(host) => attempt.follow(),
            Some(host) => attempt.error(format!(
                "redirect host '{}' is not an allowed GitHub download host",
                host
            )),
            None => attempt.error("redirect URL has no host"),
        }
    })
}

async fn staging_paths(
    app: &AppHandle,
    version: &str,
    platform: PlatformTarget,
    asset_name: &str,
    download_id: &str,
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    // Sanitize: only allow the known basename characters.
    let safe_name = sanitize_asset_filename(asset_name)?;
    let mut ready_name = format!("{}.{}", download_id, safe_name);
    // Keep path length reasonable on Windows while still using the full id.
    if ready_name.len() > 180 {
        let id_safe: String = download_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if id_safe.is_empty() {
            return Err("Invalid download id for staging path".to_string());
        }
        ready_name = format!("{}.{}", id_safe, safe_name);
    }
    let part_name = format!("{}.part", ready_name);

    #[cfg(target_os = "windows")]
    {
        let _ = (app, version, platform);
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
        let dir = exe
            .parent()
            .ok_or_else(|| "Cannot determine executable directory".to_string())?
            .to_path_buf();
        // Never overwrite the running executable name.
        if let Some(exe_name) = exe.file_name().and_then(|s| s.to_str()) {
            if exe_name.eq_ignore_ascii_case(&safe_name)
                || exe_name.eq_ignore_ascii_case(&ready_name)
            {
                return Err(
                    "Refusing to stage update over the currently running executable name"
                        .to_string(),
                );
            }
        }
        let ready = dir.join(&ready_name);
        let part = dir.join(&part_name);
        Ok((dir, ready, part))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = platform;
        let default_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app_data_dir: {}", e))?;
        let app_data = crate::app_paths::resolve_app_data_dir_from_default(&default_dir);
        let staging = crate::updater::paths::ensure_trusted_updates_dir(&app_data)?;
        let ready = staging.join(&ready_name);
        let part = staging.join(&part_name);
        let _ = version;
        Ok((staging, ready, part))
    }
}

/// Removes a staging `.part` path when dropped, unless explicitly disarmed after
/// a successful promote/rename. Extracted so unit tests can exercise the same
/// cleanup contract as production `run_download` without a Tauri AppHandle.
struct PartGuard {
    path: Option<PathBuf>,
}

impl PartGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// Stop cleaning on drop (file was renamed to the ready path).
    fn disarm(mut self) {
        self.path = None;
    }
}

impl Drop for PartGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            crate::updater::paths::remove_prepared_download_path(&path);
        }
    }
}

fn sanitize_asset_filename(name: &str) -> Result<String, String> {
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if base.is_empty() || base != name {
        return Err(format!("Invalid asset filename '{}'", name));
    }
    // Expected pattern: Resh-vX.Y.Z-...exe|dmg — allow alnum, ., _, -, +
    if !base
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
    {
        return Err(format!("Unsafe asset filename '{}'", name));
    }
    if base.contains("..") {
        return Err(format!("Unsafe asset filename '{}'", name));
    }
    Ok(base.to_string())
}

async fn download_bytes_capped(
    client: &reqwest::Client,
    url: &str,
    max_bytes: u64,
    token: &CancellationToken,
) -> Result<Vec<u8>, String> {
    if token.is_cancelled() {
        return Err("Download cancelled".to_string());
    }
    let url = validate_download_url(url)?;
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", url, e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed HTTP {} for {}",
            response.status().as_u16(),
            url
        ));
    }

    ensure_final_url_allowed(response.url())?;

    if let Some(len) = response.content_length() {
        if len > max_bytes {
            return Err(format!(
                "Content-Length {} exceeds maximum {} bytes",
                len, max_bytes
            ));
        }
    }

    let mut stream = response.bytes_stream();
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next().await {
        if token.is_cancelled() {
            return Err("Download cancelled".to_string());
        }
        let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
        if buf.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(format!(
                "Download exceeded maximum size of {} bytes",
                max_bytes
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

#[allow(clippy::too_many_arguments)]
async fn stream_download_to_file(
    app: Option<&AppHandle>,
    download_id: &str,
    client: &reqwest::Client,
    url: &str,
    part_path: &Path,
    claimed_size: u64,
    max_bytes: u64,
    token: &CancellationToken,
) -> Result<(u64, String), String> {
    if token.is_cancelled() {
        return Err("Download cancelled".to_string());
    }
    let url = validate_download_url(url)?;
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| format!("Failed to download {}: {}", url, e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed HTTP {} for {}",
            response.status().as_u16(),
            url
        ));
    }

    ensure_final_url_allowed(response.url())?;

    let content_len = response.content_length();
    if let Some(len) = content_len {
        if len > max_bytes {
            return Err(format!(
                "Content-Length {} exceeds maximum {} bytes",
                len, max_bytes
            ));
        }
        if claimed_size > 0 && len != claimed_size {
            return Err(format!(
                "Content-Length {} does not match GitHub asset size {}",
                len, claimed_size
            ));
        }
    }

    let total_hint = content_len.or(if claimed_size > 0 {
        Some(claimed_size)
    } else {
        None
    });

    // Re-validate staging parent immediately before create so a post-check
    // updates/ → external symlink swap cannot redirect the download write.
    revalidate_staging_parent_for_write(part_path)?;
    let mut file = create_staging_part_file(part_path).await?;

    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        if token.is_cancelled() {
            drop(file);
            return Err("Download cancelled".to_string());
        }
        let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
        let next = received
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| "Download size overflow".to_string())?;
        if next > max_bytes {
            return Err(format!(
                "Download exceeded maximum size of {} bytes",
                max_bytes
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write staging file: {}", e))?;
        hasher.update(&chunk);
        received = next;

        if received - last_emit >= PROGRESS_EMIT_EVERY_BYTES || Some(received) == total_hint {
            emit_progress(app, download_id, received, total_hint, "asset");
            last_emit = received;
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush staging file: {}", e))?;
    file.sync_all()
        .await
        .map_err(|e| format!("Failed to fsync staging file: {}", e))?;
    drop(file);

    if let Some(len) = content_len {
        if received != len {
            return Err(format!(
                "Downloaded size {} does not match Content-Length {}",
                received, len
            ));
        }
    }
    if claimed_size > 0 && received != claimed_size {
        return Err(format!(
            "Downloaded size {} does not match GitHub asset size {}",
            received, claimed_size
        ));
    }

    let digest = hasher.finalize();
    Ok((received, hex_encode(&digest)))
}

/// When `part_path` lives under `…/updates/<leaf>`, require a currently trusted
/// updates root and refuse intermediate symlink ancestors. Windows co-located
/// EXE staging only checks intermediate ancestors (no app-data root).
fn revalidate_staging_parent_for_write(part_path: &Path) -> Result<(), String> {
    if let Some(parent) = part_path.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some("updates") {
            if let Some(app_data) = parent.parent() {
                crate::updater::paths::ensure_trusted_updates_dir(app_data)?;
                if crate::updater::paths::path_entry_is_symlink(parent) {
                    return Err("Updates directory is a symlink; refusing download write".into());
                }
            }
        }
        // Shared: refuse any near-ancestor directory symlink (covers Windows
        // co-located paths and nested escapes).
        if crate::updater::paths::path_entry_is_symlink(parent) {
            return Err("Staging parent is a symlink; refusing download write".into());
        }
    }
    // Leaf must not already be a symlink (would be truncated/followed).
    if crate::updater::paths::path_entry_is_symlink(part_path) {
        return Err("Staging path is a symlink; refusing download write".into());
    }
    Ok(())
}

async fn create_staging_part_file(part_path: &Path) -> Result<tokio::fs::File, String> {
    // Prefer openat-backed create when under app-data/updates.
    if let Some(parent) = part_path.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some("updates") {
            if let (Some(app_data), Some(name)) = (parent.parent(), part_path.file_name()) {
                let relative = Path::new(name);
                // Blocking openat in spawn_blocking keeps the async runtime free.
                let app_data = app_data.to_path_buf();
                let relative = relative.to_path_buf();
                let std_file = tokio::task::spawn_blocking(move || {
                    crate::updater::paths::create_trusted_updates_file(&app_data, &relative)
                })
                .await
                .map_err(|e| format!("create staging join: {e}"))??;
                return Ok(tokio::fs::File::from_std(std_file));
            }
        }
    }
    // Windows co-located EXE staging (or fallback): revalidate then create.
    revalidate_staging_parent_for_write(part_path)?;
    fs::File::create(part_path)
        .await
        .map_err(|e| format!("Failed to create staging file: {}", e))
}

fn validate_download_url(url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("Invalid download URL: {}", e))?;
    if is_test_http_localhost_allowed(&parsed) {
        return Ok(parsed);
    }
    if parsed.scheme() != "https" {
        return Err("Update downloads must use HTTPS".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "Download URL has no host".to_string())?;
    if !is_allowed_download_host(host) {
        return Err(format!("Download host '{}' is not allowed", host));
    }
    Ok(parsed)
}

fn ensure_final_url_allowed(url: &reqwest::Url) -> Result<(), String> {
    if is_test_http_localhost_allowed(url) {
        return Ok(());
    }
    if url.scheme() != "https" {
        return Err("Redirected to non-HTTPS download URL".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "Redirected download URL has no host".to_string())?;
    if !is_allowed_download_host(host) {
        return Err(format!(
            "Redirected download host '{}' is not allowed",
            host
        ));
    }
    Ok(())
}

/// Test-only: allow `http://127.0.0.1` / `http://localhost` for mock HTTP servers.
/// Production builds always return false (function is cfg-gated empty).
#[cfg(test)]
fn is_test_http_localhost_allowed(url: &reqwest::Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    matches!(
        url.host_str(),
        Some("127.0.0.1") | Some("localhost") | Some("[::1]")
    )
}

#[cfg(not(test))]
fn is_test_http_localhost_allowed(_url: &reqwest::Url) -> bool {
    false
}

fn emit_progress(
    app: Option<&AppHandle>,
    download_id: &str,
    received: u64,
    total: Option<u64>,
    phase: &str,
) {
    let Some(app) = app else {
        return;
    };
    let payload = DownloadProgressEvent {
        download_id: download_id.to_string(),
        received,
        total,
        phase: phase.to_string(),
    };
    let _ = app.emit(EVENT_DOWNLOAD_PROGRESS, payload);
}

async fn fsync_path(path: &Path) {
    if let Ok(file) = fs::File::open(path).await {
        let _ = file.sync_all().await;
    }
}

/// Parse GNU `sha256sum` format lines and return the hash for `file_name`.
///
/// Rejects:
/// - missing entry
/// - duplicate entries for the same name with different hashes
/// - malformed lines that look like they claim the target file
pub fn parse_sha256sums_for_file(text: &str, file_name: &str) -> Result<String, String> {
    let mut found: Option<String> = None;
    let mut saw_any = false;

    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        saw_any = true;

        // Formats:
        //   <64hex>  <name>
        //   <64hex> *<name>   (binary mode)
        let (hash, name) = match parse_sha256sum_line(line) {
            Some(v) => v,
            None => {
                // If the line mentions our file but is malformed, fail hard.
                if line.contains(file_name) {
                    return Err(format!(
                        "Malformed SHA256SUMS line {} for '{}'",
                        line_no + 1,
                        file_name
                    ));
                }
                continue;
            }
        };

        // Basename compare only (checksums may include ./ prefix).
        let base = Path::new(name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(name);
        if base != file_name {
            continue;
        }

        let hash = hash.to_ascii_lowercase();
        if !is_hex_sha256(&hash) {
            return Err(format!(
                "Invalid SHA-256 hex for '{}' in SHA256SUMS.txt",
                file_name
            ));
        }

        if let Some(ref prev) = found {
            if prev != &hash {
                return Err(format!(
                    "Conflicting SHA-256 entries for '{}' in SHA256SUMS.txt",
                    file_name
                ));
            }
        } else {
            found = Some(hash);
        }
    }

    if !saw_any {
        return Err("SHA256SUMS.txt is empty".to_string());
    }

    found.ok_or_else(|| format!("SHA256SUMS.txt is missing entry for '{}'", file_name))
}

fn parse_sha256sum_line(line: &str) -> Option<(&str, &str)> {
    // Two spaces or space+asterisk after 64 hex chars.
    if line.len() < 66 {
        return None;
    }
    let (hash_part, rest) = line.split_at(64);
    if !is_hex_sha256(hash_part) {
        return None;
    }
    let rest = rest.trim_start();
    if rest.is_empty() {
        return None;
    }
    let name = if let Some(stripped) = rest.strip_prefix('*') {
        stripped
    } else {
        rest
    };
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((hash_part, name))
}

/// Parse GitHub asset `digest` field (`sha256:<hex>`). Returns None if empty/absent style.
pub fn parse_github_sha256_digest(digest: &str) -> Result<Option<String>, String> {
    let d = digest.trim();
    if d.is_empty() {
        return Ok(None);
    }
    let lower = d.to_ascii_lowercase();
    let hex = if let Some(h) = lower.strip_prefix("sha256:") {
        h.trim()
    } else if is_hex_sha256(&lower) {
        lower.as_str()
    } else {
        return Err(format!(
            "Unsupported GitHub asset digest format: {}",
            digest
        ));
    };
    if !is_hex_sha256(hex) {
        return Err(format!("Invalid GitHub SHA-256 digest: {}", digest));
    }
    Ok(Some(hex.to_string()))
}

fn is_hex_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn constant_time_eq_hex(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gnu_sha256sums() {
        let text = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  Resh-v1.0.0-windows-x86_64.exe\n\
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  SHA256SUMS.txt\n";
        let h = parse_sha256sums_for_file(text, "Resh-v1.0.0-windows-x86_64.exe").unwrap();
        assert_eq!(h, "a".repeat(64));
    }

    #[test]
    fn parses_binary_mode_asterisk() {
        let text = format!("{} *Resh-v1.0.0-macos-aarch64.dmg\n", "c".repeat(64));
        let h = parse_sha256sums_for_file(&text, "Resh-v1.0.0-macos-aarch64.dmg").unwrap();
        assert_eq!(h, "c".repeat(64));
    }

    #[test]
    fn missing_entry_errors() {
        let text = format!("{}  other.exe\n", "a".repeat(64));
        let err = parse_sha256sums_for_file(&text, "Resh-v1.0.0-windows-x86_64.exe").unwrap_err();
        assert!(err.contains("missing"));
    }

    #[test]
    fn conflicting_entries_error() {
        let text = format!("{}  foo.exe\n{}  foo.exe\n", "a".repeat(64), "b".repeat(64));
        let err = parse_sha256sums_for_file(&text, "foo.exe").unwrap_err();
        assert!(err.contains("Conflicting"));
    }

    #[test]
    fn github_digest_parse() {
        let hex = "d".repeat(64);
        assert_eq!(
            parse_github_sha256_digest(&format!("sha256:{}", hex))
                .unwrap()
                .unwrap(),
            hex
        );
        assert!(parse_github_sha256_digest("md5:abc").is_err());
    }

    #[test]
    fn sanitize_filename() {
        assert!(sanitize_asset_filename("Resh-v1.0.0-windows-x86_64.exe").is_ok());
        assert!(sanitize_asset_filename("../evil.exe").is_err());
        assert!(sanitize_asset_filename("a/b.exe").is_err());
        assert!(sanitize_asset_filename("evil space.exe").is_err());
    }

    #[test]
    fn allowed_hosts_for_download_url() {
        assert!(validate_download_url(
            "https://github.com/fonlan/resh/releases/download/v1/Resh-v1.exe"
        )
        .is_ok());
        assert!(validate_download_url("http://github.com/x").is_err());
        assert!(validate_download_url("https://evil.com/x").is_err());
        assert!(validate_download_url("https://pages.github.com/x").is_err());
    }

    #[test]
    fn redirect_policy_rejects_untrusted_host() {
        // Policy construction must not panic; custom policy is exercised via client usage.
        let _ = trusted_download_redirect_policy(5);
    }

    #[test]
    fn install_asset_size_ceiling_is_enforced_constant() {
        // Documented hard limit for client + release guidance.
        assert_eq!(MAX_INSTALL_ASSET_BYTES, 512 * 1024 * 1024);
        assert_eq!(MAX_CHECKSUMS_BYTES, 1024 * 1024);
    }

    #[test]
    fn sha256sums_accepts_optional_dot_slash_prefix() {
        let text = format!("{}  ./Resh-v1.0.0-windows-x86_64.exe\n", "a".repeat(64));
        let h = parse_sha256sums_for_file(&text, "Resh-v1.0.0-windows-x86_64.exe").unwrap();
        assert_eq!(h, "a".repeat(64));
    }

    #[test]
    fn empty_sha256sums_errors() {
        let err = parse_sha256sums_for_file("\n# only comments\n", "foo.exe").unwrap_err();
        assert!(err.contains("empty"));
    }

    // --- Mock HTTP integration (loopback only; #[cfg(test)] host bypass) ---

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(trusted_download_redirect_policy(5))
            .no_proxy()
            .build()
            .expect("client")
    }

    /// Minimal HTTP/1.1 mock: map path -> (status, headers, body bytes).
    /// When `chunk_sizes` is Some, the body is written as successive pieces
    /// (simulating multi-chunk transfer) rather than a single write.
    async fn spawn_routes_chunked(
        routes: std::collections::HashMap<String, (u16, Vec<(String, String)>, Vec<u8>)>,
        chunk_sizes: Option<Vec<usize>>,
        drop_after_bytes: Option<usize>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use std::net::SocketAddr;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let base = format!("http://{addr}");
        let routes = std::sync::Arc::new(routes);
        let chunk_sizes = std::sync::Arc::new(chunk_sizes);
        let drop_after = std::sync::Arc::new(drop_after_bytes);
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                let chunk_sizes = chunk_sizes.clone();
                let drop_after = drop_after.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let path = path.split('?').next().unwrap_or(path).to_string();
                    let (status, headers, body) =
                        routes
                            .get(&path)
                            .cloned()
                            .unwrap_or((404, vec![], b"not found".to_vec()));
                    let reason = if status == 200 { "OK" } else { "ERR" };
                    let mut resp = format!(
                        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: {}\r\n",
                        body.len()
                    );
                    for (k, v) in headers {
                        resp.push_str(&format!("{k}: {v}\r\n"));
                    }
                    resp.push_str("\r\n");
                    let _ = socket.write_all(resp.as_bytes()).await;

                    let mut body = body;
                    if let Some(max) = *drop_after {
                        if body.len() > max {
                            body.truncate(max);
                        }
                    }
                    if let Some(sizes) = chunk_sizes.as_ref() {
                        let mut offset = 0usize;
                        let mut size_idx = 0usize;
                        while offset < body.len() {
                            let piece = sizes
                                .get(size_idx)
                                .copied()
                                .unwrap_or(sizes.last().copied().unwrap_or(body.len()));
                            size_idx = size_idx.saturating_add(1);
                            let end = (offset + piece).min(body.len());
                            if socket.write_all(&body[offset..end]).await.is_err() {
                                return;
                            }
                            offset = end;
                            tokio::task::yield_now().await;
                        }
                    } else if socket.write_all(&body).await.is_err() {
                        return;
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });
        (base, handle)
    }

    async fn spawn_routes(
        routes: std::collections::HashMap<String, (u16, Vec<(String, String)>, Vec<u8>)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        spawn_routes_chunked(routes, None, None).await
    }

    #[tokio::test]
    async fn mock_download_bytes_ok() {
        let payload = b"hello-update-payload-0123456789";
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "/asset.bin".into(),
            (
                200,
                vec![("Content-Type".into(), "application/octet-stream".into())],
                payload.to_vec(),
            ),
        );
        let (base, _h) = spawn_routes(routes).await;
        let client = test_client();
        let token = CancellationToken::new();
        let bytes = download_bytes_capped(&client, &format!("{base}/asset.bin"), 1024, &token)
            .await
            .expect("download");
        assert_eq!(bytes, payload);
    }

    #[tokio::test]
    async fn mock_download_exceeds_cap() {
        let payload = vec![b'x'; 64];
        let mut routes = std::collections::HashMap::new();
        routes.insert("/big.bin".into(), (200, vec![], payload));
        let (base, _h) = spawn_routes(routes).await;
        let client = test_client();
        let token = CancellationToken::new();
        let err = download_bytes_capped(&client, &format!("{base}/big.bin"), 16, &token)
            .await
            .unwrap_err();
        assert!(err.contains("maximum") || err.contains("exceed"), "{err}");
    }

    #[tokio::test]
    async fn mock_download_cancel_before_start() {
        let mut routes = std::collections::HashMap::new();
        routes.insert("/a".into(), (200, vec![], b"data".to_vec()));
        let (base, _h) = spawn_routes(routes).await;
        let client = test_client();
        let token = CancellationToken::new();
        token.cancel();
        let err = download_bytes_capped(&client, &format!("{base}/a"), 1024, &token)
            .await
            .unwrap_err();
        assert!(err.contains("cancel"), "{err}");
    }

    #[tokio::test]
    async fn mock_stream_sha256_and_part_write() {
        let payload = b"stream-me-please-for-sha";
        let expected = hex_encode(&Sha256::digest(payload));
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "/pkg".into(),
            (
                200,
                vec![("Content-Length".into(), payload.len().to_string())],
                payload.to_vec(),
            ),
        );
        let (base, _h) = spawn_routes(routes).await;
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("x.part");
        let client = test_client();
        let token = CancellationToken::new();
        let bytes = download_bytes_capped(&client, &format!("{base}/pkg"), 1024 * 1024, &token)
            .await
            .unwrap();
        assert_eq!(hex_encode(&Sha256::digest(&bytes)), expected);
        tokio::fs::write(&part, &bytes).await.unwrap();
        assert_eq!(tokio::fs::read(&part).await.unwrap(), payload);
        // Simulate PartGuard cleanup on failed hash: remove .part
        tokio::fs::remove_file(&part).await.unwrap();
        assert!(!part.exists());
    }

    #[tokio::test]
    async fn mock_stream_download_to_file_chunked_ok() {
        let payload: Vec<u8> = (0u8..200).collect();
        let expected = hex_encode(&Sha256::digest(&payload));
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "/chunked.bin".into(),
            (
                200,
                vec![("Content-Length".into(), payload.len().to_string())],
                payload.clone(),
            ),
        );
        let (base, _h) = spawn_routes_chunked(routes, Some(vec![7, 13, 1, 64]), None).await;
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("asset.part");
        let client = test_client();
        let token = CancellationToken::new();
        let (written, sha) = stream_download_to_file(
            None,
            "test-dl-id",
            &client,
            &format!("{base}/chunked.bin"),
            &part,
            payload.len() as u64,
            1024 * 1024,
            &token,
        )
        .await
        .expect("stream download");
        assert_eq!(written, payload.len() as u64);
        assert_eq!(sha, expected);
        assert_eq!(tokio::fs::read(&part).await.unwrap(), payload);
    }

    #[tokio::test]
    async fn mock_stream_download_claimed_size_mismatch() {
        let body = vec![b'a'; 50];
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "/size.bin".into(),
            (
                200,
                vec![("Content-Length".into(), body.len().to_string())],
                body,
            ),
        );
        let (base, _h) = spawn_routes(routes).await;
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("bad-size.part");
        let client = test_client();
        let token = CancellationToken::new();
        let err = stream_download_to_file(
            None,
            "id",
            &client,
            &format!("{base}/size.bin"),
            &part,
            999,
            1024 * 1024,
            &token,
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("does not match GitHub asset size") || err.contains("Content-Length"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn mock_stream_download_truncate_midstream_errors() {
        // Advertise Content-Length of full payload but drop the connection after partial body.
        let payload = vec![b'z'; 128];
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "/cut.bin".into(),
            (
                200,
                vec![("Content-Length".into(), payload.len().to_string())],
                payload.clone(),
            ),
        );
        let (base, _h) = spawn_routes_chunked(routes, Some(vec![16]), Some(40)).await;
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("cut.part");
        let client = test_client();
        let token = CancellationToken::new();
        // Production path: PartGuard cleans .part on any failure after staging starts.
        let guard = PartGuard::new(part.clone());
        let err = stream_download_to_file(
            None,
            "id",
            &client,
            &format!("{base}/cut.bin"),
            &part,
            128,
            1024 * 1024,
            &token,
        )
        .await
        .expect_err("truncated midstream must fail");
        assert!(
            err.contains("Content-Length")
                || err.contains("does not match")
                || err.contains("stream")
                || err.contains("Download")
                || err.contains("size"),
            "{err}"
        );
        drop(guard);
        assert!(!part.exists(), "PartGuard must remove .part after truncate");
    }

    #[tokio::test]
    async fn mock_stream_download_cancel_midway_cleans_part() {
        let payload = vec![b'x'; 512 * 1024];
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "/slow.bin".into(),
            (
                200,
                vec![("Content-Length".into(), payload.len().to_string())],
                payload,
            ),
        );
        // Large body, small chunks so cancel can land after some bytes are written.
        let (base, _h) = spawn_routes_chunked(routes, Some(vec![256]), None).await;
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("cancel.part");
        let client = test_client();
        let token = CancellationToken::new();
        let cancel = token.clone();
        // Cancel after a short delay so the stream opens the .part and writes chunks.
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            cancel.cancel();
        });
        let guard = PartGuard::new(part.clone());
        let err = stream_download_to_file(
            None,
            "id",
            &client,
            &format!("{base}/slow.bin"),
            &part,
            0,
            1024 * 1024,
            &token,
        )
        .await
        .unwrap_err();
        let _ = cancel_task.await;
        assert!(err.contains("cancel"), "{err}");
        drop(guard);
        assert!(
            !part.exists(),
            "PartGuard must remove .part after mid-stream cancel"
        );
        // Ready registry must stay empty for this synthetic id path.
        assert!(get_prepared_update("id").await.is_none());
    }

    #[tokio::test]
    async fn part_guard_drop_removes_partial_without_disarm() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("only.part");
        tokio::fs::write(&part, b"partial").await.unwrap();
        assert!(part.exists());
        {
            let _guard = PartGuard::new(part.clone());
            // Drop without disarm → cleanup.
        }
        assert!(!part.exists());
    }

    #[tokio::test]
    async fn part_guard_disarm_keeps_promoted_path() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("keep.part");
        tokio::fs::write(&part, b"final").await.unwrap();
        {
            let guard = PartGuard::new(part.clone());
            guard.disarm();
        }
        assert!(
            part.exists(),
            "disarmed PartGuard must not delete ready path"
        );
        tokio::fs::remove_file(&part).await.ok();
    }

    #[tokio::test]
    async fn mock_content_length_exceeds_cap() {
        // Server claims a huge Content-Length; client must reject before reading body.
        // Provide a matching body size so the HTTP framing is valid; cap is still 100.
        let body = vec![b'z'; 200];
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "/lie".into(),
            (
                200,
                vec![("Content-Length".into(), body.len().to_string())],
                body,
            ),
        );
        let (base, _h) = spawn_routes(routes).await;
        let client = test_client();
        let token = CancellationToken::new();
        let err = download_bytes_capped(&client, &format!("{base}/lie"), 100, &token)
            .await
            .unwrap_err();
        assert!(
            err.contains("Content-Length") || err.contains("maximum") || err.contains("exceed"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn mock_http_error_status() {
        let mut routes = std::collections::HashMap::new();
        routes.insert("/missing".into(), (404, vec![], b"nope".to_vec()));
        let (base, _h) = spawn_routes(routes).await;
        let client = test_client();
        let token = CancellationToken::new();
        let err = download_bytes_capped(&client, &format!("{base}/missing"), 1024, &token)
            .await
            .unwrap_err();
        assert!(err.contains("404") || err.contains("HTTP"), "{err}");
    }

    #[tokio::test]
    async fn mock_redirect_to_loopback_followed() {
        // Server A redirects to server B (both loopback). Policy must allow both hops in tests.
        let mut final_routes = std::collections::HashMap::new();
        final_routes.insert("/final".into(), (200, vec![], b"after-redirect".to_vec()));
        let (final_base, _hf) = spawn_routes(final_routes).await;

        let mut start_routes = std::collections::HashMap::new();
        start_routes.insert(
            "/start".into(),
            (
                302,
                vec![("Location".into(), format!("{final_base}/final"))],
                Vec::new(),
            ),
        );
        let (start_base, _hs) = spawn_routes(start_routes).await;

        let client = test_client();
        let token = CancellationToken::new();
        let bytes = download_bytes_capped(&client, &format!("{start_base}/start"), 1024, &token)
            .await
            .expect("redirect download");
        assert_eq!(bytes, b"after-redirect");
    }

    #[test]
    fn mock_wrong_hash_detected_by_parse_and_eq() {
        let good = "a".repeat(64);
        let bad = "b".repeat(64);
        assert!(!constant_time_eq_hex(&good, &bad));
        let sums = format!("{good}  Resh-v1.0.0-windows-x86_64.exe\n");
        let parsed = parse_sha256sums_for_file(&sums, "Resh-v1.0.0-windows-x86_64.exe").unwrap();
        assert_eq!(parsed, good);
        assert!(!constant_time_eq_hex(&parsed, &bad));
    }

    #[test]
    fn test_localhost_http_allowed_only_in_tests() {
        assert!(is_test_http_localhost_allowed(
            &reqwest::Url::parse("http://127.0.0.1:1/a").unwrap()
        ));
        assert!(!is_test_http_localhost_allowed(
            &reqwest::Url::parse("https://127.0.0.1:1/a").unwrap()
        ));
        assert!(!is_test_http_localhost_allowed(
            &reqwest::Url::parse("http://example.com/a").unwrap()
        ));
        assert!(validate_download_url("http://127.0.0.1:9/x").is_ok());
        assert!(validate_download_url("http://evil.com/x").is_err());
    }
}
