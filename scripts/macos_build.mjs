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

function removeMatching(directory, predicate, label) {
  if (!existsSync(directory)) {
    return 0;
  }

  let removed = 0;
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    let stats;
    try {
      stats = statSync(path);
    } catch {
      continue;
    }
    if (!predicate(name, path, stats)) {
      continue;
    }
    rmSync(path, { recursive: true, force: true });
    console.log(`Removed ${label}: ${path}`);
    removed += 1;
  }
  return removed;
}

function removeAppBundles(directory) {
  // ponytail: Tauri needs a temporary .app to create the DMG; delete it after so only the DMG remains.
  return removeMatching(
    directory,
    (name, _path, stats) => name.endsWith('.app') && stats.isDirectory(),
    '.app',
  );
}

function removeTempRwDmgs(directory) {
  // Leftover rw.*.dmg in the DMG source dir can be re-bundled and break hdiutil ("no space left").
  return removeMatching(
    directory,
    (name, _path, stats) => stats.isFile() && /^rw\.\d+\..+\.dmg$/.test(name),
    'temp RW DMG',
  );
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

function bundleRoots(target) {
  const roots = [join('src-tauri', 'target', 'release', 'bundle')];
  if (target) {
    roots.push(join('src-tauri', 'target', target, 'release', 'bundle'));
  }
  return roots;
}

function cleanupAfterDmgBuild(target) {
  let removedApps = 0;
  let removedTemps = 0;

  for (const root of bundleRoots(target)) {
    const macosDir = join(root, 'macos');
    const dmgDir = join(root, 'dmg');
    removedApps += removeAppBundles(macosDir);
    removedTemps += removeTempRwDmgs(macosDir);
    removedTemps += removeTempRwDmgs(dmgDir);
  }

  if (removedApps === 0) {
    console.log('No leftover .app bundles to remove (DMG-only artifact).');
  }
  if (removedTemps > 0) {
    console.log(`Cleaned ${removedTemps} temporary RW DMG file(s).`);
  }
}

try {
  // Always force DMG-only packaging; ignore any caller --bundles value.
  const extraArgs = stripBundleArgs(process.argv.slice(2));
  if (!hasArg(extraArgs, '--config')) {
    extraArgs.unshift('--config', 'src-tauri/tauri.macos.conf.json');
  }
  extraArgs.unshift('--bundles', 'dmg');

  run('tauri', ['build', ...extraArgs]);
  cleanupAfterDmgBuild(parseTarget(extraArgs));
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
