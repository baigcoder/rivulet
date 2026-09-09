/**
 * Free-TV channel health.
 *
 * A public playlist is a snapshot of what answered when someone last
 * looked, so some fraction of it is dead at any moment and no amount of
 * curation changes that. Two mitigations, both deliberately lazy:
 *
 * - **Do not probe the browse grid.** Opening twenty strangers' streams
 *   while someone scrolls saturates the local proxy and freezes the page
 *   — Back stops answering. The player marks a channel dead when it
 *   fails to open, and that is enough.
 * - **Zap past a failure.** A channel that will not open is one the player
 *   should leave, not one the viewer should stare at. `nextPlayable` picks
 *   the next channel that is not already known-dead, and `MAX_AUTO_SKIPS`
 *   bounds it — five black channels in a row is a broken *list*, and
 *   walking a hundred of them is worse than saying so.
 *
 * A probe is advisory, never a gate: it goes through the local proxy with
 * the proxy's own headers, and an upstream that refuses that request can
 * still open in mpv. So an offline verdict dims a card and reorders the
 * zap list; it never disables the click.
 */

/** A stream that has not answered in five seconds is not worth a card. */
export const PROBE_TIMEOUT_MS = 5000

/** In flight at once. The bound is the upstreams' patience, not ours. */
export const PROBE_CONCURRENCY = 4

/** Consecutive automatic zaps before the player admits defeat. */
export const MAX_AUTO_SKIPS = 5

export type Health = 'live' | 'offline'

/**
 * Anything that answered with a status the player could follow counts as
 * live. The proxy turns a refused connection into 502 and passes every
 * other upstream status through, so this reads both.
 */
export function probeVerdict(status: number): Health {
  return status >= 200 && status < 400 ? 'live' : 'offline'
}

/**
 * The next index in `list` that is not known-dead, walking in `step`
 * direction, or -1 when the rest of the list is dead. `from` is the index
 * that just failed, so the walk starts past it.
 */
export function nextPlayable(
  list: ReadonlyArray<{ id: string }>,
  from: number,
  offline: ReadonlySet<string>,
  step: 1 | -1 = 1,
): number {
  for (let i = from + step; i >= 0 && i < list.length; i += step) {
    const item = list[i]
    if (item && !offline.has(item.id))
      return i
  }
  return -1
}

/** Run `task` over `items` with at most `limit` of them in flight. */
export async function pool<T>(
  items: readonly T[],
  limit: number,
  task: (item: T) => Promise<void>,
): Promise<void> {
  let cursor = 0
  const workers = Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, async () => {
    while (cursor < items.length) {
      const item = items[cursor++]
      if (item !== undefined)
        await task(item)
    }
  })
  await Promise.all(workers)
}

/**
 * Open a proxied stream URL and keep only the status line. The body is
 * cancelled immediately: a live stream has no end, and reading one to
 * decide whether it exists would download it.
 */
export async function probeStream(
  proxiedUrl: string,
  timeoutMs: number = PROBE_TIMEOUT_MS,
): Promise<Health> {
  const ctrl = new AbortController()
  const timer = setTimeout(() => ctrl.abort(), timeoutMs)
  try {
    const res = await fetch(proxiedUrl, { signal: ctrl.signal, cache: 'no-store' })
    void res.body?.cancel().catch(() => {})
    return probeVerdict(res.status)
  }
  catch {
    // A timeout, a DNS failure and a TLS error are the same fact here:
    // nothing answered.
    return 'offline'
  }
  finally {
    clearTimeout(timer)
  }
}
