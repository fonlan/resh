# SFTP Speed Rollout Notes

Last updated: 2026-03-23

## What Changed In This Patch
- Added transfer diagnostics primitives in backend SFTP path:
  - structured start/progress/finish logging
  - unified speed sampling model for UI progress and diagnostics
- Added OpenSSH limits capability detection/cache (`limits@openssh.com`) at session level.
- Added transfer tuning calculator with profiles:
  - `safe`
  - `balanced` (default)
  - `fast`
- Applied tuning model to upload path (chunk size + max in-flight) as low-risk first rollout.
- Added upload adaptive stabilization for single-file transfer:
  - bounded retry on chunk write timeout
  - consecutive timeout streak triggers in-flight downgrade
  - diagnostics now include consecutive timeout and downgrade counters
- Added SFTP tuning config fields in backend/frontend types.
- Added settings UI controls and i18n strings for tuning options.
- Added baseline matrix scaffold script:
  - `scripts/sftp_baseline_matrix.ps1`
  - output: `results.csv`, `results.md`, per-case logs

## Behavior Intentionally Unchanged
- Single-file download remains single-thread sequential (one in-flight read loop).
- Existing download integrity checks and cancel semantics are preserved.
- Multi-connection striping remains disabled by default.

## Why This Rollout Order
- Prioritize observability and safe tuning foundation first.
- Apply dynamic tuning to upload first (lower compatibility risk).
- Keep download behavior conservative until same-handle read pipeline is fully guarded by fallback logic.

## Next Phases
1. M2: Same-handle multi in-flight read scheduler for download with ordered writeback.
2. M2: Adaptive ramp up/down and session fallback lock on timeout/error.
3. M4/M5: Optional multi-connection modes (small-file parallel and large-file striping) behind guarded switches.

## Operator Notes
- Use matrix scaffold for before/after comparisons to prevent regressions.
- Keep profile defaults conservative when server limits are unknown.
- If timeout spikes occur, prioritize fallback behavior over aggressive retry.
