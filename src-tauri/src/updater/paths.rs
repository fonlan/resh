//! Trusted filesystem helpers for updater I/O under app-data `updates/`.
//!
//! All delete/write entry-points that touch app-data must refuse symlink/junction
//! escapes of the logical app-data root or `updates/` itself, and must not
//! follow intermediate directory links when creating, renaming, or unlinking.
//!
//! On Unix, file creates under `updates/` use openat(O_NOFOLLOW) against an
//! already-validated directory fd so intermediate root swaps cannot redirect
//! writes after an earlier path-string check.

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

pub fn updates_root(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("updates")
}

pub fn path_entry_is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// True when `app_data_dir` is present as a real (non-symlink) directory.
/// A missing path is allowed for write bootstrapping, but not for delete auth.
pub fn app_data_dir_is_real_dir(app_data_dir: &Path) -> bool {
    match fs::symlink_metadata(app_data_dir) {
        Ok(meta) => !meta.file_type().is_symlink() && meta.is_dir(),
        Err(_) => false,
    }
}

/// Reject when the logical app-data path is a symlink/junction (escape root).
pub fn app_data_dir_is_symlink_escape(app_data_dir: &Path) -> bool {
    match fs::symlink_metadata(app_data_dir) {
        Ok(meta) => meta.file_type().is_symlink(),
        Err(_) => false,
    }
}

/// Resolve app-data `updates/` when it is a real directory under a non-symlink
/// app-data root. When `allow_missing` is true and `updates` does not exist,
/// returns the logical path so callers can treat it as an empty root that cannot
/// authorize external deletes.
pub fn resolve_trusted_updates_root(app_data_dir: &Path, allow_missing: bool) -> Option<PathBuf> {
    if app_data_dir_is_symlink_escape(app_data_dir) {
        tracing::warn!("app-data root is a symlink; refusing updates-root operations");
        return None;
    }

    let updates = updates_root(app_data_dir);
    if !updates.exists() {
        return if allow_missing { Some(updates) } else { None };
    }

    if let Ok(meta) = fs::symlink_metadata(&updates) {
        if meta.file_type().is_symlink() {
            tracing::warn!("updates root is a symlink; refusing updates-root operations");
            return None;
        }
        if !meta.is_dir() {
            tracing::warn!("updates root is not a directory; refusing updates-root operations");
            return None;
        }
    } else {
        return None;
    }

    // Prefer real app-data dir; if missing after updates exists (odd race), fail closed.
    if !app_data_dir_is_real_dir(app_data_dir) && app_data_dir.exists() {
        tracing::warn!("app-data root is not a real directory; refusing updates-root operations");
        return None;
    }

    let app_data_canon = fs::canonicalize(app_data_dir).ok()?;
    let updates_canon = fs::canonicalize(&updates).ok()?;
    let expected = app_data_canon.join("updates");
    if updates_canon != expected {
        if !updates_canon.starts_with(&app_data_canon)
            || updates_canon.file_name().and_then(|n| n.to_str()) != Some("updates")
        {
            tracing::warn!("updates root escapes app data; refusing updates-root operations");
            return None;
        }
    }
    Some(updates_canon)
}

/// Strict: `updates/` must currently exist as a real directory under app-data.
pub fn updates_root_exists_and_trusted(app_data_dir: &Path) -> bool {
    resolve_trusted_updates_root(app_data_dir, false).is_some()
}

fn is_safe_token_leaf(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_safe_download_artifact_leaf(leaf: &str) -> bool {
    // Prepared download names are `{download_id}.{asset}` and `{...}.part` under updates/.
    // Keep this narrower than "any file": require safe charset + known suffixes.
    if leaf.is_empty() || leaf.len() > 240 {
        return false;
    }
    if leaf.eq_ignore_ascii_case("Resh.exe") || leaf == "Resh.app" {
        return false;
    }
    if !leaf
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return false;
    }
    if leaf.contains("..") {
        return false;
    }
    leaf.ends_with(".part")
        || leaf.ends_with(".dmg")
        || leaf.ends_with(".exe")
        || leaf.ends_with(".ready")
}

fn is_allowed_updates_relative(components: &[String]) -> bool {
    if components.is_empty() || components.len() > 2 {
        return false;
    }
    match components.len() {
        1 => {
            let leaf = components[0].as_str();
            leaf == "install-manifest.json"
                || leaf == "install-manifest.json.part"
                || leaf == "last-install-failure.txt"
                || leaf == "last-install-result.json"
                || leaf
                    .strip_prefix("install-alive-")
                    .and_then(|rest| rest.strip_suffix(".ready"))
                    .is_some_and(is_safe_token_leaf)
                || is_safe_download_artifact_leaf(leaf)
        }
        2 => {
            let parent = components[0].as_str();
            let leaf = components[1].as_str();
            match parent {
                "helpers" => leaf == "apply-windows-update.ps1",
                "restarts" => {
                    if let Some(token) = leaf.strip_suffix(".json.part") {
                        is_safe_token_leaf(token)
                    } else if let Some(token) = leaf.strip_suffix(".json") {
                        is_safe_token_leaf(token)
                    } else {
                        false
                    }
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn intermediate_dirs_are_real(updates: &Path, components: &[String]) -> bool {
    if components.len() < 2 {
        return true;
    }
    // Only one intermediate component is allowed (helpers/ or restarts/).
    let mid = updates.join(&components[0]);
    match fs::symlink_metadata(&mid) {
        Ok(meta) if meta.file_type().is_symlink() => false,
        Ok(meta) if meta.is_dir() => true,
        Ok(_) => false,
        Err(_) => true, // missing mid dir → nothing to delete; treated as success later
    }
}

/// Remove a relative path under a currently trusted app-data `updates/` tree.
///
/// Returns true if the root was trusted and the unlink was attempted (or the
/// leaf was already absent). Returns false when the root is missing, a
/// symlink/junction, or escapes app-data — callers must not fall back to an
/// unprotected delete.
pub fn remove_trusted_updates_relative(app_data_dir: &Path, relative: &Path) -> bool {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
    {
        return false;
    }
    let components: Vec<String> = relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(|s| s.to_string()),
            _ => None,
        })
        .collect();
    if !is_allowed_updates_relative(&components) {
        return false;
    }

    let updates = updates_root(app_data_dir);
    if !updates_root_exists_and_trusted(app_data_dir) {
        return false;
    }
    if !intermediate_dirs_are_real(&updates, &components) {
        return false;
    }

    // Re-validate root immediately before open/unlink to shrink TOCTOU.
    if !updates_root_exists_and_trusted(app_data_dir) {
        return false;
    }
    if components.len() == 2 {
        let mid = updates.join(&components[0]);
        if let Ok(meta) = fs::symlink_metadata(&mid) {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return false;
            }
        } else {
            return true; // intermediate missing — nothing to delete
        }
    }

    let path = updates.join(relative);
    // Refuse if any intermediate component under updates is now a symlink.
    if path_has_symlink_component_under_root(&path, &updates) {
        return false;
    }

    #[cfg(unix)]
    {
        return remove_trusted_updates_relative_unix(&updates, &components);
    }

    #[cfg(not(unix))]
    {
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => {
                let _ = fs::remove_file(&path);
                true
            }
            Ok(_) => true,  // unexpected type; do not delete
            Err(_) => true, // already absent
        }
    }
}

#[cfg(unix)]
fn remove_trusted_updates_relative_unix(updates: &Path, components: &[String]) -> bool {
    use std::ffi::CString;

    let updates_fd = match open_dir_nofollow(updates) {
        Ok(fd) => fd,
        Err(_) => return false,
    };

    let (parent_fd, leaf, close_updates): (i32, &str, bool) = if components.len() == 1 {
        (updates_fd, components[0].as_str(), true)
    } else {
        let mid = components[0].as_str();
        let c_mid = match CString::new(mid) {
            Ok(s) => s,
            Err(_) => {
                let _ = unsafe { libc::close(updates_fd) };
                return false;
            }
        };
        let mid_fd = unsafe {
            libc::openat(
                updates_fd,
                c_mid.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        let _ = unsafe { libc::close(updates_fd) };
        if mid_fd < 0 {
            // Intermediate missing → nothing to delete.
            return true;
        }
        (mid_fd, components[1].as_str(), false)
    };
    let _ = close_updates;

    let c_leaf = match CString::new(leaf) {
        Ok(s) => s,
        Err(_) => {
            let _ = unsafe { libc::close(parent_fd) };
            return false;
        }
    };

    // AT_SYMLINK_NOFOLLOW on fstatat: inspect leaf without following.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let st_rc = unsafe {
        libc::fstatat(
            parent_fd,
            c_leaf.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if st_rc != 0 {
        let _ = unsafe { libc::close(parent_fd) };
        return true; // absent
    }
    let mode = st.st_mode & libc::S_IFMT;
    if mode != libc::S_IFREG && mode != libc::S_IFLNK {
        let _ = unsafe { libc::close(parent_fd) };
        return true; // refuse unexpected types
    }
    let _ = unsafe { libc::unlinkat(parent_fd, c_leaf.as_ptr(), 0) };
    let _ = unsafe { libc::close(parent_fd) };
    true
}

/// True if any path component from updates root down to (but not necessarily
/// including) the leaf is a directory symlink.
/// Returns false when `path` is not under `updates` (check is N/A for live-parent paths).
pub fn path_has_symlink_component_under_root(path: &Path, updates: &Path) -> bool {
    if path_entry_is_symlink(updates) {
        return true;
    }
    // If path is clearly not under the logical updates tree, skip.
    if !path.starts_with(updates) {
        if let (Ok(path_c), Ok(updates_c)) = (fs::canonicalize(path), fs::canonicalize(updates)) {
            if !path_c.starts_with(&updates_c) && path_c != updates_c {
                return false;
            }
            return walk_symlink_components(
                &updates_c,
                path_c.strip_prefix(&updates_c).unwrap_or(Path::new("")),
            );
        }
        // Path may not exist yet (leaf missing): only walk if logical prefix matches.
        return false;
    }
    let rel = match path.strip_prefix(updates) {
        Ok(r) => r,
        Err(_) => return false,
    };
    walk_symlink_components(updates, rel)
}

fn walk_symlink_components(root: &Path, rel: &Path) -> bool {
    let mut cur = root.to_path_buf();
    let comps: Vec<_> = rel.components().collect();
    for (i, c) in comps.iter().enumerate() {
        let Component::Normal(name) = c else {
            return true;
        };
        cur.push(name);
        let is_leaf = i + 1 == comps.len();
        if let Ok(meta) = fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                // Intermediate symlink is always refused; leaf symlink is ok for unlink.
                if !is_leaf {
                    return true;
                }
            }
        }
    }
    false
}

/// Ensure `app-data/updates` exists as a trusted real directory (not a symlink).
/// Used before writing staging files / helpers under updates/.
pub fn ensure_trusted_updates_dir(app_data_dir: &Path) -> Result<PathBuf, String> {
    if app_data_dir_is_symlink_escape(app_data_dir) {
        return Err("App data directory is a symlink; refusing updates write".to_string());
    }
    let updates = updates_root(app_data_dir);
    if updates.exists() {
        if path_entry_is_symlink(&updates) {
            return Err("Updates directory is a symlink; refusing updates write".to_string());
        }
        if !updates.is_dir() {
            return Err("Updates path exists but is not a directory".to_string());
        }
    }
    fs::create_dir_all(&updates).map_err(|e| format!("create updates dir: {e}"))?;
    if !updates_root_exists_and_trusted(app_data_dir) {
        return Err("Updates directory is no longer trusted after create".to_string());
    }
    Ok(updates)
}

/// Ensure `app-data/updates/<subdir>` exists as a real (non-symlink) directory.
pub fn ensure_trusted_updates_subdir(app_data_dir: &Path, subdir: &str) -> Result<PathBuf, String> {
    if subdir != "helpers" && subdir != "restarts" {
        return Err(format!("disallowed updates subdir '{subdir}'"));
    }
    let updates = ensure_trusted_updates_dir(app_data_dir)?;

    #[cfg(unix)]
    {
        // Create/open intermediate relative to an O_NOFOLLOW updates fd so a
        // concurrent updates/ → symlink swap cannot redirect mkdir.
        let updates_fd = open_dir_nofollow(&updates)?;
        let mid_fd = match open_or_mkdir_subdir_nofollow(updates_fd, subdir) {
            Ok(fd) => fd,
            Err(e) => {
                let _ = unsafe { libc::close(updates_fd) };
                return Err(e);
            }
        };
        let _ = unsafe { libc::close(updates_fd) };
        let _ = unsafe { libc::close(mid_fd) };
        if !updates_root_exists_and_trusted(app_data_dir) {
            return Err("Updates directory is no longer trusted after subdir create".to_string());
        }
        return Ok(updates.join(subdir));
    }

    #[cfg(not(unix))]
    {
        let dir = updates.join(subdir);
        if dir.exists() {
            if path_entry_is_symlink(&dir) {
                return Err(format!("Updates {subdir}/ is a symlink; refusing write"));
            }
            if !dir.is_dir() {
                return Err(format!("Updates {subdir}/ exists but is not a directory"));
            }
        }
        fs::create_dir_all(&dir).map_err(|e| format!("create updates/{subdir}: {e}"))?;
        if path_entry_is_symlink(&dir) || !dir.is_dir() {
            return Err(format!(
                "Updates {subdir}/ is no longer trusted after create"
            ));
        }
        if !updates_root_exists_and_trusted(app_data_dir) {
            return Err("Updates directory is no longer trusted after subdir create".to_string());
        }
        Ok(dir)
    }
}

/// Create/truncate a file under trusted `updates/` without following intermediate
/// directory links. Relative path may be a single leaf or `helpers|restarts/<leaf>`.
///
/// On Unix this opens the parent directory with `O_DIRECTORY|O_NOFOLLOW` and
/// creates the leaf with `openat(..., O_NOFOLLOW|O_CREAT|O_TRUNC)`. On other
/// platforms it re-validates the trusted root immediately before create.
pub fn create_trusted_updates_file(
    app_data_dir: &Path,
    relative: &Path,
) -> Result<fs::File, String> {
    let components = parse_allowed_relative(relative)?;
    let updates = ensure_trusted_updates_dir(app_data_dir)?;

    #[cfg(unix)]
    {
        return create_trusted_updates_file_unix(&updates, &components);
    }

    #[cfg(not(unix))]
    {
        create_trusted_updates_file_portable(app_data_dir, &updates, &components)
    }
}

/// Write all bytes to a trusted updates-relative file (create/truncate).
pub fn write_trusted_updates_file(
    app_data_dir: &Path,
    relative: &Path,
    data: &[u8],
) -> Result<(), String> {
    let mut file = create_trusted_updates_file(app_data_dir, relative)?;
    file.write_all(data)
        .map_err(|e| format!("write trusted updates file: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("fsync trusted updates file: {e}"))?;
    Ok(())
}

/// Atomically write `relative` via `relative.part` then rename, both under the
/// trusted updates tree. Parent intermediate dirs are created when needed.
pub fn write_trusted_updates_file_atomic(
    app_data_dir: &Path,
    relative: &Path,
    data: &[u8],
) -> Result<(), String> {
    let components = parse_allowed_relative(relative)?;
    let part_name = format!("{}.part", components.last().ok_or("empty relative")?);
    let mut part_components = components[..components.len().saturating_sub(1)].to_vec();
    part_components.push(part_name);
    // Temporarily allow the .part leaf for write even if not in delete allowlist
    // as a single-component download leaf — restarts/*.json.part is already allowed.
    let part_rel = PathBuf::from_iter(part_components.iter().map(|s| s.as_str()));
    write_trusted_updates_file(app_data_dir, &part_rel, data)?;
    rename_trusted_updates_relative(app_data_dir, &part_rel, relative)?;
    Ok(())
}

/// Rename within a trusted updates tree without following intermediate links.
pub fn rename_trusted_updates_relative(
    app_data_dir: &Path,
    from_relative: &Path,
    to_relative: &Path,
) -> Result<(), String> {
    let from_components = parse_allowed_relative_or_part(from_relative)?;
    let to_components = parse_allowed_relative_or_part(to_relative)?;
    if from_components.len() != to_components.len() {
        return Err("rename source/dest depth mismatch".to_string());
    }
    if from_components.len() == 2 && from_components[0] != to_components[0] {
        return Err("rename cannot cross updates subdirectories".to_string());
    }

    let updates = ensure_trusted_updates_dir(app_data_dir)?;

    #[cfg(unix)]
    {
        return rename_trusted_updates_unix(&updates, &from_components, &to_components);
    }

    #[cfg(not(unix))]
    {
        rename_trusted_updates_portable(app_data_dir, &updates, &from_components, &to_components)
    }
}

fn parse_allowed_relative(relative: &Path) -> Result<Vec<String>, String> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
    {
        return Err("invalid updates-relative path".to_string());
    }
    let components: Vec<String> = relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(|s| s.to_string()),
            _ => None,
        })
        .collect();
    if !is_allowed_updates_relative(&components)
        && !is_allowed_updates_relative_write_extra(&components)
    {
        return Err(format!(
            "updates-relative path not allowlisted: {}",
            relative.display()
        ));
    }
    Ok(components)
}

/// Like [`parse_allowed_relative`], but also accepts intermediate `.part` names
/// used only for atomic promote of allowlisted leaves.
fn parse_allowed_relative_or_part(relative: &Path) -> Result<Vec<String>, String> {
    if let Ok(c) = parse_allowed_relative(relative) {
        return Ok(c);
    }
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
    {
        return Err("invalid updates-relative path".to_string());
    }
    let components: Vec<String> = relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str().map(|s| s.to_string()),
            _ => None,
        })
        .collect();
    if components.len() == 1 {
        let leaf = components[0].as_str();
        if let Some(base) = leaf.strip_suffix(".part") {
            if is_allowed_updates_relative(&[base.to_string()])
                || is_allowed_updates_relative_write_extra(&[base.to_string()])
            {
                return Ok(components);
            }
        }
    }
    if components.len() == 2 {
        let parent = components[0].as_str();
        let leaf = components[1].as_str();
        if parent == "helpers" || parent == "restarts" {
            if let Some(base) = leaf.strip_suffix(".part") {
                let base_comps = vec![parent.to_string(), base.to_string()];
                if is_allowed_updates_relative(&base_comps)
                    || is_allowed_updates_relative_write_extra(&base_comps)
                {
                    return Ok(components);
                }
            }
        }
    }
    Err(format!(
        "updates-relative path not allowlisted: {}",
        relative.display()
    ))
}

/// Extra write-only allowlist entries (e.g. helper script part files).
fn is_allowed_updates_relative_write_extra(components: &[String]) -> bool {
    if components.len() == 2 && components[0] == "helpers" {
        let leaf = components[1].as_str();
        return leaf == "apply-windows-update.ps1" || leaf == "apply-windows-update.ps1.part";
    }
    false
}

#[cfg(unix)]
fn open_dir_nofollow(path: &Path) -> Result<std::os::unix::io::RawFd, String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "path contains interior NUL".to_string())?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    let fd = unsafe { libc::open(c_path.as_ptr(), flags) };
    if fd < 0 {
        return Err(format!(
            "open dir {}: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    // Validate the opened fd is still a real directory (not a sneaky link race).
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        let _ = unsafe { libc::close(fd) };
        return Err(format!(
            "fstat dir {}: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    if (st.st_mode & libc::S_IFMT) != libc::S_IFDIR {
        let _ = unsafe { libc::close(fd) };
        return Err(format!("{} is not a directory after open", path.display()));
    }
    Ok(fd)
}

#[cfg(unix)]
fn open_or_mkdir_subdir_nofollow(parent_fd: i32, name: &str) -> Result<i32, String> {
    use std::ffi::CString;

    let c_name = CString::new(name).map_err(|_| "invalid intermediate name".to_string())?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    let mut mid_fd = unsafe { libc::openat(parent_fd, c_name.as_ptr(), flags) };
    if mid_fd < 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ENOENT) {
            return Err(format!("open intermediate {name}: {err}"));
        }
        // Create intermediate relative to the already-validated parent fd.
        let rc = unsafe { libc::mkdirat(parent_fd, c_name.as_ptr(), 0o700) };
        if rc != 0 {
            let mkdir_err = io::Error::last_os_error();
            // Another process may have created it; only continue on EEXIST.
            if mkdir_err.raw_os_error() != Some(libc::EEXIST) {
                return Err(format!("mkdirat intermediate {name}: {mkdir_err}"));
            }
        }
        mid_fd = unsafe { libc::openat(parent_fd, c_name.as_ptr(), flags) };
        if mid_fd < 0 {
            return Err(format!(
                "open intermediate {name} after mkdir: {}",
                io::Error::last_os_error()
            ));
        }
    }
    // Confirm fd is a real directory (O_NOFOLLOW already rejected a symlink node).
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(mid_fd, &mut st) } != 0 {
        let _ = unsafe { libc::close(mid_fd) };
        return Err(format!(
            "fstat intermediate {name}: {}",
            io::Error::last_os_error()
        ));
    }
    if (st.st_mode & libc::S_IFMT) != libc::S_IFDIR {
        let _ = unsafe { libc::close(mid_fd) };
        return Err(format!("intermediate {name} is not a directory"));
    }
    Ok(mid_fd)
}

#[cfg(unix)]
fn create_trusted_updates_file_unix(
    updates: &Path,
    components: &[String],
) -> Result<fs::File, String> {
    use std::ffi::CString;
    use std::os::unix::io::{FromRawFd, RawFd};

    if components.is_empty() || components.len() > 2 {
        return Err("invalid relative depth for trusted create".to_string());
    }

    let updates_fd = open_dir_nofollow(updates)?;
    let (parent_fd, leaf): (RawFd, &str) = if components.len() == 1 {
        (updates_fd, components[0].as_str())
    } else {
        let mid_name = components[0].as_str();
        let mid_fd = match open_or_mkdir_subdir_nofollow(updates_fd, mid_name) {
            Ok(fd) => fd,
            Err(e) => {
                let _ = unsafe { libc::close(updates_fd) };
                return Err(e);
            }
        };
        let _ = unsafe { libc::close(updates_fd) };
        (mid_fd, components[1].as_str())
    };

    let c_leaf = CString::new(leaf).map_err(|_| "invalid leaf name".to_string())?;
    // O_NOFOLLOW on create: if leaf is a symlink, open fails instead of following.
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    let file_fd = unsafe { libc::openat(parent_fd, c_leaf.as_ptr(), flags, 0o600) };
    let _ = unsafe { libc::close(parent_fd) };
    if file_fd < 0 {
        return Err(format!(
            "openat create {leaf}: {}",
            io::Error::last_os_error()
        ));
    }
    // Refuse if we somehow opened a non-regular file.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(file_fd, &mut st) } != 0 {
        let _ = unsafe { libc::close(file_fd) };
        return Err(format!(
            "fstat created file: {}",
            io::Error::last_os_error()
        ));
    }
    if (st.st_mode & libc::S_IFMT) != libc::S_IFREG {
        let _ = unsafe { libc::close(file_fd) };
        return Err("created updates path is not a regular file".to_string());
    }
    // Harden mode via the open fd (no path re-resolution).
    let _ = unsafe { libc::fchmod(file_fd, 0o600) };
    Ok(unsafe { fs::File::from_raw_fd(file_fd) })
}

#[cfg(unix)]
fn rename_trusted_updates_unix(
    updates: &Path,
    from: &[String],
    to: &[String],
) -> Result<(), String> {
    use std::ffi::CString;

    let parent_name = if from.len() == 2 {
        Some(from[0].as_str())
    } else {
        None
    };
    let updates_fd = open_dir_nofollow(updates)?;
    let parent_fd = if let Some(mid) = parent_name {
        let c_mid = CString::new(mid).map_err(|_| "invalid intermediate name".to_string())?;
        let mid_fd = unsafe {
            libc::openat(
                updates_fd,
                c_mid.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        let _ = unsafe { libc::close(updates_fd) };
        if mid_fd < 0 {
            return Err(format!(
                "open intermediate {mid} for rename: {}",
                io::Error::last_os_error()
            ));
        }
        mid_fd
    } else {
        updates_fd
    };

    let from_leaf = from.last().map(|s| s.as_str()).unwrap_or("");
    let to_leaf = to.last().map(|s| s.as_str()).unwrap_or("");
    let c_from = CString::new(from_leaf).map_err(|_| "invalid from leaf".to_string())?;
    let c_to = CString::new(to_leaf).map_err(|_| "invalid to leaf".to_string())?;
    let rc = unsafe { libc::renameat(parent_fd, c_from.as_ptr(), parent_fd, c_to.as_ptr()) };
    let _ = unsafe { libc::close(parent_fd) };
    if rc != 0 {
        return Err(format!(
            "renameat {from_leaf} -> {to_leaf}: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_trusted_updates_file_portable(
    app_data_dir: &Path,
    updates: &Path,
    components: &[String],
) -> Result<fs::File, String> {
    if components.len() == 2 {
        let mid = updates.join(&components[0]);
        if mid.exists() {
            if path_entry_is_symlink(&mid) || !mid.is_dir() {
                return Err(format!("intermediate dir {} is not trusted", components[0]));
            }
        } else {
            fs::create_dir_all(&mid).map_err(|e| format!("create intermediate: {e}"))?;
            if path_entry_is_symlink(&mid) || !mid.is_dir() {
                return Err(format!(
                    "intermediate dir {} is no longer trusted",
                    components[0]
                ));
            }
        }
    }
    // Re-check immediately before open to shrink TOCTOU.
    if !updates_root_exists_and_trusted(app_data_dir) {
        return Err("Updates directory is no longer trusted before write".to_string());
    }
    if has_intermediate_symlink_ancestor(
        &updates.join(PathBuf::from_iter(components.iter().map(|s| s.as_str()))),
    ) {
        return Err("refusing write: intermediate path component is a symlink".to_string());
    }
    let path = updates.join(PathBuf::from_iter(components.iter().map(|s| s.as_str())));
    // Refuse pre-existing leaf symlink (would be followed by create/truncate on some FS).
    if path_entry_is_symlink(&path) {
        return Err("refusing write: leaf path is a symlink".to_string());
    }
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| format!("create trusted updates file: {e}"))
}

#[cfg(not(unix))]
fn rename_trusted_updates_portable(
    app_data_dir: &Path,
    updates: &Path,
    from: &[String],
    to: &[String],
) -> Result<(), String> {
    if !updates_root_exists_and_trusted(app_data_dir) {
        return Err("Updates directory is no longer trusted before rename".to_string());
    }
    let from_path = updates.join(PathBuf::from_iter(from.iter().map(|s| s.as_str())));
    let to_path = updates.join(PathBuf::from_iter(to.iter().map(|s| s.as_str())));
    if has_intermediate_symlink_ancestor(&from_path) || has_intermediate_symlink_ancestor(&to_path)
    {
        return Err("refusing rename: intermediate path component is a symlink".to_string());
    }
    if path_entry_is_symlink(&from_path) {
        // Leaf symlink rename is OK (moves the link node), but refuse if parent
        // chain already failed above.
    }
    if path_entry_is_symlink(&to_path) {
        return Err("refusing rename: destination leaf is a symlink".to_string());
    }
    fs::rename(&from_path, &to_path).map_err(|e| format!("rename trusted updates file: {e}"))
}

/// Delete a backup/staging/co-located leaf under a live install parent without
/// following directory symlinks (including recursive `.app` trees on Unix).
///
/// `leaf_path` must be a direct child of `live_parent` (exact basename only).
pub fn remove_live_parent_artifact_nofollow(live_parent: &Path, leaf_path: &Path) -> bool {
    let Some(leaf) = leaf_path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if leaf.is_empty() || leaf == "." || leaf == ".." || leaf.contains('/') || leaf.contains('\\') {
        return false;
    }
    // Refuse live product names even if a caller mis-binds.
    if leaf.eq_ignore_ascii_case("Resh.exe") || leaf == "Resh.app" {
        return false;
    }
    let Some(parent) = leaf_path.parent() else {
        return false;
    };
    // Parent must match the live install parent (logical or canonical).
    let parent_ok = parent == live_parent
        || fs::canonicalize(parent).ok().as_ref() == Some(&live_parent.to_path_buf())
        || (live_parent.exists()
            && fs::canonicalize(live_parent).ok().as_ref() == Some(&parent.to_path_buf()));
    if !parent_ok {
        // Also accept when both canonicalize equal.
        let both = match (fs::canonicalize(parent), fs::canonicalize(live_parent)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        if !both {
            return false;
        }
    }
    // Live parent itself must not be a symlink node.
    if path_entry_is_symlink(live_parent) || path_entry_is_symlink(parent) {
        tracing::warn!("refusing live-parent delete: parent is a symlink");
        return false;
    }

    #[cfg(unix)]
    {
        return remove_live_parent_artifact_unix(live_parent, leaf);
    }

    #[cfg(not(unix))]
    {
        // Re-check intermediate ancestors then delete leaf file/dir without following
        // a directory symlink at the leaf (unlink symlink node or refuse recursive
        // follow via remove_path_nofollow_best_effort).
        if has_intermediate_symlink_ancestor(leaf_path) {
            return false;
        }
        remove_path_nofollow_best_effort(leaf_path);
        true
    }
}

#[cfg(unix)]
fn remove_live_parent_artifact_unix(live_parent: &Path, leaf: &str) -> bool {
    use std::ffi::CString;

    let parent_fd = match open_dir_nofollow(live_parent) {
        Ok(fd) => fd,
        Err(_) => return false,
    };
    let c_leaf = match CString::new(leaf) {
        Ok(s) => s,
        Err(_) => {
            let _ = unsafe { libc::close(parent_fd) };
            return false;
        }
    };
    let ok = unlink_tree_at(parent_fd, &c_leaf);
    let _ = unsafe { libc::close(parent_fd) };
    ok
}

/// Recursively unlink `name` relative to `parent_fd` without following directory
/// or leaf symlinks into external trees. Directory symlinks are unlinked as nodes.
#[cfg(unix)]
fn unlink_tree_at(parent_fd: i32, name: &std::ffi::CStr) -> bool {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let st_rc =
        unsafe { libc::fstatat(parent_fd, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
    if st_rc != 0 {
        return true; // already absent
    }
    let mode = st.st_mode & libc::S_IFMT;
    if mode == libc::S_IFLNK || mode == libc::S_IFREG {
        let _ = unsafe { libc::unlinkat(parent_fd, name.as_ptr(), 0) };
        return true;
    }
    if mode != libc::S_IFDIR {
        // Refuse unexpected types (fifo/socket/device).
        return true;
    }

    // Directory: open with O_NOFOLLOW so a concurrent swap to symlink fails closed.
    let dir_fd = unsafe {
        libc::openat(
            parent_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if dir_fd < 0 {
        // Could not open as real directory (race to symlink) — do not follow.
        return false;
    }

    // Read directory entries via fdopendir (takes ownership of dir_fd on success).
    let dir = unsafe { libc::fdopendir(dir_fd) };
    if dir.is_null() {
        let _ = unsafe { libc::close(dir_fd) };
        return false;
    }

    loop {
        // readdir is not thread-safe for the same DIR*; cleanup is single-threaded.
        let entry = unsafe { libc::readdir(dir) };
        if entry.is_null() {
            break;
        }
        let d_name = unsafe { (*entry).d_name.as_ptr() };
        // Skip "." / ".."
        let name_bytes = unsafe { std::ffi::CStr::from_ptr(d_name) }.to_bytes();
        if name_bytes == b"." || name_bytes == b".." {
            continue;
        }
        let child = unsafe { std::ffi::CStr::from_ptr(d_name) };
        // Recurse relative to the opened directory fd. Need a separate open for
        // the parent of children — use dirfd.
        let child_parent = unsafe { libc::dirfd(dir) };
        if child_parent < 0 {
            continue;
        }
        let _ = unlink_tree_at(child_parent, child);
    }
    let _ = unsafe { libc::closedir(dir) };

    // Remove the now-empty directory node relative to original parent.
    let _ = unsafe { libc::unlinkat(parent_fd, name.as_ptr(), libc::AT_REMOVEDIR) };
    true
}

/// Delete a prepared download path safely.
///
/// - When the file is a direct leaf under `…/updates/`, require a trusted
///   updates root and use the allowlisted relative delete (no intermediate
///   symlink follow). If the root is untrusted, refuse — do not fall back.
/// - Otherwise (Windows co-located EXE staging), use nofollow unlink that also
///   refuses intermediate symlink ancestors.
pub fn remove_prepared_download_path(path: &Path) {
    if let Some(parent) = path.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some("updates") {
            if let (Some(app_data), Some(name)) = (parent.parent(), path.file_name()) {
                let relative = Path::new(name);
                if remove_trusted_updates_relative(app_data, relative) {
                    return;
                }
                // Path looks like updates/<leaf> but root is missing/symlink/untrusted:
                // never fall through to a plain remove that could follow the link.
                if path_entry_is_symlink(parent)
                    || app_data_dir_is_symlink_escape(app_data)
                    || !updates_root_exists_and_trusted(app_data)
                {
                    tracing::warn!(
                        "refusing prepared download delete: untrusted updates root for {}",
                        path.display()
                    );
                    return;
                }
            }
        }
    }
    remove_file_nofollow_best_effort(path);
}

/// Unlink a regular file or leaf symlink without following directory links.
/// Used for prepared-download artifacts that may live under updates or next to
/// the Windows EXE.
///
/// Refuses the delete when any ancestor directory is a symlink/junction, so a
/// redirected `updates/` (or any intermediate link) cannot cause the unlink to
/// land on an external file of the same basename.
pub fn remove_file_nofollow_best_effort(path: &Path) {
    if has_intermediate_symlink_ancestor(path) {
        tracing::warn!("refusing file delete: intermediate path component is a symlink");
        return;
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => {
            let _ = fs::remove_file(path);
        }
        Ok(_) => {
            // Refuse to remove directories via this helper.
        }
        Err(_) => {}
    }
}

/// True when a near ancestor of `path` is a symlink.
///
/// Only walks a small number of parents (enough for `updates/` and
/// `updates/helpers|restarts/` nesting). Walking to the filesystem root would
/// false-positive on macOS system links such as `/var` → `/private/var`.
fn has_intermediate_symlink_ancestor(path: &Path) -> bool {
    let mut cur = path.parent();
    for _ in 0..4 {
        let Some(p) = cur else {
            return false;
        };
        if p.as_os_str().is_empty() {
            return false;
        }
        if path_entry_is_symlink(p) {
            return true;
        }
        cur = p.parent();
    }
    false
}

/// Delete without following directory symlinks (`remove_dir_all` does follow).
pub fn remove_path_nofollow_best_effort(path: &Path) {
    if has_intermediate_symlink_ancestor(path) {
        tracing::warn!("refusing path delete: intermediate path component is a symlink");
        return;
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => {
            // Missing leaf: still refuse if ancestors are links (already checked).
            return;
        }
    };
    if meta.file_type().is_symlink() {
        let _ = fs::remove_file(path);
        return;
    }
    if meta.is_dir() {
        remove_dir_all_nofollow(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn remove_dir_all_nofollow(dir: &Path) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            let _ = fs::remove_dir(dir);
            return;
        }
    };
    for entry in entries.flatten() {
        let child = entry.path();
        let child_meta = match fs::symlink_metadata(&child) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if child_meta.file_type().is_symlink() {
            let _ = fs::remove_file(&child);
        } else if child_meta.is_dir() {
            remove_dir_all_nofollow(&child);
        } else {
            let _ = fs::remove_file(&child);
        }
    }
    let _ = fs::remove_dir(dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn refuses_missing_updates_root_for_deletes() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        assert!(!app_data.join("updates").exists());
        assert!(!updates_root_exists_and_trusted(&app_data));
        assert!(!remove_trusted_updates_relative(
            &app_data,
            Path::new("last-install-result.json")
        ));
        assert!(resolve_trusted_updates_root(&app_data, false).is_none());
        // allow_missing may return a logical path for write bootstrapping, but
        // delete auth must use allow_missing=false.
        assert!(resolve_trusted_updates_root(&app_data, true).is_some());
    }

    #[test]
    fn refuses_symlink_updates_root_for_result_markers() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("last-install-result.json");
        fs::write(&secret, b"keep").unwrap();
        symlink(&external, app_data.join("updates")).unwrap();

        assert!(!remove_trusted_updates_relative(
            &app_data,
            Path::new("last-install-result.json")
        ));
        assert!(secret.exists());
    }

    #[test]
    fn refuses_symlink_app_data_root() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let real_app = tmp.path().join("real-app");
        let external = tmp.path().join("external");
        fs::create_dir_all(real_app.join("updates")).unwrap();
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("last-install-failure.txt");
        fs::write(&secret, b"keep").unwrap();
        // Logical app-data is a symlink to external; plant matching names there.
        let logical = tmp.path().join("logical-app");
        symlink(&external, &logical).unwrap();
        fs::write(external.join("last-install-failure.txt"), b"keep").unwrap();

        assert!(app_data_dir_is_symlink_escape(&logical));
        assert!(!updates_root_exists_and_trusted(&logical));
        assert!(!remove_trusted_updates_relative(
            &logical,
            Path::new("last-install-failure.txt")
        ));
        assert!(secret.exists());
    }

    #[test]
    fn deletes_legitimate_alive_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        let updates = app_data.join("updates");
        fs::create_dir_all(&updates).unwrap();
        let name = "install-alive-abc-123.ready";
        let path = updates.join(name);
        fs::write(&path, b"alive\n").unwrap();
        assert!(remove_trusted_updates_relative(&app_data, Path::new(name)));
        assert!(!path.exists());
    }

    #[test]
    fn deletes_legitimate_restart_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        let restarts = app_data.join("updates").join("restarts");
        fs::create_dir_all(&restarts).unwrap();
        let path = restarts.join("tok_1.json");
        fs::write(&path, b"{}").unwrap();
        assert!(remove_trusted_updates_relative(
            &app_data,
            Path::new("restarts/tok_1.json")
        ));
        assert!(!path.exists());
    }

    #[test]
    fn refuses_restarts_when_restarts_dir_is_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        let updates = app_data.join("updates");
        fs::create_dir_all(&updates).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("tok_1.json");
        fs::write(&secret, b"keep").unwrap();
        symlink(&external, updates.join("restarts")).unwrap();

        assert!(!remove_trusted_updates_relative(
            &app_data,
            Path::new("restarts/tok_1.json")
        ));
        assert!(secret.exists());
    }

    #[test]
    fn nofollow_remove_refuses_when_parent_is_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("payload.part");
        fs::write(&secret, b"keep").unwrap();
        let logical = tmp.path().join("logical-updates");
        symlink(&external, &logical).unwrap();
        let target = logical.join("payload.part");
        remove_file_nofollow_best_effort(&target);
        assert!(
            secret.exists(),
            "must not delete external file through intermediate updates symlink"
        );
    }

    #[test]
    fn prepared_download_delete_refuses_symlink_updates_root() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let leaf = "dlid.Resh-v1.0.0-macos-aarch64.dmg";
        let secret = external.join(leaf);
        fs::write(&secret, b"keep").unwrap();
        symlink(&external, app_data.join("updates")).unwrap();

        let target = app_data.join("updates").join(leaf);
        remove_prepared_download_path(&target);
        assert!(
            secret.exists(),
            "prepared delete must not follow symlink updates/ to external ready file"
        );

        let part_leaf = format!("{leaf}.part");
        let secret_part = external.join(&part_leaf);
        fs::write(&secret_part, b"keep-part").unwrap();
        remove_prepared_download_path(&app_data.join("updates").join(&part_leaf));
        assert!(
            secret_part.exists(),
            "prepared delete must not follow symlink updates/ to external .part"
        );
    }

    #[test]
    fn prepared_download_delete_removes_legitimate_updates_leaf() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        let updates = app_data.join("updates");
        fs::create_dir_all(&updates).unwrap();
        let leaf = "dlid.Resh-v1.0.0-macos-aarch64.dmg";
        let path = updates.join(leaf);
        fs::write(&path, b"ready").unwrap();
        remove_prepared_download_path(&path);
        assert!(!path.exists());
    }

    #[test]
    fn ensure_trusted_updates_dir_refuses_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        symlink(&external, app_data.join("updates")).unwrap();
        assert!(ensure_trusted_updates_dir(&app_data).is_err());
    }

    #[test]
    fn ensure_trusted_updates_dir_creates_real_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        let updates = ensure_trusted_updates_dir(&app_data).unwrap();
        assert!(updates.is_dir());
        assert!(!path_entry_is_symlink(&updates));
        assert!(updates_root_exists_and_trusted(&app_data));
    }

    #[test]
    fn trusted_write_refuses_when_updates_is_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("install-alive-tok.ready");
        fs::write(&secret, b"keep").unwrap();
        symlink(&external, app_data.join("updates")).unwrap();

        let err =
            write_trusted_updates_file(&app_data, Path::new("install-alive-tok.ready"), b"alive\n");
        assert!(err.is_err());
        assert_eq!(fs::read(&secret).unwrap(), b"keep");
    }

    #[test]
    fn trusted_write_refuses_preexisting_leaf_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        let updates = app_data.join("updates");
        fs::create_dir_all(&updates).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("target");
        fs::write(&secret, b"keep").unwrap();
        symlink(&secret, updates.join("install-alive-tok.ready")).unwrap();

        let err = write_trusted_updates_file(
            &app_data,
            Path::new("install-alive-tok.ready"),
            b"overwrite\n",
        );
        assert!(err.is_err(), "must refuse writing through leaf symlink");
        assert_eq!(fs::read(&secret).unwrap(), b"keep");
    }

    #[test]
    fn trusted_write_creates_regular_alive_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        write_trusted_updates_file(&app_data, Path::new("install-alive-tok.ready"), b"alive\n")
            .unwrap();
        let path = app_data.join("updates").join("install-alive-tok.ready");
        assert!(path.is_file());
        assert!(!path_entry_is_symlink(&path));
        assert_eq!(fs::read(&path).unwrap(), b"alive\n");
    }

    #[test]
    fn trusted_write_creates_helpers_via_mkdirat() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        write_trusted_updates_file(
            &app_data,
            Path::new("helpers/apply-windows-update.ps1"),
            b"# helper\n",
        )
        .unwrap();
        let helpers = app_data.join("updates").join("helpers");
        let script = helpers.join("apply-windows-update.ps1");
        assert!(helpers.is_dir());
        assert!(!path_entry_is_symlink(&helpers));
        assert!(script.is_file());
        assert!(!path_entry_is_symlink(&script));
        assert_eq!(fs::read_to_string(&script).unwrap(), "# helper\n");
    }

    #[test]
    fn trusted_write_refuses_helpers_when_helpers_is_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        let updates = app_data.join("updates");
        fs::create_dir_all(&updates).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("apply-windows-update.ps1");
        fs::write(&secret, b"keep").unwrap();
        symlink(&external, updates.join("helpers")).unwrap();

        let err = write_trusted_updates_file(
            &app_data,
            Path::new("helpers/apply-windows-update.ps1"),
            b"overwrite\n",
        );
        assert!(err.is_err());
        assert_eq!(fs::read(&secret).unwrap(), b"keep");
    }

    #[test]
    fn live_parent_artifact_delete_removes_regular_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("Applications");
        fs::create_dir_all(&live).unwrap();
        let backup = live.join("Resh.backup.v1.2.3.abcd1234.app");
        fs::create_dir_all(backup.join("Contents")).unwrap();
        fs::write(backup.join("Contents").join("Info.plist"), b"x").unwrap();
        assert!(remove_live_parent_artifact_nofollow(&live, &backup));
        assert!(!backup.exists());
    }

    #[test]
    fn live_parent_artifact_delete_unlinks_dir_symlink_without_following() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("Applications");
        fs::create_dir_all(&live).unwrap();
        let external = tmp.path().join("external-app");
        fs::create_dir_all(external.join("Contents")).unwrap();
        let secret = external.join("Contents").join("keep");
        fs::write(&secret, b"keep").unwrap();
        let link = live.join("Resh.backup.v1.2.3.abcd1234.app");
        symlink(&external, &link).unwrap();
        assert!(remove_live_parent_artifact_nofollow(&live, &link));
        assert!(!link.exists(), "symlink node should be unlinked");
        assert!(
            secret.exists(),
            "must not recurse into external via dir symlink"
        );
    }

    #[test]
    fn live_parent_artifact_refuses_wrong_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let live = tmp.path().join("Applications");
        let other = tmp.path().join("Other");
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&other).unwrap();
        let target = other.join("Resh.backup.v1.2.3.abcd1234.app");
        fs::create_dir_all(&target).unwrap();
        assert!(!remove_live_parent_artifact_nofollow(&live, &target));
        assert!(target.exists());
    }

    #[test]
    fn trusted_atomic_write_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app");
        fs::create_dir_all(&app_data).unwrap();
        write_trusted_updates_file_atomic(
            &app_data,
            Path::new("install-manifest.json"),
            b"{\"ok\":true}",
        )
        .unwrap();
        let path = app_data.join("updates").join("install-manifest.json");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"ok\":true}");
        assert!(!app_data
            .join("updates")
            .join("install-manifest.json.part")
            .exists());
    }

    #[test]
    fn nofollow_remove_does_not_follow_dir_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let external = tmp.path().join("external");
        fs::create_dir_all(&external).unwrap();
        let secret = external.join("secret.txt");
        {
            let mut f = fs::File::create(&secret).unwrap();
            f.write_all(b"secret").unwrap();
        }
        let link = tmp.path().join("link-dir");
        symlink(&external, &link).unwrap();
        remove_path_nofollow_best_effort(&link);
        assert!(secret.exists(), "must not follow dir symlink into external");
        assert!(!link.exists(), "symlink node itself may be unlinked");
    }
}
