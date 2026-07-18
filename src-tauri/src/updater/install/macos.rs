//! macOS DMG install: validate, swap Resh.app, clear quarantine, relaunch.
//!
//! Only compiled on macOS. Security constraints:
//! - Validate bundle id / version / arch **before** any elevate/replace/xattr
//! - `/usr/bin/xattr -dr com.apple.quarantine` only (never `-c`/`-cr`)
//! - Quarantine clear + recheck must succeed or rollback (no `|| true`)
//! - No spctl master-disable / global Gatekeeper changes
//! - Helper body is streamed on stdin to `/bin/sh -s` (not a user-writable
//!   admin script file). Privilege elevation uses fixed AppleScript templates
//!   only, with realpath + parent/name checks before each elevated op.
//! - New process must write an alive marker; helper waits before treating
//!   launch as success, otherwise rolls back.

#![cfg(target_os = "macos")]

use super::manifest::{write_install_manifest, InstallManifest};
use super::result::{install_alive_marker_path, INSTALL_ALIVE_WAIT_SECS};
use super::{sanitize_path_component, PreparedInstallContext};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

const BUNDLE_ID: &str = "com.fonlan.resh";

pub fn preflight_and_spawn(ctx: &PreparedInstallContext) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let app_bundle = current_macos_app_bundle_from_exe(&current_exe).ok_or_else(|| {
        "Current installation is not a standard Resh.app bundle. Move Resh to Applications and try again.".to_string()
    })?;

    // Resolve real path (may fail under translocation — treat as blocked).
    let app_bundle = fs::canonicalize(&app_bundle).unwrap_or(app_bundle);
    reject_non_upgradable_location(&app_bundle)?;

    if !ctx.staged_path.is_file() {
        return Err("Staged DMG is missing".to_string());
    }
    let staged = fs::canonicalize(&ctx.staged_path).unwrap_or_else(|_| ctx.staged_path.clone());
    if staged.extension().and_then(|s| s.to_str()) != Some("dmg") {
        return Err("Staged update is not a DMG package".to_string());
    }

    let parent = app_bundle
        .parent()
        .ok_or_else(|| "Cannot resolve application parent directory".to_string())?;

    // Writable parent is preferred; non-writable standard locations (e.g. /Applications
    // for a non-admin user) may still proceed and elevate via fixed osascript templates
    // after DMG validation. Translocation / DMG mounts are already rejected above.
    let parent_writable = probe_dir_writable(parent).is_ok();

    let version_comp = sanitize_path_component(&ctx.prepared.version, "version")?;
    let token_short: String = Uuid::new_v4().to_string().chars().take(8).collect();
    let backup_name = format!("Resh.backup.v{version_comp}.{token_short}.app");
    let new_name = format!("Resh.staging.v{version_comp}.{token_short}.app");
    let backup_path = parent.join(&backup_name);
    let new_path = parent.join(&new_name);
    if backup_path.exists() || new_path.exists() {
        return Err("Update staging path already exists; refuse to overwrite".to_string());
    }

    if ctx.failure_path.parent().is_some() {
        let _ = crate::updater::paths::ensure_trusted_updates_dir(&ctx.app_data_dir);
    }

    let alive_path = install_alive_marker_path(&ctx.app_data_dir, &ctx.restore_token);
    // Clear stale marker only through the trusted updates-root primitive.
    let _ = super::manifest::remove_trusted_updates_relative(
        &ctx.app_data_dir,
        Path::new(&format!("install-alive-{}.ready", ctx.restore_token)),
    );

    let arch = current_process_arch();
    let old_pid = std::process::id().to_string();
    let install_parent = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());

    // Record exact paths for post-ack cleanup (no glob deletes).
    // Note: no on-disk helper script — body is fed via stdin.
    let mut cleanup_paths = vec![
        backup_path.to_string_lossy().to_string(),
        new_path.to_string_lossy().to_string(),
        staged.to_string_lossy().to_string(),
        alive_path.to_string_lossy().to_string(),
    ];
    if ctx.staged_path != staged {
        cleanup_paths.push(ctx.staged_path.to_string_lossy().to_string());
    }
    write_install_manifest(
        &ctx.app_data_dir,
        &InstallManifest {
            prepared_id: ctx.prepared.id.clone(),
            platform: "macos".to_string(),
            target_version: ctx.prepared.version.clone(),
            install_nonce: token_short.clone(),
            install_parent: install_parent.to_string_lossy().to_string(),
            backup_name: backup_name.clone(),
            staging_name: new_name.clone(),
            paths: cleanup_paths,
            restore_token: ctx.restore_token.clone(),
        },
    )?;

    let mut command = Command::new("/bin/sh");
    command
        .arg("-s")
        .env("RESH_UPDATE_DMG", &staged)
        .env("RESH_UPDATE_APP", &app_bundle)
        .env("RESH_UPDATE_NEW", &new_path)
        .env("RESH_UPDATE_OLD", &backup_path)
        .env("RESH_UPDATE_PID", &old_pid)
        .env("RESH_UPDATE_VERSION", &ctx.prepared.version)
        .env("RESH_UPDATE_BUNDLE_ID", BUNDLE_ID)
        .env("RESH_UPDATE_ARCH", arch)
        .env("RESH_UPDATE_TOKEN", &ctx.restore_token)
        .env("RESH_UPDATE_RESULT", &ctx.failure_path)
        .env("RESH_UPDATE_ALIVE", &alive_path)
        .env(
            "RESH_UPDATE_ALIVE_WAIT",
            INSTALL_ALIVE_WAIT_SECS.to_string(),
        )
        .env(
            "RESH_UPDATE_PARENT_WRITABLE",
            if parent_writable { "1" } else { "0" },
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            super::manifest::clear_install_manifest(&ctx.app_data_dir);
            return Err(format!("Failed to start macOS update helper: {e}"));
        }
    };

    // Stream the static helper body to the child; do not leave a writable
    // privileged script on disk for the elevation path to re-exec.
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(macos_update_script().as_bytes()) {
            let _ = child.kill();
            super::manifest::clear_install_manifest(&ctx.app_data_dir);
            return Err(format!("Failed to feed macOS update helper: {e}"));
        }
        // Drop closes stdin so /bin/sh sees EOF and runs the script.
    } else {
        let _ = child.kill();
        super::manifest::clear_install_manifest(&ctx.app_data_dir);
        return Err("Failed to open stdin for macOS update helper".to_string());
    }

    // Detach — helper continues after we exit.
    std::mem::forget(child);

    tracing::info!(
        target_version = %ctx.prepared.version,
        parent_writable,
        "spawned macOS update helper (stdin)"
    );
    Ok(())
}

pub fn cleanup_after_ack(
    app_data_dir: &Path,
    prepared_id: Option<&str>,
    live_install_parent: Option<PathBuf>,
) {
    use super::manifest::{cleanup_manifest_paths, load_install_manifest, CleanupBinding};
    if let Some(manifest) = load_install_manifest(app_data_dir) {
        cleanup_manifest_paths(
            app_data_dir,
            &manifest,
            &CleanupBinding {
                live_install_parent,
                prepared_id: prepared_id.map(|s| s.to_string()),
                platform: Some("macos".to_string()),
                restore_token: super::super::get_pending_restore_token(),
            },
        );
    }
    // No on-disk helper script to remove.
}

pub fn current_macos_app_bundle_from_exe(exe_path: &Path) -> Option<PathBuf> {
    let macos_dir = exe_path.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()? != "Contents" {
        return None;
    }
    let app_bundle = contents_dir.parent()?;
    (app_bundle.extension()? == "app").then(|| app_bundle.to_path_buf())
}

/// Block only truly non-upgradable locations. Permission-denied writable parents
/// (standard /Applications for a normal user) are allowed — elevation may follow.
fn reject_non_upgradable_location(app_bundle: &Path) -> Result<(), String> {
    let s = app_bundle.to_string_lossy();
    if s.contains("AppTranslocation") {
        return Err(
            "Resh is running from App Translocation (temporary quarantine path). Move Resh.app to Applications, open it from there, then try updating again.".to_string(),
        );
    }
    if s.starts_with("/Volumes/") {
        return Err(
            "Resh is running from a disk image. Copy Resh.app to Applications and open it from there before updating.".to_string(),
        );
    }
    // Detect read-only *mount* (not merely permission-denied directories).
    if path_on_readonly_filesystem(app_bundle) {
        return Err(
            "Resh is on a read-only volume. Move Resh.app to a writable location such as Applications.".to_string(),
        );
    }
    Ok(())
}

fn path_on_readonly_filesystem(path: &Path) -> bool {
    // 1) Resolve the mount point via `df -P`.
    let mount_point = match Command::new("/bin/df")
        .args(["-P", path.to_string_lossy().as_ref()])
        .output()
    {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            // Last column of the second line is the mount point.
            text.lines()
                .nth(1)
                .and_then(|line| line.split_whitespace().last())
                .map(|s| s.to_string())
        }
        _ => None,
    };

    // 2) Inspect `mount` for that mount point and look for read-only flags.
    if let Ok(out) = Command::new("/sbin/mount").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let lower = line.to_ascii_lowercase();
                let matches_mount = if let Some(ref mp) = mount_point {
                    // "on /Applications (" or "on / ("
                    line.contains(&format!(" on {mp} ")) || line.contains(&format!(" on {mp}("))
                } else {
                    // Fallback: any line mentioning the path prefix.
                    let s = path.to_string_lossy();
                    line.contains(s.as_ref())
                };
                if matches_mount
                    && (lower.contains("read-only")
                        || lower.contains(",ro,")
                        || lower.contains("(ro,")
                        || lower.contains(" ro,"))
                {
                    return true;
                }
            }
        }
    }
    false
}

fn probe_dir_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".resh-update-write-probe-{}", Uuid::new_v4()));
    fs::write(&probe, b"ok").map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

fn current_process_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => other,
    }
}

fn macos_update_script() -> &'static str {
    r#"#!/bin/sh
# Resh macOS DMG update helper. Static script streamed on stdin; params via env.
# Privilege elevation uses fixed AppleScript command templates only —
# never re-executes a second user-writable admin script file.
set -eu

: "${RESH_UPDATE_DMG:?}"
: "${RESH_UPDATE_APP:?}"
: "${RESH_UPDATE_NEW:?}"
: "${RESH_UPDATE_OLD:?}"
: "${RESH_UPDATE_PID:?}"
: "${RESH_UPDATE_VERSION:?}"
: "${RESH_UPDATE_BUNDLE_ID:?}"
: "${RESH_UPDATE_ARCH:?}"
: "${RESH_UPDATE_TOKEN:?}"
: "${RESH_UPDATE_RESULT:?}"
: "${RESH_UPDATE_ALIVE:?}"
RESH_UPDATE_ALIVE_WAIT="${RESH_UPDATE_ALIVE_WAIT:-45}"

XATTR="/usr/bin/xattr"
HDIUTIL="/usr/bin/hdiutil"
DITTO="/usr/bin/ditto"
PLUTIL="/usr/bin/plutil"
OPEN="/usr/bin/open"
LIPO="/usr/bin/lipo"
OSASCRIPT="/usr/bin/osascript"
FIND="/usr/bin/find"
GREP="/usr/bin/grep"
RM="/bin/rm"
MV="/bin/mv"
MKDIR="/bin/mkdir"
REALPATH="/usr/bin/realpath"
SLEEP="/bin/sleep"

volume=""
source_app=""

write_result() {
  message="$1"
  dir="$(dirname "$RESH_UPDATE_RESULT")"
  "$MKDIR" -p "$dir" 2>/dev/null || true
  printf '%s\n' "$message" >"$RESH_UPDATE_RESULT" 2>/dev/null || true
}

detach_volume() {
  if [ -n "${volume:-}" ]; then
    "$HDIUTIL" detach "$volume" -quiet 2>/dev/null || \
      "$HDIUTIL" detach "$volume" -force -quiet 2>/dev/null || true
  fi
}

relaunch_app() {
  target="$1"
  if [ -d "$target" ]; then
    "$OPEN" -n "$target" || return 1
    return 0
  fi
  return 1
}

# Normalize to absolute real path when possible.
abs_path() {
  p="$1"
  if [ -e "$p" ] && [ -x "$REALPATH" ]; then
    "$REALPATH" "$p" 2>/dev/null || printf '%s' "$p"
  else
    case "$p" in
      /*) printf '%s' "$p" ;;
      *) printf '%s' "$PWD/$p" ;;
    esac
  fi
}

# Reject empty, relative, or traversal paths; require absolute.
require_abs_no_dotdot() {
  p="$1"
  case "$p" in
    ""|*..*) return 1 ;;
    /*) ;;
    *) return 1 ;;
  esac
  case "$p" in
    */AppTranslocation/*) return 1 ;;
  esac
  return 0
}

# After realpath: app/new/old must share the same parent; names must match policy.
validate_install_paths() {
  app="$1"
  new="$2"
  old="$3"
  require_abs_no_dotdot "$app" || return 1
  require_abs_no_dotdot "$new" || return 1
  require_abs_no_dotdot "$old" || return 1
  app_base="$(basename "$app")"
  new_base="$(basename "$new")"
  old_base="$(basename "$old")"
  [ "$app_base" = "Resh.app" ] || return 1
  case "$new_base" in
    Resh.staging.v*.app) ;;
    *) return 1 ;;
  esac
  case "$old_base" in
    Resh.backup.v*.app) ;;
    *) return 1 ;;
  esac
  app_parent="$(dirname "$app")"
  new_parent="$(dirname "$new")"
  old_parent="$(dirname "$old")"
  [ "$app_parent" = "$new_parent" ] && [ "$app_parent" = "$old_parent" ] || return 1
  return 0
}

# Fixed privileged command templates. Never load or execute a path as a script.
# argv: op, optional_src
try_priv() {
  op="$1"
  src="${2:-}"

  # Re-resolve and re-validate immediately before elevation (TOCTOU defense).
  app="$(abs_path "$RESH_UPDATE_APP")"
  new="$(abs_path "$RESH_UPDATE_NEW")"
  old="$(abs_path "$RESH_UPDATE_OLD")"
  case "$new" in
    /*) ;;
    *) new="$(dirname "$app")/$(basename "$RESH_UPDATE_NEW")" ;;
  esac
  case "$old" in
    /*) ;;
    *) old="$(dirname "$app")/$(basename "$RESH_UPDATE_OLD")" ;;
  esac
  RESH_UPDATE_APP="$app"
  RESH_UPDATE_NEW="$new"
  RESH_UPDATE_OLD="$old"

  if ! validate_install_paths "$app" "$new" "$old"; then
    return 1
  fi

  case "$op" in
    clear_quarantine)
      "$OSASCRIPT" - "$app" <<'APPLESCRIPT' >/dev/null 2>&1
on run argv
  set appPath to item 1 of argv
  do shell script "/usr/bin/xattr -dr com.apple.quarantine " & quoted form of appPath with administrator privileges
end run
APPLESCRIPT
      ;;
    ditto_copy)
      src="$(abs_path "$src")"
      require_abs_no_dotdot "$src" || return 1
      case "$src" in
        /Volumes/*) ;;
        *) return 1 ;;
      esac
      # Source must still be the unique validated Resh.app under /Volumes.
      [ "$(basename "$src")" = "Resh.app" ] || return 1
      "$OSASCRIPT" - "$src" "$new" <<'APPLESCRIPT' >/dev/null 2>&1
on run argv
  set srcPath to item 1 of argv
  set newPath to item 2 of argv
  do shell script "/bin/rm -rf " & quoted form of newPath & " && /usr/bin/ditto " & quoted form of srcPath & " " & quoted form of newPath with administrator privileges
end run
APPLESCRIPT
      ;;
    move_aside)
      "$OSASCRIPT" - "$app" "$old" <<'APPLESCRIPT' >/dev/null 2>&1
on run argv
  set appPath to item 1 of argv
  set oldPath to item 2 of argv
  do shell script "/bin/mv " & quoted form of appPath & " " & quoted form of oldPath with administrator privileges
end run
APPLESCRIPT
      ;;
    move_in)
      "$OSASCRIPT" - "$new" "$app" <<'APPLESCRIPT' >/dev/null 2>&1
on run argv
  set newPath to item 1 of argv
  set appPath to item 2 of argv
  do shell script "/bin/mv " & quoted form of newPath & " " & quoted form of appPath with administrator privileges
end run
APPLESCRIPT
      ;;
    restore_old)
      "$OSASCRIPT" - "$app" "$old" <<'APPLESCRIPT' >/dev/null 2>&1
on run argv
  set appPath to item 1 of argv
  set oldPath to item 2 of argv
  do shell script "/bin/rm -rf " & quoted form of appPath & " && /bin/mv " & quoted form of oldPath & " " & quoted form of appPath with administrator privileges
end run
APPLESCRIPT
      ;;
    *)
      return 1
      ;;
  esac
}

# Strict quarantine residual check. xattr -lr failure is NOT "clean".
quarantine_remains() {
  target="$1"
  listing=""
  if ! listing="$("$XATTR" -lr "$target" 2>/dev/null)"; then
    # Read failed — treat as residual so we force re-clear / rollback.
    return 0
  fi
  if printf '%s\n' "$listing" | "$GREP" -F 'com.apple.quarantine' >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

clear_quarantine_strict() {
  target="$1"
  if ! "$XATTR" -dr com.apple.quarantine "$target"; then
    if ! try_priv clear_quarantine; then
      return 1
    fi
  fi
  if quarantine_remains "$target"; then
    if ! try_priv clear_quarantine; then
      return 1
    fi
    if quarantine_remains "$target"; then
      return 1
    fi
  fi
  return 0
}

wait_alive_marker() {
  i=0
  max="$RESH_UPDATE_ALIVE_WAIT"
  # Poll ~4 times per second.
  max_iters=$((max * 4))
  while [ "$i" -lt "$max_iters" ]; do
    if [ -f "$RESH_UPDATE_ALIVE" ]; then
      return 0
    fi
    "$SLEEP" 0.25
    i=$((i + 1))
  done
  return 1
}

rollback_and_relaunch() {
  reason="$1"
  restored=0
  if [ -d "$RESH_UPDATE_OLD" ]; then
    "$RM" -rf "$RESH_UPDATE_APP" 2>/dev/null || true
    if "$MV" "$RESH_UPDATE_OLD" "$RESH_UPDATE_APP" 2>/dev/null; then
      restored=1
    else
      if try_priv restore_old; then
        restored=1
      elif relaunch_app "$RESH_UPDATE_OLD"; then
        write_result "Update failed: $reason. Previous version available but could not replace current install; launched from backup."
        detach_volume
        exit 1
      else
        write_result "Update failed: $reason. Previous version available but could not replace or launch."
        detach_volume
        exit 1
      fi
    fi
  fi
  "$RM" -rf "$RESH_UPDATE_NEW" 2>/dev/null || true
  if [ "$restored" -eq 1 ]; then
    if relaunch_app "$RESH_UPDATE_APP"; then
      write_result "Update failed: $reason. Rolled back to the previous version."
    else
      write_result "Update failed: $reason. Rolled back but could not relaunch the previous version."
    fi
    detach_volume
    exit 1
  fi
  if relaunch_app "$RESH_UPDATE_APP"; then
    write_result "Update failed: $reason. Could not restore the previous version; relaunched the current install."
  else
    write_result "Update failed: $reason. Could not restore the previous version or relaunch the app."
  fi
  detach_volume
  exit 1
}

fail_early() {
  reason="$1"
  write_result "Update failed: $reason"
  relaunch_app "$RESH_UPDATE_APP" || true
  detach_volume
  exit 1
}

# --- normalize + validate paths before any privileged work ---
RESH_UPDATE_APP="$(abs_path "$RESH_UPDATE_APP")"
RESH_UPDATE_NEW="$(abs_path "$RESH_UPDATE_NEW")"
# NEW/OLD may not exist yet; abs_path still yields absolute form.
case "$RESH_UPDATE_NEW" in
  /*) ;;
  *) RESH_UPDATE_NEW="$(dirname "$RESH_UPDATE_APP")/$(basename "$RESH_UPDATE_NEW")" ;;
esac
case "$RESH_UPDATE_OLD" in
  /*) ;;
  *) RESH_UPDATE_OLD="$(dirname "$RESH_UPDATE_APP")/$(basename "$RESH_UPDATE_OLD")" ;;
esac
RESH_UPDATE_OLD="$(abs_path "$RESH_UPDATE_OLD")"
RESH_UPDATE_DMG="$(abs_path "$RESH_UPDATE_DMG")"

if ! validate_install_paths "$RESH_UPDATE_APP" "$RESH_UPDATE_NEW" "$RESH_UPDATE_OLD"; then
  fail_early "install paths failed safety validation"
fi
if ! require_abs_no_dotdot "$RESH_UPDATE_DMG"; then
  fail_early "staged DMG path failed safety validation"
fi
case "$RESH_UPDATE_DMG" in
  *.dmg) ;;
  *) fail_early "staged package is not a DMG" ;;
esac

# --- wait for old process ---
while kill -0 "$RESH_UPDATE_PID" 2>/dev/null; do
  sleep 0.5
done
sleep 0.5

# --- mount DMG readonly with plist ---
plist_out="$(mktemp -t resh-update-mount.XXXXXX)"
if ! "$HDIUTIL" attach "$RESH_UPDATE_DMG" -plist -readonly -nobrowse >"$plist_out" 2>/dev/null; then
  "$RM" -f "$plist_out"
  fail_early "could not mount the update disk image"
fi

volume="$("$PLUTIL" -extract "system-entities" xml1 -o - "$plist_out" 2>/dev/null | \
  "$GREP" -A1 "mount-point" | "$GREP" "<string>" | head -n 1 | \
  sed -E 's/.*<string>(.*)<\/string>.*/\1/' || true)"
if [ -z "$volume" ]; then
  volume="$("$HDIUTIL" info 2>/dev/null | awk -v dmg="$RESH_UPDATE_DMG" '
    index($0, dmg) {found=1}
    found && /\/Volumes\// {
      for (i=1;i<=NF;i++) if ($i ~ /^\/Volumes\//) { print $i; exit }
    }
  ' || true)"
fi
"$RM" -f "$plist_out"

if [ -z "$volume" ] || [ ! -d "$volume" ]; then
  fail_early "could not locate the mounted update volume"
fi

trap 'detach_volume' EXIT HUP INT TERM

# --- find unique Resh.app ---
apps="$("$FIND" "$volume" -maxdepth 3 -name 'Resh.app' -type d 2>/dev/null || true)"
count="$(printf '%s\n' "$apps" | sed '/^$/d' | wc -l | tr -d ' ')"
if [ "$count" != "1" ]; then
  fail_early "expected exactly one Resh.app in the disk image (found $count)"
fi
source_app="$(printf '%s\n' "$apps" | sed '/^$/d' | head -n 1)"
source_app="$(abs_path "$source_app")"
case "$source_app" in
  /Volumes/*) ;;
  *) fail_early "update bundle is not on a mounted volume" ;;
esac

info_plist="$source_app/Contents/Info.plist"
if [ ! -f "$info_plist" ]; then
  fail_early "Info.plist missing in update bundle"
fi

bundle_id="$("$PLUTIL" -extract CFBundleIdentifier raw -o - "$info_plist" 2>/dev/null || true)"
if [ "$bundle_id" != "$RESH_UPDATE_BUNDLE_ID" ]; then
  fail_early "bundle identifier mismatch (expected $RESH_UPDATE_BUNDLE_ID)"
fi

bundle_ver="$("$PLUTIL" -extract CFBundleShortVersionString raw -o - "$info_plist" 2>/dev/null || true)"
if [ -z "$bundle_ver" ]; then
  bundle_ver="$("$PLUTIL" -extract CFBundleVersion raw -o - "$info_plist" 2>/dev/null || true)"
fi
norm_ver() { printf '%s' "$1" | sed 's/^[vV]//'; }
if [ "$(norm_ver "$bundle_ver")" != "$(norm_ver "$RESH_UPDATE_VERSION")" ]; then
  fail_early "bundle version mismatch (expected $RESH_UPDATE_VERSION, found ${bundle_ver:-unknown})"
fi

exec_name="$("$PLUTIL" -extract CFBundleExecutable raw -o - "$info_plist" 2>/dev/null || true)"
if [ -z "$exec_name" ]; then
  exec_name="resh"
fi
main_bin="$source_app/Contents/MacOS/$exec_name"
if [ ! -f "$main_bin" ]; then
  fail_early "main executable missing in update bundle"
fi

archs="$("$LIPO" -archs "$main_bin" 2>/dev/null || true)"
case " $archs " in
  *" $RESH_UPDATE_ARCH "*) ;;
  *)
    if [ "$RESH_UPDATE_ARCH" = "arm64" ]; then
      case " $archs " in
        *" arm64 "*|*" aarch64 "*) ;;
        *) fail_early "main executable architecture mismatch (need $RESH_UPDATE_ARCH, have ${archs:-unknown})" ;;
      esac
    else
      fail_early "main executable architecture mismatch (need $RESH_UPDATE_ARCH, have ${archs:-unknown})"
    fi
    ;;
esac

# Validation complete — only now may we replace / elevate / xattr.

"$RM" -rf "$RESH_UPDATE_NEW" 2>/dev/null || true
"$RM" -rf "$RESH_UPDATE_OLD" 2>/dev/null || true
if [ -e "$RESH_UPDATE_NEW" ] || [ -e "$RESH_UPDATE_OLD" ]; then
  fail_early "could not clear leftover update staging directories"
fi

if ! "$DITTO" "$source_app" "$RESH_UPDATE_NEW"; then
  if ! try_priv ditto_copy "$source_app"; then
    fail_early "could not copy the new app bundle from the update image"
  fi
fi

if [ ! -d "$RESH_UPDATE_NEW/Contents" ]; then
  "$RM" -rf "$RESH_UPDATE_NEW" 2>/dev/null || true
  fail_early "staged new app bundle is incomplete"
fi

if [ -d "$RESH_UPDATE_APP" ]; then
  if ! "$MV" "$RESH_UPDATE_APP" "$RESH_UPDATE_OLD"; then
    if ! try_priv move_aside; then
      "$RM" -rf "$RESH_UPDATE_NEW" 2>/dev/null || true
      fail_early "could not move the current app aside for replacement"
    fi
  fi
fi

if ! "$MV" "$RESH_UPDATE_NEW" "$RESH_UPDATE_APP"; then
  if ! try_priv move_in; then
    rollback_and_relaunch "could not install the new app bundle"
  fi
fi

# Strict quarantine clear on final target only.
if ! clear_quarantine_strict "$RESH_UPDATE_APP"; then
  rollback_and_relaunch "could not clear or verify quarantine attributes on the new app"
fi

# Remove any stale alive marker before launch.
"$RM" -f "$RESH_UPDATE_ALIVE" 2>/dev/null || true

if ! "$OPEN" -n "$RESH_UPDATE_APP" --args --restore-update-session "$RESH_UPDATE_TOKEN"; then
  rollback_and_relaunch "could not launch the updated app after install"
fi

# Wait for new process to prove it accepted the restore token.
if ! wait_alive_marker; then
  rollback_and_relaunch "updated application did not confirm startup in time"
fi

detach_volume
trap - EXIT HUP INT TERM
exit 0
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_requires_strict_quarantine() {
        let s = macos_update_script();
        assert!(s.contains("/usr/bin/xattr"));
        assert!(s.contains("-dr com.apple.quarantine"));
        assert!(s.contains("rollback_and_relaunch"));
        assert!(s.contains("--restore-update-session"));
        assert!(s.contains("hdiutil"));
        assert!(s.contains("readonly"));
        assert!(s.contains("CFBundleIdentifier"));
        assert!(s.contains("quarantine_remains"));
        assert!(s.contains("clear_quarantine_strict"));
        assert!(s.contains("with administrator privileges"));
        assert!(s.contains("wait_alive_marker"));
        assert!(s.contains("RESH_UPDATE_ALIVE"));
        // No user-writable admin script path.
        assert!(!s.contains("RESH_UPDATE_PRIV_HELPER"));
        assert!(!s.contains("xattr -c "));
        assert!(!s.contains("xattr -cr"));
        assert!(!s.contains("spctl --master-disable"));
        assert!(!s.contains("spctl --master-enable"));
        assert!(!s.contains("|| true\n  if \"$XATTR\" -lr"));
        // Must not treat xattr list failure as clean via pipe without status.
        assert!(s.contains("if ! listing="));
        // Quarantine clear is not best-effort: no `|| true` on the -dr line.
        for line in s.lines() {
            if line.contains("xattr")
                && line.contains("com.apple.quarantine")
                && line.contains("-dr")
            {
                assert!(
                    !line.contains("|| true"),
                    "xattr -dr quarantine must not ignore failures with || true: {line}"
                );
            }
        }
    }

    #[test]
    fn open_new_app_only_after_quarantine_clear_and_recheck() {
        let s = macos_update_script();
        let clear_marker = "clear_quarantine_strict \"$RESH_UPDATE_APP\"";
        let open_marker = "\"$OPEN\" -n \"$RESH_UPDATE_APP\" --args --restore-update-session";
        let clear_idx = s
            .find(clear_marker)
            .expect("clear_quarantine_strict on final app");
        let open_idx = s
            .find(open_marker)
            .expect("open -n new app with restore token");
        assert!(
            open_idx > clear_idx,
            "open -n must run only after quarantine clear/recheck"
        );
        // Failure path must rollback, not launch.
        assert!(s.contains(
            "rollback_and_relaunch \"could not clear or verify quarantine attributes on the new app\""
        ));
        // Success path: wait for alive marker after the open call (not the earlier function def).
        let after_open = &s[open_idx..];
        assert!(
            after_open.contains("wait_alive_marker"),
            "alive wait must follow open -n on the success path"
        );
    }

    #[test]
    fn script_forbids_global_gatekeeper_and_blanket_xattr() {
        let s = macos_update_script();
        let lower = s.to_ascii_lowercase();
        assert!(!lower.contains("master-disable"));
        assert!(!lower.contains("master-enable"));
        assert!(!s.contains("xattr -c"));
        assert!(!s.contains("xattr -cr"));
        // Only quarantine attribute name is targeted for recursive delete.
        assert!(s.contains("com.apple.quarantine"));
        assert!(!s.contains("xattr -c "));
    }

    #[test]
    fn script_uses_fixed_priv_templates() {
        let s = macos_update_script();
        assert!(s.contains("validate_install_paths"));
        assert!(s.contains("try_priv"));
        assert!(s.contains("clear_quarantine"));
        assert!(s.contains("move_aside"));
        assert!(s.contains("move_in"));
        assert!(s.contains("ditto_copy"));
        assert!(!s.contains("resh-update-priv-helper"));
        assert!(!s.contains("spctl"));
        assert!(!s.contains("xattr -c"));
    }

    #[test]
    fn bundle_from_exe_path() {
        let p = PathBuf::from("/Applications/Resh.app/Contents/MacOS/resh");
        let b = current_macos_app_bundle_from_exe(&p).unwrap();
        assert_eq!(b, PathBuf::from("/Applications/Resh.app"));
        assert!(current_macos_app_bundle_from_exe(Path::new("/usr/bin/resh")).is_none());
    }

    #[test]
    fn translocation_and_volumes_rejected() {
        assert!(reject_non_upgradable_location(Path::new(
            "/private/var/folders/xx/AppTranslocation/abc/Resh.app"
        ))
        .is_err());
        assert!(reject_non_upgradable_location(Path::new("/Volumes/Resh/Resh.app")).is_err());
    }

    #[test]
    fn preflight_uses_stdin_sh_s() {
        // Helper body is fed via stdin to `/bin/sh -s`, not a disk script path.
        let src = include_str!("macos.rs");
        assert!(src.contains("arg(\"-s\")") || src.contains("\"-s\""));
        assert!(src.contains("Stdio::piped()"));
        assert!(src.contains("write_all(macos_update_script()"));
        assert!(src.contains("mem::forget(child)"));
    }
}
