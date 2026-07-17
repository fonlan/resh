// GitHub Releases based update checker and trusted asset download (stable releases only).

mod assets;
mod check;
mod download;
mod github;
mod types;
mod version;

pub use assets::{
    expected_install_asset_name, expected_sha256sums_name, select_release_assets, PlatformTarget,
    SHA256SUMS_FILE_NAME,
};
pub use check::{check_for_update, get_discovered_update, CheckForUpdateOptions};
pub use download::{
    cancel_update_download, download_update, get_prepared_update, parse_github_sha256_digest,
    parse_sha256sums_for_file, MAX_INSTALL_ASSET_BYTES,
};
pub use types::{
    CheckUpdateResult, DownloadProgressEvent, GitHubAssetDto, GitHubReleaseDto, PreparedUpdate,
    UpdateAssetInfo, UpdateInfo,
};
pub use version::{
    compare_semver, is_newer_than, parse_release_tag, parse_semver, VersionCompare,
};

pub const GITHUB_OWNER: &str = "fonlan";
pub const GITHUB_REPO: &str = "resh";
pub const GITHUB_API_BASE: &str = "https://api.github.com";
pub const GITHUB_API_VERSION: &str = "2022-11-28";
pub const GITHUB_ACCEPT: &str = "application/vnd.github+json";

/// Allowed hosts for GitHub API and release asset downloads (including CDN redirects).
pub const ALLOWED_DOWNLOAD_HOSTS: &[&str] = &[
    "api.github.com",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "github-releases.githubusercontent.com",
];

pub fn current_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn releases_latest_url() -> String {
    format!(
        "{}/repos/{}/{}/releases/latest",
        GITHUB_API_BASE, GITHUB_OWNER, GITHUB_REPO
    )
}

/// Exact host match only (no arbitrary subdomains such as GitHub Pages).
pub fn is_allowed_download_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    ALLOWED_DOWNLOAD_HOSTS
        .iter()
        .any(|allowed| host == *allowed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_url_is_stable_api_path() {
        assert_eq!(
            releases_latest_url(),
            "https://api.github.com/repos/fonlan/resh/releases/latest"
        );
    }

    #[test]
    fn allowed_hosts() {
        assert!(is_allowed_download_host("api.github.com"));
        assert!(is_allowed_download_host("objects.githubusercontent.com"));
        assert!(is_allowed_download_host("github.com"));
        assert!(!is_allowed_download_host("evil.example.com"));
        assert!(!is_allowed_download_host("github.com.evil.com"));
        // Subdomains of allowed hosts are not trusted (e.g. GitHub Pages).
        assert!(!is_allowed_download_host("pages.github.com"));
        assert!(!is_allowed_download_host("evil.github.com"));
        assert!(!is_allowed_download_host("cdn.objects.githubusercontent.com"));
    }
}
