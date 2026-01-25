# Resh SSH Client - Design Document

**Date:** 2026-01-25
**Status:** Approved
**MVP Scope:** SSH + Full Config Management (AI integration in later phase)

---

## 1. Overview

**Resh** is a modern multi-tab SSH client built with Tauri + React, designed for developers who need:
- Multiple simultaneous SSH sessions in a clean tabbed interface
- Comprehensive server/auth/proxy configuration with WebDAV sync
- Frameless, draggable UI with embedded window controls
- Cross-machine config portability with security

### Tech Stack
- **Frontend:** React + xterm.js (terminal emulation)
- **Backend:** Rust (Tauri) + `russh` (SSH library)
- **Config Storage:** Encrypted JSON files (AES-256-GCM)
- **Sync:** WebDAV protocol

---

## 2. Architecture

### High-Level Separation

**Frontend (React):**
- Main SSH terminal interface with tabbed navigation
- xterm.js instances (one per SSH connection)
- Settings management UI (tabbed modal)
- Connection list and quick connect

**Backend (Rust/Tauri):**
- SSH connection management (`russh` library)
- PTY (pseudo-terminal) handling
- Port forwarding and tunnel management
- Configuration encryption/decryption (master password)
- WebDAV sync orchestration

**Data Flow:**
```
User Input → React UI → Tauri Command → Rust Backend → SSH/Config → Response → React → xterm.js
```

---

## 3. Configuration System

### File Structure
```
%AppData%\Resh\
├── sync.json        (WebDAV synced, encrypted)
├── local.json       (local only, encrypted)
├── .sync_metadata   (last sync timestamp, conflict data)
└── logs\            (connection logs)
```

### Shared Schema (sync.json & local.json)
```json
{
  "version": "1.0",
  "servers": [
    {
      "id": "uuid",
      "name": "Production Server",
      "host": "example.com",
      "port": 22,
      "username": "admin",
      "authId": "uuid-ref",
      "proxyId": "uuid-ref-or-null",
      "jumphostId": "uuid-ref-or-null",
      "portForwards": [{"local": 3000, "remote": 5432}],
      "keepAlive": 60,
      "autoExecCommands": ["source ~/.profile"],
      "envVars": {"TERM": "xterm-256color"}
    }
  ],
  "authentications": [
    {
      "id": "uuid",
      "name": "My SSH Key",
      "type": "key",
      "keyContent": "-----BEGIN RSA PRIVATE KEY-----...",
      "passphrase": "encrypted-if-set"
    },
    {
      "id": "uuid",
      "name": "Password Auth",
      "type": "password",
      "username": "admin",
      "password": "encrypted"
    }
  ],
  "proxies": [
    {
      "id": "uuid",
      "name": "Office HTTP Proxy",
      "type": "http",
      "host": "proxy.corp.com",
      "port": 8080,
      "username": "optional",
      "password": "encrypted-if-set"
    },
    {
      "id": "uuid",
      "name": "SOCKS Proxy",
      "type": "socks5",
      "host": "localhost",
      "port": 1080
    }
  ],
  "general": {
    "theme": "dark",
    "language": "zh-CN",
    "terminal": {
      "fontFamily": "Consolas",
      "fontSize": 14,
      "cursorStyle": "block",
      "scrollback": 5000
    },
    "webdav": {
      "url": "https://webdav.example.com/resh/",
      "username": "user",
      "password": "encrypted"
    }
  }
}
```

### Config Merge Logic
- Load both `sync.json` and `local.json`
- Merge by UUID: items in `local.json` override items in `sync.json` with same ID
- Items only in one file: included as-is
- Result: unified config for runtime use

### Sync Control Granularity

**Server-level sync:**
- Main toggle: "Sync this server config" (entire server in sync.json or local.json)
- **Independent proxy/jumphost sync:** "Sync proxy/jumphost setting"
  - If ON: proxy/jumphost reference stored in sync.json
  - If OFF: proxy/jumphost reference stored in local.json (overrides synced value)
  - Allows same server config to use different proxies on different machines

**Authentication sync:**
- For SSH Key: stores **certificate content** (not path) in sync.json when synced
- For Password: stores encrypted password
- Ensures portability across machines

**Proxy sync:**
- Toggle per proxy item

**General settings:**
- WebDAV credentials: **always local** (in local.json)
- Theme, language, terminal prefs: **always local**

---

## 4. UI Layout & Window Design

### Frameless Window
```
┌────────────────────────────────────────────────────────────┐
│ [Tab1] [Tab2] [+]                        □ ─ ✕ │ ← Controls
├────────────────────────────────────────────────────────────┤
│                                          │                 │
│                                          │                 │
│        xterm.js Terminal                 │   (AI sidebar   │
│        (SSH session output)              │    reserved     │
│                                          │    for future)  │
│                                          │                 │
└────────────────────────────────────────────────────────────┘
```

**Tab Bar Features:**
- Each tab: `[Server Name] [Clone] [✕]`
- Right-click: context menu with "Clone Connection", "Close"
- Drag to reorder tabs
- `+` button: opens quick connect dialog
- Entire tab bar (except buttons): `data-tauri-drag-region` for window dragging
- Window controls (min/max/close) embedded in tab bar right corner

**Sizing:**
- Tab bar: ~40px height
- Terminal: remaining vertical space
- AI sidebar (future): ~300px width, collapsible

---

## 5. Settings UI (Modal Window)

Tabbed interface with 5 sections:

### Tab 1: Servers
**List View:**
- Columns: Server name, host:port, auth method, proxy/jumphost indicator
- Actions: Add, Edit, Delete, Clone

**Edit Dialog:**
- **Basic Info:** Name, Host, Port, Username
- **Authentication:** Dropdown (from authentications list)
- **Proxy/Jumphost Section:**
  - Selector: None / Select Proxy / Select Jumphost
  - Mutual exclusion: cannot set both proxy and jumphost
  - **Independent toggle:** "Sync proxy/jumphost setting"
- **Port Forwards:** Add/edit list (local port → remote port)
- **Keep-alive interval** (seconds)
- **Auto-exec commands** (multi-line text, runs after login)
- **Environment variables** (key-value pairs)
- **Main Sync Toggle:** "Sync this server config via WebDAV"

### Tab 2: Authentications
**List View:**
- Columns: Name, Type (SSH Key / Password), Sync status

**Add/Edit Dialog:**
- Name
- Type selector (SSH Key / Password)
- **If SSH Key:**
  - File path selector (loads content into memory)
  - Passphrase (optional, encrypted)
  - Content stored in JSON, not path (for sync portability)
- **If Password:**
  - Username
  - Password (encrypted)
- **Sync toggle**

### Tab 3: Proxies
**List View:**
- Columns: Name, Type (HTTP / SOCKS5), Host:Port

**Add/Edit Dialog:**
- Name
- Type (HTTP / SOCKS5)
- Host, Port
- Optional auth (username/password for HTTP proxies)
- **Sync toggle**

### Tab 4: General
**Appearance:**
- Theme: Light / Dark / System
- Language: 中文 / English

**Terminal:**
- Font family (monospace dropdown)
- Font size (10-24px slider)
- Cursor style (block / underline / bar)
- Scrollback lines (1000-10000)

**Behavior:**
- Confirm before closing tab (checkbox)
- Confirm before exiting app (checkbox)

**WebDAV Sync:**
- WebDAV URL
- Username/Password
- "Test Connection" button
- Last sync timestamp display
- Manual "Sync Now" button
- Auto-sync interval (disabled / 5min / 15min / 1hour)

### Tab 5: Advanced
**Security:**
- "Change Master Password" button
- Auto-lock after N minutes of inactivity

**Logs:**
- Open logs folder
- Export logs for debugging

---

## 6. SSH Connection Flow

### Connection Lifecycle

**1. User initiates connection:**
- Clicks server in list OR creates new tab with quick connect
- React calls: `connect_to_server(server_id)`

**2. Backend (Rust) processing:**
- Load server config from merged sync.json + local.json
- Resolve authentication (fetch credentials by authId)
- Resolve proxy/jumphost:
  - If `proxyId` set: Create HTTP/SOCKS tunnel
  - If `jumphostId` set: Open SSH tunnel through jumphost server (option 2: direct tunnel)
  - Otherwise: direct connection
- Establish SSH connection using `russh` library
- Create PTY for interactive terminal
- Set up port forwards (if configured)
- Execute auto-exec commands
- Generate unique `session_id` (UUID)
- Return `session_id` to React

**3. Terminal I/O (bidirectional):**
- **User input:** React captures keystrokes → `send_command(session_id, input)` → Rust sends to PTY
- **Output:** Rust reads PTY → emits `terminal-output(session_id, data)` event → React → xterm.js renders

**4. Connection close:**
- User clicks ✕ on tab → `close_session(session_id)`
- Rust closes SSH connection, PTY, and all port forwards
- Cleanup session state

### Tauri Commands (Rust ↔ React)

**Commands (React → Rust):**
```rust
connect_to_server(server_id) → session_id
send_command(session_id, input) → ()
close_session(session_id) → ()
clone_session(session_id) → new_session_id
get_merged_config() → Config
save_config(sync_part, local_part) → Result
sync_webdav() → Result
```

**Events (Rust → React):**
```rust
terminal-output(session_id, data)
connection-closed(session_id, reason)
connection-error(session_id, error)
sync-status(status, progress)
```

### Jumphost/Proxy Implementation
- **Tunnel-based approach (option 2):**
  - Open tunnel to jumphost/proxy first
  - Then connect directly through that tunnel to target server
  - User types commands locally, executed on target server through tunnel
  - Cleaner for terminal interaction (no nested SSH sessions)

### Port Forwarding
- **Per-connection lifetime (option 3):**
  - Each SSH connection manages its own port forwards
  - Tauri binds local port, forwards traffic through SSH tunnel
  - When tab closes, forwards automatically close
  - Simple MVP behavior

---

## 7. Security & Encryption

### Master Password System

**First Launch:**
1. App detects no master password set
2. Modal: "Set Master Password"
3. User enters password (strength indicator shown)
4. Confirmation field
5. Hashed key stored securely (Windows DPAPI for key storage, not for file encryption)

**Subsequent Launches:**
1. Modal: "Enter Master Password to unlock Resh"
2. User enters password
3. App decrypts both sync.json and local.json
4. If decryption fails: error shown, app does not proceed

**Encryption Details:**
- **Algorithm:** AES-256-GCM (authenticated encryption)
- **Key derivation:** PBKDF2 with salt (stored in file header)
- Each JSON file encrypted separately (same master password, different derived keys)
- Sensitive fields (passwords, key passphrases, certificate content): encrypted within JSON

**Password Change:**
- Settings → Advanced → "Change Master Password"
- Old password verification required
- Re-encrypt both sync.json and local.json with new key
- User must manually sync to update WebDAV

**Security Notes:**
- Master password never leaves local machine
- No password recovery (by design - maximum security)
- WebDAV credentials separate from master password
- If master password forgotten: data unrecoverable

---

## 8. WebDAV Sync

### Sync Triggers
- **Manual:** User clicks "Sync Now" in settings
- **Auto-sync:** Configurable interval (5min / 15min / 1hour)
- **On app start:** Optional
- **On config change:** Optional immediate push

### Sync Flow

**Download (Pull):**
1. Fetch `sync.json` from WebDAV (`/resh/sync.json`)
2. Decrypt with master password
3. Compare with local `sync.json` by modification timestamp
4. If conflict: show conflict resolution dialog
5. User chooses: "Use remote" / "Use local" / "Manual merge"
6. Update `.sync_metadata` with last sync timestamp

**Upload (Push):**
1. Encrypt `sync.json` with master password
2. Upload to WebDAV path
3. Update `.sync_metadata`

**Conflict Resolution:**
- Detect by comparing modification timestamps in `.sync_metadata`
- Show side-by-side diff UI
- User selects which version to keep per item (server/auth/proxy)

**WebDAV File Structure:**
```
/resh/
└── sync.json (encrypted blob)
```

Simple one-file sync for MVP. No partial sync or delta updates.

---

## 9. Error Handling

### Connection Errors
- **Auth failure:** Modal with error details, "Retry" and "Edit Config" buttons
- **Host unreachable:** Clear message, suggest checking firewall/proxy
- **SSH handshake timeout:** Configurable timeout (default 10s in settings)
- **Proxy/jumphost failure:** Show which hop failed in chain

### Configuration Errors
- **Invalid credentials:** Validation on save, highlight problematic field
- **Circular jumphost reference:** Detect and prevent (Server A → B → A)
- **Port conflict:** Detect local port already in use, suggest alternatives
- **WebDAV sync failure:** Non-blocking notification, retry on next interval

### UI Feedback
- **Connection in progress:** Loading spinner in tab
- **Sync in progress:** Indicator in settings WebDAV section
- **Unsaved changes:** Warn before closing settings modal
- **Terminal disconnect:** Banner at top with "Reconnect" button

### Logging
- Connection logs saved to `%AppData%\Resh\logs\{date}.log`
- Useful for debugging sync/connection issues
- User can open logs folder or export from settings

---

## 10. Component Structure

### React Components
```
src/
├── components/
│   ├── MainWindow.tsx          (root layout)
│   ├── TabBar.tsx              (tab management, window controls)
│   ├── TerminalTab.tsx         (xterm.js container)
│   ├── QuickConnect.tsx        (server picker modal)
│   ├── settings/
│   │   ├── SettingsModal.tsx   (main settings window)
│   │   ├── ServerTab.tsx
│   │   ├── AuthTab.tsx
│   │   ├── ProxyTab.tsx
│   │   ├── GeneralTab.tsx
│   │   └── AdvancedTab.tsx
│   └── ConflictResolver.tsx    (WebDAV conflict UI)
├── hooks/
│   ├── useTerminal.ts          (xterm.js lifecycle)
│   ├── useConfig.ts            (config loading/saving)
│   └── useSSHConnection.ts     (Tauri command wrappers)
└── types/
    └── config.ts               (TypeScript types for config schema)
```

### Rust/Tauri Backend Structure
```
src-tauri/src/
├── main.rs                     (Tauri app entry)
├── commands/                   (Tauri command handlers)
│   ├── connection.rs
│   ├── config.rs
│   └── sync.rs
├── ssh_manager/                (SSH core)
│   ├── connection.rs           (russh connection handling)
│   ├── pty.rs                  (PTY creation/management)
│   └── port_forward.rs
├── config/
│   ├── loader.rs               (load/merge sync.json + local.json)
│   ├── encryption.rs           (AES-256-GCM encrypt/decrypt)
│   └── types.rs                (Rust types for config schema)
└── webdav/
    ├── client.rs               (WebDAV upload/download)
    └── conflict.rs             (conflict detection)
```

---

## 11. Testing Strategy (MVP)

### Unit Tests
- Config merge logic (overlap/override scenarios)
- Encryption/decryption (AES-256-GCM)
- WebDAV conflict detection
- Circular jumphost reference detection

### Integration Tests
- SSH connection with mock server
- Port forwarding functionality
- Proxy/jumphost tunneling

### Manual E2E Testing
- Full connection flow (direct, proxy, jumphost)
- Settings CRUD operations
- WebDAV sync (happy path + conflicts)
- Cross-machine portability test

---

## 12. Build & Deployment

**Build Configuration:**
- Tauri in portable mode (no installer)
- Output: Single `.exe` file (Windows x64)
- All data in `%AppData%\Resh\` (no registry, no Program Files)
- Uninstall: Delete exe + AppData folder

**Development:**
```bash
npm run tauri dev
```

**Production Build:**
```bash
npm run tauri build
```
- Configure `tauri.conf.json` to skip MSI/NSIS generation
- Output: `src-tauri/target/release/Resh.exe`

---

## 13. Future Phase: AI Integration

**Designed-in architecture for later:**
- AI sidebar (right panel, ~300px, collapsible)
- Each SSH tab has isolated chat context
- Chat history: `%AppData%\Resh\chat_history\{session_id}.json` (local only, never synced)
- AI channel/model config in sync.json (if user enables sync per channel)

**Current design considerations:**
- Layout already reserves space for sidebar toggle
- Terminal resize handler will accommodate sidebar
- Session state already isolated (easy to attach per-tab chat)
- No major refactoring needed when adding AI

**AI Features (future):**
- Add OpenAI-compatible API channels
- Add GitHub Copilot login channel
- Auto-fetch available models (`/v1/models`)
- Send system info + SSH history + chat history to AI
- Parse AI responses for command-line snippets (copy/paste to terminal)

---

## 14. Success Criteria (MVP)

The MVP is complete when:
- [ ] User can add/edit/delete servers, auths, proxies in settings
- [ ] User can connect to SSH server in new tab
- [ ] Multiple tabs work simultaneously with independent sessions
- [ ] Tab operations: close, clone, reorder
- [ ] Frameless window with drag-to-move and window controls
- [ ] Jumphost and proxy connections work
- [ ] Port forwarding works
- [ ] Master password encrypts/decrypts configs
- [ ] WebDAV sync uploads/downloads sync.json
- [ ] Config merge logic (sync.json + local.json) works correctly
- [ ] Theme, language, terminal prefs persist
- [ ] Logs saved for debugging

---

## Appendix: Design Decisions Log

| Decision | Rationale |
|----------|-----------|
| xterm.js for terminal | Battle-tested, handles ANSI/colors/input well |
| Rust SSH library (`russh`) | Cross-platform, good control over connection lifecycle |
| Tunnel-based jumphost | Cleaner UX (user types locally, executes on target) |
| Per-connection port forwards | Simpler MVP (auto-cleanup on tab close) |
| Master password (not DPAPI for files) | Cross-machine portability requirement |
| SSH key content sync (not path) | Ensures auth works on any machine |
| Independent proxy/jumphost sync | Different machines have different network environments |
| Portable exe (no installer) | Simpler distribution, no admin rights needed |
| WebDAV one-file sync | Simplest MVP, easy to implement conflict resolution |
