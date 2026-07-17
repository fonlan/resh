# In-app updater ↔ GitHub Release asset contract

This document is the **shared long-term API** between:

1. Automatic tag releases (`.github/workflows/release.yml` / `plan-github-tag-release-20260717` assets)
2. The Resh in-app updater (`src-tauri/src/updater/`)

No Tauri updater keys, no Apple Developer ID signing secrets, and no extra update-only assets are required.

## Fixed assets (exactly four)

| Asset | Role |
| --- | --- |
| `Resh-<tag>-windows-x86_64.exe` | Portable Windows x64 binary (unsigned, no installer) |
| `Resh-<tag>-macos-aarch64.dmg` | Apple Silicon DMG (**unsigned / unnotarized**) |
| `Resh-<tag>-macos-x86_64.dmg` | Intel DMG (**unsigned / unnotarized**) |
| `SHA256SUMS.txt` | GNU `sha256sum` format, **lexicographic** filenames |

Example for tag `v1.2.3`:

```text
Resh-v1.2.3-windows-x86_64.exe
Resh-v1.2.3-macos-aarch64.dmg
Resh-v1.2.3-macos-x86_64.dmg
SHA256SUMS.txt
```

Renaming any install asset or changing the checksums format will break the client. Validate locally or in CI:

```bash
npm run check:updater-assets -- --tag v1.2.3 --dir ./release-assets
# built-in fixtures:
node scripts/check-updater-release-assets.mjs --self-test
```

The release publish job runs this check after generating `SHA256SUMS.txt`.

## Client check / download behavior

| Behavior | Detail |
| --- | --- |
| Channel | Latest **stable** GitHub Release only (`fonlan/resh`); draft and prerelease ignored |
| Auto check | Default **on** (`GeneralSettings.update.autoCheck`); local-only, not WebDAV-synced |
| Schedule | ~8s after config load, then every **6 hours**; visibility/resume may backfill one check |
| Manual check | Always available (About / update UI); bypasses the auto-check switch |
| Proxy | `GeneralSettings.update.proxyId` (HTTP or SOCKS5); invalid proxy **fails** (no silent direct fallback) |
| Download trust | Opaque `updateId` from check only; stream to `.part`, fsync, rename to ready; **must** verify `SHA256SUMS.txt` and optional GitHub asset digest |
| Size ceiling | Install package ≤ 512 MiB; checksums file ≤ 1 MiB |
| Retries | GitHub API uses ETag / retry / rate-limit handling; download errors are surfaced for user retry |

## Platform install constraints

### Windows

- Portable EXE must live in a **writable** directory (same folder as the running EXE).
- Helper: hidden PowerShell (`CREATE_NO_WINDOW`, `-NoProfile`, `-NonInteractive`, `-WindowStyle Hidden`).
- Flow: wait for old PID → rename current to backup → move staged into place → launch with `--restore-update-session` → wait for alive marker → else rollback.

### macOS

- Prefer running from **Applications** (or another writable install parent). App Translocation / read-only DMG mounts are rejected for in-place update.
- DMG is still **not** Developer ID signed and **not** notarized.
- Helper: `/bin/sh -s` with a static script on stdin (no user-writable privileged helper file).
- After SHA-256 + bundle id (`com.fonlan.resh`) + version + arch checks, the helper **recursively removes only** `com.apple.quarantine` via `/usr/bin/xattr -dr com.apple.quarantine` and **rechecks**; failure rolls back and **does not** launch the new app.
- Admin authorization may appear for `/Applications` when the user cannot write the install parent.
- **Not done:** `spctl --master-disable`, global Gatekeeper changes, `xattr -c` / `xattr -cr`, stripping non-quarantine xattrs, or trusting arbitrary external paths.

### First manual install vs in-app update

| Path | Gatekeeper / quarantine |
| --- | --- |
| First open of a downloaded DMG | User may need to allow an unsigned app (System Settings → Privacy & Security, or right-click → Open) |
| In-app update | After release checksum + bundle validation, helper clears quarantine **only on the just-validated new `Resh.app`** so launch is not blocked by “damaged / unidentified developer” for that trusted package |

This is **not** Apple signing or notarization and does **not** change system Gatekeeper policy.

## Safe restart & session restore

Before replace, Resh enters a write-operation drain (`configWrite`, `webdavSync`, `sftpTransfer`, `sftpEditUpload`). Dirty editors, saving documents, and active recordings block restart with explicit UI (continue wait / cancel only—no force kill). A one-time session snapshot restores terminals, Quick Connect structure (no secrets), saved editors, active tab, and split layout after launch.

## Helper / regression tests (isolated)

```bash
npm run test:updater-helpers
# or individually:
node scripts/test-windows-update-helper.mjs
node scripts/test-macos-update-helper.mjs
cargo test --package resh --lib -- updater::
```

These tests use temporary directories and fake bundles only. They must not replace a developer’s real install under `/Applications` or a production Windows folder.

## Automated CI guards

| Check | Where |
| --- | --- |
| `npm run check:updater-assets -- --self-test` | macOS CI + local; fixtures for names / uniqueness / SHA256SUMS |
| `npm run check:updater-assets -- --tag <tag> --dir <dir>` | release publish job after generating `SHA256SUMS.txt` |
| `npm run test:updater-helpers` | macOS CI + Windows release build job + local; static contracts and isolated temp swap/quarantine/DMG paths (Windows full helper body only on `windows-latest`) |
| `cargo test --package resh --lib -- updater::` | unit tests including download stream / hash / size / redirect host policy |

## Manual E2E checklist (two adjacent tags)

Full production-like pass is **required before treating a first public dual-platform updater ship as done**, but is intentionally outside PR CI (needs real adjacent tags and human Gatekeeper/admin prompts). Track completion with:

```bash
# open and complete:
# scripts/checklists/updater-e2e-checklist.md
```

This document lives at `scripts/checklists/updater-release-contract.md` (tracked in git; the repo ignores a top-level `docs/` directory).

Minimum scope:

1. Publish or download two adjacent stable tags (e.g. `vX.Y.Z` → `vX.Y.Z+1`).
2. Install the older build; enable auto-check or use About → Check for updates.
3. Cover HTTP and SOCKS5 update proxies.
4. Download → checksum → confirm → wait for sync/transfer drain → install → relaunch.
5. Confirm session restore: SSH reconnect, Quick Connect, saved editor reopen; dirty editor must block restart.
6. Failure paths: bad checksum, interrupted download, helper spawn failure, alive-marker timeout (rollback), non-writable install dir, translocation/read-only volume on macOS.
7. After success: no leftover DMG mount, backup artifact (after ack), or restart manifest; macOS app opens without “damaged” prompt when quarantine clear succeeded.
