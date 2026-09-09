import { invoke, isTauri } from '@tauri-apps/api/core'
import { isDesktop, isLinux } from './platform'

/** Loopback shim on the IPTV proxy port — see iptv/proxy.rs `/youtube-embed`. */
const RELAY = 'http://127.0.0.1:3031/youtube-embed'

/** Direct-stream proxy — see iptv/proxy.rs `/youtube-stream`. */
const STREAM = 'http://127.0.0.1:3031/youtube-stream'

/**
 * Build a trailer iframe src.
 *
 * Tauri loads from `tauri://` on Linux/macOS production, which YouTube
 * rejects — route through loopback HTTP instead.
 *
 * A one-id `playlist` is how YouTube honours `loop`, but it also paints
 * previous / next on the cover. The hero loops from the page instead
 * (`youtubeEnded` → seek 0) and leaves `playlist` off when chrome is hidden.
 */
export function youtubeEmbedSrc(key: string, opts: { mute?: boolean, loop?: boolean, controls?: boolean } = {}) {
  const hideChrome = opts.controls === false
  if (isTauri()) {
    const q = new URLSearchParams({ v: key, autoplay: '1' })
    if (opts.mute)
      q.set('mute', '1')
    if (opts.loop)
      q.set('loop', '1')
    if (hideChrome)
      q.set('controls', '0')
    return `${RELAY}?${q}`
  }
  const q = new URLSearchParams({ autoplay: '1', rel: '0', playsinline: '1', enablejsapi: '1', vq: 'hd1080' })
  if (typeof location !== 'undefined')
    q.set('origin', location.origin)
  if (opts.mute)
    q.set('mute', '1')
  if (opts.loop && !hideChrome) {
    q.set('loop', '1')
    q.set('playlist', key)
  }
  if (hideChrome) {
    q.set('controls', '0')
    q.set('modestbranding', '1')
    q.set('fs', '0')
    q.set('disablekb', '1')
    q.set('iv_load_policy', '3')
    q.set('cc_load_policy', '0')
  }
  return `https://www.youtube.com/embed/${key}?${q}`
}

/**
 * The native `<video>` src for a trailer. The loopback `/youtube-stream` route
 * resolves the ID to a direct media file (via the bundled yt-dlp, at up to
 * 1080p) and proxies the bytes back through the loopback, so a WebKit `<video>`
 * can play what a YouTube iframe embed would refuse to (error 153 — "Your
 * browser can't play this video"). Empty outside desktop Tauri — Android keeps
 * the iframe (no yt-dlp there), and browser dev has no proxy. Muted/loop/
 * autoplay are element attributes, not here.
 *
 * Linux is the reason this exists. WebKitGTK's YouTube iframe still dies with
 * 153 even after spoofing Chrome, and an AppImage that did not bundle GStreamer's
 * H.264 plugins cannot decode the MSE player either. A progressive mp4 through
 * this proxy is the same path Windows and macOS already use.
 */
export function youtubeStreamSrc(key: string): string {
  if (!isTauri() || !isDesktop())
    return ''
  // Linux is excluded on purpose. WebKitGTK's media pipeline *waits* on this
  // endpoint while it mounts the element, so a slow resolve wedges the whole
  // detail page — a frozen app, not a failed video, which means no timeout or
  // `error` handler in the page can recover it (see 66ce479). Linux plays a
  // trailer in mpv instead: `playYoutubeTrailer`.
  if (isLinux())
    return ''
  return `${STREAM}?${new URLSearchParams({ v: key })}`
}

/**
 * Linux AppImage / WebKitGTK cannot play a YouTube iframe (error 153) and often
 * cannot decode the proxied `<video>` either. System mpv plus the bundled yt-dlp
 * can. Returns true when mpv was launched — the caller should not open a dialog.
 */
export async function playYoutubeTrailer(key: string): Promise<boolean> {
  if (!isTauri() || !isLinux() || !key)
    return false
  try {
    await invoke('play_url_mpv', { url: `https://www.youtube.com/watch?v=${key}` })
    return true
  }
  catch {
    return false
  }
}

/** YouTube IFrame command. Quality lock stops the player climbing to 1080/4K. */
export function youtubeCommand(func: string, args: unknown[] = []) {
  return JSON.stringify({ event: 'command', func, args })
}

function youtubePayload(data: unknown): { event?: string, info?: { playerState?: number } } | null {
  let payload = data
  if (typeof payload === 'string') {
    try {
      payload = JSON.parse(payload)
    }
    catch {
      return null
    }
  }
  if (!payload || typeof payload !== 'object')
    return null
  return payload as { event?: string, info?: { playerState?: number } }
}

export function youtubePlaying(data: unknown): boolean {
  return youtubePayload(data)?.info?.playerState === 1
}

/** Playlist ended — loop failed or YouTube ignored `loop=1`. */
export function youtubeEnded(data: unknown): boolean {
  return youtubePayload(data)?.info?.playerState === 0
}

/** Embed blocked, missing, or geo-restricted — try the next TMDB key. */
export function youtubeError(data: unknown): boolean {
  return youtubePayload(data)?.event === 'onError'
}
