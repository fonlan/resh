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
///
/// `id` is an opaque backend-issued handle. Download only accepts this id and
/// never trusts client-supplied URLs or asset digests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// Opaque id bound to the backend-selected Release assets (not a path).
    pub id: String,
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
///
/// The frontend reads `currentVersion` / `latestVersion` / `fromCache` / `retryAfterSecs`, so the
/// struct-variant fields need `rename_all_fields` — `rename_all` alone only renames variants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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

/// Immutable handle returned after a verified download. Install only accepts this id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedUpdate {
    pub id: String,
    pub version: String,
    pub tag_name: String,
    pub asset_name: String,
    pub size: u64,
    pub sha256: String,
    pub current_version: String,
}

/// Progress payload emitted on `update-download-progress`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressEvent {
    pub download_id: String,
    pub received: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Phase: `checksums` | `asset` | `verifying` | `ready`
    pub phase: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `src/types/update.ts` reads `currentVersion` / `latestVersion` / `tagName` / `fromCache` /
    /// `retryAfterSecs`. `rename_all` on an enum only renames its variants, so `rename_all_fields`
    /// is what keeps those reads from silently resolving to `undefined`.
    #[test]
    fn check_update_result_matches_the_frontend_camel_case_contract() {
        let up_to_date = CheckUpdateResult::UpToDate {
            current_version: "1.0.0".to_string(),
            latest_version: "1.1.0".to_string(),
            tag_name: "v1.1.0".to_string(),
            from_cache: true,
        };
        let value = serde_json::to_value(&up_to_date).unwrap();
        assert_eq!(value["status"], "upToDate");
        assert_eq!(value["currentVersion"], "1.0.0");
        assert_eq!(value["latestVersion"], "1.1.0");
        assert_eq!(value["tagName"], "v1.1.0");
        assert_eq!(value["fromCache"], serde_json::json!(true));
        assert!(
            value.get("current_version").is_none(),
            "result must not expose snake_case fields: {value}"
        );

        let rate_limited = CheckUpdateResult::RateLimited {
            message: "slow down".to_string(),
            retry_after_secs: Some(30),
        };
        let value = serde_json::to_value(&rate_limited).unwrap();
        assert_eq!(value["status"], "rateLimited");
        assert_eq!(value["retryAfterSecs"], 30);
    }
}
