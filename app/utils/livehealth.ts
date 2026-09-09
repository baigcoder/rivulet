/**
 * Live-channel health, Free TV and Premium TV.
 *
 * A playlist is a snapshot of what answered when someone last looked, so
 * some fraction of it is dead at any moment. Two rules follow, and they
 * are the same on both libraries:
 *
 * - **Playback is the source of truth.** A GET through the proxy is not:
 *   these CDNs refuse a ranged fetch (403/404) and then serve the other
 *   container, which is exactly the log line that used to mark a working
 *   channel offline. Opening twenty strangers while someone scrolls also
 *   saturates the proxy and freezes Back. So the player marks a channel
 *   when a *picture* arrives (live) or when retries and the format
 *   fallback are spent (offline). A probe stays advisory and is never a
 *   gate on the click.
 * - **Zap past a failure.** `nextPlayable` picks the next channel that is
 *   not already known-dead, and `MAX_AUTO_SKIPS` bounds it — five black
 *   channels in a row is a broken *list*, and walking a hundred of them
 *   is worse than saying so.
 *
 * Verdicts are session-only. A channel that was dead this morning is
 * worth trying again tonight; a persisted tag would hide it for good.
 *
 * Premium never probes from the page at all: the UI must not see a
 * provider URL, and a background GET would steal the account's one
 * connection slot. Same book, filled only from the player.
 */
import { ref } from 'vue'

/** A stream that has not answered in 3.5 seconds is not worth a card. */
export const PROBE_TIMEOUT_MS = 3500

/** In flight at once. The bound is the upstreams' patience, not ours. */
export const PROBE_CONCURRENCY = 4

/** Consecutive automatic zaps before the player admits defeat. */
export const MAX_AUTO_SKIPS = 5

export type Health = 'unknown' | 'live' | 'offline'

/**
 * Anything that answered with a status the player could follow counts as
 * live. The proxy turns a refused connection into 502 and passes every
 * other upstream status through, so this reads both.
 *
 * Advisory only — see the file header. A 403 here is "this GET was
 * refused", not "mpv cannot play it".
 */
export function probeVerdict(status: number): Exclude<Health, 'unknown'> {
  return status >= 200 && status < 400 ? 'live' : 'offline'
}

/**
 * mpv's `audio-params` once a decoder is up. Live HLS often has sound
 * before `video-params` (and hiding the window until a frame exists
 * can keep `video-params` at zero forever) — treating that as "still
 * connecting" is what auto-skipped a channel the viewer could already
 * hear.
 */
export function audioParamsReady(params: unknown): boolean {
  if (!params || typeof params !== 'object')
    return false
  const o = params as Record<string, unknown>
  const sr = o.samplerate
  const cc = o['channel-count'] ?? o.channelCount
  return (typeof sr === 'number' && sr > 0) || (typeof cc === 'number' && cc > 0)
}

/** Picture or sound: either means this channel is up. */
export function liveLocked(videoWidth: number, hasAudio: boolean): boolean {
  return videoWidth > 0 || hasAudio
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
): Promise<Exclude<Health, 'unknown'>> {
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

/**
 * Session map both live-TV stores keep. Replacing the Set (not mutating
 * it) is what makes a card's computed re-run: a Set is not deeply
 * reactive.
 */
export function createChannelHealth() {
  const liveIds = ref<Set<string>>(new Set())
  const offlineIds = ref<Set<string>>(new Set())

  function healthOf(id: string): Health {
    if (!id)
      return 'unknown'
    if (offlineIds.value.has(id))
      return 'offline'
    if (liveIds.value.has(id))
      return 'live'
    return 'unknown'
  }

  function markOffline(channelId: string): void {
    if (!channelId || offlineIds.value.has(channelId))
      return
    offlineIds.value = new Set(offlineIds.value).add(channelId)
    if (!liveIds.value.has(channelId))
      return
    const next = new Set(liveIds.value)
    next.delete(channelId)
    liveIds.value = next
  }

  function markLive(channelId: string): void {
    if (!channelId)
      return
    if (!liveIds.value.has(channelId))
      liveIds.value = new Set(liveIds.value).add(channelId)
    if (!offlineIds.value.has(channelId))
      return
    const next = new Set(offlineIds.value)
    next.delete(channelId)
    offlineIds.value = next
  }

  function reset(): void {
    if (liveIds.value.size === 0 && offlineIds.value.size === 0)
      return
    liveIds.value = new Set()
    offlineIds.value = new Set()
  }

  return { liveIds, offlineIds, healthOf, markLive, markOffline, reset }
}
