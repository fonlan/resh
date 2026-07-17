use serde::{Deserialize, Serialize};

/// Installable asset selected from a GitHub Release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAssetInfo {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    /// Optional digest from GitHub asset metadata, e.g. `sha256:abcd...`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// Update metadata exposed to the frontend when a newer stable release is available.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub tag_name: String,
    pub version: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub html_url: String,
    pub published_at: Option<String>,
    pub install_asset: UpdateAssetInfo,
    pub checksums_asset: UpdateAssetInfo,
    pub current_version: String,
}

/// Result of `check_for_update`. Discriminated by `status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CheckUpdateResult {
    #[serde(rename = "upToDate")]
    UpToDate {
        current_version: String,
        latest_version: String,
        tag_name: String,
        #[serde(default)]
        from_cache: bool,
    },
    #[serde(rename = "updateAvailable")]
    UpdateAvailable {
        update: UpdateInfo,
        #[serde(default)]
        from_cache: bool,
    },
    #[serde(rename = "rateLimited")]
    RateLimited {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_secs: Option<u64>,
    },
    #[serde(rename = "error")]
    Error { message: String, code: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubReleaseDto {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
    pub published_at: Option<String>,
    pub assets: Vec<GitHubAssetDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubAssetDto {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    /// GitHub may expose asset digests as `sha256:<hex>`.
    #[serde(default)]
    pub digest: Option<String>,
}
