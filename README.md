# Resh SSH Client

A modern, secure multi-tab SSH client built with Tauri 2 + React, designed for developers who need seamless SSH connection management with cross-machine configuration portability.

## Features

### Core Features
- **Multi-Tab Interface** - Manage multiple simultaneous SSH sessions in a clean tabbed interface with drag-to-reorder support
- **Split View Workspace** - Switch between left-right, top-bottom, and four-pane layouts without interrupting active terminal connections
- **Comprehensive Configuration** - Full server, authentication, and proxy management with WebDAV sync
- **Proxy & Jumphost Support** - Connect through HTTP/SOCKS5 proxies or SSH jumphosts
- **Port Forwarding** - Easy local-to-remote port forwarding configuration per connection
- **Frameless UI** - Modern frameless window with embedded controls and custom drag regions
- **Single-Instance Mode** - Re-launching the app focuses the existing main window instead of opening another process
- **Cross-Machine Sync** - WebDAV-based configuration sync with conflict resolution

### Advanced Features
- **SSH Key Management** - Store SSH key content (not paths) for true portability
- **Auto-Execute Commands** - Run commands automatically after connection
- **Environment Variables** - Set custom environment variables per connection
- **Connection Cloning** - Quickly duplicate existing SSH sessions
- **Keep-Alive** - Configurable keep-alive intervals to maintain connections

## Tech Stack

**Frontend:**
- React 19 with TypeScript
- React Compiler enabled in Vite build (babel-plugin-react-compiler)
- xterm.js for terminal emulation
- Vite for build tooling
- Tailwind CSS v4 for styling

**Backend:**
- Rust with Tauri 2 framework
- russh for SSH protocol implementation

**Storage:**
- WebDAV protocol for sync

## Platform Support

| Platform | Status | Distribution |
| --- | --- | --- |
| Windows 10+ x64 | Supported | Portable `.exe` (unsigned) via GitHub Releases |
| macOS 11+ Apple Silicon | Supported | Unsigned / unnotarized `.dmg` via GitHub Releases |
| macOS 10.15+ Intel | Supported | Unsigned / unnotarized `.dmg` via GitHub Releases |

## Installation

### Prerequisites
- Node.js 22.12.x and npm 10.9.x
- Rust 1.88+ (for building from source)
- Platform build dependencies:
  - Windows 10+ with Microsoft C++ Build Tools and WebView2
  - macOS 10.15+ (Intel) / macOS 11+ (Apple Silicon) with Xcode Command Line Tools for source builds

### Quick Start

1. Clone the repository:
```bash
git clone https://github.com/fonlan/resh.git
cd resh
```

2. Install dependencies:
```bash
npm install
```

3. Run in development mode:
```bash
npm run tauri-dev
```

## Configuration

### Configuration Files

Resh stores configuration in a platform-specific application data directory:

```
Windows: %AppData%\Resh\
macOS:   ~/Library/Application Support/Resh/

Resh/
├── local.json        # Local-only config and settings
├── config.db         # Local application database
└── logs/             # Application and connection logs
```

### Configuration Schema

Both `sync.json` and `local.json` support:

- **Servers** - SSH server configurations with connection details
- **Authentications** - SSH keys and password credentials
- **Proxies** - HTTP/SOCKS5 proxy configurations

Only `local.json` support:
- **General Settings** - Theme, language, terminal preferences, WebDAV settings

### Sync Strategy

- **Synced Items** - Stored in `sync.json`, synced via WebDAV across machines
- **Local-Only Items** - Stored in `local.json`, machine-specific overrides
- **Merge Logic** - Items in `local.json` override matching UUIDs in `sync.json`

## Development

### Available Scripts

```bash
# Frontend development
npm run dev              # Run Vite dev server
npm run build            # Build frontend (TypeScript + Vite)
npm run build:macos      # Build macOS DMG bundle (prefer CI=true for unsigned CI-like builds)

# Release checks
npm run check:release-version -- vX.Y.Z  # Tag must match package/Cargo/tauri versions
npm run ci:pin-actions                   # Third-party Actions must be full-SHA pinned

# Tauri commands
npm run tauri-dev        # Run Tauri in development mode
npm run tauri-build      # Build production Tauri app
npm run tauri            # Direct access to Tauri CLI

# SFTP performance harness (pass args after --)
npm run sftp:baseline -- -ServerHost <host> -User <user>
npm run sftp:fairness -- -ServerHost <host> -User <user>
npm run sftp:perf-suite -- -ServerHost <host> -User <user>
```

SFTP harness outputs are written under `artifacts/` by default. Use script flags (for example `-PrivateKeyPath`, `-Port`, `-OutputDir`) to adapt runs for your environment. The current npm wrappers require Windows PowerShell and are not yet qualified on macOS.


### Adding New Features

**Frontend:**
- Add components in `src/components/`
- Use TypeScript for type safety
- Follow React hooks pattern
- Use Tailwind CSS v4 for styling (CSS-first configuration)

**Backend:**
- Add Tauri commands in `src-tauri/src/commands/`
- Implement business logic in appropriate modules
- Use `#[tauri::command]` macro for frontend-callable functions

## Building for Production

### Windows x64 Build

```bash
npm run tauri-build
```

Output: `src-tauri/target/release/Resh.exe` (portable executable)

### Build Configuration

- **Portable Mode** - Single `.exe` file, no installer required
- **No Admin Rights** - Runs from any location
- **AppData Storage** - All data in `%AppData%\Resh\`

### Local macOS Build (unsigned DMG)

Automatic GitHub Releases use unsigned / unnotarized DMGs (see [GitHub Releases](#github-releases-vx-tags) below). For a local unsigned DMG with the same cleanup rules as CI (`CI=true` skips Finder DMG layout AppleScript):

```bash
npm ci
cargo test --manifest-path src-tauri/Cargo.toml --locked
CI=true npm run build:macos -- --target aarch64-apple-darwin
# or Intel:
# CI=true npm run build:macos -- --target x86_64-apple-darwin
```

Typical output paths:

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Resh_<version>_aarch64.dmg
src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/Resh_<version>_x64.dmg
```

`npm run build:macos` forces DMG-only bundling, deletes leftover `.app` bundles, and cleans temporary `rw.*.dmg` files. Direct `npm run tauri-build` on macOS does **not** apply those release cleanup rules.

The Intel bundle retains the configured macOS 10.15 deployment target. Rust's Apple Silicon target has a minimum deployment target of macOS 11.0.

Optional signed/notarized local builds still exist (`npm run macos:release` / `macos:verify` and a manual `macos-ci` workflow dispatch). They require `APPLE_*` secrets and are **not** used by the automatic tag release path.

### GitHub Releases (`v*` tags)

Pushing a version tag is the **only automatic** GitHub Release entry point (`.github/workflows/release.yml`). The macOS CI workflow does **not** run on tags and does **not** create Releases.

#### Prerequisites before tagging

Keep these three version sources identical (semver without a leading `v`):

| File | Field |
| --- | --- |
| `package.json` | `"version"` |
| `src-tauri/Cargo.toml` | `package.version` |
| `src-tauri/tauri.conf.json` | `version` |

Local check (example for current project version):

```bash
npm run check:release-version -- v1.1.0
```

Tag format: `v` + semver (prerelease/build suffixes allowed, e.g. `v1.2.0-beta.1`). Mismatched or invalid tags fail the release workflow in `check-version`.

#### Publish

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

No `APPLE_*` Repository Secrets are required for automatic releases. macOS jobs run `CI=true npm run build:macos` (unsigned DMG). Windows jobs run `npm run tauri-build` and ship the portable EXE only (no MSI/NSIS installer).

#### Release assets

Each successful tag release publishes exactly **four** files:

| Asset | Description |
| --- | --- |
| `Resh-vX.Y.Z-windows-x86_64.exe` | Portable Windows x64 binary (**unsigned**, no installer) |
| `Resh-vX.Y.Z-macos-aarch64.dmg` | Apple Silicon DMG (**unsigned / unnotarized**) |
| `Resh-vX.Y.Z-macos-x86_64.dmg` | Intel DMG (**unsigned / unnotarized**) |
| `SHA256SUMS.txt` | SHA-256 checksums (GNU `sha256sum` format, sorted filenames) |

Example for tag `v1.1.0`:

```text
Resh-v1.1.0-windows-x86_64.exe
Resh-v1.1.0-macos-aarch64.dmg
Resh-v1.1.0-macos-x86_64.dmg
SHA256SUMS.txt
```

Verify downloads:

```bash
sha256sum -c SHA256SUMS.txt
```

#### macOS Gatekeeper note

Automatic release DMGs are **not** Apple-signed or notarized. On first open, users may need to bypass Gatekeeper (System Settings → Privacy & Security, or right-click the app → Open).

#### Failure and re-runs

- Re-run the failed GitHub Actions workflow for the same tag; publish is idempotent and replaces assets with `--clobber`.
- Re-runs do **not** rewrite existing release notes (first create uses download notes + auto-generated changelog).
- Tags with a pre-release segment (e.g. `v1.2.0-beta.1`) are marked as GitHub prereleases.
- Local static guards: `npm run check:release-version -- vX.Y.Z` and `npm run ci:pin-actions`.

## Usage

### Connecting to Servers

1. Click the **+** button in the tab bar
2. Select a configured server
3. Or use Quick Connect for one-time connections

### Managing Configurations

1. Open Settings (gear icon or menu)
2. Navigate through tabs:
   - **Servers** - Add/edit SSH server configurations
   - **Authentications** - Manage SSH keys and passwords
   - **Proxies** - Configure HTTP/SOCKS5 proxies
   - **General** - Theme, terminal, and WebDAV settings

### WebDAV Sync

1. Configure WebDAV in Settings > General
2. Use "Sync Now" for manual sync
3. Default auto-sync when application startup

## Features Roadmap

### Current (MVP)
- [x] Multi-tab SSH terminal interface
- [x] Server/authentication/proxy management
- [x] WebDAV configuration sync
- [x] Jumphost and proxy support
- [x] Port forwarding
- [x] Frameless window with custom controls

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

### Reporting Security Issues

Please report security vulnerabilities privately to the maintainers rather than using public issues.

## Acknowledgments

- Built with [Tauri](https://tauri.app/)
- Terminal emulation by [xterm.js](https://term.js.org/)
- SSH implementation via [russh](https://github.com/Eugeny/russh)
- Styling by [Tailwind CSS](https://tailwindcss.com/)

## Support

For issues and questions:
- GitHub Issues: [Report a bug or request a feature](https://github.com/fonlan/resh/issues)

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
