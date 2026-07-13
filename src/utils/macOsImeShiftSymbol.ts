/**
 * macOS Chinese IME often reports Shift+symbol as keyCode 229 without updating
 * xterm's hidden textarea, so CompositionHelper drops the key. Map physical
 * `event.code` (+ Shift) to US-QWERTY half-width ASCII shift symbols.
 *
 * ponytail: US-layout only; non-US shift labels need layout-aware mapping later.
 */

/** US QWERTY shift characters for common terminal symbol keys. */
export const MAC_OS_IME_SHIFT_BY_CODE: Readonly<Record<string, string>> = {
  Digit1: "!",
  Digit2: "@",
  Digit3: "#",
  Digit4: "$",
  Digit5: "%",
  Digit6: "^",
  Digit7: "&",
  Digit8: "*",
  Digit9: "(",
  Digit0: ")",
  Minus: "_",
  Equal: "+",
  BracketLeft: "{",
  BracketRight: "}",
  Backslash: "|",
  Semicolon: ":",
  Quote: '"',
  Comma: "<",
  Period: ">",
  Slash: "?",
  Backquote: "~",
}

export type MacOsImeShiftKeyLike = {
  type?: string
  keyCode: number
  key: string
  code: string
  shiftKey: boolean
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  isComposing: boolean
}

/**
 * Returns a single shift-symbol to inject for the macOS IME 229 drop path,
 * or null when the event should be left to xterm.
 */
export function resolveMacOsImeDroppedShiftSymbol(
  event: MacOsImeShiftKeyLike,
  options?: { imeComposing?: boolean },
): string | null {
  if (event.type && event.type !== "keydown") return null
  if (!event.shiftKey || event.metaKey || event.ctrlKey || event.altKey) {
    return null
  }
  // Shift alone / non-character modifiers
  if (event.keyCode === 16 || event.code === "ShiftLeft" || event.code === "ShiftRight") {
    return null
  }
  // Only patch IME Process path (keyCode 229). English IME keeps normal xterm path.
  if (event.keyCode !== 229) return null
  // True Chinese composition (candidate window): never force a Latin shift symbol.
  if (event.isComposing || options?.imeComposing) return null

  const fromCode = MAC_OS_IME_SHIFT_BY_CODE[event.code]
  if (fromCode) return fromCode

  // Rare: 229 but browser still exposes a single non-alphanumeric key.
  if (
    event.key.length === 1 &&
    event.key !== " " &&
    !/[a-zA-Z0-9]/.test(event.key)
  ) {
    return event.key
  }

  return null
}

/** Small self-check; fails loud if mapping/guards regress. */
export function assertMacOsImeShiftSymbolSelfCheck(): void {
  const base = {
    type: "keydown",
    keyCode: 229,
    key: "Process",
    code: "Digit1",
    shiftKey: true,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    isComposing: false,
  } as const

  const cases: Array<[MacOsImeShiftKeyLike, string | null]> = [
    [base, "!"],
    [{ ...base, code: "Digit2" }, "@"],
    [{ ...base, code: "Minus" }, "_"],
    [{ ...base, code: "Equal" }, "+"],
    [{ ...base, code: "Backquote" }, "~"],
    [{ ...base, code: "Slash" }, "?"],
    [{ ...base, code: "Quote" }, '"'],
    [{ ...base, shiftKey: false }, null],
    [{ ...base, keyCode: 49, key: "!" }, null],
    [{ ...base, isComposing: true }, null],
    [{ ...base, metaKey: true }, null],
    [{ ...base, type: "keyup" }, null],
    [{ ...base, keyCode: 16, code: "ShiftLeft", key: "Shift" }, null],
  ]

  for (const [ev, expected] of cases) {
    const got = resolveMacOsImeDroppedShiftSymbol(ev)
    if (got !== expected) {
      throw new Error(
        `macOsImeShiftSymbol: expected ${JSON.stringify(expected)} for ${ev.code} keyCode=${ev.keyCode}, got ${JSON.stringify(got)}`,
      )
    }
  }

  if (resolveMacOsImeDroppedShiftSymbol(base, { imeComposing: true }) !== null) {
    throw new Error("macOsImeShiftSymbol: imeComposing must block inject")
  }
}
