/**
 * What a live player does when its stream fails — shared by Free TV and
 * Premium TV so the two recover the same way.
 */

/**
 * How many times a dead stream is retried before the player gives up and
 * says so. Four attempts over the backoff below is about three quarters
 * of a minute — long enough to ride out a provider restart, short enough
 * that a genuinely dead channel does not look like a hung app.
 */
export const MAX_RECONNECT_ATTEMPTS = 4

/**
 * Exponential, capped. `attempt` is 1-based.
 *
 * The first step is two seconds rather than one, and the cap is twenty
 * rather than eight, for a reason particular to this provider protocol:
 * a panel counts a *connection*, and it does not stop counting the one
 * that just died the instant our player exits. An account whose
 * `maxConnections` is 1 — the common case for a single-device
 * subscription — answers 401 to every request until its own slot is
 * released, so a schedule that spends all four attempts inside that
 * window reports "dead channel" for something that was only busy.
 */
export function reconnectDelayMs(attempt: number): number {
  return Math.min(2000 * 2 ** (attempt - 1), 20_000)
}

export type LiveFailureAction = 'reconnect' | 'fallback' | 'skip' | 'error'

/**
 * A channel that has shown a picture is alive and the drop is the network's:
 * reconnect it, and never zap the viewer away from what they chose. One that
 * never played is dead on arrival: try the other protocol once, then move on
 * down the lineup — bounded, so a dead category doesn't flash past.
 */
export function nextLiveAction(s: {
  hadPicture: boolean
  reconnects: number
  canFallback: boolean
  fallbackTried: boolean
  autoSkips: number
  maxAutoSkips: number
}): LiveFailureAction {
  if (s.hadPicture)
    return s.reconnects < MAX_RECONNECT_ATTEMPTS ? 'reconnect' : 'error'
  if (s.canFallback && !s.fallbackTried)
    return 'fallback'
  return s.autoSkips < s.maxAutoSkips ? 'skip' : 'error'
}
