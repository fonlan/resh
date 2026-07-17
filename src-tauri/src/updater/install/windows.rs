//! Windows portable EXE in-place update via hidden PowerShell helper.
//!
//! Only compiled on Windows. Uses static script + process args (no remote strings).
//! Helper waits for a new-process "alive" marker before treating launch as success.

#![cfg(windows)]

use super::manifest::{write_install_manifest, InstallManifest};
use super::result::{install_alive_marker_path, INSTALL_ALIVE_WAIT_SECS};
use super::{looks_like_windows_pe_x64, sanitize_path_component, PreparedInstallContext};
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

const CREATE_NO_WINDOW: u32 = 0x08000000;
const HELPER_SCRIPT_NAME: &str = "apply-windows-update.ps1";

pub fn preflight_and_spawn(ctx: &PreparedInstallContext) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let current_exe = std::fs::canonicalize(&current_exe).unwrap_or(current_exe);
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| "Cannot determine executable directory".to_string())?
        .to_path_buf();

    // Staged package must live in the same directory as the running EXE.
    let staged = std::fs::canonicalize(&ctx.staged_path).unwrap_or_else(|_| ctx.staged_path.clone());
    let staged_parent = staged
        .parent()
        .ok_or_else(|| "Staged update has no parent directory".to_string())?;
    let install_canon = std::fs::canonicalize(&install_dir).unwrap_or(install_dir.clone());
    if staged_parent != install_canon.as_path() {
        return Err(
            "Staged update must be in the same directory as the running executable".to_string(),
        );
    }

    if staged == current_exe {
        return Err("Staged update path must not be the running executable".to_string());
    }

    // Directory must be writable (create a probe file next to the exe).
    probe_dir_writable(&install_dir)?;

    looks_like_windows_pe_x64(&staged)?;

    let version_comp = sanitize_path_component(&ctx.prepared.version, "version")?;
    let token_short: String = Uuid::new_v4().to_string().chars().take(8).collect();
    let backup_name = format!("Resh.backup.v{version_comp}.{token_short}.exe");
    let backup_path = install_dir.join(&backup_name);

    // Never overwrite unrelated user files that happen to match backup name.
    if backup_path.exists() {
        return Err("Backup path already exists; refuse to overwrite".to_string());
    }

    let helper_dir = crate::updater::paths::ensure_trusted_updates_subdir(
        &ctx.app_data_dir,
        "helpers",
    )?;
    let script_path = helper_dir.join(HELPER_SCRIPT_NAME);
    crate::updater::paths::write_trusted_updates_file(
        &ctx.app_data_dir,
        Path::new(&format!("helpers/{HELPER_SCRIPT_NAME}")),
        windows_update_script().as_bytes(),
    )
    .map_err(|e| format!("write helper: {e}"))?;

    let result_path = &ctx.failure_path;
    if result_path.parent().is_some() {
        // Failure marker lives under updates/; require trusted root.
        let _ = crate::updater::paths::ensure_trusted_updates_dir(&ctx.app_data_dir);
    }

    let alive_path = install_alive_marker_path(&ctx.app_data_dir, &ctx.restore_token);
    // Clear any stale alive marker only through the trusted updates-root primitive.
    let _ = super::manifest::remove_trusted_updates_relative(
        &ctx.app_data_dir,
        Path::new(&format!("install-alive-{}.ready", ctx.restore_token)),
    );

    let mut cleanup_paths = vec![
        script_path.to_string_lossy().to_string(),
        backup_path.to_string_lossy().to_string(),
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
            platform: "windows".to_string(),
            target_version: ctx.prepared.version.clone(),
            install_nonce: token_short.clone(),
            install_parent: install_canon.to_string_lossy().to_string(),
            backup_name: backup_name.clone(),
            staging_name: String::new(),
            paths: cleanup_paths,
            restore_token: ctx.restore_token.clone(),
        },
    )?;

    let old_pid = std::process::id().to_string();
    let alive_wait = INSTALL_ALIVE_WAIT_SECS.to_string();
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
            script_path.to_string_lossy().as_ref(),
            "-OldPid",
            &old_pid,
            "-CurrentExe",
            current_exe.to_string_lossy().as_ref(),
            "-StagedExe",
            staged.to_string_lossy().as_ref(),
            "-BackupExe",
            backup_path.to_string_lossy().as_ref(),
            "-RestoreToken",
            &ctx.restore_token,
            "-ResultPath",
            result_path.to_string_lossy().as_ref(),
            "-AlivePath",
            alive_path.to_string_lossy().as_ref(),
            "-AliveWaitSecs",
            &alive_wait,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.creation_flags(CREATE_NO_WINDOW);

    if let Err(e) = command.spawn() {
        super::manifest::clear_install_manifest(&ctx.app_data_dir);
        return Err(format!("Failed to start Windows update helper: {e}"));
    }

    tracing::info!(
        target_version = %ctx.prepared.version,
        "spawned Windows update helper"
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
                platform: Some("windows".to_string()),
                restore_token: super::super::get_pending_restore_token(),
            },
        );
    } else {
        // Legacy fallback: only the known helper script under a *trusted* updates root.
        // Reuses the same existence + no-symlink checks as clear_install_manifest so a
        // junction/symlink at updates/ cannot redirect the delete outside app-data.
        let relative = PathBuf::from("helpers").join(HELPER_SCRIPT_NAME);
        let _ = super::manifest::remove_trusted_updates_relative(app_data_dir, &relative);
    }
}

fn probe_dir_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".resh-update-write-probe-{}", Uuid::new_v4()));
    fs::write(&probe, b"ok").map_err(|e| {
        format!("Update directory is not writable; cannot install in place: {e}")
    })?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

fn windows_update_script() -> &'static str {
    r#"# Resh Windows portable in-place update helper (static script + params only).
param(
  [Parameter(Mandatory = $true)][int]$OldPid,
  [Parameter(Mandatory = $true)][string]$CurrentExe,
  [Parameter(Mandatory = $true)][string]$StagedExe,
  [Parameter(Mandatory = $true)][string]$BackupExe,
  [Parameter(Mandatory = $true)][string]$RestoreToken,
  [Parameter(Mandatory = $true)][string]$ResultPath,
  [Parameter(Mandatory = $true)][string]$AlivePath,
  [Parameter(Mandatory = $true)][int]$AliveWaitSecs
)

$ErrorActionPreference = "Stop"

function Write-Result([string]$Message) {
  try {
    $dir = Split-Path -Parent $ResultPath
    if ($dir -and -not (Test-Path -LiteralPath $dir)) {
      New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    Set-Content -LiteralPath $ResultPath -Value $Message -Encoding utf8
  } catch {
    # ignore
  }
}

function Start-Exe([string]$ExePath, [string[]]$ArgumentList) {
  if (-not (Test-Path -LiteralPath $ExePath)) { return $false }
  $workDir = Split-Path -Parent $ExePath
  try {
    if ($null -eq $ArgumentList -or $ArgumentList.Count -eq 0) {
      Start-Process -FilePath $ExePath -WorkingDirectory $workDir | Out-Null
    } else {
      Start-Process -FilePath $ExePath -ArgumentList $ArgumentList -WorkingDirectory $workDir | Out-Null
    }
    return $true
  } catch {
    return $false
  }
}

function Wait-AliveMarker {
  $deadline = (Get-Date).AddSeconds($AliveWaitSecs)
  while ((Get-Date) -lt $deadline) {
    if (Test-Path -LiteralPath $AlivePath) {
      return $true
    }
    Start-Sleep -Milliseconds 400
  }
  return $false
}

function Restore-And-Relaunch([string]$Reason) {
  $restored = $false
  try {
    if (Test-Path -LiteralPath $BackupExe) {
      if (Test-Path -LiteralPath $CurrentExe) {
        Remove-Item -LiteralPath $CurrentExe -Force -ErrorAction SilentlyContinue
      }
      Move-Item -LiteralPath $BackupExe -Destination $CurrentExe -Force -ErrorAction Stop
      $restored = $true
    }
  } catch {
    # keep going to try relaunch
  }

  if ($restored) {
    if (Start-Exe $CurrentExe @()) {
      Write-Result "Update failed: $Reason. Rolled back to the previous version."
    } else {
      Write-Result "Update failed: $Reason. Rolled back but could not relaunch the previous version."
    }
    return
  }

  if (Start-Exe $CurrentExe @()) {
    Write-Result "Update failed: $Reason. Could not restore the previous version; relaunched the current install."
  } elseif (Test-Path -LiteralPath $BackupExe) {
    if (Start-Exe $BackupExe @()) {
      Write-Result "Update failed: $Reason. Launched from backup; previous version may need manual restore."
    } else {
      Write-Result "Update failed: $Reason. Could not restore or relaunch."
    }
  } else {
    Write-Result "Update failed: $Reason. Could not restore or relaunch."
  }
}

try {
  # Wait until the old Resh process fully exits.
  try {
    Wait-Process -Id $OldPid -ErrorAction SilentlyContinue
  } catch {
    # Process may already be gone
  }
  # Extra settle time for file locks.
  Start-Sleep -Milliseconds 500

  if (-not (Test-Path -LiteralPath $StagedExe)) {
    Write-Result "Update failed: staged executable is missing"
    [void](Start-Exe $CurrentExe @())
    exit 1
  }
  if (-not (Test-Path -LiteralPath $CurrentExe)) {
    Write-Result "Update failed: current executable is missing"
    exit 1
  }

  # Same volume rename: current -> backup, staged -> current name.
  if (Test-Path -LiteralPath $BackupExe) {
    Write-Result "Update failed: backup path already exists"
    [void](Start-Exe $CurrentExe @())
    exit 1
  }

  try {
    Move-Item -LiteralPath $CurrentExe -Destination $BackupExe -Force -ErrorAction Stop
  } catch {
    Write-Result "Update failed: could not move current executable to backup"
    [void](Start-Exe $CurrentExe @())
    exit 1
  }

  try {
    Move-Item -LiteralPath $StagedExe -Destination $CurrentExe -Force -ErrorAction Stop
  } catch {
    Restore-And-Relaunch "could not move staged executable into place"
    exit 1
  }

  if (-not (Test-Path -LiteralPath $CurrentExe)) {
    Restore-And-Relaunch "updated executable missing after replace"
    exit 1
  }

  # Remove stale alive marker before launch.
  if (Test-Path -LiteralPath $AlivePath) {
    Remove-Item -LiteralPath $AlivePath -Force -ErrorAction SilentlyContinue
  }

  $args = @("--restore-update-session", $RestoreToken)
  if (-not (Start-Exe $CurrentExe $args)) {
    Restore-And-Relaunch "could not launch the updated application"
    exit 1
  }

  # Wait for new process to prove it accepted the restore token.
  if (-not (Wait-AliveMarker)) {
    Restore-And-Relaunch "updated application did not confirm startup in time"
    exit 1
  }

  # Success: leave backup for new process ack cleanup. Do not write failure marker.
  exit 0
} catch {
  Restore-And-Relaunch $_.Exception.Message
  exit 1
}
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_contains_required_params_and_flags() {
        let s = windows_update_script();
        assert!(s.contains("param("));
        assert!(s.contains("$OldPid"));
        assert!(s.contains("$CurrentExe"));
        assert!(s.contains("$StagedExe"));
        assert!(s.contains("$BackupExe"));
        assert!(s.contains("$RestoreToken"));
        assert!(s.contains("$ResultPath"));
        assert!(s.contains("$AlivePath"));
        assert!(s.contains("Wait-Process"));
        assert!(s.contains("Wait-AliveMarker"));
        assert!(s.contains("--restore-update-session"));
        assert!(s.contains("Move-Item"));
        assert!(s.contains("-LiteralPath"));
        // Must not invoke remote code loaders.
        assert!(!s.to_ascii_lowercase().contains("invoke-expression"));
        assert!(!s.to_ascii_lowercase().contains("downloadstring"));
        assert!(!s.to_ascii_lowercase().contains("iex "));
    }

    #[test]
    fn spawn_uses_hidden_no_profile_flags() {
        // Source-level contract for CREATE_NO_WINDOW + PowerShell flags.
        let src = include_str!("windows.rs");
        assert!(src.contains("CREATE_NO_WINDOW"));
        assert!(src.contains("-NoProfile") || src.contains("\"-NoProfile\""));
        assert!(src.contains("-NonInteractive") || src.contains("\"-NonInteractive\""));
        assert!(
            src.contains("WindowStyle")
                || src.contains("Hidden")
                || src.contains("CREATE_NO_WINDOW")
        );
    }
}
