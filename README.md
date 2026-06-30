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
| Windows 10+ x64 | Supported | Portable `.exe` |
| macOS 10.15+ Intel / macOS 11+ Apple Silicon | In progress | Unsigned Apple Silicon `.app`/`.dmg` builds work; Intel qualification, signing, notarization, and CI remain |

## Installation

### Prerequisites
- Node.js 22.12.x and npm 10.9.x
- Rust 1.88+ (for building from source)
- Platform build dependencies:
  - Windows 10+ with Microsoft C++ Build Tools and WebView2
  - macOS 10.15+ with Xcode Command Line Tools for experimental source builds

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

### Deployment

1. Build the application
2. Distribute the `Resh.exe` file
3. Users run the exe - no installation needed

### macOS Build Status

The codebase has a cross-platform Tauri/Rust foundation, but macOS production
bundles are not yet supported releases. Apple Silicon development, `.app`, and
`.dmg` builds have been verified, but Intel qualification, automated CI, code
signing, and notarization remain. Do not publish locally produced `.app` or
`.dmg` files as official releases until release qualification is complete.

For a reproducible Apple Silicon build, use the versions declared in `.nvmrc`
and `rust-toolchain.toml`:

```bash
npm ci
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri-build -- --target aarch64-apple-darwin
```

Unsigned development bundles are written to:

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Resh.app
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Resh_<version>_aarch64.dmg
```

The Intel bundle retains the configured macOS 10.15 deployment target. Rust's
Apple Silicon target has a minimum deployment target of macOS 11.0, the first
macOS release available for Apple Silicon hardware.

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
