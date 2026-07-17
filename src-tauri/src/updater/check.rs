use super::assets::{select_release_assets, PlatformTarget};
use super::github::{fetch_latest_stable_release, GithubFetchCache, GithubFetchOutcome};
use super::types::{CheckUpdateResult, GitHubReleaseDto, UpdateInfo};
use super::version::{is_newer_than, parse_release_tag};
use super::current_app_version;
use crate::config::types::Proxy;
use crate::http::resolve_proxy_by_id;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Minimum interval between automatic (non-force) network checks.
/// Floor is 6 hours so resume/visibility catch-ups cannot densify traffic.
pub const AUTO_CHECK_MIN_INTERVAL_SECS: u64 = 6 * 60 * 60;

#[derive(Debug, Clone)]
pub struct CheckForUpdateOptions {
    /// When true, bypass the minimum auto-check interval (manual check).
    pub force: bool,
    /// Optional override for tests.
    pub current_version: Option<String>,
    /// Optional platform override for tests.
    pub platform: Option<PlatformTarget>,
}

impl Default for CheckForUpdateOptions {
    fn default() -> Self {
        Self {
            force: false,
            current_version: None,
            platform: None,
        }
    }
}

struct UpdaterRuntime {
    cache: Mutex<GithubFetchCache>,
    /// Shared in-flight check so concurrent callers await one request.
    in_flight: Mutex<Option<tokio::sync::watch::Receiver<Option<CheckUpdateResult>>>>,
    /// Trusted updates discovered by check; download accepts only these ids.
    available: Mutex<HashMap<String, UpdateInfo>>,
}

impl UpdaterRuntime {
    fn new() -> Self {
        Self {
            cache: Mutex::new(GithubFetchCache::default()),
            in_flight: Mutex::new(None),
            available: Mutex::new(HashMap::new()),
        }
    }
}

fn runtime() -> &'static UpdaterRuntime {
    static RUNTIME: OnceLock<UpdaterRuntime> = OnceLock::new();
    RUNTIME.get_or_init(UpdaterRuntime::new)
}

/// Look up a check-discovered update by opaque id (for trusted download).
pub async fn get_discovered_update(id: &str) -> Option<UpdateInfo> {
    let map = runtime().available.lock().await;
    map.get(id).cloned()
}

/// Register a backend-selected update and return it with a fresh opaque id.
async fn register_discovered_update(mut info: UpdateInfo) -> UpdateInfo {
    let id = Uuid::new_v4().to_string();
    info.id = id.clone();
    let mut map = runtime().available.lock().await;
    // Keep at most one entry per tag/platform asset name to limit memory.
    map.retain(|_, existing| existing.install_asset.name != info.install_asset.name);
    map.insert(id, info.clone());
    info
}

/// Check GitHub for the latest stable release and compare with the running version.
///
/// - Only non-draft, non-prerelease releases are accepted (API `/releases/latest` already
///   excludes drafts/prereleases, and we re-validate).
/// - Missing platform asset or `SHA256SUMS.txt` yields a diagnostic error (not up-to-date).
/// - Concurrent callers share one in-flight request.
/// - Non-force checks respect a minimum interval and may return the last cached outcome.
pub async fn check_for_update(
    proxies: &[crate::config::types::Proxy],
    update_proxy_id: Option<&str>,
    options: CheckForUpdateOptions,
) -> CheckUpdateResult {
    // Join any existing in-flight check first.
    {
        let mut guard = runtime().in_flight.lock().await;
        if let Some(rx) = guard.as_ref() {
            let mut rx = rx.clone();
            drop(guard);
            // Wait until the sender publishes a result (Some) or closes.
            loop {
                if let Some(result) = rx.borrow().clone() {
                    return result;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        } else {
            // Reserve in-flight slot with a placeholder channel.
            let (tx, rx) = tokio::sync::watch::channel(None);
            *guard = Some(rx);
            drop(guard);

            let result = run_check(proxies, update_proxy_id, options).await;
            let _ = tx.send(Some(result.clone()));

            let mut guard = runtime().in_flight.lock().await;
            *guard = None;
            return result;
        }
    }

    // Previous in-flight finished without value; run ourselves.
    run_check(proxies, update_proxy_id, options).await
}

async fn run_check(
    proxies: &[Proxy],
    update_proxy_id: Option<&str>,
    options: CheckForUpdateOptions,
) -> CheckUpdateResult {
    let current_version = options
        .current_version
        .unwrap_or_else(|| current_app_version().to_string());
    let platform = match options.platform.or_else(|| PlatformTarget::current().ok()) {
        Some(p) => p,
        None => {
            return CheckUpdateResult::Error {
                code: "unsupported_platform".to_string(),
                message: PlatformTarget::current()
                    .err()
                    .unwrap_or_else(|| "Unsupported platform".to_string()),
            };
        }
    };

    let proxy = match resolve_proxy_by_id(proxies, update_proxy_id) {
        Ok(p) => p.cloned(),
        Err(message) => {
            return CheckUpdateResult::Error {
                code: "proxy_not_found".to_string(),
                message,
            };
        }
    };

    // Interval gate for automatic checks (still allows cache re-evaluation without network).
    if !options.force {
        let cache = runtime().cache.lock().await;
        let now = now_unix();
        if cache.checked_at_unix > 0
            && now.saturating_sub(cache.checked_at_unix) < AUTO_CHECK_MIN_INTERVAL_SECS as i64
        {
            if let Some(ref release) = cache.body {
                return finalize_check_result(
                    evaluate_release(release, &current_version, platform, true),
                )
                .await;
            }
        }
    }

    let etag = {
        let cache = runtime().cache.lock().await;
        cache.etag.clone()
    };

    let outcome = match fetch_latest_stable_release(proxy.as_ref(), etag.as_deref()).await {
        Ok(o) => o,
        Err(message) => {
            return CheckUpdateResult::Error {
                code: "network_error".to_string(),
                message,
            };
        }
    };

    let result = match outcome {
        GithubFetchOutcome::RateLimited { retry_after_secs } => CheckUpdateResult::RateLimited {
            message: "GitHub API rate limit exceeded. Try again later.".to_string(),
            retry_after_secs,
        },
        GithubFetchOutcome::NotModified => {
            let mut cache = runtime().cache.lock().await;
            cache.checked_at_unix = now_unix();
            match cache.body.clone() {
                Some(release) => evaluate_release(&release, &current_version, platform, true),
                None => CheckUpdateResult::Error {
                    code: "cache_miss".to_string(),
                    message: "GitHub returned 304 Not Modified but no local release cache exists."
                        .to_string(),
                },
            }
        }
        GithubFetchOutcome::Fresh { release, etag } => {
            if release.draft || release.prerelease {
                return CheckUpdateResult::Error {
                    code: "unstable_release".to_string(),
                    message: format!(
                        "Latest release {} is draft/prerelease; only stable releases are tracked.",
                        release.tag_name
                    ),
                };
            }

            let mut cache = runtime().cache.lock().await;
            cache.etag = etag;
            cache.body = Some(release.clone());
            cache.checked_at_unix = now_unix();
            drop(cache);

            evaluate_release(&release, &current_version, platform, false)
        }
    };

    finalize_check_result(result).await
}

async fn finalize_check_result(result: CheckUpdateResult) -> CheckUpdateResult {
    match result {
        CheckUpdateResult::UpdateAvailable { update, from_cache } => {
            let registered = register_discovered_update(update).await;
            CheckUpdateResult::UpdateAvailable {
                update: registered,
                from_cache,
            }
        }
        other => other,
    }
}

fn evaluate_release(
    release: &GitHubReleaseDto,
    current_version: &str,
    platform: PlatformTarget,
    from_cache: bool,
) -> CheckUpdateResult {
    if release.draft || release.prerelease {
        return CheckUpdateResult::Error {
            code: "unstable_release".to_string(),
            message: format!(
                "Release {} is draft/prerelease; only stable releases are tracked.",
                release.tag_name
            ),
        };
    }

    let latest_version = match parse_release_tag(&release.tag_name) {
        Ok(v) => v,
        Err(message) => {
            return CheckUpdateResult::Error {
                code: "invalid_tag".to_string(),
                message: format!(
                    "Release tag '{}' is not a valid semver: {}",
                    release.tag_name, message
                ),
            };
        }
    };

    let newer = match is_newer_than(&latest_version, current_version) {
        Ok(v) => v,
        Err(message) => {
            return CheckUpdateResult::Error {
                code: "version_compare_error".to_string(),
                message,
            };
        }
    };

    if !newer {
        return CheckUpdateResult::UpToDate {
            current_version: current_version.to_string(),
            latest_version,
            tag_name: release.tag_name.clone(),
            from_cache,
        };
    }

    let (install_asset, checksums_asset) = match select_release_assets(release, platform) {
        Ok(pair) => pair,
        Err(message) => {
            return CheckUpdateResult::Error {
                code: "incomplete_release".to_string(),
                message,
            };
        }
    };

    CheckUpdateResult::UpdateAvailable {
        update: UpdateInfo {
            // Placeholder; finalize_check_result assigns a real opaque id.
            id: String::new(),
            tag_name: release.tag_name.clone(),
            version: latest_version,
            name: release.name.clone(),
            body: release.body.clone(),
            html_url: release.html_url.clone(),
            published_at: release.published_at.clone(),
            install_asset,
            checksums_asset,
            current_version: current_version.to_string(),
        },
        from_cache,
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Pure evaluation helper for unit tests (no network).
#[cfg(test)]
pub fn evaluate_release_for_test(
    release: &GitHubReleaseDto,
    current_version: &str,
    platform: PlatformTarget,
) -> CheckUpdateResult {
    evaluate_release(release, current_version, platform, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::types::GitHubAssetDto;

    fn asset(name: &str) -> GitHubAssetDto {
        GitHubAssetDto {
            name: name.to_string(),
            browser_download_url: format!("https://example.com/{name}"),
            size: 10,
            digest: None,
        }
    }

    fn stable_release(tag: &str) -> GitHubReleaseDto {
        GitHubReleaseDto {
            tag_name: tag.to_string(),
            name: Some(tag.to_string()),
            body: Some("notes".to_string()),
            html_url: format!("https://github.com/fonlan/resh/releases/tag/{tag}"),
            draft: false,
            prerelease: false,
            published_at: Some("2026-07-01T00:00:00Z".to_string()),
            assets: vec![
                asset(&format!("Resh-{tag}-windows-x86_64.exe")),
                asset(&format!("Resh-{tag}-macos-aarch64.dmg")),
                asset(&format!("Resh-{tag}-macos-x86_64.dmg")),
                asset("SHA256SUMS.txt"),
            ],
        }
    }

    #[test]
    fn up_to_date_when_equal() {
        let rel = stable_release("v1.1.0");
        let result =
            evaluate_release_for_test(&rel, "1.1.0", PlatformTarget::WindowsX86_64);
        match result {
            CheckUpdateResult::UpToDate {
                current_version,
                latest_version,
                ..
            } => {
                assert_eq!(current_version, "1.1.0");
                assert_eq!(latest_version, "1.1.0");
            }
            other => panic!("expected upToDate, got {:?}", other),
        }
    }

    #[test]
    fn no_update_when_older_remote() {
        let rel = stable_release("v1.0.0");
        let result =
            evaluate_release_for_test(&rel, "1.1.0", PlatformTarget::WindowsX86_64);
        assert!(matches!(result, CheckUpdateResult::UpToDate { .. }));
    }

    #[test]
    fn update_available_when_newer() {
        let rel = stable_release("v1.2.0");
        let result =
            evaluate_release_for_test(&rel, "1.1.0", PlatformTarget::MacOsAarch64);
        match result {
            CheckUpdateResult::UpdateAvailable { update, .. } => {
                assert_eq!(update.version, "1.2.0");
                assert_eq!(update.install_asset.name, "Resh-v1.2.0-macos-aarch64.dmg");
                assert_eq!(update.checksums_asset.name, "SHA256SUMS.txt");
                // Opaque id is assigned only when going through finalize_check_result.
                assert!(update.id.is_empty());
            }
            other => panic!("expected updateAvailable, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn register_assigns_opaque_id() {
        let info = UpdateInfo {
            id: String::new(),
            tag_name: "v1.2.0".to_string(),
            version: "1.2.0".to_string(),
            name: None,
            body: None,
            html_url: "https://github.com/fonlan/resh/releases/tag/v1.2.0".to_string(),
            published_at: None,
            install_asset: crate::updater::types::UpdateAssetInfo {
                name: "Resh-v1.2.0-windows-x86_64.exe".to_string(),
                browser_download_url: "https://github.com/fonlan/resh/releases/download/v1.2.0/Resh-v1.2.0-windows-x86_64.exe".to_string(),
                size: 10,
                digest: None,
            },
            checksums_asset: crate::updater::types::UpdateAssetInfo {
                name: "SHA256SUMS.txt".to_string(),
                browser_download_url: "https://github.com/fonlan/resh/releases/download/v1.2.0/SHA256SUMS.txt".to_string(),
                size: 100,
                digest: None,
            },
            current_version: "1.0.0".to_string(),
        };
        let registered = register_discovered_update(info).await;
        assert!(!registered.id.is_empty());
        let looked_up = get_discovered_update(&registered.id).await;
        assert_eq!(
            looked_up.as_ref().map(|u| u.id.as_str()),
            Some(registered.id.as_str())
        );
    }

    #[test]
    fn incomplete_release_is_error() {
        let mut rel = stable_release("v9.0.0");
        rel.assets.retain(|a| a.name != "SHA256SUMS.txt");
        let result =
            evaluate_release_for_test(&rel, "1.0.0", PlatformTarget::WindowsX86_64);
        match result {
            CheckUpdateResult::Error { code, message } => {
                assert_eq!(code, "incomplete_release");
                assert!(message.contains("SHA256SUMS.txt"));
            }
            other => panic!("expected error, got {:?}", other),
        }
    }

    #[test]
    fn invalid_tag_is_error() {
        let mut rel = stable_release("v1.0.0");
        rel.tag_name = "not-semver".to_string();
        let result =
            evaluate_release_for_test(&rel, "1.0.0", PlatformTarget::WindowsX86_64);
        assert!(matches!(
            result,
            CheckUpdateResult::Error {
                code,
                ..
            } if code == "invalid_tag"
        ));
    }

    #[test]
    fn rejects_prerelease_flag() {
        let mut rel = stable_release("v2.0.0");
        rel.prerelease = true;
        let result =
            evaluate_release_for_test(&rel, "1.0.0", PlatformTarget::WindowsX86_64);
        assert!(matches!(
            result,
            CheckUpdateResult::Error {
                code,
                ..
            } if code == "unstable_release"
        ));
    }
}
