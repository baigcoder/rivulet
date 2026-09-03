/**
 * Vertical edge swipes on the picture: left = volume, right = brightness.
 * A mostly-horizontal drag is a seek, where the title has a duration.
 *
 * The middle third stays a tap (chrome / double-tap seek) until the finger
 * has clearly gone sideways. Numbers are in `bun run check:swipe`.
 */

export type EdgeAdjust = 'volume' | 'brightness'

/** Left 40% volume, right 40% brightness. Middle is a tap (or a seek). */
export function edgeAdjust(x: number, width: number): EdgeAdjust | null {
  if (width <= 0)
    return null
  const t = x / width
  if (t < 0.4)
    return 'volume'
  if (t > 0.6)
    return 'brightness'
  return null
}

/** Up increases. A full-height swipe covers 0–100. */
export function edgeDelta(dy: number, height: number) {
  if (height <= 0)
    return 0
  return Math.round((-dy / height) * 100)
}

export function clampLevel(n: number) {
  return Math.max(0, Math.min(100, Math.round(n)))
}

/** Lock in once the drag is clearly vertical, so a tap is never a swipe. */
export function isVerticalSwipe(dx: number, dy: number) {
  return Math.abs(dy) > 16 && Math.abs(dy) > Math.abs(dx) * 1.2
}

/** Same threshold the other way: a scrub, not a tap. */
export function isHorizontalSwipe(dx: number, dy: number) {
  return Math.abs(dx) > 16 && Math.abs(dx) > Math.abs(dy) * 1.2
}

/**
 * One picture-width is 90 seconds, or 12% of the title — whichever is
 * smaller — so a short film is not a three-screen scrub and a long one
 * still moves.
 */
export function seekSeconds(dx: number, width: number, duration: number) {
  if (width <= 0 || duration <= 0)
    return 0
  const span = Math.min(90, Math.max(20, duration * 0.12))
  return (dx / width) * span
}
