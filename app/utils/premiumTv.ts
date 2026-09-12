import type {
  CategoryCount,
  ConnectResult,
  EpgProgram,
  IPTVCategory,
  IPTVChannel,
  IPTVChannelPage,
  PlaybackSource,
  PremiumAccount,
  PremiumDashboard,
  PremiumSeriesDetail,
  PremiumSeriesItem,
  PremiumStatus,
  PremiumVodItem,
  SyncReport,
  VodCategory,
  VodPage,
} from '~/types/premium'
import { invoke, isTauri } from '@tauri-apps/api/core'

// ── HTTP client ──────────────────────────────────────────────

/**
 * The loopback address the Rust side binds (`src-tauri/src/api/mod.rs`'s
 * `ADDR`). Absolute rather than relative because the page's own origin is
 * `tauri://localhost`, which has no server behind it.
 */
const API_BASE = 'http://127.0.0.1:3032'

/** How long any one IPC call may take before it is treated as unanswered. */
const IPC_TIMEOUT_MS = 8_000

/**
 * A timeout, told apart from a caller's own cancel.
 *
 * `usePlaybackSource` drops `AbortError` on the floor by design — a zap that
 * superseded an in-flight request is not a failure worth showing — so a
 * timeout must not arrive wearing that name or it would be swallowed too.
 */
function ipcTimedOut(): DOMException {
  return new DOMException('Premium TV did not answer.', 'TimeoutError')
}

/**
 * Bound a promise that has no timeout of its own.
 *
 * `invoke` is exactly that. `premium_api_token` is a *synchronous* Tauri
 * command that reads the OS keychain, so a stall there never returns — and
 * `mintToken` below caches its in-flight promise, which means one wedged call
 * wedged every request made after it for the life of the process. No fetch
 * timeout could help: the hang happened before the fetch was ever reached.
 * That is what left the Premium settings page spinning with disabled buttons
 * and no error anywhere on it.
 */
async function bounded<T>(work: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined
  try {
    return await Promise.race([
      work,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(ipcTimedOut()), ms)
      }),
    ])
  }
  finally {
    clearTimeout(timer)
  }
}

/**
 * Channel logos, on an origin of their own.
 *
 * A browser opens about six connections per origin. The grid asks for logos by
 * the hundred and each holds its connection until the upstream answers, so a
 * grid that was scrolling used every connection the API had and `/status` could
 * not get one — blank tiles on Free TV and a Premium settings spinner that never
 * stopped were one bug, seen from two ends. See `LOGO_ADDR` in
 * `src-tauri/src/api/mod.rs`; if that port is taken the images fail fast and the
 * cards fall back to initials, which is a duller grid but not a hung one.
 */
const LOGO_BASE = 'http://127.0.0.1:3033'

/**
 * Proxy a channel logo URL through the local API to avoid mixed-content
 * blocking on Android, where the webview origin is `https://tauri.localhost`
 * and `http://` image URLs from the provider are silently rejected.
 *
 * Pass the original provider URL; returns a `/api/premium-tv/proxy/image`
 * path that the local server will fetch and relay with proper headers.
 * Returns the original URL unchanged if it is already on localhost (e.g.
 * dev server) or is falsy.
 */
export function proxyLogo(url: string | null | undefined): string {
  if (!url)
    return ''
  // Already local — don't double-proxy
  if (url.startsWith('http://127.') || url.startsWith('http://localhost') || url.startsWith('/'))
    return url
  return `${LOGO_BASE}/api/premium-tv/proxy/image?url=${encodeURIComponent(url)}`
}

let token: string | null = null

/** The in-flight mint, so ten parallel requests share one IPC round trip. */
let minting: Promise<string | null> | null = null

/**
 * Cache the bearer token in `sessionStorage` and not `localStorage`.
 *
 * It is a one-hour credential for a socket every process on the machine
 * can reach, so it should not outlive the window that minted it — and
 * `app/utils/backup.ts` carries every `rivulet.` key in `localStorage`,
 * which would put it in an exported settings file.
 */
export function setAuthToken(t: string | null): void {
  token = t
  try {
    if (t)
      sessionStorage.setItem('rivulet.premiumApiToken', t)
    else
      sessionStorage.removeItem('rivulet.premiumApiToken')
  }
  catch { /* private mode / quota */ }
}

export function getAuthToken(): string | null {
  if (token)
    return token
  try {
    token = sessionStorage.getItem('rivulet.premiumApiToken')
  }
  catch { /* */ }
  return token
}

/**
 * Mint a token over Tauri IPC.
 *
 * It has to be IPC and cannot be an HTTP route: a route that handed out
 * the token authorizing its own routes would authorize nothing. IPC is
 * reachable only from this app's own webview, which is exactly the
 * boundary the token is standing in for.
 *
 * `null` in a plain browser (`bun run dev`), where there is no Rust side
 * to ask. Premium TV is unavailable there and the caller renders its
 * disconnected state rather than throwing.
 */
async function mintToken(): Promise<string | null> {
  if (!isTauri())
    return null
  if (minting)
    return minting
  minting = (async () => {
    try {
      const res = await bounded(
        invoke<{ token: string, expiresAt: number }>('premium_api_token'),
        IPC_TIMEOUT_MS,
      )
      setAuthToken(res.token)
      return res.token
    }
    catch {
      setAuthToken(null)
      return null
    }
    finally {
      minting = null
    }
  })()
  return minting
}

/**
 * Push the local subscription state to the API's gate.
 *
 * Called at boot and on every subscription change. The gate defaults to
 * denied, so a build that never calls this has no Premium TV at all —
 * which is the right failure direction.
 *
 * `expiresAtMs` distinguishes two things the frontend's `0` cannot:
 * `null` is a plan with no expiry, and the settings store's `0` means
 * "not subscribed". So a falsy timestamp is sent as `null` only when the
 * tier is premium, and the tier is what closes the gate otherwise.
 */
export async function pushEntitlement(tier: string, expiresAtMs: number | null): Promise<void> {
  if (!isTauri())
    return
  await bounded(
    invoke('premium_set_entitlement', {
      tier,
      expiresAtMs: expiresAtMs && expiresAtMs > 0 ? expiresAtMs : null,
    }),
    IPC_TIMEOUT_MS,
  )
}

class PremiumApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message)
    this.name = 'PremiumApiError'
  }
}

interface RequestOptions {
  body?: unknown
  signal?: AbortSignal
  /** Internal: set while replaying a request after a fresh mint. */
  retried?: boolean
}

/** How long any one API call may take before it is treated as unanswered. */
const REQUEST_TIMEOUT_MS = 20_000

/**
 * A timeout, told apart from a caller's own cancel.
 *
 * `usePlaybackSource` drops `AbortError` on the floor by design — a zap that
 * superseded an in-flight request is not a failure worth showing — so a
 * timeout must not arrive wearing that name or it would be swallowed too.
 */
function timedOut(): DOMException {
  return new DOMException('The Premium TV API did not answer.', 'TimeoutError')
}

async function request<T>(
  method: 'GET' | 'POST' | 'DELETE',
  path: string,
  opts: RequestOptions = {},
): Promise<T> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  const t = getAuthToken() ?? await mintToken()
  if (t)
    headers.Authorization = `Bearer ${t}`

  // Nothing on loopback may hang forever. `fetch` has no timeout of its own,
  // and the API shares this process — so a request that never settles means it
  // is not listening yet, which on Android is an ordinary cold start. Without a
  // ceiling the Premium TV page sat on its spinner for good: the `catch` that
  // would have shown an error was never reached, because nothing ever rejected.
  const ctrl = new AbortController()
  const timer = setTimeout(() => ctrl.abort(timedOut()), REQUEST_TIMEOUT_MS)
  const relay = () => ctrl.abort(opts.signal?.reason)
  if (opts.signal?.aborted)
    ctrl.abort(opts.signal.reason)
  else
    opts.signal?.addEventListener('abort', relay, { once: true })

  let resp: Response
  try {
    resp = await fetch(`${API_BASE}${path}`, {
      method,
      headers,
      body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
      signal: ctrl.signal,
    })
  }
  finally {
    clearTimeout(timer)
    opts.signal?.removeEventListener('abort', relay)
  }

  if (!resp.ok) {
    // A 401 on a token we had is the expected end of a long session: the
    // JWT lasts an hour and the app can be left open for days. Mint once
    // and replay — but only once, or a genuinely rejected key becomes an
    // infinite loop against the keychain.
    if (resp.status === 401 && !opts.retried) {
      setAuthToken(null)
      const fresh = await mintToken()
      if (fresh)
        return request<T>(method, path, { ...opts, retried: true })
    }
    let code = 'UNKNOWN'
    let message = resp.statusText
    try {
      const err = await resp.json() as { code?: string, message?: string }
      code = err.code ?? code
      message = err.message ?? message
    }
    catch { /* not JSON */ }
    throw new PremiumApiError(resp.status, code, message)
  }

  if (resp.status === 204)
    return undefined as T
  return await resp.json() as T
}

// ── API surface ──────────────────────────────────────────────

export const premiumApi = {
  /** Lightweight check that the API server is reachable. No auth. */
  health(): Promise<string> {
    return fetch(`${API_BASE}/api/premium-tv/health`).then(r => r.text())
  },

  /** Who is connected and how fresh their catalog is. */
  status(): Promise<PremiumStatus> {
    return request('GET', '/api/premium-tv/status')
  },

  connect(body: {
    serverUrl?: string
    username?: string
    password?: string
    m3uUrl?: string
    accountName?: string
  }): Promise<ConnectResult> {
    return request('POST', '/api/premium-tv/connect', { body })
  },

  /**
   * Ask the provider itself who we are, rather than reading the row the
   * last import wrote. The only field worth a round trip is the live
   * connection count — `activeConnections` of `maxConnections` is what
   * turns "this channel failed" into "your other device is watching".
   */
  account(): Promise<PremiumAccount> {
    return request('GET', '/api/premium-tv/account')
  },

  disconnect(): Promise<void> {
    return request<void>('POST', '/api/premium-tv/disconnect')
  },

  /**
   * Re-import what has gone stale. `force` re-imports regardless of age,
   * which is what the *Refresh* button means; the default respects the
   * TTLs and is what a page load wants.
   */
  refresh(force = false): Promise<SyncReport> {
    return request('POST', `/api/premium-tv/refresh${force ? '?force=true' : ''}`)
  },

  dashboard(): Promise<PremiumDashboard> {
    return request('GET', '/api/premium-tv/dashboard')
  },

  /** The provider's declared group list, empty groups included. */
  categories(): Promise<IPTVCategory[]> {
    return request('GET', '/api/premium-tv/categories')
  },

  /**
   * Alphabetical groups that actually have channels, with counts. What a
   * sidebar renders — `/categories` includes a provider's empty groups
   * and `/dashboard` orders by size, and neither is navigable.
   */
  categoryCounts(args: { hideAdult?: boolean } = {}): Promise<CategoryCount[]> {
    const params = new URLSearchParams()
    if (args.hideAdult)
      params.set('hideAdult', 'true')
    const qs = params.toString()
    return request('GET', `/api/premium-tv/categories/counts${qs ? `?${qs}` : ''}`)
  },

  channels(args: {
    cursor?: string
    category?: string
    country?: string
    search?: string
    favoritesOnly?: boolean
    hideAdult?: boolean
    limit?: number
    signal?: AbortSignal
  } = {}): Promise<IPTVChannelPage> {
    const params = new URLSearchParams()
    if (args.cursor)
      params.set('cursor', args.cursor)
    if (args.category)
      params.set('category', args.category)
    if (args.country)
      params.set('country', args.country)
    if (args.search)
      params.set('search', args.search)
    if (args.favoritesOnly)
      params.set('favoritesOnly', 'true')
    if (args.hideAdult)
      params.set('hideAdult', 'true')
    if (args.limit)
      params.set('limit', String(args.limit))
    const qs = params.toString()
    return request('GET', `/api/premium-tv/channels${qs ? `?${qs}` : ''}`, { signal: args.signal })
  },

  channel(id: string): Promise<IPTVChannel> {
    return request('GET', `/api/premium-tv/channels/${encodeURIComponent(id)}`)
  },

  epg(channelId: string, limit = 8): Promise<EpgProgram[]> {
    return request('GET', `/api/premium-tv/channels/${encodeURIComponent(channelId)}/epg?limit=${limit}`)
  },

  /**
   * Now-and-next for a page of channels in one request. Per-card calls
   * would be sixty round trips per page and a visibly staggered grid.
   */
  epgNowNext(channelIds: string[], signal?: AbortSignal): Promise<EpgProgram[]> {
    return request('POST', '/api/premium-tv/epg/now-next', { body: { channelIds }, signal })
  },

  /**
   * Resolve a playable URL. Short-lived by design — the answer is a
   * signed redirector token good for about thirty seconds, so this is
   * called per play and per zap rather than cached.
   */
  play(channelId: string, signal?: AbortSignal): Promise<PlaybackSource> {
    return request('POST', `/api/premium-tv/channels/${encodeURIComponent(channelId)}/play`, { signal })
  },

  favorites(): Promise<IPTVChannel[]> {
    return request('GET', '/api/premium-tv/favorites')
  },

  /** Returns the state the server settled on, which is what the UI must show. */
  toggleFavorite(channelId: string): Promise<{ isFavorite: boolean }> {
    return request('POST', `/api/premium-tv/favorites/${encodeURIComponent(channelId)}`)
  },

  recent(): Promise<IPTVChannel[]> {
    return request('GET', '/api/premium-tv/recent')
  },

  addRecent(channelId: string): Promise<void> {
    return request<void>('POST', '/api/premium-tv/recent', { body: { channelId } })
  },

  clearRecent(): Promise<void> {
    return request<void>('DELETE', '/api/premium-tv/recent')
  },

  /**
   * Find quality variants for a channel — channels with a similar base
   * name but different quality labels (e.g. "Sky Sports HD" and "Sky
   * Sports 4K"). Used by the premium watch page to offer a quality picker.
   */
  qualityVariants(channelId: string, signal?: AbortSignal): Promise<IPTVChannel[]> {
    return request('GET', `/api/premium-tv/channels/${encodeURIComponent(channelId)}/qualities`, { signal })
  },

  /** Xtream movie categories from the panel (not cached locally). */
  vodMovieCategories(): Promise<VodCategory[]> {
    return request('GET', '/api/premium-tv/vod/movies/categories')
  },

  vodSeriesCategories(): Promise<VodCategory[]> {
    return request('GET', '/api/premium-tv/vod/series/categories')
  },

  vodMovies(args: {
    category?: string
    search?: string
    cursor?: string
    hideAdult?: boolean
    limit?: number
    signal?: AbortSignal
  } = {}): Promise<VodPage<PremiumVodItem>> {
    const params = new URLSearchParams()
    if (args.category)
      params.set('category', args.category)
    if (args.search)
      params.set('search', args.search)
    if (args.cursor)
      params.set('cursor', args.cursor)
    if (args.hideAdult)
      params.set('hideAdult', 'true')
    if (args.limit)
      params.set('limit', String(args.limit))
    const qs = params.toString()
    return request('GET', `/api/premium-tv/vod/movies${qs ? `?${qs}` : ''}`, { signal: args.signal })
  },

  vodSeries(args: {
    category?: string
    search?: string
    cursor?: string
    hideAdult?: boolean
    limit?: number
    signal?: AbortSignal
  } = {}): Promise<VodPage<PremiumSeriesItem>> {
    const params = new URLSearchParams()
    if (args.category)
      params.set('category', args.category)
    if (args.search)
      params.set('search', args.search)
    if (args.cursor)
      params.set('cursor', args.cursor)
    if (args.hideAdult)
      params.set('hideAdult', 'true')
    if (args.limit)
      params.set('limit', String(args.limit))
    const qs = params.toString()
    return request('GET', `/api/premium-tv/vod/series${qs ? `?${qs}` : ''}`, { signal: args.signal })
  },

  vodSeriesDetail(id: string): Promise<PremiumSeriesDetail> {
    return request('GET', `/api/premium-tv/vod/series/${encodeURIComponent(id)}`)
  },

  vodPlayMovie(id: string, ext?: string, signal?: AbortSignal): Promise<PlaybackSource> {
    const params = ext ? `?ext=${encodeURIComponent(ext)}` : ''
    return request('POST', `/api/premium-tv/vod/movies/${encodeURIComponent(id)}/play${params}`, { signal })
  },

  vodPlayEpisode(id: string, ext?: string, signal?: AbortSignal): Promise<PlaybackSource> {
    const params = ext ? `?ext=${encodeURIComponent(ext)}` : ''
    return request('POST', `/api/premium-tv/vod/episodes/${encodeURIComponent(id)}/play${params}`, { signal })
  },
}

export { API_BASE, PremiumApiError }
