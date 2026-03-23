# SFTP Speed TODO (Execution Tracker)

Last updated: 2026-03-23 22:36
Source plan: `speed_sftp.md`

## Rules For This File
- This file is the single source of truth for implementation progress.
- After each completed function slice, update checklist status and append a progress log entry.
- Keep items executable and verifiable (code path + acceptance note).

## Milestone Checklist

### M0 - Baseline & Observability
- [x] M0.1 Add structured transfer diagnostics for download/upload path.
  - Target files: `src-tauri/src/sftp_manager/mod.rs`
  - Acceptance: logs include task/session/type, chunk, in-flight, throughput, elapsed, timeout/error markers.
- [x] M0.2 Unify speed sampling semantics for transfer progress and diagnostics.
  - Target files: `src-tauri/src/sftp_manager/mod.rs`
  - Acceptance: both display speed and diagnostics use same interval-based sampling model.
- [x] M0.3 Add baseline script scaffold for reproducible matrix tests.
  - Target files: `scripts/` (new), optional docs.
  - Acceptance: script supports low/high RTT + small/large file runs and outputs simple result table/logs.

### M1 - Capability Detection & Tuning Model
- [x] M1.1 Add session-level OpenSSH limits cache (`limits@openssh.com`).
  - Target files: `src-tauri/src/sftp_manager/mod.rs`
  - Acceptance: cache includes max packet/read/write/open-handles and is cleared on session removal.
- [x] M1.2 Add transfer tuning calculator (`safe` / `balanced` / `fast`, default `balanced`).
  - Target files: `src-tauri/src/sftp_manager/mod.rs`, config types.
  - Acceptance: final chunk/in-flight values are bounded by local caps and server limits.
- [x] M1.3 Wire tuning model to upload path first (low-risk rollout).
  - Target files: `src-tauri/src/sftp_manager/mod.rs`
  - Acceptance: upload uses computed chunk/in-flight instead of fixed `32KB` + `16`.

### M2 - Download Pipeline (next phase, not in first patch)
- [ ] M2.1 Implement same-handle multi in-flight READ scheduler (ordered writeback).
- [ ] M2.2 Add adaptive ramp up/down and per-session fallback lock.
- [ ] M2.3 Keep integrity checks/cancel semantics unchanged.

### M3 - Upload Adaptive Stabilization
- [x] M3.1 Add adaptive in-flight downgrade on consecutive timeout.
- [x] M3.2 Add upload stability report fields.

### Config/UI/Data
- [x] C1 Add SFTP tuning config schema fields.
  - `transferProfile`
  - `downloadMaxInflight`
  - `uploadMaxInflight`
  - `chunkSizeMin`
  - `chunkSizeMax`
  - `enableMultiConnectionForSmallFiles`
  - `enableLargeFileStriping` (default `false`)
- [x] C2 Add settings UI controls for above fields (with conservative defaults).
- [x] C3 Add i18n strings (`en`, `zh-CN`).

### Docs & Guardrails
- [x] D1 Update `AGENTS.md` constraints if runtime policy changes.
- [x] D2 Keep backward compatibility for old config files (serde default + alias).
- [x] D3 Add rollout notes (what changed now vs next phase).

## Current Implementation Scope (This Turn)
- [x] Scope-A: Complete M0.1 + M0.2
- [x] Scope-B: Complete M1.1 + M1.2 + M1.3
- [x] Scope-C: Complete C1 + C2 + C3
- [x] Scope-D: Run formatting/checks and sync this file status
- [x] Scope-E: Complete M0.3 + D3 + M3.1 + M3.2

## Progress Log

- 2026-03-23 22:18 (start): Created execution tracker from `speed_sftp.md`. Implementation started with M0/M1 low-risk rollout.
- 2026-03-23 22:42 (M0): Added transfer diagnostics primitives (`TransferDiagnostics`, unified `SpeedSampler`, transfer start/progress/finish logs) and wired them into single-file upload/download paths.
- 2026-03-23 22:56 (M1): Added session-level OpenSSH `limits@openssh.com` cache in `get_session` and cleanup in `remove_session`; added transfer tuning calculator with profile + server-limit bounding.
- 2026-03-23 23:08 (Config/UI): Added SFTP tuning config fields in backend/frontend models, exposed controls in `SFTPTab`, and added `en`/`zh-CN` i18n entries.
- 2026-03-23 23:15 (Docs/Validation): Updated `AGENTS.md` SFTP rules to reflect capability-driven tuning while preserving current single-thread download phase; ran `cargo fmt`, `cargo check`, `tsc`, and frontend `prettier`.
- 2026-03-23 23:22 (M0.3): Added `scripts/sftp_baseline_matrix.ps1` matrix scaffold (low/high RTT x small/large x upload/download) with `results.csv` and `results.md` outputs plus per-case logs; added `scripts/sftp_baseline_report_template.md`.
- 2026-03-23 23:27 (D3): Added rollout notes document `speed_sftp_rollout_notes.md` to clarify current delivered scope vs next phases (M2/M3/M4/M5).
- 2026-03-23 22:34 (M3): Added upload adaptive timeout handling in `_upload_file`: timed-out chunks now support bounded retry, consecutive timeout streak triggers in-flight window downgrade, and transfer diagnostics logs now include `consecutive_timeout_count` + `downgrade_count`; verified by `cargo fmt` and `cargo check`.
