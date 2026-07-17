# Updater end-to-end checklist (two adjacent stable tags)

Use isolated temp installs only. Do not replace a developer’s daily Resh under `/Applications` until the final acceptance pass.

## Preconditions

- [ ] Two adjacent **stable** GitHub Releases exist (or local fixtures that match the four-asset contract).
- [ ] Assets pass `npm run check:updater-assets -- --tag <tag> --dir <dir>`.
- [ ] Older build installed; newer tag is what the client should discover.

## Happy path

- [ ] Auto-check (default on) or About → Check for updates finds the newer stable tag only (not prerelease).
- [ ] Title bar update entry appears; dialog shows version/size.
- [ ] Download with **direct**, then with **HTTP proxy**, then with **SOCKS5** (`GeneralSettings.update.proxyId`).
- [ ] Checksum failure is not left as ready (tamper fixture or wrong hash).
- [ ] Confirm install → drain waits for config sync / SFTP transfer if active.
- [ ] Dirty editor blocks restart; stopping recording / saving clears block.
- [ ] Replace succeeds; app relaunches with `--restore-update-session`.
- [ ] Tabs: SSH reconnect, Quick Connect structure, saved editor reopen, active tab + split restored.
- [ ] After ack: no leftover DMG mount, backup, install manifest, or restart snapshot.

## macOS Gatekeeper / quarantine

- [ ] Fake or real staged app had `com.apple.quarantine` before helper; after success recursive attribute is gone.
- [ ] Custom xattrs (if any) other than quarantine remain.
- [ ] Unnotarized app launches without “damaged / can’t verify developer” after in-app update.
- [ ] xattr permission failure / admin cancel / residual quarantine after clear → **rollback**, old app restored, new app **not** launched.

## Failure matrix

- [ ] App on read-only DMG / App Translocation rejected before replace.
- [ ] Install parent not writable (and elevation cancel) fails safely.
- [ ] Download interrupt / cancel leaves no ready package.
- [ ] Helper spawn failure: old process remains running.
- [ ] Alive-marker timeout: rollback to backup and relaunch previous version.

## Logs / artifacts

- [ ] Capture helper result file + app logs for failures; delete temp mounts and staging after the run.
