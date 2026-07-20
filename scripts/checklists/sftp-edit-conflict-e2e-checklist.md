# SFTP remote-edit conflict end-to-end checklist

Use two independent SSH/SFTP sessions to the same disposable text file. One session represents the in-app editor or the local external editor; the other represents an external writer. Do not run these cases against production files.

## Preconditions

- [ ] Both Resh sessions can read and write the same remote text file.
- [ ] Record the initial size and modification time reported by SFTP.
- [ ] Use a small UTF-8 text file for the post-write hash-verification cases.
- [ ] Open the file once with the built-in editor and once through **Edit locally** in separate runs, so each flow has its own baseline.

## Built-in editor: fast path and conflict handling

- [ ] Modify and save through the built-in editor without changing the remote file elsewhere. Confirm the save succeeds, the editor becomes clean, and the normal save path issues only the metadata check before writing; it must not fetch a pre-save remote content snapshot.
- [ ] Change the file through Session B so its size or mtime changes. Save the dirty built-in editor in Session A. Confirm Resh leaves the local editor dirty, does not truncate the remote file, and shows the conflict dialog with the current remote content when it can be decoded.
- [ ] Select **View diff** and confirm local unsaved content and remote content are shown on the correct sides.
- [ ] Select **Adopt remote**. Confirm editor content and revision become the remote version and a subsequent save is not treated as a stale conflict.
- [ ] Recreate the conflict, select **Overwrite remote**, then change the remote file again from Session B before accepting overwrite. Confirm Resh refreshes the conflict instead of overwriting Session B's second change.
- [ ] Delete the remote file from Session B. Confirm a normal save returns a deleted conflict and does not recreate it until the user explicitly chooses an allowed overwrite/recreate flow.
- [ ] With a clean editor tab active, change the remote file from Session B. Confirm the throttled activation check refreshes the clean document. Repeat while the editor is dirty; confirm it only marks a remote change and never replaces local edits.

## External editor watcher: pause, coalesce, and resume

- [ ] Open the same file with **Edit locally**, edit and save locally with no remote change. Confirm watcher upload completes without a pre-upload remote body download.
- [ ] Change the remote file through Session B, then save locally. Confirm watcher emits one `sftp-edit-conflict`, pauses automatic upload, and does not truncate the remote file.
- [ ] Save the local file repeatedly while the conflict dialog remains open. Confirm the watcher remains paused, retains one pending local version, and does not emit duplicate conflict dialogs or concurrent uploads.
- [ ] Choose **Adopt remote**. Confirm the local file is replaced atomically, watcher feedback from that replacement is suppressed, and the next genuine local edit uploads normally.
- [ ] Recreate a metadata conflict, then select **Overwrite remote** or **Recreate remote** as appropriate. Change the remote file a second time just before resolution. Confirm the operation refreshes the conflict instead of writing over the second remote change.
- [ ] Disconnect the watched SSH session while a conflict is pending. Confirm its watcher, pending conflict UI, and temporary local directory are cleaned up without affecting another session.

## Update-restart interaction

- [ ] Trigger a watcher conflict, leave the decision dialog open, then start an update safe-restart drain. Confirm `sftpEditUpload` is not held merely while waiting for the user's decision; the drain can continue once actual check/upload work has completed.
- [ ] While an upload is actively checking or writing, start drain. Confirm the operation is reported until it reaches a terminal result, then its permit is released even if that result is a conflict.

## Known fast-mode boundary

- [ ] Record that the default metadata gate intentionally cannot guarantee detection when an external writer changes content while preserving both file size and the server's observable mtime precision window. This is a known limitation, not a passing detection test.

## Evidence

- [ ] Record server type, observed SFTP size/mtime behavior, outcome status, and any conflict/restart-barrier messages.
- [ ] Confirm no test file or local temporary directory remains after the sessions are closed.
