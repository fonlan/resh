#!/usr/bin/env node
import { spawnSync } from 'node:child_process';

const requiredEnv = [
  'APPLE_CERTIFICATE',
  'APPLE_CERTIFICATE_PASSWORD',
  'APPLE_API_KEY',
  'APPLE_API_ISSUER',
  'APPLE_API_KEY_PATH',
];

const optionalEnv = ['APPLE_SIGNING_IDENTITY', 'APPLE_PROVIDER_SHORT_NAME'];

function parseArgs(argv) {
  const args = {
    target: process.arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin',
    bundles: 'app,dmg',
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--target') {
      args.target = argv[++index];
    } else if (arg === '--bundles') {
      args.bundles = argv[++index];
    } else if (arg === '--help' || arg === '-h') {
      args.help = true;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return args;
}

function printHelp() {
  console.log(`Usage: npm run macos:release -- [--target aarch64-apple-darwin|x86_64-apple-darwin] [--bundles app,dmg]

Required environment:
  ${requiredEnv.join('\n  ')}

Optional environment:
  ${optionalEnv.join('\n  ')}

APPLE_CERTIFICATE must be the base64-encoded Developer ID Application .p12.
APPLE_API_KEY_PATH must point to the App Store Connect API key .p8 file.`);
}

function requireEnvironment() {
  const missing = requiredEnv.filter((name) => !process.env[name]);
  if (missing.length > 0) {
    throw new Error(`Missing macOS signing/notarization environment: ${missing.join(', ')}`);
  }

  for (const name of optionalEnv) {
    if (!process.env[name]) {
      delete process.env[name];
    }
  }

  if (process.env.SKIP_STAPLING === 'true') {
    throw new Error('SKIP_STAPLING=true is not allowed for release builds.');
  }
}

function run(command, args, options = {}) {
  console.log(`$ ${command} ${args.join(' ')}`);
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    shell: false,
    ...options,
  });

  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
}

try {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    printHelp();
    process.exit(0);
  }

  requireEnvironment();
  run('npm', [
    'run',
    'tauri-build',
    '--',
    '--config',
    'src-tauri/tauri.macos.conf.json',
    '--target',
    args.target,
    '--bundles',
    args.bundles,
  ]);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
