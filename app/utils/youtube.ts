import { isTauri } from '@tauri-apps/api/core'

/** Loopback shim on the IPTV proxy port — see iptv/proxy.rs `/youtube-embed`. */
const RELAY = 'http://127.0.0.1:3031/youtube-embed'

/**
 * Build a trailer iframe src.
 *
 * Tauri loads from `tauri://` on Linux/macOS production, which YouTube
 * rejects — route through loopback HTTP instead.
 *
 * `controls: false` is the cover hero, which is background art and not a
 * player: it hides YouTube's bar, its centre play button, and its
 * fullscreen, caption and annotation furniture. It also changes how the
 * loop is done. `loop` alone is ignored by YouTube unless `playlist`
 * names the same video — and a playlist, even a one-video one, is what
 * makes it draw **previous and next buttons** over the picture. So a
 * hero loops on the ended event instead (the relay, and `onHeroMessage`
 * for the browser build), and only a visible player uses the playlist.
 */
export function youtubeEmbedSrc(
  key: string,
  opts: { mute?: boolean, loop?: boolean, controls?: boolean } = {},
) {
  const controls = opts.controls !== false
  if (isTauri()) {
    const q = new URLSearchParams({ v: key, autoplay: '1' })
    if (opts.mute)
      q.set('mute', '1')
    if (opts.loop)
      q.set('loop', '1')
    if (!controls)
      q.set('controls', '0')
    return `${RELAY}?${q}`
  }
  const q = new URLSearchParams({ autoplay: '1', rel: '0', playsinline: '1', enablejsapi: '1', vq: 'hd720' })
  if (typeof location !== 'undefined')
    q.set('origin', location.origin)
  if (opts.mute)
    q.set('mute', '1')
  if (opts.loop && controls) {
    q.set('loop', '1')
    q.set('playlist', key)
  }
  if (!controls) {
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
 * The player reached the end. A hero loops by seeking back rather than
 * with a playlist, which would put skip buttons on the picture.
 */
export function youtubeEnded(data: unknown): boolean {
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
  return (payload as { info?: { playerState?: number } }).info?.playerState === 0
}

/** Probed once: what a webview can decode does not change while it runs. */
let codecsMissing: boolean | null = null

/**
 * Can this webview decode what a YouTube embed will hand it?
 *
 * Worth asking *before* the embed loads, because the answer is knowable and
 * the alternative is nine seconds of YouTube's own "your browser can't play
 * this video" — which names no cause, no package, and not the Open on YouTube
 * button below it.
 *
 * The case this catches is Linux. WebKitGTK decodes through the *host's*
 * GStreamer (the AppImage deliberately carries none — a bundled core finds no
 * plugins at all, see `scripts/build/linux/appimage.ts`), so a machine without
 * `gst-plugins-good` or `gst-libav` has a webview that supports MSE and can
 * decode nothing through it. YouTube's player then gives up on its own.
 *
 * `false` is "no reason to think it is broken", not "verified working": a
 * browser that answers no to every probe is more likely to be one that
 * declines to answer than one that cannot play video, and calling a working
 * machine broken is the worse of the two mistakes. Only a clear "MSE is here
 * and supports nothing" returns `true`. It follows that this does *not* catch
 * a host whose decoders are fine and whose embed fails for another reason —
 * the 9s timeout in `MediaDetailView` still backs it up.
 */
export function youtubeCodecsMissing(): boolean {
  if (codecsMissing === null)
    codecsMissing = probeCodecsMissing()
  return codecsMissing
}

function probeCodecsMissing(): boolean {
  if (typeof window === 'undefined')
    return false
  const mse = (window as { MediaSource?: { isTypeSupported?: (t: string) => boolean } }).MediaSource
  if (typeof mse?.isTypeSupported !== 'function')
    return false
  // What the embed actually offers, in the order it prefers: H.264 in MP4 is
  // the one that needs `gst-libav`, and VP9/WebM the one that needs
  // `gst-plugins-good`. A host with either can play a trailer.
  const CANDIDATES = [
    'video/mp4; codecs="avc1.42E01E"',
    'video/mp4; codecs="avc1.4D401F"',
    'video/webm; codecs="vp9"',
    'video/webm; codecs="vp8"',
  ]
  for (const type of CANDIDATES) {
    try {
      if (mse.isTypeSupported(type))
        return false
    }
    catch {
      // A probe that throws is not a probe that said no.
      return false
    }
  }
  return true
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
