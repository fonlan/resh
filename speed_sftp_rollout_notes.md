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
- Added same-handle pipelined download for single-file transfer:
  - multiple in-flight `READ` requests on one SFTP handle
  - ordered local writeback by offset to preserve file correctness
  - short-read guard with missing-range requeue and duplicate-offset protection
- Added adaptive download stabilization:
  - in-flight window downgrade on timeout streak
  - gradual window ramp-up on stable chunk completions
  - session-level fallback lock to single-flight mode after repeated timeouts
- Added SFTP tuning config fields in backend/frontend types.
- Added settings UI controls and i18n strings for tuning options.
- Added baseline matrix scaffold script:
  - `scripts/sftp_baseline_matrix.ps1`
  - output: `results.csv`, `results.md`, per-case logs

## Behavior Intentionally Unchanged
- Existing download integrity checks and cancel semantics are preserved.
- Multi-connection striping remains disabled by default.

## Why This Rollout Order
- Prioritize observability and safe tuning foundation first.
- Apply dynamic tuning to upload first (lower compatibility risk).
- Keep download behavior conservative until same-handle read pipeline is fully guarded by fallback logic.

## Next Phases
1. Run baseline matrix and unstable-link regression to quantify M2 gains and failure characteristics.
2. M4/M5: Optional multi-connection modes (small-file parallel and large-file striping) behind guarded switches.

## Operator Notes
- Use matrix scaffold for before/after comparisons to prevent regressions.
- Keep profile defaults conservative when server limits are unknown.
- If timeout spikes occur, prioritize fallback behavior over aggressive retry.
