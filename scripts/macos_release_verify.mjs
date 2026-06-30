#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { copyFileSync, createReadStream, existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const targetArch = {
  'aarch64-apple-darwin': 'aarch64',
  'x86_64-apple-darwin': 'x64',
};

function parseArgs(argv) {
  const args = {
    target: process.arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin',
    outDir: null,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--target') {
      args.target = argv[++index];
    } else if (arg === '--app') {
      args.app = argv[++index];
    } else if (arg === '--dmg') {
      args.dmg = argv[++index];
    } else if (arg === '--out-dir') {
      args.outDir = argv[++index];
    } else if (arg === '--help' || arg === '-h') {
      args.help = true;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return args;
}

function printHelp() {
  console.log(`Usage: npm run macos:verify -- [--target aarch64-apple-darwin|x86_64-apple-darwin] [--app path] [--dmg path] [--out-dir path]

Verifies codesign, Gatekeeper assessment, and stapled notarization tickets for
the macOS .app and .dmg. It also creates a release-safe .app.zip and writes
SHA-256 checksums for the .zip and .dmg.`);
}

function run(command, args) {
  console.log(`$ ${command} ${args.join(' ')}`);
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    shell: false,
  });

  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
}

function findFirst(directory, predicate) {
  if (!existsSync(directory)) {
    throw new Error(`Directory does not exist: ${directory}`);
  }

  const entries = readdirSync(directory)
    .map((name) => join(directory, name))
    .filter(predicate)
    .sort((left, right) => statSync(right).mtimeMs - statSync(left).mtimeMs);

  if (entries.length === 0) {
    throw new Error(`No matching artifact found in ${directory}`);
  }

  return entries[0];
}

function sha256(path) {
  return new Promise((resolveHash, reject) => {
    const hash = createHash('sha256');
    const stream = createReadStream(path);
    stream.on('error', reject);
    stream.on('data', (chunk) => hash.update(chunk));
    stream.on('end', () => resolveHash(hash.digest('hex')));
  });
}

async function writeChecksums(paths, checksumPath) {
  const lines = [];
  for (const path of paths) {
    lines.push(`${await sha256(path)}  ${basename(path)}`);
  }
  writeFileSync(checksumPath, `${lines.join('\n')}\n`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    printHelp();
    return;
  }

  const bundleRoot = join('src-tauri', 'target', args.target, 'release', 'bundle');
  const app = resolve(args.app ?? findFirst(join(bundleRoot, 'macos'), (path) => path.endsWith('.app') && statSync(path).isDirectory()));
  const dmg = resolve(args.dmg ?? findFirst(join(bundleRoot, 'dmg'), (path) => path.endsWith('.dmg') && statSync(path).isFile()));
  const outDir = resolve(args.outDir ?? join('artifacts', 'macos', args.target));
  const arch = targetArch[args.target] ?? args.target;
  const zipPath = join(outDir, `${basename(app, '.app')}_${arch}.app.zip`);
  const dmgOutPath = join(outDir, basename(dmg));
  const checksumPath = join(outDir, 'SHA256SUMS.txt');

  mkdirSync(outDir, { recursive: true });
  rmSync(zipPath, { force: true });
  if (resolve(dmgOutPath) !== dmg) {
    rmSync(dmgOutPath, { force: true });
  }

  run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', app]);
  run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', dmg]);
  run('spctl', ['--assess', '--type', 'execute', '--verbose=4', app]);
  run('spctl', ['--assess', '--type', 'open', '--context', 'context:primary-signature', '--verbose=4', dmg]);
  run('xcrun', ['stapler', 'validate', app]);
  run('xcrun', ['stapler', 'validate', dmg]);
  run('ditto', ['-c', '-k', '--keepParent', app, zipPath]);
  if (resolve(dmgOutPath) !== dmg) {
    copyFileSync(dmg, dmgOutPath);
  }

  await writeChecksums([zipPath, dmgOutPath], checksumPath);
  console.log(`Wrote ${zipPath}`);
  console.log(`Wrote ${dmgOutPath}`);
  console.log(`Wrote ${checksumPath}`);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
