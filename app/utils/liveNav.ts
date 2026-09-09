/**
 * Live TV back targets.
 *
 * `router.back()` is wrong here: play pushes `/watch`, a failed or
 * half-mounted player stays in history, and Back from the browse page
 * lands on that blank screen — which is how the section felt stuck.
 */
export function liveTvBackPath(path: string): string {
  if (/\/live-tv\/premium\/watch/.test(path))
    return '/live-tv/premium'
  if (/\/live-tv\/watch/.test(path))
    return '/live-tv/free'
  if (/\/live-tv\/free\//.test(path))
    return '/live-tv/free'
  if (/\/live-tv\/premium\//.test(path))
    return '/live-tv/premium'
  // The hub itself. `router.back()` from here walks a stack of replaced
  // browse/player entries and never reaches Home.
  if (/\/live-tv\/?$/.test(path))
    return '/'
  return '/live-tv'
}

/** Keep `?from=` only when it is a browse path, never another player. */
export function liveTvFrom(from: string, fallback: string): string {
  const v = from.trim()
  if (!v.startsWith('/') || v.includes('/watch') || v.length > 400)
    return fallback
  return v
}

export interface LivePlayChannel {
  id: string
  name: string
  logoUrl?: string | null
  streamUrl?: string | null
  userAgent?: string | null
  referer?: string | null
}

export interface LivePlay {
  id: string
  title: string
  logo: string
  sourceId: string
  streamUrl: string
  userAgent?: string | null
  referer?: string | null
  zapList: LivePlayChannel[]
}

const PLAY = 'rivulet.livePlay'
let staged: LivePlay | null = null

export function saveLivePlay(play: LivePlay): void {
  staged = play
  try {
    sessionStorage.setItem(PLAY, JSON.stringify(play))
  }
  catch { /* quota or no window — memory still has it for this tab */ }
}

export function readLivePlay(): LivePlay | null {
  if (staged?.id)
    return staged
  try {
    const raw = sessionStorage.getItem(PLAY)
    if (!raw)
      return null
    const parsed = JSON.parse(raw) as LivePlay
    if (!parsed?.id)
      return null
    staged = parsed
    return parsed
  }
  catch {
    return null
  }
}
