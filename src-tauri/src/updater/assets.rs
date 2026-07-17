use super::types::{GitHubAssetDto, GitHubReleaseDto, UpdateAssetInfo};

/// Runtime platform/arch target used to select release install assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformTarget {
    WindowsX86_64,
    MacOsAarch64,
    MacOsX86_64,
}

pub const SHA256SUMS_FILE_NAME: &str = "SHA256SUMS.txt";

impl PlatformTarget {
    pub fn current() -> Result<Self, String> {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            return Ok(Self::WindowsX86_64);
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            return Ok(Self::MacOsAarch64);
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            return Ok(Self::MacOsX86_64);
        }
        #[allow(unreachable_code)]
        Err(format!(
            "Unsupported platform for updates: {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    }

    pub fn os_arch_label(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "windows-x86_64",
            Self::MacOsAarch64 => "macos-aarch64",
            Self::MacOsX86_64 => "macos-x86_64",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "exe",
            Self::MacOsAarch64 | Self::MacOsX86_64 => "dmg",
        }
    }
}

/// Build the expected install asset filename for a release tag.
/// Tag may include a leading `v` (e.g. `v1.2.3`); the file name keeps the tag as-is.
pub fn expected_install_asset_name(tag: &str, target: PlatformTarget) -> String {
    format!(
        "Resh-{}-{}.{}",
        tag,
        target.os_arch_label(),
        target.extension()
    )
}

pub fn expected_sha256sums_name() -> &'static str {
    SHA256SUMS_FILE_NAME
}

/// Select install + checksum assets from a release. Both must be present.
pub fn select_release_assets(
    release: &GitHubReleaseDto,
    target: PlatformTarget,
) -> Result<(UpdateAssetInfo, UpdateAssetInfo), String> {
    let install_name = expected_install_asset_name(&release.tag_name, target);
    let checksums_name = expected_sha256sums_name();

    let install = find_asset(&release.assets, &install_name).ok_or_else(|| {
        format!(
            "Release {} is missing required install asset '{}'",
            release.tag_name, install_name
        )
    })?;
    let checksums = find_asset(&release.assets, checksums_name).ok_or_else(|| {
        format!(
            "Release {} is missing required checksums file '{}'",
            release.tag_name, checksums_name
        )
    })?;

    Ok((to_update_asset(install), to_update_asset(checksums)))
}

fn find_asset<'a>(assets: &'a [GitHubAssetDto], name: &str) -> Option<&'a GitHubAssetDto> {
    assets.iter().find(|a| a.name == name)
}

fn to_update_asset(asset: &GitHubAssetDto) -> UpdateAssetInfo {
    UpdateAssetInfo {
        name: asset.name.clone(),
        browser_download_url: asset.browser_download_url.clone(),
        size: asset.size,
        digest: asset.digest.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GitHubAssetDto {
        GitHubAssetDto {
            name: name.to_string(),
            browser_download_url: format!("https://github.com/fonlan/resh/releases/download/x/{name}"),
            size: 100,
            digest: None,
        }
    }

    fn release(tag: &str, assets: Vec<GitHubAssetDto>) -> GitHubReleaseDto {
        GitHubReleaseDto {
            tag_name: tag.to_string(),
            name: Some(tag.to_string()),
            body: None,
            html_url: "https://github.com/fonlan/resh/releases/tag/v1.0.0".to_string(),
            draft: false,
            prerelease: false,
            published_at: None,
            assets,
        }
    }

    #[test]
    fn asset_names_match_release_contract() {
        assert_eq!(
            expected_install_asset_name("v1.2.3", PlatformTarget::WindowsX86_64),
            "Resh-v1.2.3-windows-x86_64.exe"
        );
        assert_eq!(
            expected_install_asset_name("v1.2.3", PlatformTarget::MacOsAarch64),
            "Resh-v1.2.3-macos-aarch64.dmg"
        );
        assert_eq!(
            expected_install_asset_name("v1.2.3", PlatformTarget::MacOsX86_64),
            "Resh-v1.2.3-macos-x86_64.dmg"
        );
        assert_eq!(expected_sha256sums_name(), "SHA256SUMS.txt");
    }

    #[test]
    fn selects_matching_assets() {
        let tag = "v2.0.0";
        let rel = release(
            tag,
            vec![
                asset("Resh-v2.0.0-windows-x86_64.exe"),
                asset("Resh-v2.0.0-macos-aarch64.dmg"),
                asset("Resh-v2.0.0-macos-x86_64.dmg"),
                asset("SHA256SUMS.txt"),
            ],
        );
        let (install, sums) =
            select_release_assets(&rel, PlatformTarget::MacOsAarch64).expect("assets");
        assert_eq!(install.name, "Resh-v2.0.0-macos-aarch64.dmg");
        assert_eq!(sums.name, "SHA256SUMS.txt");
    }

    #[test]
    fn missing_install_asset_errors() {
        let rel = release(
            "v2.0.0",
            vec![asset("SHA256SUMS.txt"), asset("Resh-v2.0.0-macos-x86_64.dmg")],
        );
        let err = select_release_assets(&rel, PlatformTarget::WindowsX86_64).unwrap_err();
        assert!(err.contains("windows-x86_64.exe"));
    }

    #[test]
    fn missing_checksums_errors() {
        let rel = release(
            "v2.0.0",
            vec![asset("Resh-v2.0.0-windows-x86_64.exe")],
        );
        let err = select_release_assets(&rel, PlatformTarget::WindowsX86_64).unwrap_err();
        assert!(err.contains("SHA256SUMS.txt"));
    }
}
