#!/usr/bin/env node
/**
 * Verify a release tag matches package.json, Cargo.toml, and tauri.conf.json.
 *
 * Usage:
 *   node scripts/check-release-version.mjs [tag]
 *   GITHUB_REF_NAME=v1.2.3 node scripts/check-release-version.mjs
 *
 * Tag must be v<semver> (optional prerelease/build: v1.2.3-beta.1, v1.2.3+build).
 * The leading "v" is stripped and compared to the three project version fields.
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** SemVer core + optional prerelease + optional build (npm/Cargo-style). */
const SEMVER_BODY =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

function fail(message) {
  console.error(message);
  process.exit(1);
}

function resolveTag() {
  const arg = process.argv[2]?.trim();
  if (arg) {
    return arg;
  }

  const fromEnv = process.env.GITHUB_REF_NAME?.trim();
  if (fromEnv) {
    return fromEnv;
  }

  fail(
    'Release tag required. Pass as argv (e.g. v1.2.3) or set GITHUB_REF_NAME.',
  );
}

function parseReleaseVersion(tag) {
  if (!tag.startsWith('v') || tag.length < 2) {
    fail(
      `Invalid release tag "${tag}": expected v<semver> (example: v1.2.3).`,
    );
  }

  const version = tag.slice(1);
  if (!SEMVER_BODY.test(version)) {
    fail(
      `Invalid release tag "${tag}": version body "${version}" is not valid semver.`,
    );
  }

  return version;
}

function readPackageVersion() {
  const path = join(root, 'package.json');
  const pkg = JSON.parse(readFileSync(path, 'utf8'));
  if (typeof pkg.version !== 'string' || !pkg.version.trim()) {
    fail(`Missing or empty version in ${path}`);
  }
  return { path: 'package.json', version: pkg.version.trim() };
}

function readCargoVersion() {
  const path = join(root, 'src-tauri/Cargo.toml');
  const content = readFileSync(path, 'utf8');
  const lines = content.split(/\r?\n/);
  let inPackage = false;

  for (const line of lines) {
    const section = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (section) {
      inPackage = section[1] === 'package';
      continue;
    }

    if (!inPackage) {
      continue;
    }

    const match = line.match(/^\s*version\s*=\s*"([^"]+)"\s*$/);
    if (match) {
      return { path: 'src-tauri/Cargo.toml', version: match[1].trim() };
    }
  }

  fail(`Could not parse package.version from ${path}`);
}

function readTauriVersion() {
  const path = join(root, 'src-tauri/tauri.conf.json');
  const conf = JSON.parse(readFileSync(path, 'utf8'));
  if (typeof conf.version !== 'string' || !conf.version.trim()) {
    fail(`Missing or empty version in ${path}`);
  }
  return { path: 'src-tauri/tauri.conf.json', version: conf.version.trim() };
}

const tag = resolveTag();
const expected = parseReleaseVersion(tag);
const sources = [readPackageVersion(), readCargoVersion(), readTauriVersion()];

const mismatches = sources.filter((s) => s.version !== expected);
if (mismatches.length > 0) {
  console.error(
    `Release version mismatch: tag ${tag} requires project version "${expected}".`,
  );
  for (const source of sources) {
    const mark = source.version === expected ? 'ok' : 'MISMATCH';
    console.error(`  [${mark}] ${source.path}: ${source.version}`);
  }
  process.exit(1);
}

console.log(
  `Release version check passed: tag ${tag} matches ${expected} in package.json, Cargo.toml, and tauri.conf.json.`,
);
