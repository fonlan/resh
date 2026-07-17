#!/usr/bin/env node
/**
 * Isolated macOS update-helper regression (temp dir only; never touches /Applications).
 *
 * Checks:
 * 1. Static shell contract from macos.rs (quarantine strictness, open -n order, no global GK)
 * 2. On macOS: fake bundle + xattr quarantine clear/recheck + custom xattrs preserved
 * 3. On macOS with hdiutil: real helper script against a minimal test DMG — attach,
 *    bundle id/version reject, swap, quarantine clear, mock open, rollback paths
 *
 * Usage:
 *   node scripts/test-macos-update-helper.mjs
 */
import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
  readdirSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const macosRs = join(root, 'src-tauri/src/updater/install/macos.rs');
const isDarwin = process.platform === 'darwin';

function fail(msg) {
  console.error(msg);
  process.exit(1);
}

function extractMacosScript() {
  const src = readFileSync(macosRs, 'utf8');
  const marker = "fn macos_update_script() -> &'static str {";
  const idx = src.indexOf(marker);
  if (idx < 0) fail('macos_update_script not found');
  const rawStart = src.indexOf('r#"', idx);
  if (rawStart < 0) fail('raw string start not found');
  const bodyStart = rawStart + 3;
  const bodyEnd = src.indexOf('"#', bodyStart);
  if (bodyEnd < 0) fail('raw string end not found');
  return src.slice(bodyStart, bodyEnd);
}

function assertStaticContract(script) {
  const required = [
    '/usr/bin/xattr',
    '-dr com.apple.quarantine',
    'clear_quarantine_strict',
    'quarantine_remains',
    'rollback_and_relaunch',
    'wait_alive_marker',
    'RESH_UPDATE_ALIVE',
    '--restore-update-session',
    'hdiutil',
    'readonly',
    'CFBundleIdentifier',
    '"$OPEN" -n',
    'with administrator privileges',
    'if ! listing=',
  ];
  for (const needle of required) {
    if (!script.includes(needle)) fail(`static contract missing: ${needle}`);
  }

  const forbidden = [
    'spctl --master-disable',
    'xattr -c ',
    'xattr -cr',
    'spctl --master-enable',
    'defaults write com.apple.LaunchServices',
  ];
  for (const needle of forbidden) {
    if (script.includes(needle)) fail(`static contract forbidden: ${needle}`);
  }
  if (/xattr[^\n]*quarantine[^\n]*\|\| true/.test(script)) {
    fail('quarantine clear must not use || true best-effort');
  }

  const clearIdx = script.indexOf('clear_quarantine_strict "$RESH_UPDATE_APP"');
  const openIdx = script.indexOf(
    '"$OPEN" -n "$RESH_UPDATE_APP" --args --restore-update-session',
  );
  if (clearIdx < 0 || openIdx < 0) {
    fail('could not locate clear_quarantine_strict / open -n sequence');
  }
  if (openIdx < clearIdx) {
    fail('open -n must run only after quarantine clear/recheck on RESH_UPDATE_APP');
  }
  const qFail = script.indexOf(
    'rollback_and_relaunch "could not clear or verify quarantine attributes on the new app"',
  );
  if (qFail < 0 || qFail < clearIdx) {
    fail('quarantine failure must rollback');
  }
  const afterOpen = script.slice(openIdx);
  if (!afterOpen.includes('wait_alive_marker')) {
    fail('alive wait must follow open -n on the success path');
  }

  console.log('static macOS helper contract OK');
}

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, {
    encoding: 'utf8',
    timeout: opts.timeout ?? 60000,
    env: opts.env ?? process.env,
    cwd: opts.cwd,
    input: opts.input,
  });
}

function hasCmd(name) {
  const r = run('which', [name]);
  return r.status === 0;
}

function createFakeBundle(rootDir, { version = '9.9.9', marker = 'OLD' } = {}) {
  const app = join(rootDir, 'Resh.app');
  const contents = join(app, 'Contents');
  const macos = join(contents, 'MacOS');
  mkdirSync(macos, { recursive: true });
  const exe = join(macos, 'resh');
  writeFileSync(exe, `#!/bin/sh\necho ${marker}\n`);
  chmodSync(exe, 0o755);
  const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.fonlan.resh</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleExecutable</key><string>resh</string>
  <key>CFBundleName</key><string>Resh</string>
</dict></plist>
`;
  writeFileSync(join(contents, 'Info.plist'), plist);
  return { app, exe };
}

function quarantineClearSimulation() {
  if (!isDarwin || !hasCmd('xattr')) {
    console.log('skip xattr quarantine simulation (not macOS or no xattr)');
    return;
  }

  const base = mkdtempSync(join(tmpdir(), 'resh-mac-update-'));
  const installParent = join(base, 'Apps');
  mkdirSync(installParent, { recursive: true });
  const { app, exe } = createFakeBundle(installParent);

  const q = run('/usr/bin/xattr', ['-w', 'com.apple.quarantine', '0081;test;Resh;uuid', app]);
  if (q.status !== 0) fail(`failed to set quarantine on app: ${q.stderr}`);
  const q2 = run('/usr/bin/xattr', [
    '-w',
    'com.apple.quarantine',
    '0081;test;Resh;uuid',
    exe,
  ]);
  if (q2.status !== 0) fail(`failed to set quarantine on exe: ${q2.stderr}`);
  const custom = run('/usr/bin/xattr', ['-w', 'com.resh.test', 'keep-me', app]);
  if (custom.status !== 0) fail(`failed to set custom xattr: ${custom.stderr}`);

  const clear = run('/usr/bin/xattr', ['-dr', 'com.apple.quarantine', app]);
  if (clear.status !== 0) fail(`xattr -dr failed: ${clear.stderr}`);

  const listing = run('/usr/bin/xattr', ['-lr', app]);
  if (listing.status !== 0) fail(`xattr -lr failed after clear: ${listing.stderr}`);
  if (listing.stdout.includes('com.apple.quarantine')) {
    fail('quarantine still present after -dr clear');
  }
  if (!listing.stdout.includes('com.resh.test')) {
    fail('custom xattr was removed; helper must only strip com.apple.quarantine');
  }

  console.log('macOS quarantine clear + custom xattr preserve OK');

  run('/usr/bin/xattr', ['-w', 'com.apple.quarantine', '0081;again;Resh;uuid', app]);
  const listing2 = run('/usr/bin/xattr', ['-lr', app]);
  if (!listing2.stdout.includes('com.apple.quarantine')) {
    fail('expected quarantine present for recheck failure simulation');
  }
  console.log('macOS quarantine recheck residual detection OK');

  rmSync(base, { recursive: true, force: true });
}

function attachRejectSimulation() {
  if (!isDarwin) {
    console.log('skip bundle validation shell snippets (not macOS)');
    return;
  }

  const base = mkdtempSync(join(tmpdir(), 'resh-mac-bundle-'));
  const { app } = createFakeBundle(base, { version: '1.0.0' });
  const plist = join(app, 'Contents/Info.plist');
  const id = run('/usr/bin/plutil', ['-extract', 'CFBundleIdentifier', 'raw', '-o', '-', plist]);
  if (id.stdout.trim() !== 'com.fonlan.resh') fail('bundle id extract failed');
  const ver = run('/usr/bin/plutil', [
    '-extract',
    'CFBundleShortVersionString',
    'raw',
    '-o',
    '-',
    plist,
  ]);
  if (ver.stdout.trim() !== '1.0.0') fail('bundle version extract failed');

  const bad = createFakeBundle(join(base, 'bad'), { version: '2.0.0' });
  let badPlist = readFileSync(join(bad.app, 'Contents/Info.plist'), 'utf8');
  badPlist = badPlist.replace('com.fonlan.resh', 'com.evil.app');
  writeFileSync(join(bad.app, 'Contents/Info.plist'), badPlist);
  const badId = run('/usr/bin/plutil', [
    '-extract',
    'CFBundleIdentifier',
    'raw',
    '-o',
    '-',
    join(bad.app, 'Contents/Info.plist'),
  ]);
  if (badId.stdout.trim() === 'com.fonlan.resh') fail('expected wrong bundle id');

  console.log('macOS bundle id/version validation primitives OK');
  rmSync(base, { recursive: true, force: true });
}

/**
 * Instrument the production helper so OPEN/LIPO/XATTR/DITTO can be mocked in temp dirs.
 * Never points at /Applications.
 *
 * When mockDittoPath is set, DITTO is pointed at a wrapper that runs real ditto then
 * stamps root + nested quarantine and a custom xattr onto RESH_UPDATE_NEW. That proves the
 * real helper clear path (not a post-hoc manual xattr -dr) is recursive and quarantine-only.
 */
function instrumentHelperScript(script, { mockXattrPath, mockDittoPath } = {}) {
  let out = script;
  out = out.replace('OPEN="/usr/bin/open"', 'OPEN="${RESH_TEST_OPEN:-/usr/bin/open}"');
  out = out.replace('LIPO="/usr/bin/lipo"', 'LIPO="${RESH_TEST_LIPO:-/usr/bin/lipo}"');
  out = out.replace(
    'OSASCRIPT="/usr/bin/osascript"',
    'OSASCRIPT="${RESH_TEST_OSASCRIPT:-/usr/bin/osascript}"',
  );
  if (mockXattrPath) {
    out = out.replace('XATTR="/usr/bin/xattr"', `XATTR="${mockXattrPath}"`);
  }
  if (mockDittoPath) {
    out = out.replace('DITTO="/usr/bin/ditto"', `DITTO="${mockDittoPath}"`);
  }
  return out;
}

function writeExecutable(path, body) {
  writeFileSync(path, body);
  chmodSync(path, 0o755);
}

function createTestDmg(base, { version, marker, badBundleId = false, badVersion = false }) {
  const src = join(base, 'dmg-src');
  mkdirSync(src, { recursive: true });
  const dmgVersion = badVersion ? '0.0.1-evil' : version;
  const { app } = createFakeBundle(src, { version: dmgVersion, marker });
  if (badBundleId) {
    let plist = readFileSync(join(app, 'Contents/Info.plist'), 'utf8');
    plist = plist.replace('com.fonlan.resh', 'com.evil.app');
    writeFileSync(join(app, 'Contents/Info.plist'), plist);
  }
  // Tag nested binary with quarantine (may not survive UDRO/ditto; success path
  // re-injects on RESH_UPDATE_NEW via instrumented DITTO wrapper).
  run('/usr/bin/xattr', ['-w', 'com.apple.quarantine', '0081;dmg;Resh;uuid', app]);
  run('/usr/bin/xattr', [
    '-w',
    'com.apple.quarantine',
    '0081;dmg;Resh;uuid',
    join(app, 'Contents/MacOS/resh'),
  ]);
  run('/usr/bin/xattr', ['-w', 'com.resh.test', 'keep-me', app]);

  const dmg = join(base, 'Resh-test.dmg');
  // Unique volume name per run to avoid residual mount collisions.
  const volName = `ReshT${Date.now().toString(36).slice(-6)}`;
  const create = run('/usr/bin/hdiutil', [
    'create',
    '-volname',
    volName,
    '-srcfolder',
    src,
    '-ov',
    '-format',
    'UDRO',
    dmg,
  ]);
  if (create.status !== 0) {
    fail(`hdiutil create failed: ${create.stderr || create.stdout}`);
  }
  return { dmg, volName };
}

function runHelper({
  base,
  scriptBody,
  dmg,
  version,
  arch,
  openMode = 'success', // success | no-alive | fail
  mockXattr = false,
  mockXattrMode = 'residual', // residual | deny-dr
  parentWritable = true,
  injectXattrsOnStaging = false,
  lipoArch, // optional override for mock lipo output (wrong-arch tests)
}) {
  const installParent = join(base, 'Apps');
  mkdirSync(installParent, { recursive: true });
  const { app } = createFakeBundle(installParent, { version: '1.0.0', marker: 'OLD' });
  // Match helper naming policy: Resh.staging.v*.app / Resh.backup.v*.app
  const newApp = join(installParent, `Resh.staging.v${version}.deadbeef.app`);
  const oldApp = join(installParent, `Resh.backup.v${version}.deadbeef.app`);
  const resultPath = join(base, 'result.txt');
  const alivePath = join(base, 'alive.ready');
  const openLog = join(base, 'open.log');
  const mockOpen = join(base, 'mock-open');
  const mockLipo = join(base, 'mock-lipo');
  const mockXattrPath = join(base, 'mock-xattr');
  const mockOsascript = join(base, 'mock-osascript');
  const mockDitto = join(base, 'mock-ditto');
  const reportedArch = lipoArch ?? arch;

  writeExecutable(
    mockOpen,
    `#!/bin/sh
echo "$@" >> "${openLog}"
# Production helper: open -n "$APP" --args --restore-update-session "$TOKEN"
mode="${openMode}"
if [ "$mode" = "fail" ]; then
  exit 1
fi
if [ "$mode" = "success" ]; then
  # Write alive marker when launching the final Resh.app with restore token.
  for a in "$@"; do
    if [ "$a" = "--restore-update-session" ]; then
      printf 'ok\\n' > "${alivePath}"
      exit 0
    fi
  done
fi
# no-alive: pretend open succeeded but never write marker
exit 0
`,
  );

  writeExecutable(
    mockLipo,
    `#!/bin/sh
# Echo requested arch so arch gate passes without a real Mach-O.
echo "${reportedArch}"
`,
  );

  // Never prompt for real admin UI in CI/local automated tests.
  writeExecutable(
    mockOsascript,
    `#!/bin/sh
# Simulate admin authorization cancel / elevation failure.
exit 1
`,
  );

  if (injectXattrsOnStaging) {
    // Real ditto copy, then stamp staging dest so helper clear path is what removes them.
    writeExecutable(
      mockDitto,
      `#!/bin/sh
/usr/bin/ditto "$@"
st=$?
if [ $st -ne 0 ]; then
  exit $st
fi
dest=""
for a in "$@"; do dest="$a"; done
if [ -d "$dest/Contents" ]; then
  /usr/bin/xattr -w com.apple.quarantine '0081;staged;Resh;uuid' "$dest" || exit 1
  if [ -f "$dest/Contents/MacOS/resh" ]; then
    /usr/bin/xattr -w com.apple.quarantine '0081;staged;Resh;uuid' "$dest/Contents/MacOS/resh" || exit 1
    /usr/bin/xattr -w com.resh.test 'keep-nested' "$dest/Contents/MacOS/resh" || exit 1
  fi
  /usr/bin/xattr -w com.resh.test 'keep-me' "$dest" || exit 1
fi
exit 0
`,
    );
  }

  if (mockXattr) {
    if (mockXattrMode === 'deny-dr') {
      // -dr fails with permission error; elevation via mock osascript also fails → rollback.
      writeExecutable(
        mockXattrPath,
        `#!/bin/sh
if [ "$1" = "-dr" ]; then
  echo "xattr: [Errno 13] Permission denied: $*" >&2
  exit 1
fi
if [ "$1" = "-lr" ]; then
  echo "Resh.app: com.apple.quarantine: residual"
  exit 0
fi
exec /usr/bin/xattr "$@"
`,
      );
    } else {
      // -dr "succeeds"; -lr always reports residual quarantine → forces recheck failure.
      writeExecutable(
        mockXattrPath,
        `#!/bin/sh
if [ "$1" = "-dr" ]; then
  exit 0
fi
if [ "$1" = "-lr" ]; then
  echo "Resh.app: com.apple.quarantine: residual"
  exit 0
fi
# passthrough other ops if any
exec /usr/bin/xattr "$@"
`,
      );
    }
  }

  const scriptPath = join(base, 'helper.sh');
  writeFileSync(
    scriptPath,
    instrumentHelperScript(scriptBody, {
      mockXattrPath: mockXattr ? mockXattrPath : undefined,
      mockDittoPath: injectXattrsOnStaging ? mockDitto : undefined,
    }),
  );
  chmodSync(scriptPath, 0o755);

  // Short-lived pid so wait loop exits quickly.
  const sleeper = spawnSync('/bin/sh', ['-c', 'sleep 0.2 & echo $!'], {
    encoding: 'utf8',
  });
  const pid = (sleeper.stdout || '').trim();
  if (!pid) fail('failed to spawn sleeper pid');

  const env = {
    ...process.env,
    RESH_UPDATE_DMG: dmg,
    RESH_UPDATE_APP: app,
    RESH_UPDATE_NEW: newApp,
    RESH_UPDATE_OLD: oldApp,
    RESH_UPDATE_PID: pid,
    RESH_UPDATE_VERSION: version,
    RESH_UPDATE_BUNDLE_ID: 'com.fonlan.resh',
    RESH_UPDATE_ARCH: arch,
    RESH_UPDATE_TOKEN: 'test-token-12345678',
    RESH_UPDATE_RESULT: resultPath,
    RESH_UPDATE_ALIVE: alivePath,
    RESH_UPDATE_ALIVE_WAIT: openMode === 'no-alive' ? '2' : '15',
    RESH_UPDATE_PARENT_WRITABLE: parentWritable ? '1' : '0',
    RESH_TEST_OPEN: mockOpen,
    RESH_TEST_LIPO: mockLipo,
    RESH_TEST_OSASCRIPT: mockOsascript,
  };

  const r = run('/bin/sh', [scriptPath], { env, timeout: 90000 });
  return {
    status: r.status,
    stdout: r.stdout,
    stderr: r.stderr,
    app,
    oldApp,
    newApp,
    resultPath,
    alivePath,
    openLog,
    installParent,
  };
}

function liveHelperIntegration() {
  if (!isDarwin) {
    console.log('skip live macOS helper integration (not macOS)');
    return;
  }
  if (!hasCmd('hdiutil') || !hasCmd('xattr') || !hasCmd('plutil')) {
    console.log('skip live macOS helper integration (missing tools)');
    return;
  }

  const script = extractMacosScript();
  const arch = process.arch === 'arm64' ? 'arm64' : 'x86_64';
  const version = '9.9.9';

  // --- Success: attach → validate → swap → quarantine clear → open -n after clear ---
  {
    const base = mkdtempSync(join(tmpdir(), 'resh-mac-helper-ok-'));
    const { dmg } = createTestDmg(base, { version, marker: 'NEW' });
    const res = runHelper({
      base,
      scriptBody: script,
      dmg,
      version,
      arch,
      openMode: 'success',
      // Inject root + nested quarantine + custom xattrs onto RESH_UPDATE_NEW after
      // real ditto copy so the helper's clear_quarantine_strict (not a post-hoc
      // manual xattr -dr) is what must remove quarantine recursively.
      injectXattrsOnStaging: true,
    });
    if (res.status !== 0) {
      const result = existsSync(res.resultPath)
        ? readFileSync(res.resultPath, 'utf8')
        : '(no result)';
      fail(
        `success helper exit ${res.status}\nresult=${result}\nstderr=${res.stderr}\nstdout=${res.stdout}`,
      );
    }
    const marker = readFileSync(join(res.app, 'Contents/MacOS/resh'), 'utf8');
    if (!marker.includes('NEW')) fail(`expected NEW app content after swap, got: ${marker}`);
    if (!existsSync(res.oldApp)) fail('expected backup app after successful swap');
    if (!existsSync(res.alivePath)) fail('expected alive marker after mock open');
    if (!existsSync(res.openLog)) fail('mock open was not invoked');
    const openArgs = readFileSync(res.openLog, 'utf8');
    if (!openArgs.includes('-n') || !openArgs.includes('--restore-update-session')) {
      fail(`open args missing -n/restore: ${openArgs}`);
    }
    // Helper-clear must leave no quarantine on root or nested executable, and
    // must preserve unrelated custom xattrs on both locations.
    const nested = join(res.app, 'Contents/MacOS/resh');
    const listing = run('/usr/bin/xattr', ['-lr', res.app]);
    if (listing.stdout.includes('com.apple.quarantine')) {
      fail(
        `quarantine remains after helper clear (must be recursive):\n${listing.stdout}`,
      );
    }
    if (!listing.stdout.includes('com.resh.test')) {
      fail(
        `custom xattr must remain after helper quarantine-only clear:\n${listing.stdout}`,
      );
    }
    // Nested path may appear as a separate -lr line; also check directly.
    const nestedListing = run('/usr/bin/xattr', ['-l', nested]);
    if (nestedListing.stdout.includes('com.apple.quarantine')) {
      fail(`nested executable still has quarantine:\n${nestedListing.stdout}`);
    }
    if (!nestedListing.stdout.includes('com.resh.test')) {
      fail(
        `nested custom xattr must remain after recursive quarantine clear:\n${nestedListing.stdout}`,
      );
    }
    // Staging name must not remain
    if (existsSync(res.newApp)) fail('staging new app must be moved into place');
    console.log('macOS helper success path (DMG attach/swap/quarantine/open) OK');
    rmSync(base, { recursive: true, force: true });
  }

  // --- Reject wrong bundle id before replace ---
  {
    const base = mkdtempSync(join(tmpdir(), 'resh-mac-helper-badid-'));
    const { dmg } = createTestDmg(base, { version, marker: 'EVIL', badBundleId: true });
    const res = runHelper({
      base,
      scriptBody: script,
      dmg,
      version,
      arch,
      openMode: 'success',
    });
    if (res.status === 0) fail('bad bundle id should fail');
    const marker = readFileSync(join(res.app, 'Contents/MacOS/resh'), 'utf8');
    if (!marker.includes('OLD')) fail('bad id must not replace install');
    if (existsSync(res.alivePath)) fail('must not launch new app on validation failure');
    const result = existsSync(res.resultPath) ? readFileSync(res.resultPath, 'utf8') : '';
    if (!/bundle identifier|mismatch/i.test(result)) {
      fail(`expected bundle id failure message, got: ${result}`);
    }
    console.log('macOS helper rejects bad bundle id OK');
    rmSync(base, { recursive: true, force: true });
  }

  // --- Reject wrong bundle version before replace ---
  {
    const base = mkdtempSync(join(tmpdir(), 'resh-mac-helper-badver-'));
    const { dmg } = createTestDmg(base, { version, marker: 'EVIL', badVersion: true });
    const res = runHelper({
      base,
      scriptBody: script,
      dmg,
      version,
      arch,
      openMode: 'success',
    });
    if (res.status === 0) fail('bad bundle version should fail');
    const marker = readFileSync(join(res.app, 'Contents/MacOS/resh'), 'utf8');
    if (!marker.includes('OLD')) fail('bad version must not replace install');
    if (existsSync(res.alivePath)) fail('must not launch new app on version mismatch');
    const result = existsSync(res.resultPath) ? readFileSync(res.resultPath, 'utf8') : '';
    if (!/version mismatch/i.test(result)) {
      fail(`expected version mismatch message, got: ${result}`);
    }
    console.log('macOS helper rejects bad bundle version OK');
    rmSync(base, { recursive: true, force: true });
  }

  // --- Reject wrong architecture before replace ---
  {
    const base = mkdtempSync(join(tmpdir(), 'resh-mac-helper-badarch-'));
    const { dmg } = createTestDmg(base, { version, marker: 'EVIL' });
    const wrongArch = arch === 'arm64' ? 'x86_64' : 'arm64';
    const res = runHelper({
      base,
      scriptBody: script,
      dmg,
      version,
      arch,
      openMode: 'success',
      lipoArch: wrongArch,
    });
    if (res.status === 0) fail('bad architecture should fail');
    const marker = readFileSync(join(res.app, 'Contents/MacOS/resh'), 'utf8');
    if (!marker.includes('OLD')) fail('bad arch must not replace install');
    if (existsSync(res.alivePath)) fail('must not launch new app on arch mismatch');
    const result = existsSync(res.resultPath) ? readFileSync(res.resultPath, 'utf8') : '';
    if (!/architecture mismatch/i.test(result)) {
      fail(`expected architecture mismatch message, got: ${result}`);
    }
    console.log('macOS helper rejects bad architecture OK');
    rmSync(base, { recursive: true, force: true });
  }

  // --- Quarantine residual after clear → rollback, no success launch ---
  {
    const base = mkdtempSync(join(tmpdir(), 'resh-mac-helper-qfail-'));
    const { dmg } = createTestDmg(base, { version, marker: 'NEW' });
    const res = runHelper({
      base,
      scriptBody: script,
      dmg,
      version,
      arch,
      openMode: 'success',
      mockXattr: true,
      mockXattrMode: 'residual',
    });
    if (res.status === 0) fail('residual quarantine must fail helper');
    // After rollback, current app should be OLD again (backup restored).
    const marker = readFileSync(join(res.app, 'Contents/MacOS/resh'), 'utf8');
    if (!marker.includes('OLD')) {
      fail(`quarantine failure should rollback to OLD, got: ${marker}`);
    }
    const result = existsSync(res.resultPath) ? readFileSync(res.resultPath, 'utf8') : '';
    if (!/quarantine/i.test(result)) {
      fail(`expected quarantine rollback message, got: ${result}`);
    }
    // New-app success launch uses --restore-update-session; rollback relaunch must not.
    if (existsSync(res.openLog)) {
      const openArgs = readFileSync(res.openLog, 'utf8');
      if (openArgs.includes('--restore-update-session')) {
        fail(
          `quarantine residual must not launch new app with restore token; open log:\n${openArgs}`,
        );
      }
    }
    if (existsSync(res.alivePath)) {
      fail('quarantine residual must not write success alive marker for new app');
    }
    console.log('macOS helper quarantine residual rollback OK');
    rmSync(base, { recursive: true, force: true });
  }

  // --- xattr -dr permission failure + elevation cancel → rollback ---
  {
    const base = mkdtempSync(join(tmpdir(), 'resh-mac-helper-xattr-deny-'));
    const { dmg } = createTestDmg(base, { version, marker: 'NEW' });
    const res = runHelper({
      base,
      scriptBody: script,
      dmg,
      version,
      arch,
      openMode: 'success',
      mockXattr: true,
      mockXattrMode: 'deny-dr',
      // Force elevation path for clear when -dr fails; mock osascript exits 1 (cancel).
      parentWritable: false,
    });
    if (res.status === 0) fail('xattr permission denial must fail helper');
    const marker = readFileSync(join(res.app, 'Contents/MacOS/resh'), 'utf8');
    if (!marker.includes('OLD')) {
      fail(`xattr deny should rollback to OLD, got: ${marker}`);
    }
    if (existsSync(res.alivePath)) {
      fail('xattr deny / elevation cancel must not launch new app');
    }
    if (existsSync(res.openLog)) {
      const openArgs = readFileSync(res.openLog, 'utf8');
      if (openArgs.includes('--restore-update-session')) {
        fail(`elevation-cancel must not restore-token launch; open log:\n${openArgs}`);
      }
    }
    const result = existsSync(res.resultPath) ? readFileSync(res.resultPath, 'utf8') : '';
    if (!/quarantine/i.test(result)) {
      fail(`expected quarantine failure on xattr deny, got: ${result}`);
    }
    console.log('macOS helper xattr permission deny + elevation cancel rollback OK');
    rmSync(base, { recursive: true, force: true });
  }

  // --- Alive marker timeout → rollback ---
  {
    const base = mkdtempSync(join(tmpdir(), 'resh-mac-helper-alive-'));
    const { dmg } = createTestDmg(base, { version, marker: 'NEW' });
    const res = runHelper({
      base,
      scriptBody: script,
      dmg,
      version,
      arch,
      openMode: 'no-alive',
    });
    if (res.status === 0) fail('missing alive marker must fail');
    const marker = readFileSync(join(res.app, 'Contents/MacOS/resh'), 'utf8');
    if (!marker.includes('OLD')) {
      fail(`alive timeout should rollback to OLD, got: ${marker}`);
    }
    const result = existsSync(res.resultPath) ? readFileSync(res.resultPath, 'utf8') : '';
    if (!/confirm startup|alive|did not confirm/i.test(result)) {
      fail(`expected alive-timeout message, got: ${result}`);
    }
    console.log('macOS helper alive-timeout rollback OK');
    rmSync(base, { recursive: true, force: true });
  }

  // Detach leftover test mounts best-effort (should already be clean).
  const info = run('/usr/bin/hdiutil', ['info']);
  if (info.stdout) {
    const matches = [
      ...(info.stdout.match(/\/Volumes\/ReshT[0-9a-z]+/gi) || []),
      ...(info.stdout.match(/\/Volumes\/ReshTest(?:\s+\d+)?/gi) || []),
    ];
    for (const vol of new Set(matches)) {
      console.log(`detaching residual volume ${vol}`);
      run('/usr/bin/hdiutil', ['detach', vol, '-force']);
    }
  }
}

const script = extractMacosScript();
assertStaticContract(script);
quarantineClearSimulation();
attachRejectSimulation();
liveHelperIntegration();
console.log('test-macos-update-helper: all checks passed');
