import { invoke } from "@tauri-apps/api/core"
import type {
  FrontendRestartBlockers,
  OperationSnapshot,
  PendingRestartSession,
  RestartSessionSnapshot,
  WaitUntilIdleResult,
} from "../types/update"
import { useUpdateStore } from "../stores/useUpdateStore"

export async function getOperationSnapshot(): Promise<OperationSnapshot> {
  return invoke<OperationSnapshot>("get_operation_snapshot_cmd")
}

export type BeginDrainingResult = {
  snapshot: OperationSnapshot
  drainSession: number
}

export async function beginRestartDraining(): Promise<BeginDrainingResult> {
  return invoke<BeginDrainingResult>("begin_restart_draining_cmd")
}

export async function cancelRestartDraining(
  drainSession?: number | null,
): Promise<OperationSnapshot> {
  return invoke<OperationSnapshot>("cancel_restart_draining_cmd", {
    drainSession: drainSession ?? null,
  })
}

export async function waitUntilOperationsIdle(
  timeoutMs = 30_000,
): Promise<WaitUntilIdleResult> {
  return invoke<WaitUntilIdleResult>("wait_until_operations_idle_cmd", {
    timeoutMs,
  })
}

export async function saveRestartSessionSnapshot(
  snapshot: RestartSessionSnapshot,
): Promise<string> {
  return invoke<string>("save_restart_session_snapshot_cmd", { snapshot })
}

export async function getPendingRestartSession(): Promise<PendingRestartSession | null> {
  return invoke<PendingRestartSession | null>("get_pending_restart_session_cmd")
}

export async function ackRestartSession(token: string): Promise<void> {
  return invoke("ack_restart_session_cmd", { token })
}

export async function verifyReadyForRestart(
  snapshotToken: string,
): Promise<void> {
  return invoke("verify_ready_for_restart_cmd", { snapshotToken })
}

export type InstallPreparedUpdateResponse = {
  helperStarted: boolean
  targetVersion: string
  message: string
}

/** Spawn platform install helper + schedule process exit. */
export async function installPreparedUpdate(
  preparedId: string,
  snapshotToken: string,
): Promise<InstallPreparedUpdateResponse> {
  return invoke<InstallPreparedUpdateResponse>("install_prepared_update_cmd", {
    preparedId,
    snapshotToken,
  })
}

export async function getLastInstallFailure(): Promise<string | null> {
  return invoke<string | null>("get_last_install_failure_cmd")
}

export async function ackUpdateInstall(
  preparedId?: string | null,
): Promise<void> {
  return invoke("ack_update_install_cmd", {
    preparedId: preparedId ?? null,
  })
}

export async function platformSupportsInstall(): Promise<boolean> {
  return invoke<boolean>("platform_supports_install_cmd")
}

type WaitGate = {
  resolve: () => void
  reject: (err: Error) => void
}

/** Soft wait-timeout gate: UI may resume via continueRestartWait(). */
let waitTimeoutGate: WaitGate | null = null
/**
 * Monotonic prepare run id. Cancel increments generation so superseded awaits
 * cannot write a snapshot or flip status to restarting.
 */
let prepareGeneration = 0
/** Generation currently allowed to proceed (0 = none). */
let activePrepareGeneration = 0
/** Drain session for the active prepare run (backend token). */
let activeDrainSession: number | null = null
/** In-flight begin IPC — cancel must await this to avoid late-begin races. */
let beginInFlight: Promise<BeginDrainingResult> | null = null
/** In-flight cancel — new prepare waits so sessions stay ordered. */
let cancelInFlight: Promise<void> | null = null

function clearWaitTimeoutGate(error?: Error) {
  const gate = waitTimeoutGate
  waitTimeoutGate = null
  if (!gate) return
  if (error) gate.reject(error)
  else gate.resolve()
}

function assertPrepareActive(runId: number): void {
  if (runId !== activePrepareGeneration) {
    throw new Error("RESTART_CANCELLED")
  }
}

/**
 * Resume waiting after a soft wait-timeout pause.
 * No-op if nothing is waiting.
 */
export function continueRestartWait(): void {
  clearWaitTimeoutGate()
}

/**
 * Prepare safe restart: frontend blockers must already be clear.
 * Enters draining, waits for backend operations, saves snapshot, final verify.
 * Returns the token once the app is ready for install+exit (Phase 4 helper).
 *
 * On soft wait timeout the barrier stays draining. Call `continueRestartWait()`
 * to resume the same prepare flow, or `cancelSafeRestart()` to abort.
 */
export async function prepareSafeRestart(options: {
  snapshot: RestartSessionSnapshot
  blockers: FrontendRestartBlockers
  onSnapshot?: (snap: OperationSnapshot) => void
  waitTimeoutMs?: number
}): Promise<{ token: string; snapshot: OperationSnapshot }> {
  const { snapshot, blockers, onSnapshot, waitTimeoutMs = 30_000 } = options

  if (blockers.dirtyEditors.length > 0) {
    throw new Error("DIRTY_EDITORS")
  }
  if (blockers.savingEditors.length > 0) {
    throw new Error("SAVING_EDITORS")
  }
  if (blockers.recordingTabs.length > 0) {
    throw new Error("RECORDING_TABS")
  }
  if (
    blockers.settingsSaving ||
    blockers.settingsDirty ||
    blockers.settingsSaveError
  ) {
    throw new Error("SETTINGS_PENDING")
  }

  // Wait for any in-flight cancel so we never begin under a concurrent cancel.
  if (cancelInFlight) {
    try {
      await cancelInFlight
    } catch {
      // ignore
    }
  }

  // Invalidate any prior in-flight prepare before starting a new one.
  prepareGeneration += 1
  const runId = prepareGeneration
  activePrepareGeneration = runId
  activeDrainSession = null
  clearWaitTimeoutGate(new Error("RESTART_SUPERSEDED"))

  useUpdateStore.getState().setStatus("waitingForSafeRestart")
  useUpdateStore.getState().setError(null)
  useUpdateStore.getState().setRestartWaitTimedOut(false)

  let drainSessionLocal: number | null = null
  try {
    const beginPromise = beginRestartDraining()
    beginInFlight = beginPromise
    let began: BeginDrainingResult
    try {
      began = await beginPromise
    } finally {
      if (beginInFlight === beginPromise) {
        beginInFlight = null
      }
    }

    // Cancel while begin was in flight: cancelSafeRestart awaits the same
    // promise and clears the session. We must not treat this begin as ours.
    if (runId !== activePrepareGeneration) {
      // Only compensate if cancel did not already own this session.
      // cancel path awaits begin and cancels; double-cancel matching session is fine.
      try {
        await cancelRestartDraining(began.drainSession)
      } catch {
        // ignore
      }
      throw new Error("RESTART_CANCELLED")
    }

    drainSessionLocal = began.drainSession
    activeDrainSession = began.drainSession
    let snap = began.snapshot
    onSnapshot?.(snap)

    // Wait in loops; never force-kill. Auto-wait until overall deadline, then soft
    // pause for UI "keep waiting" / cancel (no force-kill).
    let deadline = Date.now() + 10 * 60 * 1000
    while (!snap.idle) {
      assertPrepareActive(runId)
      const wait = await waitUntilOperationsIdle(waitTimeoutMs)
      assertPrepareActive(runId)
      snap = wait.snapshot
      onSnapshot?.(snap)
      if (wait.idle) break

      if (Date.now() <= deadline) {
        continue
      }

      useUpdateStore.getState().setRestartWaitTimedOut(true)
      useUpdateStore.getState().setStatus("waitingForSafeRestart")
      await new Promise<void>((resolve, reject) => {
        clearWaitTimeoutGate(new Error("RESTART_SUPERSEDED"))
        waitTimeoutGate = { resolve, reject }
      })
      assertPrepareActive(runId)
      useUpdateStore.getState().setRestartWaitTimedOut(false)
      deadline = Date.now() + 10 * 60 * 1000
    }

    assertPrepareActive(runId)

    const token = await saveRestartSessionSnapshot({
      ...snapshot,
      token: snapshot.token || "",
    })
    assertPrepareActive(runId)

    await verifyReadyForRestart(token)
    assertPrepareActive(runId)

    snap = await getOperationSnapshot()
    assertPrepareActive(runId)
    onSnapshot?.(snap)

    if (!snap.idle) {
      throw new Error("OPERATIONS_NOT_IDLE")
    }

    useUpdateStore.getState().setRestartWaitTimedOut(false)
    useUpdateStore.getState().setStatus("restarting")
    return { token, snapshot: snap }
  } catch (err) {
    // If we still own the generation and hold a drain session, leave it for
    // cancelSafeRestart / retry; only compensate when superseded mid-flight.
    if (
      drainSessionLocal != null &&
      runId !== activePrepareGeneration
    ) {
      try {
        await cancelRestartDraining(drainSessionLocal)
      } catch {
        // ignore
      }
    }
    throw err
  }
}

export async function cancelSafeRestart(): Promise<void> {
  // Snapshot state before invalidating generation so we can clear the right session.
  const pendingBegin = beginInFlight
  const sessionToCancel = activeDrainSession

  prepareGeneration += 1
  activePrepareGeneration = 0
  activeDrainSession = null
  clearWaitTimeoutGate(new Error("RESTART_CANCELLED"))

  const work = (async () => {
    if (pendingBegin) {
      try {
        const began = await pendingBegin
        await cancelRestartDraining(began.drainSession)
      } catch {
        // Begin failed or cancel failed — try known session / unconditional.
        try {
          await cancelRestartDraining(sessionToCancel)
        } catch {
          // ignore
        }
      }
    } else {
      await cancelRestartDraining(sessionToCancel)
    }
  })()

  cancelInFlight = work
  try {
    await work
  } finally {
    if (cancelInFlight === work) {
      cancelInFlight = null
    }
    const store = useUpdateStore.getState()
    store.setRestartWaitTimedOut(false)
    if (
      store.status === "waitingForSafeRestart" ||
      store.status === "restarting"
    ) {
      store.setStatus(
        store.prepared ? "ready" : store.update ? "available" : "idle",
      )
    }
  }
}
