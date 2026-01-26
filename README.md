# Resh SSH Client

A modern, secure multi-tab SSH client built with Tauri 2 + React, designed for developers who need seamless SSH connection management with cross-machine configuration portability.

## Features

### Core Features
- **Multi-Tab Interface** - Manage multiple simultaneous SSH sessions in a clean tabbed interface with drag-to-reorder support
- **Comprehensive Configuration** - Full server, authentication, and proxy management with WebDAV sync
- **Secure Encryption** - AES-256-GCM encryption for all sensitive data with master password protection
- **Proxy & Jumphost Support** - Connect through HTTP/SOCKS5 proxies or SSH jumphosts
- **Port Forwarding** - Easy local-to-remote port forwarding configuration per connection
- **Frameless UI** - Modern frameless window with embedded controls and custom drag regions
- **Cross-Machine Sync** - WebDAV-based configuration sync with conflict resolution

### Advanced Features
- **SSH Key Management** - Store SSH key content (not paths) for true portability
- **Auto-Execute Commands** - Run commands automatically after connection
- **Environment Variables** - Set custom environment variables per connection
- **Connection Cloning** - Quickly duplicate existing SSH sessions
- **Keep-Alive** - Configurable keep-alive intervals to maintain connections

## Tech Stack

**Frontend:**
- React 18 with TypeScript
- xterm.js for terminal emulation
- Vite for build tooling

**Backend:**
- Rust with Tauri 2 framework
- russh for SSH protocol implementation
- AES-256-GCM for encryption (via aes-gcm crate)
- PBKDF2 for key derivation

**Storage:**
- Encrypted JSON configuration files
- WebDAV protocol for sync

## Installation

### Prerequisites
- Node.js 18+ and npm
- Rust 1.60+ (for building from source)
- Windows 10+ (current version targets Windows x64)

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

## Project Structure

```
Resh/
├── src/                          # React frontend
│   ├── components/
│   │   ├── MainWindow.tsx        # Main application window
│   │   ├── TerminalTab.tsx       # Individual terminal tab
│   │   ├── WindowControls.tsx    # Custom window controls
│   │   └── settings/             # Settings modal components
│   │       ├── SettingsModal.tsx
│   │       ├── ServerTab.tsx
│   │       ├── AuthTab.tsx
│   │       ├── ProxyTab.tsx
│   │       └── GeneralTab.tsx
│   ├── App.tsx
│   └── main.tsx
├── src-tauri/                    # Rust backend
│   ├── src/
│   │   ├── commands/             # Tauri command handlers
│   │   │   ├── connection.rs
│   │   │   ├── config.rs
│   │   │   └── sync.rs
│   │   ├── ssh_manager/          # SSH connection management
│   │   │   ├── connection.rs
│   │   │   ├── handler.rs
│   │   │   └── ssh.rs
│   │   ├── config/               # Configuration handling
│   │   │   ├── encryption.rs
│   │   │   ├── loader.rs
│   │   │   └── types.rs
│   │   ├── webdav/               # WebDAV sync implementation
│   │   │   ├── client.rs
│   │   │   └── conflict.rs
│   │   └── main.rs
│   └── Cargo.toml
├── package.json
└── README.md
```

## Configuration

### Configuration Files

Resh stores configuration in `%AppData%\Resh\`:

```
%AppData%\Resh\
├── sync.json         # Synced config (servers, auth, proxies)
├── local.json        # Local-only config (overrides)
├── .sync_metadata    # Sync timestamps and conflict data
└── logs\             # Connection logs
```

### Configuration Schema

Both `sync.json` and `local.json` support:

- **Servers** - SSH server configurations with connection details
- **Authentications** - SSH keys and password credentials
- **Proxies** - HTTP/SOCKS5 proxy configurations
- **General Settings** - Theme, language, terminal preferences, WebDAV settings

### Sync Strategy

- **Synced Items** - Stored in `sync.json`, synced via WebDAV across machines
- **Local-Only Items** - Stored in `local.json`, machine-specific overrides
- **Merge Logic** - Items in `local.json` override matching UUIDs in `sync.json`

### Security

- **Master Password** - Required on first launch, encrypts all configuration files
- **AES-256-GCM** - Authenticated encryption for configuration files
- **PBKDF2** - Key derivation with salt for master password
- **No Password Recovery** - By design for maximum security

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
```

### Development Workflow

1. Run `npm run tauri-dev` for hot-reload development
2. Frontend changes auto-reload via Vite
3. Rust changes require restart (Cargo rebuilds automatically)

### Adding New Features

**Frontend:**
- Add components in `src/components/`
- Use TypeScript for type safety
- Follow React hooks pattern

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

## Usage

### First Launch

1. Launch Resh.exe
2. Set a master password (required)
3. Add your first SSH server in Settings

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
   - **Advanced** - Security and logging options

### WebDAV Sync

1. Configure WebDAV in Settings > General
2. Click "Test Connection" to verify
3. Use "Sync Now" for manual sync
4. Enable auto-sync for periodic synchronization

## Features Roadmap

### Current (MVP)
- [x] Multi-tab SSH terminal interface
- [x] Server/authentication/proxy management
- [x] Master password encryption
- [x] WebDAV configuration sync
- [x] Jumphost and proxy support
- [x] Port forwarding
- [x] Frameless window with custom controls

### Future Phases
- [ ] AI Assistant Integration (per-tab chat context)
- [ ] Multi-platform support (macOS, Linux)
- [ ] Session recording and playback
- [ ] Custom themes and color schemes
- [ ] SFTP file browser integration

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.

### Guidelines

1. Follow existing code style
2. Add tests for new features
3. Update documentation as needed
4. Ensure all tests pass before submitting

## Security Considerations

- **Master password never leaves local machine**
- **WebDAV credentials stored separately**
- **No telemetry or analytics**
- **Full encryption for sensitive data**
- **Open source - audit the code yourself**

### Reporting Security Issues

Please report security vulnerabilities privately to the maintainers rather than using public issues.

## License

[Add your license here]

## Acknowledgments

- Built with [Tauri](https://tauri.app/)
- Terminal emulation by [xterm.js](https://xtermjs.org/)
- SSH implementation via [russh](https://github.com/warp-tech/russh)

## Support

For issues and questions:
- GitHub Issues: [Report a bug or request a feature](https://github.com/yourusername/resh/issues)
- Documentation: See `docs/` folder for detailed guides

---

**Note:** Resh is designed for developers who need secure, portable SSH connection management. If you forget your master password, your configuration data cannot be recovered - please keep it safe!
