use std::cmp::Ordering;

/// Result of comparing two semantic versions (ignoring leading `v`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionCompare {
    Less,
    Equal,
    Greater,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemVer {
    major: u64,
    minor: u64,
    patch: u64,
    /// Pre-release identifiers (empty for stable).
    pre: Vec<PrePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrePart {
    Num(u64),
    Text(String),
}

impl PartialOrd for PrePart {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrePart {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (PrePart::Num(a), PrePart::Num(b)) => a.cmp(b),
            (PrePart::Text(a), PrePart::Text(b)) => a.cmp(b),
            (PrePart::Num(_), PrePart::Text(_)) => Ordering::Less,
            (PrePart::Text(_), PrePart::Num(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        match (
            self.major.cmp(&other.major),
            self.minor.cmp(&other.minor),
            self.patch.cmp(&other.patch),
        ) {
            (Ordering::Equal, Ordering::Equal, Ordering::Equal) => {
                // No pre-release > any pre-release (semver 11.4)
                match (self.pre.is_empty(), other.pre.is_empty()) {
                    (true, true) => Ordering::Equal,
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                    (false, false) => {
                        let len = self.pre.len().min(other.pre.len());
                        for i in 0..len {
                            match self.pre[i].cmp(&other.pre[i]) {
                                Ordering::Equal => continue,
                                non_eq => return non_eq,
                            }
                        }
                        self.pre.len().cmp(&other.pre.len())
                    }
                }
            }
            (major, _, _) if major != Ordering::Equal => major,
            (_, minor, _) if minor != Ordering::Equal => minor,
            (_, _, patch) => patch,
        }
    }
}

/// Strip optional leading `v` / `V` and parse a semver core (+ optional pre-release).
/// Build metadata (`+...`) is accepted and ignored for comparison.
pub fn parse_semver(raw: &str) -> Result<String, String> {
    let parsed = parse_semver_inner(raw)?;
    Ok(format_semver_display(&parsed, raw))
}

/// Parse a release tag that may start with `v`. Returns the normalized version string
/// without the leading `v` (e.g. `v1.2.3` -> `1.2.3`).
pub fn parse_release_tag(tag: &str) -> Result<String, String> {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        return Err("Empty release tag".to_string());
    }
    let without_v = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    let _ = parse_semver_inner(without_v)?;
    // Preserve original pre-release / build text after stripping only the leading v.
    Ok(without_v.to_string())
}

pub fn compare_semver(a: &str, b: &str) -> Result<VersionCompare, String> {
    let left = parse_semver_inner(a)?;
    let right = parse_semver_inner(b)?;
    Ok(match left.cmp(&right) {
        Ordering::Less => VersionCompare::Less,
        Ordering::Equal => VersionCompare::Equal,
        Ordering::Greater => VersionCompare::Greater,
    })
}

/// Returns true when `candidate` is strictly newer than `current`.
pub fn is_newer_than(candidate: &str, current: &str) -> Result<bool, String> {
    Ok(compare_semver(candidate, current)? == VersionCompare::Greater)
}

fn format_semver_display(v: &SemVer, original: &str) -> String {
    let without_v = original
        .trim()
        .strip_prefix('v')
        .or_else(|| original.trim().strip_prefix('V'))
        .unwrap_or(original.trim());
    // Prefer normalized core from parse; keep pre/build from input path via re-parse display.
    let _ = v;
    without_v.to_string()
}

fn parse_semver_inner(raw: &str) -> Result<SemVer, String> {
    let s = raw.trim();
    let s = s
        .strip_prefix('v')
        .or_else(|| s.strip_prefix('V'))
        .unwrap_or(s);

    if s.is_empty() {
        return Err("Empty version".to_string());
    }

    let (core_and_pre, _build) = match s.split_once('+') {
        Some((left, build)) => {
            if build.is_empty() {
                return Err(format!("Invalid version build metadata: {}", raw));
            }
            (left, Some(build))
        }
        None => (s, None),
    };

    let (core, pre_str) = match core_and_pre.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (core_and_pre, None),
    };

    let mut parts = core.split('.');
    let major = parse_num(
        parts.next().ok_or_else(|| format!("Invalid version: {}", raw))?,
        raw,
    )?;
    let minor = parse_num(
        parts.next().ok_or_else(|| format!("Invalid version: {}", raw))?,
        raw,
    )?;
    let patch = parse_num(
        parts.next().ok_or_else(|| format!("Invalid version: {}", raw))?,
        raw,
    )?;
    if parts.next().is_some() {
        return Err(format!("Invalid version (too many core segments): {}", raw));
    }

    let pre = if let Some(pre) = pre_str {
        if pre.is_empty() {
            return Err(format!("Invalid pre-release in version: {}", raw));
        }
        pre.split('.')
            .map(|part| {
                if part.is_empty() {
                    return Err(format!("Invalid pre-release identifier in: {}", raw));
                }
                if part.chars().all(|c| c.is_ascii_digit()) {
                    // Leading zeros not allowed for numeric identifiers in strict semver,
                    // except for single "0".
                    if part.len() > 1 && part.starts_with('0') {
                        return Err(format!(
                            "Invalid numeric pre-release identifier (leading zero): {}",
                            raw
                        ));
                    }
                    Ok(PrePart::Num(part.parse().map_err(|_| {
                        format!("Invalid numeric pre-release in: {}", raw)
                    })?))
                } else if part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
                {
                    Ok(PrePart::Text(part.to_string()))
                } else {
                    Err(format!("Invalid pre-release identifier in: {}", raw))
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };

    Ok(SemVer {
        major,
        minor,
        patch,
        pre,
    })
}

fn parse_num(s: &str, raw: &str) -> Result<u64, String> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid version number in: {}", raw));
    }
    if s.len() > 1 && s.starts_with('0') {
        return Err(format!("Invalid version number (leading zero) in: {}", raw));
    }
    s.parse()
        .map_err(|_| format!("Invalid version number in: {}", raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_with_leading_v() {
        assert_eq!(parse_release_tag("v1.2.3").unwrap(), "1.2.3");
        assert_eq!(parse_release_tag("V1.0.0").unwrap(), "1.0.0");
        assert_eq!(parse_release_tag("1.2.3").unwrap(), "1.2.3");
    }

    #[test]
    fn rejects_invalid_tags() {
        assert!(parse_release_tag("").is_err());
        assert!(parse_release_tag("latest").is_err());
        assert!(parse_release_tag("v1.2").is_err());
        assert!(parse_release_tag("v01.2.3").is_err());
        assert!(parse_release_tag("not-a-version").is_err());
    }

    #[test]
    fn compares_core_versions() {
        assert_eq!(
            compare_semver("1.2.3", "1.2.4").unwrap(),
            VersionCompare::Less
        );
        assert_eq!(
            compare_semver("2.0.0", "1.9.9").unwrap(),
            VersionCompare::Greater
        );
        assert_eq!(
            compare_semver("1.0.0", "1.0.0").unwrap(),
            VersionCompare::Equal
        );
    }

    #[test]
    fn pre_release_is_older_than_stable() {
        assert_eq!(
            compare_semver("1.0.0-beta.1", "1.0.0").unwrap(),
            VersionCompare::Less
        );
        assert!(is_newer_than("1.0.0", "1.0.0-rc.1").unwrap());
        assert!(!is_newer_than("1.0.0-rc.1", "1.0.0").unwrap());
    }

    #[test]
    fn build_metadata_ignored() {
        assert_eq!(
            compare_semver("1.0.0+build.1", "1.0.0+build.2").unwrap(),
            VersionCompare::Equal
        );
    }

    #[test]
    fn equal_or_older_not_newer() {
        assert!(!is_newer_than("1.1.0", "1.1.0").unwrap());
        assert!(!is_newer_than("1.0.9", "1.1.0").unwrap());
        assert!(is_newer_than("1.1.1", "1.1.0").unwrap());
    }
}
