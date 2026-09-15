/**
 * Cross-path duplicate suppression for terminal keyboard input.
 *
 * One physical keystroke can reach the SSH session through two independent
 * layers:
 *   - `terminal`    — xterm's own paths (keydown / keypress / textarea diff /
 *                     paste), surfaced via `Terminal.onData`
 *   - `beforeinput` — our synchronous takeover of IME text insertions on the
 *                     hidden helper textarea (Scheme B in `useTerminal`)
 *   - `manual`      — direct injections we emit ourselves (Scheme A Shift
 *                     symbols, Scheme C 229-Backspace)
 *
 * Each layer is correct in isolation, but with a macOS Chinese IME active the
 * engine can fire two of them for the *same* keystroke: WKWebView reports e.g.
 * Space as keyCode 229 on keydown (Scheme B blocks xterm's diff, our
 * `beforeinput` delivers the character) while the space keypress still carries
 * charCode 32, so xterm's `_keyPress` forwards it as well — the space landed
 * twice in the terminal.
 *
 * Only a delivery whose text matches the *previous* delivery from a *different*
 * path inside the window is dropped. Two consecutive deliveries from the same
 * path (a user typing the same key twice, however fast) are never suppressed,
 * and a dropped copy consumes the marker so the next identical delivery goes
 * through as well.
 */

export type InputDeliveryPath = "terminal" | "beforeinput" | "manual"

/**
 * Both copies of a duplicated keystroke are produced from the same DOM event,
 * so they arrive within a few milliseconds. 30ms leaves room for an engine
 * deferring the IME insertion by a task or two while staying far below the
 * fastest human same-key repeat (~60ms+).
 */
export const CROSS_PATH_DEDUPE_MS = 30

export type InputDeliveryTracker = {
  /**
   * Whether this delivery should be forwarded. `false` means the same text was
   * just delivered by a different path and this is the duplicate copy. Empty
   * data is always rejected.
   */
  accept: (data: string, path: InputDeliveryPath) => boolean
}

export function createInputDeliveryTracker(
  dedupeMs: number = CROSS_PATH_DEDUPE_MS,
  now: () => number = () => performance.now(),
): InputDeliveryTracker {
  let last: { data: string; at: number; path: InputDeliveryPath } | null = null

  return {
    accept(data, path) {
      if (!data) return false

      const at = now()
      if (
        last &&
        last.path !== path &&
        last.data === data &&
        at - last.at <= dedupeMs
      ) {
        // Consume the marker: an immediately following genuine repeat of the
        // same character must not be swallowed too.
        last = null
        return false
      }

      last = { data, at, path }
      return true
    },
  }
}

/** Small self-check; fails loud if the dedupe rules regress. */
export function assertInputDeliveryTrackerSelfCheck(): void {
  const run = (
    steps: Array<[string, InputDeliveryPath, number, boolean]>,
  ): void => {
    let clock = 0
    const tracker = createInputDeliveryTracker(CROSS_PATH_DEDUPE_MS, () => clock)
    for (const [data, path, advanceMs, expected] of steps) {
      clock += advanceMs
      const got = tracker.accept(data, path)
      if (got !== expected) {
        throw new Error(
          `imeInputDelivery: accept(${JSON.stringify(data)}, ${path}) after +${advanceMs}ms expected ${expected}, got ${got}`,
        )
      }
    }
  }

  // The reported bug: our beforeinput delivery followed by xterm's keypress.
  run([
    [" ", "beforeinput", 0, true],
    [" ", "terminal", 1, false],
    // marker consumed → the next identical delivery is a real character again
    [" ", "terminal", 1, true],
  ])

  // Reverse order (engine defers the IME insertion) behaves the same.
  run([
    [" ", "terminal", 0, true],
    [" ", "beforeinput", 2, false],
  ])

  // Genuine fast repeat of one key from a single path is never dropped.
  run([
    ["a", "terminal", 0, true],
    ["a", "terminal", 1, true],
    ["a", "terminal", 1, true],
  ])

  // Different text from another path is untouched.
  run([
    ["a", "beforeinput", 0, true],
    ["b", "terminal", 1, true],
  ])

  // Same text from another path outside the window is a new keystroke.
  run([
    ["a", "beforeinput", 0, true],
    ["a", "terminal", CROSS_PATH_DEDUPE_MS + 1, true],
  ])

  // Scheme A manual injection followed by the IME committing the same char.
  run([
    ["!", "manual", 0, true],
    ["!", "beforeinput", 3, false],
  ])

  // Empty data is never forwarded.
  run([["", "terminal", 0, false]])
}
