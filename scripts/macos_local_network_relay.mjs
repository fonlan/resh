#!/usr/bin/env node
// Loopback CONNECT relay for Resh on macOS.
//
// Why this exists: macOS Local Network privacy (Apple TN3179) requires user approval for traffic to
// addresses on a broadcast-capable interface. Resh's WebDAV server (and any SSH target reached
// without a jumphost) can live on the LAN. Unsigned/ad-hoc builds — which is what ships until the
// phase-4 Developer ID work lands — get a new code identity on every build, and the stored approval
// stops matching (macOS then denies with EHOSTUNREACH and does not prompt again, because a rule
// already exists). See docs/macos/phase-4-signing-notarization.md.
//
// Loopback is not a local network, so a process that IS allowed to reach the LAN can relay for the
// app: Resh talks only to 127.0.0.1 and this process opens the LAN connection. The tunnel is a
// blind byte pipe, so TLS stays end-to-end and certificate validation is unaffected.
//
// Usage:
//   node scripts/macos_local_network_relay.mjs --install --allow=host[,host]   install as LaunchAgent
//   node scripts/macos_local_network_relay.mjs --status                        report agent state
//   node scripts/macos_local_network_relay.mjs --uninstall                     remove it
//   node scripts/macos_local_network_relay.mjs --allow=host                    run in the foreground
//
// Options:
//   --port=<n>     listen port on 127.0.0.1        (default 18080, env RELAY_PORT)
//   --allow=<list> comma-separated CONNECT targets (default empty = deny everything, env RELAY_ALLOW_HOSTS)
//                  the token "lan" allows any RFC1918 / link-local address
//   --node=<path>  Node binary for the LaunchAgent (default: the running interpreter)
//
// After installing, point Resh at it: Settings → proxy 127.0.0.1:<port>, then select that proxy for
// WebDAV and/or for the servers that need direct LAN access.

import net from 'node:net';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const SELF_PATH = fileURLToPath(import.meta.url);
const LABEL = 'com.fonlan.resh.local-network-relay';
const INSTALL_DIR = join(os.homedir(), 'Library', 'Application Support', 'resh-local-network-relay');
const INSTALLED_SCRIPT = join(INSTALL_DIR, 'relay.mjs');
const PLIST_PATH = join(os.homedir(), 'Library', 'LaunchAgents', `${LABEL}.plist`);
const LOG_PATH = join(os.homedir(), 'Library', 'Logs', 'resh-local-network-relay.log');
const LISTEN_HOST = '127.0.0.1';
const CONNECT_TIMEOUT_MS = 8000;

function parseArgs(argv) {
  const options = { mode: 'run', port: null, allow: null, node: null };
  const allow = [];

  for (const arg of argv) {
    if (arg === '--install') options.mode = 'install';
    else if (arg === '--uninstall') options.mode = 'uninstall';
    else if (arg === '--status') options.mode = 'status';
    else if (arg === '--help' || arg === '-h') options.mode = 'help';
    else if (arg.startsWith('--port=')) options.port = Number(arg.slice('--port='.length));
    else if (arg.startsWith('--allow=')) allow.push(arg.slice('--allow='.length));
    else if (arg.startsWith('--node=')) options.node = arg.slice('--node='.length);
    else throw new Error(`Unknown argument: ${arg}`);
  }

  const fromEnv = process.env.RELAY_ALLOW_HOSTS?.split(',') ?? [];
  const raw = [...allow.flatMap((value) => value.split(',')), ...fromEnv];
  const seen = new Set();
  for (const entry of raw) {
    const host = entry.trim().toLowerCase();
    if (host && !seen.has(host)) seen.add(host);
  }
  options.allow = [...seen];
  options.port = options.port ?? Number(process.env.RELAY_PORT ?? 18080);
  options.node = options.node ?? stableNodePath();
  return options;
}

// Prefer a version-independent shim over process.execPath: the latter usually points at a
// version-pinned path (…/opt/node-vX.Y.Z/bin/node) that disappears on the next Node upgrade, which
// would silently take WebDAV down with it. Only shims resolving to the same binary are accepted.
function stableNodePath() {
  let resolved;
  try {
    resolved = realpathSync(process.execPath);
  } catch {
    return process.execPath;
  }

  const candidates = [
    join(os.homedir(), '.local', 'bin', 'node'),
    '/opt/homebrew/bin/node',
    '/usr/local/bin/node',
  ];
  for (const candidate of candidates) {
    try {
      if (existsSync(candidate) && realpathSync(candidate) === resolved) return candidate;
    } catch {
      // Unreadable or dangling shim: fall through to the next candidate.
    }
  }
  return process.execPath;
}

function usage() {
  console.log(`Resh macOS local-network relay

  --install              install and start a per-user LaunchAgent
  --status               report the LaunchAgent state
  --uninstall            stop and remove the LaunchAgent and its files
  --port=<n>             listen port on 127.0.0.1 (default 18080)
  --allow=<host[,host]>  CONNECT allowlist; "lan" allows any RFC1918/link-local address
  --node=<path>          Node binary used by the LaunchAgent
  --help                 this message

With no mode flag the relay runs in the foreground.

Install must name its targets explicitly: with an empty --allow the relay refuses every CONNECT.`);
}

function launchctl(args) {
  return spawnSync('launchctl', args, { encoding: 'utf8', shell: false });
}

function isPrivateIpv4(host) {
  const match = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host);
  if (!match) return false;
  const [a, b] = [Number(match[1]), Number(match[2])];
  if (a === 10) return true; // 10/8
  if (a === 172 && b >= 16 && b <= 31) return true; // 172.16/12
  if (a === 192 && b === 168) return true; // 192.168/16
  if (a === 169 && b === 254) return true; // link-local
  return false;
}

function buildPlist({ port, allow, node }) {
  const env = [
    ['RELAY_PORT', String(port)],
    ['RELAY_ALLOW_HOSTS', allow.join(',')],
  ];
  const envXml = env
    .map(([key, value]) => `        <key>${key}</key>\n        <string>${value}</string>`)
    .join('\n');

  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>${node}</string>
        <string>${INSTALLED_SCRIPT}</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
${envXml}
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>${LOG_PATH}</string>
    <key>StandardErrorPath</key>
    <string>${LOG_PATH}</string>
</dict>
</plist>
`;
}

function install(options) {
  if (options.allow.length === 0) {
    console.error('Refusing to install with an empty allowlist: pass --allow=host[,host].');
    process.exitCode = 1;
    return;
  }

  mkdirSync(INSTALL_DIR, { recursive: true });
  mkdirSync(join(os.homedir(), 'Library', 'LaunchAgents'), { recursive: true });
  copyFileSync(SELF_PATH, INSTALLED_SCRIPT);
  writeFileSync(PLIST_PATH, buildPlist(options));

  const domain = `gui/${process.getuid()}`;
  launchctl(['bootout', `${domain}/${LABEL}`]); // ignore: may not be loaded yet
  const boot = launchctl(['bootstrap', domain, PLIST_PATH]);
  if (boot.status !== 0) {
    console.error(`launchctl bootstrap failed: ${boot.stderr?.trim() || boot.status}`);
    process.exitCode = 1;
    return;
  }

  console.log(`Installed  ${INSTALLED_SCRIPT}`);
  console.log(`Agent      ${PLIST_PATH}`);
  console.log(`Listening  ${LISTEN_HOST}:${options.port}`);
  console.log(`Allowlist  ${options.allow.join(', ')}`);
  console.log(`Log        ${LOG_PATH}`);
  console.log('');
  console.log('Next: in Resh add a proxy for 127.0.0.1 with that port, then select it for WebDAV');
  console.log('and for any server that needs direct LAN access.');
}

function uninstall() {
  const domain = `gui/${process.getuid()}`;
  launchctl(['bootout', `${domain}/${LABEL}`]);
  if (existsSync(PLIST_PATH)) rmSync(PLIST_PATH, { force: true });
  if (existsSync(INSTALL_DIR)) rmSync(INSTALL_DIR, { recursive: true, force: true });
  console.log('Removed the relay LaunchAgent and its files.');
  console.log('Remember to clear the proxy selection in Resh.');
}

function status() {
  const domain = `gui/${process.getuid()}`;
  const print = launchctl(['print', `${domain}/${LABEL}`]);
  console.log(`agent      ${print.status === 0 ? 'loaded' : 'not loaded'}`);
  console.log(`plist      ${PLIST_PATH}${existsSync(PLIST_PATH) ? '' : ' (missing)'}`);
  console.log(`script     ${INSTALLED_SCRIPT}${existsSync(INSTALLED_SCRIPT) ? '' : ' (missing)'}`);
  if (print.status === 0) {
    for (const line of print.stdout.split('\n')) {
      if (/^\s*(state|pid|program) =/.test(line)) console.log(`  ${line.trim()}`);
    }
  }
}

function runRelay(options) {
  const allowLan = options.allow.includes('lan');
  const isAllowed = (host) =>
    options.allow.length === 0 ||
    options.allow.includes(host) ||
    (allowLan && isPrivateIpv4(host));

  const log = (entry) => {
    const line = JSON.stringify({ ts: new Date().toISOString(), ...entry });
    if (entry.level === 'warn' || entry.level === 'error') console.error(line);
    else console.log(line);
  };

  const server = net.createServer((client) => {
    client.setNoDelay(true);
    client.on('error', () => {});

    let upstream = null;
    let settled = false;

    const onHandshake = (chunk) => {
      client.off('data', onHandshake);
      const firstLine = chunk.toString('latin1').split('\r\n', 1)[0] ?? '';
      const match = /^CONNECT\s+([^\s:]+):(\d+)\s+HTTP\/1\.[01]$/i.exec(firstLine.trim());

      if (!match) {
        settled = true;
        client.end('HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n');
        log({ level: 'warn', event: 'unsupported-request', firstLine });
        return;
      }

      const host = match[1].toLowerCase();
      const port = Number(match[2]);

      if (!isAllowed(host)) {
        settled = true;
        client.end('HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n');
        log({ level: 'warn', event: 'host-not-allowed', host });
        return;
      }

      // A black-holed upstream (host asleep, firewall dropping SYNs) must fail rather than leave the
      // client hanging, which otherwise looks like a frozen sync.
      upstream = net.connect({ host, port });
      upstream.setTimeout(CONNECT_TIMEOUT_MS);
      upstream.setNoDelay(true);

      upstream.on('connect', () => {
        upstream.setTimeout(0);
        settled = true;
        client.write('HTTP/1.1 200 Connection Established\r\n\r\n');
        client.pipe(upstream);
        upstream.pipe(client);
      });

      upstream.on('timeout', () => {
        log({ level: 'warn', event: 'upstream-timeout', host, port });
        if (!settled) {
          settled = true;
          client.end('HTTP/1.1 504 Gateway Timeout\r\nConnection: close\r\n\r\n');
        }
        upstream.destroy();
      });

      upstream.on('error', (error) => {
        log({ level: 'error', event: 'upstream-error', host, code: error.code });
        if (!settled) {
          settled = true;
          client.end('HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n');
        }
        client.destroy();
      });

      upstream.on('close', () => client.destroy());
    };

    client.on('data', onHandshake);
    client.on('close', () => upstream?.destroy());
  });

  server.on('error', (error) => {
    log({ level: 'error', event: 'listen-error', code: error.code, message: error.message });
    process.exit(1);
  });

  server.listen(options.port, LISTEN_HOST, () => {
    log({ event: 'listening', address: `${LISTEN_HOST}:${options.port}`, allow: options.allow });
  });

  for (const signal of ['SIGINT', 'SIGTERM']) {
    process.on(signal, () => server.close(() => process.exit(0)));
  }
}

const options = parseArgs(process.argv.slice(2));
switch (options.mode) {
  case 'help':
    usage();
    break;
  case 'install':
    install(options);
    break;
  case 'uninstall':
    uninstall();
    break;
  case 'status':
    status();
    break;
  default:
    runRelay(options);
}
