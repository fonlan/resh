//! Install result / failure markers written by platform helpers.

use super::updates_root;
use crate::updater::paths::{
    remove_trusted_updates_relative, updates_root_exists_and_trusted,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Durable failure marker consumed once at next app start.
pub const LAST_INSTALL_FAILURE_FILE: &str = "last-install-failure.txt";
/// Structured result written by helpers (ok / error).
pub const INSTALL_RESULT_FILE: &str = "last-install-result.json";
/// Helper waits this long for the new process to prove it is alive.
pub const INSTALL_ALIVE_WAIT_SECS: u64 = 45;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallResultFile {
    pub ok: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

pub fn install_failure_path(app_data_dir: &Path) -> PathBuf {
    updates_root(app_data_dir).join(LAST_INSTALL_FAILURE_FILE)
}

pub fn install_result_path(app_data_dir: &Path) -> PathBuf {
    updates_root(app_data_dir).join(INSTALL_RESULT_FILE)
}

/// Marker path written by the new process as soon as the restore token is accepted.
/// Helpers wait for this file before treating launch as successful.
pub fn install_alive_marker_path(app_data_dir: &Path, restore_token: &str) -> PathBuf {
    updates_root(app_data_dir).join(format!("install-alive-{restore_token}.ready"))
}

/// Best-effort alive signal for the install helper handshake.
pub fn write_install_alive_marker(app_data_dir: &Path, restore_token: &str) {
    if restore_token.is_empty() {
        return;
    }
    // Refuse to write through a symlink-escaped updates root: openat(O_NOFOLLOW)
    // against a validated updates directory fd (Unix) / revalidated root (others).
    // Mode 0600 is applied on the open fd (Unix fchmod); do not re-resolve the path.
    let relative = PathBuf::from(format!("install-alive-{restore_token}.ready"));
    if let Err(e) = crate::updater::paths::write_trusted_updates_file(
        app_data_dir,
        &relative,
        b"alive\n",
    ) {
        tracing::warn!("skipping alive marker write: {e}");
    }
}

pub fn clear_install_alive_marker(app_data_dir: &Path, restore_token: &str) {
    if restore_token.is_empty() {
        return;
    }
    let relative = PathBuf::from(format!("install-alive-{restore_token}.ready"));
    if !remove_trusted_updates_relative(app_data_dir, &relative) {
        tracing::warn!("skipping alive-marker clear: untrusted updates directory");
    }
}

#[allow(dead_code)]
pub fn write_install_result_message(path: &Path, ok: bool, message: &str, version: Option<&str>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let payload = InstallResultFile {
        ok,
        message: message.to_string(),
        version: version.map(|s| s.to_string()),
    };
    if let Ok(json) = serde_json::to_string_pretty(&payload) {
        let _ = fs::write(path, json);
    }
}

pub fn read_install_result(path: &Path) -> Result<InstallResultFile, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read install result: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse install result: {e}"))
}

/// Load and clear the one-shot install failure marker (plain text from helper).
pub fn load_last_install_failure(app_data_dir: &Path) -> Option<String> {
    // Fail closed when updates/ is not a trusted real directory under app-data.
    if !updates_root_exists_and_trusted(app_data_dir) {
        return None;
    }
    let path = install_failure_path(app_data_dir);
    if !path.is_file() {
        // Also check structured result for failure.
        let result_path = install_result_path(app_data_dir);
        if result_path.is_file() {
            if let Ok(parsed) = read_install_result(&result_path) {
                if !parsed.ok {
                    let _ = remove_trusted_updates_relative(
                        app_data_dir,
                        Path::new(INSTALL_RESULT_FILE),
                    );
                    return Some(sanitize_user_message(&parsed.message));
                }
            }
        }
        return None;
    }
    let message = fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| sanitize_user_message(&s));
    let _ = remove_trusted_updates_relative(app_data_dir, Path::new(LAST_INSTALL_FAILURE_FILE));
    message
}

pub fn clear_last_install_failure(app_data_dir: &Path) {
    if !remove_trusted_updates_relative(app_data_dir, Path::new(LAST_INSTALL_FAILURE_FILE)) {
        tracing::warn!("skipping install-failure clear: untrusted updates directory");
    }
    // Do not remove a successful result here; only clear failure text marker.
}

/// Strip absolute paths and credentials from helper messages before UI.
pub fn sanitize_user_message(raw: &str) -> String {
    let mut out = raw.trim().to_string();
    // Collapse Windows drive paths and Unix absolute paths to a placeholder.
    out = regex_replace_paths(&out);
    // Cap length for toast/dialog.
    if out.chars().count() > 400 {
        out = out.chars().take(400).collect::<String>() + "…";
    }
    out
}

fn regex_replace_paths(s: &str) -> String {
    // Lightweight path redaction without the regex crate.
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Windows: C:\...
        if i + 2 < chars.len()
            && chars[i].is_ascii_alphabetic()
            && chars[i + 1] == ':'
            && (chars[i + 2] == '\\' || chars[i + 2] == '/')
        {
            result.push_str("<path>");
            i += 3;
            while i < chars.len()
                && !chars[i].is_whitespace()
                && chars[i] != '"'
                && chars[i] != '\''
            {
                i += 1;
            }
            continue;
        }
        // Unix absolute path
        if chars[i] == '/'
            && i + 1 < chars.len()
            && (chars[i + 1].is_ascii_alphanumeric() || chars[i + 1] == '.')
        {
            // Keep short absolute tools like /usr/bin/xattr out of redaction only if
            // they are known short prefixes; still redact long home paths.
            let start = i;
            i += 1;
            while i < chars.len()
                && !chars[i].is_whitespace()
                && chars[i] != '"'
                && chars[i] != '\''
            {
                i += 1;
            }
            let segment: String = chars[start..i].iter().collect();
            if segment.starts_with("/Users/")
                || segment.starts_with("/home/")
                || segment.starts_with("/var/")
                || segment.starts_with("/private/")
                || segment.starts_with("/tmp/")
                || segment.starts_with("/Volumes/")
                || segment.len() > 40
            {
                result.push_str("<path>");
            } else {
                result.push_str(&segment);
            }
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn sanitize_redacts_home_paths() {
        let msg = "Update failed at /Users/alice/Applications/Resh.app after copy";
        let s = sanitize_user_message(msg);
        assert!(!s.contains("alice"));
        assert!(s.contains("<path>"));
    }

    #[test]
    fn load_failure_consumes_file() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        fs::create_dir_all(updates_root(app)).unwrap();
        let path = install_failure_path(app);
        fs::write(&path, "boom").unwrap();
        let msg = load_last_install_failure(app).unwrap();
        assert!(msg.contains("boom"));
        assert!(!path.exists());
    }

    #[test]
    fn load_failure_refuses_symlink_updates_root() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join(LAST_INSTALL_FAILURE_FILE);
        fs::write(&secret, "external-boom").unwrap();
        symlink(&external, app_data.join("updates")).unwrap();

        assert!(load_last_install_failure(&app_data).is_none());
        assert!(secret.exists(), "must not follow updates symlink to external");
    }

    #[test]
    fn clear_alive_marker_refuses_symlink_updates_root() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("install-alive-tok1.ready");
        fs::write(&secret, b"alive\n").unwrap();
        symlink(&external, app_data.join("updates")).unwrap();

        clear_install_alive_marker(&app_data, "tok1");
        assert!(secret.exists());
    }
}
