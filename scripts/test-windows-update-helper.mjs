#!/usr/bin/env node
/**
 * Isolated Windows portable-update helper regression (temp dir only).
 *
 * Does NOT replace a real Resh install. Checks:
 * 1. Static PowerShell contract extracted from windows.rs
 * 2. On Windows: full helper body (instrumented Start-Exe) swap + rollback with
 *    paths containing spaces / Unicode / single quotes
 * 3. On non-Windows: static checks only + documented skip for live Move-Item
 *
 * Usage:
 *   node scripts/test-windows-update-helper.mjs
 */
import { spawnSync } from 'node:child_process';
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
  existsSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const windowsRs = join(root, 'src-tauri/src/updater/install/windows.rs');
const isWin = process.platform === 'win32';

function fail(msg) {
  console.error(msg);
  process.exit(1);
}

function extractWindowsScript() {
  const src = readFileSync(windowsRs, 'utf8');
  const marker = "fn windows_update_script() -> &'static str {";
  const idx = src.indexOf(marker);
  if (idx < 0) fail('windows_update_script not found');
  const rawStart = src.indexOf('r#"', idx);
  if (rawStart < 0) fail('raw string start not found');
  const bodyStart = rawStart + 3;
  const bodyEnd = src.indexOf('"#', bodyStart);
  if (bodyEnd < 0) fail('raw string end not found');
  return src.slice(bodyStart, bodyEnd);
}

function assertStaticContract(script) {
  const required = [
    'param(',
    '$OldPid',
    '$CurrentExe',
    '$StagedExe',
    '$BackupExe',
    '$RestoreToken',
    '$ResultPath',
    '$AlivePath',
    'Wait-Process',
    'Wait-AliveMarker',
    '--restore-update-session',
    'Move-Item',
    '-LiteralPath',
    'Start-Process',
  ];
  const forbidden = ['invoke-expression', 'downloadstring', 'iex '];
  for (const needle of required) {
    if (!script.includes(needle)) fail(`static contract missing: ${needle}`);
  }
  const lower = script.toLowerCase();
  for (const needle of forbidden) {
    if (lower.includes(needle)) fail(`static contract forbidden: ${needle}`);
  }
  const rust = readFileSync(windowsRs, 'utf8');
  if (!rust.includes('CREATE_NO_WINDOW')) fail('Rust spawn missing CREATE_NO_WINDOW');
  if (!rust.includes('-WindowStyle') && !rust.includes('Hidden')) {
    fail('Rust spawn should pass Hidden window style or CREATE_NO_WINDOW');
  }
  if (!rust.includes('-NoProfile') || !rust.includes('-NonInteractive')) {
    fail('Rust spawn missing -NoProfile/-NonInteractive');
  }
  if (!rust.includes('-ExecutionPolicy') || !rust.includes('Bypass')) {
    fail('Rust spawn missing ExecutionPolicy Bypass');
  }
  console.log('static PowerShell contract OK');
}

/** Replace Start-Exe so we can prove control flow without launching a real GUI. */
function instrumentStartExe(script, { writeAlive = true, failStart = false } = {}) {
  const mock = `
function Start-Exe([string]$ExePath, [string[]]$ArgumentList) {
  if (-not (Test-Path -LiteralPath $ExePath)) { return $false }
  if (${failStart ? '$true' : '$false'}) { return $false }
  if (${writeAlive ? '$true' : '$false'}) {
    if ($null -ne $ArgumentList -and ($ArgumentList -contains '--restore-update-session')) {
      try {
        Set-Content -LiteralPath $AlivePath -Value 'ok' -Encoding utf8
      } catch {
        return $false
      }
    }
  }
  return $true
}
`;
  if (!/function Start-Exe\b/.test(script)) {
    fail('Start-Exe function not found in Windows helper script');
  }
  return script.replace(/function Start-Exe\([\s\S]*?\n\}/, mock.trim() + '\n');
}

function runPowershellFile(scriptPath, args, { timeout = 60000 } = {}) {
  return spawnSync(
    'powershell.exe',
    [
      '-NoProfile',
      '-NonInteractive',
      '-ExecutionPolicy',
      'Bypass',
      '-WindowStyle',
      'Hidden',
      '-File',
      scriptPath,
      ...args,
    ],
    { encoding: 'utf8', timeout, windowsHide: true },
  );
}

function runPathSafeSwapOnly() {
  if (!isWin) {
    console.log('skip live Move-Item primitive tests (not Windows)');
    return;
  }

  const base = mkdtempSync(join(tmpdir(), 'resh-win-update-'));
  const installDir = join(base, "Resh Update 'Test' 目录");
  mkdirSync(installDir, { recursive: true });

  const currentExe = join(installDir, 'Resh.exe');
  const stagedExe = join(installDir, 'Resh-v9.9.9-windows-x86_64.exe');
  const backupExe = join(installDir, 'Resh.backup.v9.9.9.test.exe');

  writeFileSync(currentExe, 'OLD_PAYLOAD');
  writeFileSync(stagedExe, 'NEW_PAYLOAD');

  const swapOnly = `
param(
  [Parameter(Mandatory = $true)][string]$CurrentExe,
  [Parameter(Mandatory = $true)][string]$StagedExe,
  [Parameter(Mandatory = $true)][string]$BackupExe
)
$ErrorActionPreference = "Stop"
Move-Item -LiteralPath $CurrentExe -Destination $BackupExe -Force
Move-Item -LiteralPath $StagedExe -Destination $CurrentExe -Force
`;
  const swapPath = join(installDir, 'swap-only.ps1');
  writeFileSync(swapPath, swapOnly);

  const r = runPowershellFile(swapPath, [
    '-CurrentExe',
    currentExe,
    '-StagedExe',
    stagedExe,
    '-BackupExe',
    backupExe,
  ]);

  if (r.status !== 0) {
    fail(`path-safe swap failed: status=${r.status}\n${r.stderr || r.stdout}`);
  }
  if (readFileSync(currentExe, 'utf8') !== 'NEW_PAYLOAD') fail('swap content wrong');
  if (readFileSync(backupExe, 'utf8') !== 'OLD_PAYLOAD') fail('backup content wrong');

  const rollback = `
param(
  [Parameter(Mandatory = $true)][string]$CurrentExe,
  [Parameter(Mandatory = $true)][string]$BackupExe
)
$ErrorActionPreference = "Stop"
if (Test-Path -LiteralPath $CurrentExe) { Remove-Item -LiteralPath $CurrentExe -Force }
Move-Item -LiteralPath $BackupExe -Destination $CurrentExe -Force
`;
  const rbPath = join(installDir, 'rollback-only.ps1');
  writeFileSync(rbPath, rollback);
  const r2 = runPowershellFile(rbPath, [
    '-CurrentExe',
    currentExe,
    '-BackupExe',
    backupExe,
  ]);
  if (r2.status !== 0) fail(`rollback failed: ${r2.stderr || r2.stdout}`);
  if (readFileSync(currentExe, 'utf8') !== 'OLD_PAYLOAD') fail('rollback content wrong');

  console.log('Windows path-safe swap + rollback OK (spaces/Unicode/quotes)');
  rmSync(base, { recursive: true, force: true });
}

function runFullHelperFlow() {
  if (!isWin) {
    console.log('skip full Windows helper body (not Windows)');
    return;
  }

  const baseScript = extractWindowsScript();
  const base = mkdtempSync(join(tmpdir(), 'resh-win-helper-'));
  const installDir = join(base, "Install Dir 'X' 更新");
  mkdirSync(installDir, { recursive: true });

  const currentExe = join(installDir, 'Resh.exe');
  const stagedExe = join(installDir, 'Resh-v9.9.9-windows-x86_64.exe');
  const backupExe = join(installDir, 'Resh.backup.v9.9.9.deadbeef.exe');
  const resultPath = join(base, 'result.txt');
  const alivePath = join(base, 'alive.ready');

  // --- Success path ---
  {
    writeFileSync(currentExe, 'OLD_PAYLOAD');
    writeFileSync(stagedExe, 'NEW_PAYLOAD');
    if (existsSync(backupExe)) rmSync(backupExe, { force: true });
    if (existsSync(alivePath)) rmSync(alivePath, { force: true });
    if (existsSync(resultPath)) rmSync(resultPath, { force: true });

    const script = instrumentStartExe(baseScript, { writeAlive: true });
    const scriptPath = join(base, 'apply-success.ps1');
    writeFileSync(scriptPath, script);

    // Short-lived PID already exited: use an impossible old pid that Wait-Process treats as gone.
    const r = runPowershellFile(scriptPath, [
      '-OldPid',
      '1',
      '-CurrentExe',
      currentExe,
      '-StagedExe',
      stagedExe,
      '-BackupExe',
      backupExe,
      '-RestoreToken',
      'test-token-win',
      '-ResultPath',
      resultPath,
      '-AlivePath',
      alivePath,
      '-AliveWaitSecs',
      '10',
    ]);

    if (r.status !== 0) {
      fail(
        `full helper success failed: ${r.status}\n${r.stderr || r.stdout}\nresult=${existsSync(resultPath) ? readFileSync(resultPath, 'utf8') : ''}`,
      );
    }
    if (readFileSync(currentExe, 'utf8') !== 'NEW_PAYLOAD') fail('success: current not NEW');
    if (readFileSync(backupExe, 'utf8') !== 'OLD_PAYLOAD') fail('success: backup not OLD');
    if (!existsSync(alivePath)) fail('success: alive marker missing');
    if (existsSync(resultPath)) {
      // success must not write failure result
      const body = readFileSync(resultPath, 'utf8').trim();
      if (body) fail(`success wrote unexpected result: ${body}`);
    }
    console.log('Windows full helper success path OK');
  }

  // --- Alive timeout → rollback ---
  {
    writeFileSync(currentExe, 'OLD_PAYLOAD');
    writeFileSync(stagedExe, 'NEW_PAYLOAD');
    if (existsSync(backupExe)) rmSync(backupExe, { force: true });
    if (existsSync(alivePath)) rmSync(alivePath, { force: true });
    if (existsSync(resultPath)) rmSync(resultPath, { force: true });

    const script = instrumentStartExe(baseScript, { writeAlive: false });
    const scriptPath = join(base, 'apply-alive-fail.ps1');
    writeFileSync(scriptPath, script);

    const r = runPowershellFile(scriptPath, [
      '-OldPid',
      '1',
      '-CurrentExe',
      currentExe,
      '-StagedExe',
      stagedExe,
      '-BackupExe',
      backupExe,
      '-RestoreToken',
      'test-token-win',
      '-ResultPath',
      resultPath,
      '-AlivePath',
      alivePath,
      '-AliveWaitSecs',
      '2',
    ]);

    if (r.status === 0) fail('alive timeout should fail helper');
    if (readFileSync(currentExe, 'utf8') !== 'OLD_PAYLOAD') {
      fail('alive timeout must restore OLD into current');
    }
    const result = existsSync(resultPath) ? readFileSync(resultPath, 'utf8') : '';
    if (!/confirm startup|did not confirm/i.test(result)) {
      fail(`expected alive timeout message, got: ${result}`);
    }
    console.log('Windows full helper alive-timeout rollback OK');
  }

  // --- Launch failure → rollback ---
  {
    writeFileSync(currentExe, 'OLD_PAYLOAD');
    writeFileSync(stagedExe, 'NEW_PAYLOAD');
    if (existsSync(backupExe)) rmSync(backupExe, { force: true });
    if (existsSync(alivePath)) rmSync(alivePath, { force: true });
    if (existsSync(resultPath)) rmSync(resultPath, { force: true });

    const script = instrumentStartExe(baseScript, { failStart: true });
    const scriptPath = join(base, 'apply-launch-fail.ps1');
    writeFileSync(scriptPath, script);

    const r = runPowershellFile(scriptPath, [
      '-OldPid',
      '1',
      '-CurrentExe',
      currentExe,
      '-StagedExe',
      stagedExe,
      '-BackupExe',
      backupExe,
      '-RestoreToken',
      'test-token-win',
      '-ResultPath',
      resultPath,
      '-AlivePath',
      alivePath,
      '-AliveWaitSecs',
      '5',
    ]);

    if (r.status === 0) fail('launch failure should fail helper');
    if (readFileSync(currentExe, 'utf8') !== 'OLD_PAYLOAD') {
      fail('launch failure must restore OLD');
    }
    const result = existsSync(resultPath) ? readFileSync(resultPath, 'utf8') : '';
    if (!/launch|could not launch/i.test(result)) {
      fail(`expected launch failure message, got: ${result}`);
    }
    console.log('Windows full helper launch-failure rollback OK');
  }

  rmSync(base, { recursive: true, force: true });
}

const script = extractWindowsScript();
assertStaticContract(script);
runPathSafeSwapOnly();
runFullHelperFlow();
console.log('test-windows-update-helper: all checks passed');
