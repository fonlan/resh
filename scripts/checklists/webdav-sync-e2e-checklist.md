# WebDAV config sync end-to-end checklist

Use two isolated Resh data directories and a disposable WebDAV account or fixture. Do not use production credentials in automated tests or shared logs.

## Preconditions

- [ ] Device A and Device B run the current Resh build and point to the same WebDAV URL and account.
- [ ] Each device starts with a successful sync, so both have an account-local `sync-state.json` baseline.
- [ ] Record the remote `sync.json` ETag before each concurrency scenario when the server exposes it.
- [ ] Keep a separate WebDAV URL or username available to verify account-scoped baseline isolation.

## Normal synchronization paths

- [ ] Restart Device A with WebDAV auto-sync enabled. Startup sync completes without replacing unrelated local-only settings.
- [ ] Save a synced configuration item on Device A. The background sync updates Device B after its next sync.
- [ ] Use **Sync Now** on Device B. An applied result updates the configuration only after the conditional remote write succeeds.
- [ ] During a background sync on Device A, save another local edit. The stale sync result does not overwrite the newer saved edit.
- [ ] Begin update restart draining while a sync is active. The `webdavSync` operation is reported until the sync completes or fails, then draining can continue.

## Offline edits and conflict resolution

- [ ] Disconnect both devices from WebDAV after a common successful baseline.
- [ ] Modify the same server differently on Device A and Device B. Sync Device A, then Device B. Device B receives a conflict dialog; it does not overwrite Device A.
- [ ] Resolve one conflict with **Keep local** and another with **Use remote**. Confirm only the selected complete entities win; no field-level hybrid is produced.
- [ ] Repeat with `additionalPrompt` changed on both devices. It follows the same conflict dialog and manual choice rules.
- [ ] Delete an unchanged synced entity on Device A while Device B leaves it unchanged. The deletion converges and a typed tombstone is retained remotely.
- [ ] Delete an entity on Device A and modify it on Device B. Confirm a delete-vs-modify conflict is shown; choosing remote retains the tombstone, and choosing local explicitly restores the entity.
- [ ] Close and reopen the application while conflicts are pending. A new sync returns a fresh, actionable attempt; stale resolution tokens are rejected rather than applied.

## Remote concurrency and migration

- [ ] Force a WebDAV `412 Precondition Failed` or `409 Conflict` on the first conditional PUT. Resh re-downloads and recomputes once using the fresh ETag.
- [ ] Force a second `412` or `409`. Resh stops with a retryable remote-concurrency result and never falls back to an unconditional PUT.
- [ ] Use a legacy `sync.json` without `syncSchema` and with an unambiguous legacy `removedIds` entry. The first current-client sync migrates it to typed tombstones and writes schema 2.
- [ ] Use a legacy `removedIds` value shared by multiple entity types. Sync stops with a format error; it must not guess which type to delete.
- [ ] After a successful schema-2 sync, simulate an old client replacing remote `sync.json` without `syncSchema`. Current Resh blocks synchronization before PUT and instructs that every syncing device must be upgraded. Do not treat mixed old/current clients as safe.
- [ ] Point a device at a new WebDAV URL or change the username. Verify it uses a distinct baseline and does not reuse the prior account's ETag, hashes, or tombstones.

## Evidence

- [ ] Record outcome status, relevant safe error text, and server status codes/ETags for failed cases.
- [ ] Verify conflict summaries and logs do not expose passwords, SSH private keys, passphrases, or API keys.
- [ ] Remove disposable remote files and local test data after completion.
