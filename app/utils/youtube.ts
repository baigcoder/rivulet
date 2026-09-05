import { isTauri } from '@tauri-apps/api/core'

/** Loopback shim on the IPTV proxy port — see iptv/proxy.rs `/youtube-embed`. */
const RELAY = 'http://127.0.0.1:3031/youtube-embed'

/**
 * Build a trailer iframe src.
 *
 * Tauri loads from `tauri://` on Linux/macOS production, which YouTube
 * rejects — route through loopback HTTP instead. `loop` needs `playlist`
 * set to the same id or YouTube plays once and stops (the cover hero).
 */
export function youtubeEmbedSrc(key: string, opts: { mute?: boolean, loop?: boolean } = {}) {
  if (isTauri()) {
    const q = new URLSearchParams({ v: key, autoplay: '1' })
    if (opts.mute)
      q.set('mute', '1')
    if (opts.loop)
      q.set('loop', '1')
    return `${RELAY}?${q}`
  }
  const q = new URLSearchParams({ autoplay: '1', rel: '0', playsinline: '1', enablejsapi: '1', vq: 'hd720' })
  if (typeof location !== 'undefined')
    q.set('origin', location.origin)
  if (opts.mute)
    q.set('mute', '1')
  if (opts.loop) {
    q.set('loop', '1')
    q.set('playlist', key)
  }
  return `https://www.youtube.com/embed/${key}?${q}`
}

/** YouTube IFrame command. Quality lock stops the player climbing to 1080/4K. */
export function youtubeCommand(func: string, args: unknown[] = []) {
  return JSON.stringify({ event: 'command', func, args })
}

export function youtubePlaying(data: unknown): boolean {
  let payload = data
  if (typeof payload === 'string') {
    try {
      payload = JSON.parse(payload)
    }
    catch {
      return false
    }
  }
  if (!payload || typeof payload !== 'object')
    return false
  return (payload as { info?: { playerState?: number } }).info?.playerState === 1
}

/** Embed blocked, missing, or geo-restricted — try the next TMDB key. */
export function youtubeError(data: unknown): boolean {
  let payload = data
  if (typeof payload === 'string') {
    try {
      payload = JSON.parse(payload)
    }
    catch {
      return false
    }
  }
  if (!payload || typeof payload !== 'object')
    return false
  return (payload as { event?: string }).event === 'onError'
}
