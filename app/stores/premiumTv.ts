import type {
  CatalogState,
  CategoryCount,
  EpgProgram,
  IPTVCategory,
  IPTVChannel,
  PremiumAccount,
  SyncReport,
} from '~/types/premium'
import { defineStore } from 'pinia'
import { computed, ref, shallowRef, triggerRef, watch } from 'vue'
import { premiumApi, PremiumApiError, setAuthToken } from '~/utils/premiumTv'

/**
 * Premium TV store.
 *
 * Everything on screen in Premium TV reads from here, and nothing here
 * talks to a provider: the only I/O is `premiumApi`, which is HTTP to the
 * Rust side on loopback. That boundary is the whole design — the UI never
 * sees a provider's response shape, a credential, or an upstream URL.
 *
 * Two state machines live in this file and they are deliberately separate.
 * `connection` is about the *provider* (is one configured, is its catalog
 * on disk); `player` is about the *stream* (is it loading, playing,
 * reconnecting). Collapsing them is how the old build ended up unable to
 * say "connected, but this one channel is dead".
 */

/** Channels per page. Sixty fills the largest grid twice over. */
const PAGE_SIZE = 60

/** Search is typed a character at a time; the query is not cheap. */
const SEARCH_DEBOUNCE_MS = 300

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

/**
 * The player's state, as one value.
 *
 * The spec's rule, and it is a real one: the previous build tracked this
 * with `loading`, `playing`, `error` and `retrying` booleans, which can
 * express "loading and playing and errored" and did.
 */
export type PremiumPlayerState
  = | 'idle'
    | 'loading'
    | 'playing'
    | 'paused'
    | 'buffering'
    | 'reconnecting'
    | 'error'
    | 'ended'

/** Which list the browser is showing. */
export type PremiumView = 'all' | 'favorites' | 'recent' | 'category'

/** What a card needs to draw its now/next line. */
export interface NowNext {
  now: EpgProgram | null
  next: EpgProgram | null
}

/** One entry in the zap list the player walks with channel up/down. */
export interface ZapChannel {
  id: string
  name: string
  logoUrl?: string | null
}

function message(e: unknown): string {
  // `PremiumApiError`'s message comes from the Rust side, whose error
  // Display impls are contractually free of credentials and are written
  // for a user to read. Anything else is a fetch failure against
  // loopback, which means the API is not up — and the raw text for that
  // ("Failed to fetch") tells a user nothing.
  if (e instanceof PremiumApiError)
    return e.message
  return $t('Premium TV is not responding. Try restarting the app.')
}

export const usePremiumTvStore = defineStore('premiumTv', () => {
  // ── The provider connection ──────────────────────────────────

  const account = shallowRef<PremiumAccount | null>(null)
  const catalog = shallowRef<CatalogState | null>(null)
  const connection = ref<'idle' | 'loading' | 'ready' | 'error'>('idle')
  const error = ref('')
  const connected = computed(() => account.value !== null)

  /**
   * True while the catalog is importing and there is nothing to show yet.
   * An empty grid under this reads as "still importing", which is a
   * different screen from "this provider has no channels".
   */
  const importing = computed(() =>
    !!catalog.value && catalog.value.syncing && catalog.value.channels === 0,
  )

  /** The last import's counts, for the connect screen's confirmation. */
  const lastReport = shallowRef<SyncReport | null>(null)

  // ── Browsing ─────────────────────────────────────────────────

  const categories = ref<IPTVCategory[]>([])
  const categoryCounts = ref<CategoryCount[]>([])
  const channels = shallowRef<IPTVChannel[]>([])
  const total = ref(0)
  const nextCursor = ref<string | null>(null)
  const listLoading = ref(false)

  const view = ref<PremiumView>('all')
  const selectedCategory = ref('')
  const searchQuery = ref('')
  const searchDebounced = ref('')

  /**
   * Both guards are needed and they do different jobs. The abort stops
   * the *network* work of a superseded query; the id stops a response
   * that was already in flight when the abort landed from writing itself
   * into the list. A fast category→search→category sequence produces
   * both situations.
   */
  let listController: AbortController | null = null
  let listRequestId = 0

  let searchTimer: ReturnType<typeof setTimeout> | null = null
  watch(searchQuery, v => {
    if (searchTimer)
      clearTimeout(searchTimer)
    searchTimer = setTimeout(() => {
      searchDebounced.value = v.trim()
    }, SEARCH_DEBOUNCE_MS)
  })
  watch(searchDebounced, () => {
    void loadChannels({ reset: true })
  })

  /** What the player walks with channel up/down: the list as displayed. */
  const zapList = computed<ZapChannel[]>(() =>
    channels.value.map(c => ({ id: c.id, name: c.name, logoUrl: c.logoUrl })),
  )

  // ── EPG ──────────────────────────────────────────────────────

  /**
   * Now/next per channel, filled by the batch endpoint as rows scroll
   * into view. A `Map` in a `ref` with an explicit `triggerRef`: sixty
   * cards reading a deep-reactive `Map` re-proxy every entry on every
   * read, which showed up on a TV as a stutter per scroll frame.
   */
  const nowNextCache = ref<Map<string, NowNext>>(new Map())
  /** Ids already asked about, so a re-scroll is not a re-fetch. */
  const nowNextAsked = new Set<string>()

  /** The fuller guide for one channel — what the watch page's panel shows. */
  const guideCache = ref<Map<string, EpgProgram[]>>(new Map())

  // ── Favourites / recent ──────────────────────────────────────

  const favoriteIds = ref<Set<string>>(new Set())
  const recent = shallowRef<IPTVChannel[]>([])

  // ── The player ───────────────────────────────────────────────

  const player = ref<PremiumPlayerState>('idle')
  const playerError = ref('')
  const reconnectAttempt = ref(0)

  // ── Connection lifecycle ─────────────────────────────────────

  /**
   * Re-ask the provider for the account, for its live connection count.
   *
   * Called after a playback failure, not on a timer: it is one HTTP
   * round trip to the panel and its answer is only interesting when
   * something went wrong. Failures are swallowed on purpose — this is a
   * diagnostic, and the message it improves has to be shown either way.
   */
  async function probeAccount(): Promise<PremiumAccount | null> {
    try {
      const fresh = await premiumApi.account()
      // Merged, not replaced: `/account` blanks the host and username on
      // purpose (they have no use on this side and a pasted bug report
      // must not carry them), and the strip in the browser header is
      // still naming the account the user typed.
      const merged: PremiumAccount = {
        ...account.value,
        ...fresh,
        serverUrl: account.value?.serverUrl ?? fresh.serverUrl,
        username: account.value?.username ?? fresh.username,
      }
      account.value = merged
      return merged
    }
    catch {
      return null
    }
  }

  /**
   * Is the account out of simultaneous connections right now?
   *
   * `null` when the provider does not say (an M3U playlist has no such
   * concept), which is different from "no" and is why this is not a
   * boolean.
   */
  const atConnectionLimit = computed<boolean | null>(() => {
    const a = account.value
    if (!a || a.activeConnections == null || a.maxConnections == null || a.maxConnections <= 0)
      return null
    return a.activeConnections >= a.maxConnections
  })

  async function loadStatus(): Promise<void> {
    connection.value = 'loading'
    error.value = ''
    try {
      const s = await premiumApi.status()
      account.value = s.account
      catalog.value = s.catalog
      connection.value = 'ready'
    }
    catch (e) {
      account.value = null
      catalog.value = null
      connection.value = 'error'
      error.value = message(e)
    }
  }

  /**
   * What a page load calls: status, and — when a provider is connected —
   * the sidebar counts, the favourite ids and the recent list.
   *
   * Deliberately not the channel list: which list to show is the page's
   * decision, and a category page would otherwise fetch "all channels"
   * and then immediately throw it away.
   */
  async function ensureLoaded(): Promise<void> {
    if (connection.value === 'idle' || connection.value === 'error')
      await loadStatus()
    if (!connected.value)
      return
    await Promise.all([
      categoryCounts.value.length === 0 ? loadCategoryCounts() : Promise.resolve(),
      favoriteIds.value.size === 0 ? loadFavorites() : Promise.resolve(),
      recent.value.length === 0 ? loadRecent() : Promise.resolve(),
    ])
  }

  async function connectXtream(serverUrl: string, username: string, password: string): Promise<void> {
    connection.value = 'loading'
    error.value = ''
    try {
      const res = await premiumApi.connect({ serverUrl, username, password, accountName: username })
      account.value = res.account
      lastReport.value = res.report
      connection.value = 'ready'
      await afterConnect()
    }
    catch (e) {
      connection.value = 'error'
      error.value = message(e)
      throw e
    }
  }

  async function connectM3u(m3uUrl: string, accountName?: string): Promise<void> {
    connection.value = 'loading'
    error.value = ''
    try {
      const res = await premiumApi.connect({ m3uUrl, accountName })
      account.value = res.account
      lastReport.value = res.report
      connection.value = 'ready'
      await afterConnect()
    }
    catch (e) {
      connection.value = 'error'
      error.value = message(e)
      throw e
    }
  }

  /** Fill the sidebar and the first grid off the catalog that just landed. */
  async function afterConnect(): Promise<void> {
    resetBrowsing()
    await Promise.all([loadStatus(), loadCategoryCounts()])
    await loadChannels({ reset: true })
  }

  async function disconnect(): Promise<void> {
    try {
      await premiumApi.disconnect()
    }
    catch { /* the row is gone either way; the UI must not stay stuck */ }
    account.value = null
    catalog.value = null
    lastReport.value = null
    // The bearer token authorizes this process's API, not the provider —
    // but a disconnect is the one moment where dropping it costs nothing
    // and a stale copy in `sessionStorage` buys nothing either.
    setAuthToken(null)
    resetBrowsing()
    favoriteIds.value = new Set()
    recent.value = []
    categoryCounts.value = []
    categories.value = []
  }

  /** Re-import. `force` ignores the TTLs, which is what *Refresh* means. */
  async function refresh(force = false): Promise<void> {
    try {
      lastReport.value = await premiumApi.refresh(force)
      nowNextCache.value = new Map()
      nowNextAsked.clear()
      guideCache.value = new Map()
      await Promise.all([loadStatus(), loadCategoryCounts()])
      await loadChannels({ reset: true })
    }
    catch (e) {
      // `AlreadySyncing` is a 409 and is not a failure: an import the user
      // double-clicked is still running, and saying so is the answer.
      error.value = message(e)
    }
  }

  function resetBrowsing(): void {
    listRequestId++
    if (listController)
      listController.abort()
    listController = null
    channels.value = []
    total.value = 0
    nextCursor.value = null
    listLoading.value = false
    view.value = 'all'
    selectedCategory.value = ''
    searchQuery.value = ''
    searchDebounced.value = ''
  }

  // ── Catalog reads ────────────────────────────────────────────

  async function loadCategories(): Promise<void> {
    try {
      categories.value = await premiumApi.categories()
    }
    catch (e) {
      error.value = message(e)
    }
  }

  async function loadCategoryCounts(): Promise<void> {
    try {
      categoryCounts.value = await premiumApi.categoryCounts()
    }
    catch (e) {
      error.value = message(e)
    }
  }

  async function loadChannels({ reset }: { reset: boolean } = { reset: true }): Promise<void> {
    if (!connected.value)
      return
    if (reset) {
      if (listController)
        listController.abort()
      channels.value = []
      total.value = 0
      nextCursor.value = null
    }
    else if (listLoading.value || !nextCursor.value) {
      return
    }

    const controller = new AbortController()
    listController = controller
    const reqId = ++listRequestId
    listLoading.value = true

    try {
      // Recently watched is its own endpoint and is not paginated — it is
      // twenty rows kept in visit order, and re-sorting it through the
      // channel query would lose exactly the ordering it exists for. Its
      // search is client-side for the same reason: twenty items.
      if (view.value === 'recent') {
        const items = await premiumApi.recent()
        if (reqId !== listRequestId)
          return
        recent.value = items
        const q = searchDebounced.value.toLowerCase()
        channels.value = q ? items.filter(c => c.name.toLowerCase().includes(q)) : items
        total.value = channels.value.length
        nextCursor.value = null
        absorbFavorites(items)
        return
      }

      const page = await premiumApi.channels({
        cursor: reset ? undefined : (nextCursor.value ?? undefined),
        category: view.value === 'category' ? (selectedCategory.value || undefined) : undefined,
        search: searchDebounced.value || undefined,
        favoritesOnly: view.value === 'favorites' || undefined,
        limit: PAGE_SIZE,
        signal: controller.signal,
      })
      if (reqId !== listRequestId)
        return
      channels.value = reset ? page.items : [...channels.value, ...page.items]
      total.value = page.total
      nextCursor.value = page.nextCursor
      // The server answers `isFavorite` on every channel, so the star
      // state is right on a fresh page without a second request.
      absorbFavorites(page.items)
    }
    catch (e) {
      if (reqId !== listRequestId)
        return
      if (e instanceof DOMException && e.name === 'AbortError')
        return
      error.value = message(e)
    }
    finally {
      if (reqId === listRequestId)
        listLoading.value = false
    }
  }

  function loadMore(): Promise<void> {
    return loadChannels({ reset: false })
  }

  const hasMore = computed(() => nextCursor.value !== null)

  // ── EPG ──────────────────────────────────────────────────────

  /**
   * Fetch now/next for the channels currently on screen.
   *
   * Ids already asked about are dropped, so scrolling back up costs
   * nothing. A channel with no guide is cached as `{now: null, next:
   * null}` for the same reason — the absence is the answer, and asking
   * again on every scroll would turn an EPG-less provider into a request
   * storm.
   */
  async function loadNowNext(ids: string[]): Promise<void> {
    const wanted = ids.filter(id => id && !nowNextAsked.has(id))
    if (wanted.length === 0)
      return
    for (const id of wanted)
      nowNextAsked.add(id)
    try {
      const programs = await premiumApi.epgNowNext(wanted)
      const map = nowNextCache.value
      for (const id of wanted)
        map.set(id, { now: null, next: null })
      const now = Math.floor(Date.now() / 1000)
      for (const p of programs) {
        const slot = map.get(p.channelId) ?? { now: null, next: null }
        const running = p.start <= now && (p.stop == null || p.stop > now)
        if (running)
          slot.now = p
        else if (p.start > now && (!slot.next || p.start < slot.next.start))
          slot.next = p
        map.set(p.channelId, slot)
      }
      triggerRef(nowNextCache)
    }
    catch {
      // A missing guide must never break a grid. The ids stay marked as
      // asked so a scroll does not retry in a loop; a manual refresh
      // clears the cache and tries again.
    }
  }

  function nowNext(channelId: string): NowNext {
    return nowNextCache.value.get(channelId) ?? { now: null, next: null }
  }

  /** The fuller guide for one channel. Cached per channel for the session. */
  async function loadGuide(channelId: string): Promise<EpgProgram[]> {
    const cached = guideCache.value.get(channelId)
    if (cached)
      return cached
    try {
      const programs = await premiumApi.epg(channelId, 12)
      guideCache.value.set(channelId, programs)
      triggerRef(guideCache)
      return programs
    }
    catch {
      // Cache the empty answer: the watch page renders "no guide", and a
      // provider without one must not be re-asked on every zap back.
      guideCache.value.set(channelId, [])
      triggerRef(guideCache)
      return []
    }
  }

  function guide(channelId: string): EpgProgram[] {
    return guideCache.value.get(channelId) ?? []
  }

  // ── Favourites / recent ──────────────────────────────────────

  function absorbFavorites(items: IPTVChannel[]): void {
    let changed = false
    const next = new Set(favoriteIds.value)
    for (const c of items) {
      if (c.isFavorite && !next.has(c.id)) {
        next.add(c.id)
        changed = true
      }
      else if (c.isFavorite === false && next.has(c.id)) {
        next.delete(c.id)
        changed = true
      }
    }
    if (changed)
      favoriteIds.value = next
  }

  function isFavorite(ch: IPTVChannel | string): boolean {
    return favoriteIds.value.has(typeof ch === 'string' ? ch : ch.id)
  }

  /**
   * Toggle, then take the server's answer rather than assuming the flip
   * landed. Two windows on the same catalog would otherwise drift, and
   * the star is the one control where being wrong is invisible until the
   * user looks for the channel in Favourites and it is not there.
   */
  async function toggleFavorite(ch: IPTVChannel | string): Promise<void> {
    const id = typeof ch === 'string' ? ch : ch.id
    try {
      const { isFavorite: state } = await premiumApi.toggleFavorite(id)
      const next = new Set(favoriteIds.value)
      if (state)
        next.add(id)
      else
        next.delete(id)
      favoriteIds.value = next
      // Favourites is a filtered query, so a channel un-starred while
      // that view is open has to leave the list it is in.
      if (view.value === 'favorites' && !state)
        channels.value = channels.value.filter(c => c.id !== id)
    }
    catch (e) {
      error.value = message(e)
    }
  }

  async function loadFavorites(): Promise<void> {
    try {
      const items = await premiumApi.favorites()
      favoriteIds.value = new Set(items.map(c => c.id))
    }
    catch { /* an empty set is the safe default: no stars, not a broken page */ }
  }

  async function addRecent(channelId: string): Promise<void> {
    try {
      await premiumApi.addRecent(channelId)
    }
    catch { /* a lost history entry is not worth an error on screen */ }
  }

  async function loadRecent(): Promise<void> {
    try {
      recent.value = await premiumApi.recent()
    }
    catch { /* */ }
  }

  // ── View selection ───────────────────────────────────────────

  function setView(next: PremiumView): void {
    if (next !== 'category')
      selectedCategory.value = ''
    view.value = next
    void loadChannels({ reset: true })
  }

  function setCategory(name: string): void {
    selectedCategory.value = name
    view.value = name ? 'category' : 'all'
    void loadChannels({ reset: true })
  }

  function clearFilters(): void {
    selectedCategory.value = ''
    view.value = 'all'
    searchQuery.value = ''
    searchDebounced.value = ''
    void loadChannels({ reset: true })
  }

  // ── The player state machine ─────────────────────────────────

  function setPlayer(state: PremiumPlayerState, reason = ''): void {
    player.value = state
    playerError.value = state === 'error' ? reason : ''
    if (state === 'playing' || state === 'paused')
      reconnectAttempt.value = 0
  }

  function resetPlayer(): void {
    player.value = 'idle'
    playerError.value = ''
    reconnectAttempt.value = 0
  }

  /**
   * Ask for the next reconnect. Returns the delay to wait, or `null` when
   * the attempts are spent — at which point the caller moves to `error`
   * and says so once, rather than retrying forever.
   */
  function nextReconnect(): number | null {
    if (reconnectAttempt.value >= MAX_RECONNECT_ATTEMPTS)
      return null
    reconnectAttempt.value += 1
    player.value = 'reconnecting'
    playerError.value = ''
    return reconnectDelayMs(reconnectAttempt.value)
  }

  return {
    // connection
    account,
    probeAccount,
    atConnectionLimit,
    catalog,
    connection,
    connected,
    importing,
    error,
    lastReport,
    loadStatus,
    ensureLoaded,
    connectXtream,
    connectM3u,
    disconnect,
    refresh,
    // browsing
    categories,
    categoryCounts,
    channels,
    total,
    hasMore,
    listLoading,
    view,
    selectedCategory,
    searchQuery,
    searchDebounced,
    zapList,
    loadCategories,
    loadCategoryCounts,
    loadChannels,
    loadMore,
    setView,
    setCategory,
    clearFilters,
    // epg
    loadNowNext,
    nowNext,
    loadGuide,
    guide,
    // favourites / recent
    favoriteIds,
    recent,
    isFavorite,
    toggleFavorite,
    loadFavorites,
    addRecent,
    loadRecent,
    // player
    player,
    playerError,
    reconnectAttempt,
    setPlayer,
    resetPlayer,
    nextReconnect,
  }
})
