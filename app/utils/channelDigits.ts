/**
 * Tuning by number, the way a set does: digits gather for a moment, then the
 * channel with that number plays. The overlay already shows each channel's
 * number ("105"), so a remote with a keypad should be able to type it.
 */

/** How long after the last digit the number is taken as finished. */
export const DIGIT_COMMIT_MS = 1500

/** Four digits is past any lineup a remote is used for; older ones scroll off. */
const MAX_DIGITS = 4

type KeyLike = Pick<KeyboardEvent, 'key' | 'code' | 'target'>

/** Typing into a field is typing, never tuning. */
function isTextTarget(target: EventTarget | null): boolean {
  const el = target as { tagName?: string, isContentEditable?: boolean } | null
  if (!el)
    return false
  const tag = (el.tagName ?? '').toUpperCase()
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || !!el.isContentEditable
}

/** The digit a key press stands for, top row or numpad, or null. */
export function channelDigitKey(e: KeyLike): string | null {
  if (isTextTarget(e.target))
    return null
  return /^\d$/.test(e.key) ? e.key : null
}

export function appendDigit(buffer: string, digit: string): string {
  return (buffer + digit).slice(-MAX_DIGITS)
}

/** The 1-based number typed, as an index into a lineup of `count`, or null. */
export function channelIndexFor(buffer: string, count: number): number | null {
  const n = Number.parseInt(buffer, 10)
  if (!Number.isFinite(n) || n < 1 || n > count)
    return null
  return n - 1
}
