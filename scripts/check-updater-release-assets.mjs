#!/usr/bin/env node
/**
 * Validate GitHub Release assets against the in-app updater contract.
 *
 * The updater API is exactly these four files (no extra updater signing secrets):
 *   Resh-<tag>-windows-x86_64.exe
 *   Resh-<tag>-macos-aarch64.dmg
 *   Resh-<tag>-macos-x86_64.dmg
 *   SHA256SUMS.txt
 *
 * Usage:
 *   node scripts/check-updater-release-assets.mjs --tag v1.2.3 --dir ./release-assets
 *   GITHUB_REF_NAME=v1.2.3 node scripts/check-updater-release-assets.mjs --dir ./release-assets
 *
 * Options:
 *   --tag <tag>     Release tag (v-prefixed semver). Defaults to GITHUB_REF_NAME.
 *   --dir <path>    Directory containing the four assets.
 *   --allow-missing-sums  Skip SHA256SUMS presence (only for partial staging checks).
 *   --self-test     Run built-in positive/negative fixtures and exit.
 */
import {
  createHash,
  randomBytes,
} from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** Matches client parse_release_tag / asset name expectations (leading v + semver body). */
const TAG_RE =
  /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

/** GNU sha256sum line: 64 hex, two spaces or " *", then basename (no path). */
const SHA256SUMS_LINE_RE =
  /^([0-9a-fA-F]{64}) ( |\*)([^\s/\\]+)\s*$/;

function fail(message) {
  console.error(message);
  process.exit(1);
}

function parseArgs(argv) {
  const out = {
    tag: process.env.GITHUB_REF_NAME?.trim() || null,
    dir: null,
    allowMissingSums: false,
    selfTest: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--self-test') {
      out.selfTest = true;
    } else if (a === '--allow-missing-sums') {
      out.allowMissingSums = true;
    } else if (a === '--tag') {
      out.tag = argv[++i]?.trim() || null;
    } else if (a === '--dir') {
      out.dir = argv[++i] || null;
    } else if (a === '--help' || a === '-h') {
      console.log(readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1, 20).join('\n'));
      process.exit(0);
    } else {
      fail(`Unknown argument: ${a}`);
    }
  }
  return out;
}

function expectedInstallNames(tag) {
  return [
    `Resh-${tag}-windows-x86_64.exe`,
    `Resh-${tag}-macos-aarch64.dmg`,
    `Resh-${tag}-macos-x86_64.dmg`,
  ];
}

/**
 * Validate directory contents for updater compatibility.
 * @returns {{ ok: true, names: string[] } | { ok: false, errors: string[] }}
 */
export function validateUpdaterReleaseAssets({ tag, dir, allowMissingSums = false }) {
  const errors = [];
  if (!tag || !TAG_RE.test(tag)) {
    errors.push(
      `Invalid tag "${tag ?? ''}": expected v<semver> (e.g. v1.2.3), matching updater release tags.`,
    );
  }
  if (!dir) {
    errors.push('Asset directory required (--dir).');
  } else if (!existsSync(dir)) {
    errors.push(`Asset directory does not exist: ${dir}`);
  }

  if (errors.length) {
    return { ok: false, errors };
  }

  const installNames = expectedInstallNames(tag);
  const required = allowMissingSums
    ? [...installNames]
    : [...installNames, 'SHA256SUMS.txt'];

  const entries = readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && !e.name.startsWith('.'))
    .map((e) => e.name)
    .sort();

  const unique = new Set(entries);
  if (unique.size !== entries.length) {
    errors.push('Duplicate filenames in asset directory.');
  }

  for (const name of required) {
    if (!unique.has(name)) {
      errors.push(`Missing required updater asset: ${name}`);
    }
  }

  const expectedSet = new Set(required);
  for (const name of entries) {
    if (!expectedSet.has(name)) {
      errors.push(
        `Unexpected file (updater expects exactly ${required.length} named assets): ${name}`,
      );
    }
  }

  // Filename uniqueness across platforms (already exact set).
  if (new Set(installNames).size !== installNames.length) {
    errors.push('Internal error: duplicate expected install names.');
  }

  if (!allowMissingSums && unique.has('SHA256SUMS.txt')) {
    const sumsPath = join(dir, 'SHA256SUMS.txt');
    const text = readFileSync(sumsPath, 'utf8');
    const sumErrors = validateSha256SumsText(text, installNames, dir);
    errors.push(...sumErrors);
  }

  if (errors.length) {
    return { ok: false, errors };
  }
  return { ok: true, names: required };
}

/**
 * Parse GNU sha256sum format; ensure each install asset has exactly one entry
 * with matching on-disk hash when files exist under dir.
 */
export function validateSha256SumsText(text, installNames, dir) {
  const errors = [];
  if (!text || !text.trim()) {
    return ['SHA256SUMS.txt is empty'];
  }

  /** @type {Map<string, string>} */
  const map = new Map();
  const lines = text.split(/\r?\n/);
  let lineNo = 0;
  for (const raw of lines) {
    lineNo += 1;
    if (!raw.trim()) {
      continue;
    }
    // Reject path separators and backslashes in names (updater basename-only).
    if (raw.includes('/') || raw.includes('\\')) {
      errors.push(
        `SHA256SUMS.txt line ${lineNo}: path components are not allowed (basename only)`,
      );
      continue;
    }
    const m = raw.match(SHA256SUMS_LINE_RE);
    if (!m) {
      errors.push(
        `SHA256SUMS.txt line ${lineNo}: not GNU sha256sum format (64 hex + two spaces or " *"+ basename)`,
      );
      continue;
    }
    const hex = m[1].toLowerCase();
    const name = m[3];
    if (map.has(name) && map.get(name) !== hex) {
      errors.push(`SHA256SUMS.txt: conflicting hashes for "${name}"`);
    } else if (map.has(name)) {
      // duplicate identical — allow once but warn as non-unique listing
      errors.push(`SHA256SUMS.txt: duplicate entry for "${name}"`);
    } else {
      map.set(name, hex);
    }
  }

  for (const name of installNames) {
    if (!map.has(name)) {
      errors.push(`SHA256SUMS.txt missing entry for "${name}"`);
      continue;
    }
    const filePath = join(dir, name);
    if (!existsSync(filePath)) {
      continue;
    }
    const actual = sha256File(filePath);
    const expected = map.get(name);
    if (actual !== expected) {
      errors.push(
        `SHA256SUMS.txt hash mismatch for "${name}": expected ${expected}, got ${actual}`,
      );
    }
  }

  // Extra entries (e.g. SHA256SUMS itself) are allowed by GNU tools; updater only
  // requires install asset names. Flag unknown install-like names that are not ours.
  for (const name of map.keys()) {
    if (name === 'SHA256SUMS.txt') {
      continue;
    }
    if (!installNames.includes(name)) {
      // Soft: extra third-party names break "exactly three install assets" contract
      // only if those files also exist in the directory (already checked above).
      if (installNames.every((n) => n !== name) && name.startsWith('Resh-')) {
        errors.push(
          `SHA256SUMS.txt lists unexpected Resh asset "${name}" (not in updater install set)`,
        );
      }
    }
  }

  return errors;
}

function sha256File(path) {
  const h = createHash('sha256');
  h.update(readFileSync(path));
  return h.digest('hex');
}

function writeFixtureFile(dir, name, content) {
  writeFileSync(join(dir, name), content);
}

function runSelfTest() {
  const tag = 'v9.9.9-test';
  const base = join(tmpdir(), `resh-updater-assets-${randomBytes(6).toString('hex')}`);
  mkdirSync(base, { recursive: true });

  try {
    const goodDir = join(base, 'good');
    mkdirSync(goodDir);
    const names = expectedInstallNames(tag);
    const blobs = names.map((n, i) => {
      const body = Buffer.from(`payload-${i}-${n}`);
      writeFixtureFile(goodDir, n, body);
      return { n, body, hash: createHash('sha256').update(body).digest('hex') };
    });
    const sums = blobs
      .slice()
      .sort((a, b) => a.n.localeCompare(b.n))
      .map((b) => `${b.hash}  ${b.n}`)
      .join('\n') + '\n';
    writeFixtureFile(goodDir, 'SHA256SUMS.txt', sums);

    const good = validateUpdaterReleaseAssets({ tag, dir: goodDir });
    if (!good.ok) {
      fail(`self-test good fixture failed:\n${good.errors.join('\n')}`);
    }
    console.log('self-test: good fixture OK');

    const badName = join(base, 'bad-name');
    mkdirSync(badName);
    for (const b of blobs) {
      writeFixtureFile(badName, b.n.replace('windows-x86_64', 'windows-amd64'), b.body);
    }
    writeFixtureFile(badName, 'SHA256SUMS.txt', sums);
    const bad = validateUpdaterReleaseAssets({ tag, dir: badName });
    if (bad.ok) {
      fail('self-test expected rename failure');
    }
    console.log('self-test: bad filename rejected OK');

    const badHash = join(base, 'bad-hash');
    mkdirSync(badHash);
    for (const b of blobs) {
      writeFixtureFile(badHash, b.n, b.body);
    }
    writeFixtureFile(
      badHash,
      'SHA256SUMS.txt',
      blobs.map((b) => `${'0'.repeat(64)}  ${b.n}`).join('\n') + '\n',
    );
    const badH = validateUpdaterReleaseAssets({ tag, dir: badHash });
    if (badH.ok) {
      fail('self-test expected hash mismatch failure');
    }
    console.log('self-test: hash mismatch rejected OK');

    const extra = join(base, 'extra');
    mkdirSync(extra);
    for (const b of blobs) {
      writeFixtureFile(extra, b.n, b.body);
    }
    writeFixtureFile(extra, 'SHA256SUMS.txt', sums);
    writeFixtureFile(extra, 'notes.txt', 'nope');
    const extraR = validateUpdaterReleaseAssets({ tag, dir: extra });
    if (extraR.ok) {
      fail('self-test expected unexpected file failure');
    }
    console.log('self-test: unexpected file rejected OK');

    console.log('All self-tests passed.');
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.selfTest) {
    runSelfTest();
    return;
  }
  if (!args.tag) {
    fail('Release tag required. Pass --tag vX.Y.Z or set GITHUB_REF_NAME.');
  }
  if (!args.dir) {
    fail('Asset directory required. Pass --dir <path>.');
  }

  const result = validateUpdaterReleaseAssets({
    tag: args.tag,
    dir: args.dir,
    allowMissingSums: args.allowMissingSums,
  });
  if (!result.ok) {
    for (const e of result.errors) {
      console.error(e);
    }
    process.exit(1);
  }
  console.log(
    `Updater release assets OK for ${args.tag}:\n${result.names.map((n) => `  - ${n}`).join('\n')}`,
  );
}

// Only run CLI when executed directly (not when imported by tests).
const isMain =
  process.argv[1] &&
  fileURLToPath(import.meta.url) ===
    // normalize for Windows
    join(process.argv[1]);

if (
  process.argv[1] &&
  (process.argv[1].endsWith('check-updater-release-assets.mjs') ||
    process.argv[1].includes('check-updater-release-assets'))
) {
  main();
}
