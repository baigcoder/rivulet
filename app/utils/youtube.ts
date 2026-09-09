import { isTauri } from '@tauri-apps/api/core'
import { platform } from '@tauri-apps/plugin-os'
import { isDesktop } from './platform'

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
 * can play what a YouTube iframe embed would refuse to (error 153). Empty
 * outside desktop Tauri — Android keeps the iframe (no yt-dlp there), and
 * browser dev has no proxy. Muted/loop/autoplay are element attributes, not
 * here.
 *
 * Linux WebKitGTK is excluded: its GStreamer media pipeline wedges when a
 * `<video>` element loads the yt-dlp-resolved stream, freezing the entire
 * detail page. The iframe embed (via the `/youtube-embed` relay) works
 * reliably there thanks to the Chrome UA spoofing in lib.rs, which makes
 * YouTube serve the Chromium player config WebKitGTK can run.
 *
 * Windows is excluded too: WebView2 plays the embed; yt-dlp console windows
 * and a failed native `<video>` (cover never fell back to the iframe) were
 * the Windows bug. macOS stays on the stream path.
 */
export function youtubeStreamSrc(key: string): string {
  if (!isTauri() || !isDesktop())
    return ''
  try {
    if (platform() === 'linux' || platform() === 'windows')
      return ''
  }
  catch {
    // platform() can throw before Tauri is ready — fall through to the stream
  }
  return `${STREAM}?${new URLSearchParams({ v: key })}`
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
