#!/usr/bin/env node
import { existsSync, readdirSync, rmSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

function run(command, args) {
  console.log(`$ ${command} ${args.join(' ')}`);
  const result = spawnSync(command, args, { stdio: 'inherit', shell: false });
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
}

function removeAppBundles(directory) {
  if (!existsSync(directory)) {
    return;
  }

  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    if (name.endsWith('.app') && statSync(path).isDirectory()) {
      rmSync(path, { recursive: true, force: true });
    }
  }
}

function hasArg(argv, name) {
  return argv.includes(name);
}

function stripBundleArgs(argv) {
  const stripped = [];
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === '--bundles') {
      index += 1;
    } else {
      stripped.push(argv[index]);
    }
  }
  return stripped;
}

function parseTarget(argv) {
  const index = argv.indexOf('--target');
  return index === -1 ? null : argv[index + 1];
}

try {
  const extraArgs = stripBundleArgs(process.argv.slice(2));
  if (!hasArg(extraArgs, '--config')) {
    extraArgs.unshift('--config', 'src-tauri/tauri.macos.conf.json');
  }
  if (!hasArg(extraArgs, '--bundles')) {
    extraArgs.unshift('--bundles', 'dmg');
  }
  run('tauri', ['build', ...extraArgs]);

  // ponytail: Tauri needs a temporary .app to create the DMG; remove final bundle dirs after build.
  removeAppBundles(join('src-tauri', 'target', 'release', 'bundle', 'macos'));
  const target = parseTarget(extraArgs);
  if (target) {
    removeAppBundles(join('src-tauri', 'target', target, 'release', 'bundle', 'macos'));
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
