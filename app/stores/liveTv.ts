import type { EpgProgram, LiveChannel, LiveDashboard, LiveTvSource } from '~/utils/iptv'
import { useLocalStorage } from '@vueuse/core'
import { defineStore } from 'pinia'
import { computed, ref, shallowRef, triggerRef, watch } from 'vue'
import { key } from '~/brand'
import {
  getFreeTvEpg,
  liveActiveSource,
  liveAddRecent,
  liveCancelImport,
  liveChannelEpgBatch,
  liveClearRecent,
  liveDashboard,
  liveFavorites,
  liveGetLiveEpg,
  liveListSources,
  liveQueryChannels,
  liveRecent,
  liveRefreshFreeTv,
  liveRemoveSource,
  liveResolveStream,
  liveSearchChannels,
  liveSetActive,
  liveToggleFavorite,
  proxyFreeStreamUrl,
} from '~/utils/iptv'
import { pool, PROBE_CONCURRENCY, probeStream } from '~/utils/livehealth'

/**
 * Free TV is the only thing in this store. Premium TV (Xtream + user-added
 * M3U) has moved to `app/stores/premiumTv.ts` and a local HTTP API at
 * 127.0.0.1:3032. The `liveTv` store here is the iptv-org public
 * playlist, a passive dataset the app ships with and the user cannot
 * remove. The store is still the right home for the browse, search,
 * favorites, recent, EPG and mini-player surfaces the Free TV page draws.
 */

export type LiveGroup = 'All' | 'Sports' | 'News' | 'Entertainment' | 'Kids' | 'Movies & Series' | 'Music' | 'Documentary' | 'Religious' | 'General'
export type LiveTvSort = 'recommended' | 'az' | 'za' | 'recently_added' | 'recently_watched' | 'favorites'
export type LiveTvViewMode = 'grid' | 'list'
export type LiveTvDensity = 'compact' | 'comfortable'
export type LiveTvTab = 'favorites' | 'recent' | 'live' | 'countries' | 'categories' | 'all'

/**
 * Where the browser is. The same four the Premium browser has, and for
 * the same reason: three fixed views a viewer uses every session, plus
 * whichever group they went looking through. `LiveTvTab` above is the
 * older, wider set the sub-routes still read; a view maps onto the
 * store's filters rather than replacing them.
 */
export type LiveView = 'all' | 'favorites' | 'recent' | 'category'

const DEFAULT_LIMIT = 60

export const useLiveTvStore = defineStore('liveTv', () => {
  // ── Source registry ──────────────────────────────────────────────
  // The single explicit identity. The page key is the source's id, and
  // everything below scopes to it. Free TV is a single fixed source —
  // there is no premium source on the Free TV path.
  const sources = shallowRef<LiveTvSource[]>([])
  const activeSource = ref<LiveTvSource | null>(null)
  const activeSourceId = computed(() => activeSource.value?.id ?? null)
  const activeSourceType = computed<LiveTvSource['kind'] | null>(() => activeSource.value?.kind ?? null)
  const activeSourceStatus = computed(() => activeSource.value?.status ?? null)

  // ── Dashboard (current source) ───────────────────────────────────
  // The bundle the page renders. Counts are pre-aggregated server-side
  // from the `iptv_*_stats` tables, so reading them is constant work
  // regardless of how many channels the source has.
  const dashboard = ref<LiveDashboard | null>(null)
  const countries = computed(() => dashboard.value?.countries ?? [])
  const categories = computed(() => dashboard.value?.categories ?? [])
  const groups = computed(() => dashboard.value?.groups ?? [])
  const totalChannels = computed(() => dashboard.value?.totalChannels ?? 0)
  const dashboardLoading = ref(false)

  // ── Visible page ────────────────────────────────────────────────
  // The result window. 60 channels by default; cursor pagination on
  // scroll. `shallowRef` so adding/removing items doesn't trigger the
  // deep reactive proxy graph the 500K-row design did.
  const visibleChannels = shallowRef<LiveChannel[]>([])
  const visibleTotal = ref(0)
  const nextCursor = ref<string | null>(null)
  const visibleLoading = ref(false)
  let currentRequestId = 0

  // ── UI state (per-source, with a global fallback) ────────────────
  // Two `useLocalStorage` slots per slot: the per-source value and the
  // "default" value used when the user hasn't picked one for this
  // source yet. The wrapper reads both and writes both.
  function perSourceStorage<T extends string>(slot: string, fallback: T) {
    const defaultStorage = useLocalStorage<T>(key(`liveTv.${slot}.default`), fallback, { initOnMounted: true })
    const overrides = useLocalStorage<Record<string, T>>(key(`liveTv.${slot}.perSource`), {}, { initOnMounted: true })
    const value = computed<T>({
      get() {
        const id = activeSourceId.value
        if (id && overrides.value[id] !== undefined)
          return overrides.value[id]
        return defaultStorage.value
      },
      set(v) {
        const id = activeSourceId.value
        if (id) {
          overrides.value = { ...overrides.value, [id]: v }
        }
        defaultStorage.value = v
      },
    })
    return value
  }

  function perSourceBoolStorage(slot: string, fallback: boolean) {
    const defaultStorage = useLocalStorage<boolean>(key(`liveTv.${slot}.default`), fallback)
    const overrides = useLocalStorage<Record<string, boolean>>(key(`liveTv.${slot}.perSource`), {})
    const value = computed<boolean>({
      get() {
        const id = activeSourceId.value
        if (id && overrides.value[id] !== undefined)
          return overrides.value[id]
        return defaultStorage.value
      },
      set(v) {
        const id = activeSourceId.value
        if (id) {
          overrides.value = { ...overrides.value, [id]: v }
        }
        defaultStorage.value = v
      },
    })
    return value
  }

  const selectedCountry = perSourceStorage('country', '' as string)
  const selectedCategory = perSourceStorage('category', '' as string)
  const selectedGroup = perSourceStorage<LiveGroup>('group', 'All')
  const selectedLanguage = perSourceStorage('language', '' as string)
  const sort = perSourceStorage<LiveTvSort>('sort', 'recommended')
  const viewMode = perSourceStorage<LiveTvViewMode>('viewMode', 'grid')
  const density = perSourceStorage<LiveTvDensity>('density', 'comfortable')
  const activeTab = perSourceStorage<LiveTvTab>('tab', 'all')
  const view = perSourceStorage<LiveView>('view', 'all')
  const favoritesOnly = perSourceBoolStorage('favoritesOnly', false)

  // Search is session-only, not persisted.
  const searchQuery = ref('')
  const searchDebounced = ref('')
  let searchTimer: ReturnType<typeof setTimeout> | null = null
  watch(searchQuery, v => {
    if (searchTimer)
      clearTimeout(searchTimer)
    searchTimer = setTimeout(() => {
      searchDebounced.value = v
    }, 350)
  })
  // Search is server-side and cursor-paginated like every other filter.
  // Updating only `searchDebounced` previously changed the input without
  // issuing a new query, which made search appear broken.
  watch(searchDebounced, () => {
    loadVisible({ reset: true })
  })

  // ── EPG ──────────────────────────────────────────────────────────
  const epgCache = ref<Map<string, EpgProgram[]>>(new Map())
  const epgPendingIds = ref<Set<string>>(new Set())
  const epgLoading = ref(false)

  // ── Mini player ──────────────────────────────────────────────────
  const miniChannel = ref<LiveChannel | null>(null)
  const miniStreamUrl = ref<string | null>(null)
  const miniVisible = ref(false)

  // ── Recent channels ──────────────────────────────────────────────
  // Kept as ids; resolved against the dashboard's `recentPreviews`.
  // Per-source: stored server-side by `live_add_recent`. The frontend
  // just keeps the order handy for the route state.
  const recentChannelIds = ref<string[]>([])

  // ── Favorites ────────────────────────────────────────────────────
  const favKeys = ref<Set<string>>(new Set())

  // ── Other UI state ───────────────────────────────────────────────
  const refreshing = ref(false)
  const error = ref('')
  const lastSyncAt = ref<number>(0)
  const m3uImporting = ref(false)
  const m3uProgress = ref({ bytesDownloaded: 0, totalBytes: 0, channels: 0, stage: 'connecting' as string })
  const lastChannel = useLocalStorage(key('liveTv.lastChannel'), '')

  // ── Source actions ───────────────────────────────────────────────

  async function refreshSources(): Promise<void> {
    try {
      sources.value = await liveListSources()
      activeSource.value = await liveActiveSource()
    }
    catch (e) {
      error.value = String(e)
    }
  }

  async function setActiveSource(id: string): Promise<void> {
    // Wipe persisted filter state from the previous source so a fresh
    // import lands on the dashboard's "all" tab, not the previous
    // user's last country/category. The "all" tab is the home view.
    if (activeSourceId.value !== id) {
      selectedCountry.value = ''
      selectedCategory.value = ''
      selectedLanguage.value = ''
      selectedGroup.value = 'All'
      favoritesOnly.value = false
      activeTab.value = 'all'
      searchQuery.value = ''
      sort.value = 'recommended'
    }
    await liveSetActive(id)
    await refreshSources()
    await loadDashboard()
    await loadVisible({ reset: true })
    await loadFavorites()
  }

  async function removeSource(id: string): Promise<void> {
    await liveRemoveSource(id)
    await refreshSources()
    if (activeSourceId.value === id) {
      activeSource.value = sources.value.find(s => s.status === 'active') ?? null
      await loadDashboard()
      await loadVisible({ reset: true })
    }
  }

  // ── Dashboard ────────────────────────────────────────────────────

  async function loadDashboard(): Promise<void> {
    if (!activeSourceId.value) {
      dashboard.value = null
      return
    }
    dashboardLoading.value = true
    try {
      dashboard.value = await liveDashboard(activeSourceId.value)
      recentChannelIds.value = (dashboard.value.recentPreviews ?? []).map(c => c.id)
    }
    catch (e) {
      error.value = String(e)
      dashboard.value = null
    }
    finally {
      dashboardLoading.value = false
    }
  }

  // ── Visible page (the only big-list surface) ────────────────────

  async function loadVisible({ reset }: { reset: boolean } = { reset: true }): Promise<void> {
    const id = activeSourceId.value
    if (!id) {
      visibleChannels.value = []
      visibleTotal.value = 0
      nextCursor.value = null
      return
    }
    if (reset) {
      currentRequestId++
      visibleChannels.value = []
      visibleTotal.value = 0
      nextCursor.value = null
    }
    const reqId = currentRequestId
    visibleLoading.value = true
    try {
      const page = await liveQueryChannels({
        sourceId: id,
        country: selectedCountry.value || undefined,
        category: selectedCategory.value || undefined,
        group: selectedGroup.value !== 'All' ? selectedGroup.value : undefined,
        language: selectedLanguage.value || undefined,
        search: searchDebounced.value || undefined,
        favoritesOnly: !!favoritesOnly.value,
        sort: sort.value,
        cursor: reset ? undefined : (nextCursor.value ?? undefined),
        limit: DEFAULT_LIMIT,
      })
      if (reqId !== currentRequestId)
        return
      visibleChannels.value = reset ? page.items : [...visibleChannels.value, ...page.items]
      visibleTotal.value = page.total
      nextCursor.value = page.nextCursor
    }
    catch (e) {
      if (reqId !== currentRequestId)
        return
      error.value = String(e)
    }
    finally {
      if (reqId === currentRequestId)
        visibleLoading.value = false
    }
  }

  async function loadMore(): Promise<void> {
    if (!nextCursor.value || visibleLoading.value)
      return
    await loadVisible({ reset: false })
  }

  async function searchChannels(query: string): Promise<LiveChannel[]> {
    const id = activeSourceId.value
    if (!id || !query)
      return []
    const page = await liveSearchChannels(id, query, undefined, 30)
    return page.items
  }

  async function loadChannelByCountry(country: string, cursor?: string): Promise<{ items: LiveChannel[], nextCursor: string | null, total: number }> {
    const id = activeSourceId.value
    if (!id)
      return { items: [], nextCursor: null, total: 0 }
    const page = await liveQueryChannels({
      sourceId: id,
      country,
      sort: 'az',
      cursor,
      limit: DEFAULT_LIMIT,
    })
    return { items: page.items, nextCursor: page.nextCursor, total: page.total }
  }

  async function loadChannelByCategory(category: string, cursor?: string): Promise<{ items: LiveChannel[], nextCursor: string | null, total: number }> {
    const id = activeSourceId.value
    if (!id)
      return { items: [], nextCursor: null, total: 0 }
    const page = await liveQueryChannels({
      sourceId: id,
      category,
      sort: 'az',
      cursor,
      limit: DEFAULT_LIMIT,
    })
    return { items: page.items, nextCursor: page.nextCursor, total: page.total }
  }

  /** Make the built-in public playlist the active source before opening Free TV. */
  async function useFreeSource(): Promise<void> {
    await refreshSources()
    if (activeSourceId.value !== 'free:iptv-org') {
      await liveSetActive('free:iptv-org')
      await refreshSources()
    }
  }

  // ── Free TV refresh ─────────────────────────────────────────────

  async function refreshFreeTv(): Promise<void> {
    refreshing.value = true
    try {
      await liveRefreshFreeTv()
      await refreshSources()
      await loadDashboard()
      await loadVisible({ reset: true })
      lastSyncAt.value = Date.now()
    }
    finally {
      refreshing.value = false
    }
  }

  async function cancelImport(): Promise<void> {
    try {
      await liveCancelImport()
    }
    catch { /* ignore */ }
  }

  // ── Favorites / recent ──────────────────────────────────────────

  // Only the id is ever read, so the player page can ask about a channel it
  // only knows by id without inventing the rest of a `LiveChannel`.
  function channelFavKey(ch: Pick<LiveChannel, 'id'>): string {
    return `${activeSourceId.value}:${ch.id}`
  }

  function isFavorite(ch: Pick<LiveChannel, 'id'>): boolean {
    return favKeys.value.has(channelFavKey(ch))
  }

  async function toggleFavorite(ch: Pick<LiveChannel, 'id'>): Promise<void> {
    const id = activeSourceId.value
    if (!id)
      return
    const nowFav = await liveToggleFavorite(id, ch.id)
    const next = new Set(favKeys.value)
    const favKey = channelFavKey(ch)
    if (nowFav)
      next.add(favKey)
    else next.delete(favKey)
    favKeys.value = next
  }

  async function loadFavorites(): Promise<void> {
    const id = activeSourceId.value
    if (!id) {
      favKeys.value = new Set()
      return
    }
    try {
      const favs = await liveFavorites(id, 200)
      // Keyed the same way `channelFavKey` reads them, or nothing loaded ever
      // matches and the star comes back empty on every reload.
      favKeys.value = new Set(favs.map(c => channelFavKey(c)))
    }
    catch { /* ignore */ }
  }

  async function addRecent(channelId: string): Promise<void> {
    const id = activeSourceId.value
    if (!id)
      return
    try {
      await liveAddRecent(id, channelId)
      recentChannelIds.value = [channelId, ...recentChannelIds.value.filter(c => c !== channelId)].slice(0, 20)
    }
    catch { /* ignore */ }
  }

  async function loadRecent(): Promise<LiveChannel[]> {
    const id = activeSourceId.value
    if (!id)
      return []
    try {
      return await liveRecent(id, 20)
    }
    catch {
      return []
    }
  }

  async function clearRecent(): Promise<void> {
    const id = activeSourceId.value
    if (!id)
      return
    try {
      await liveClearRecent(id)
      recentChannelIds.value = []
      if (dashboard.value)
        dashboard.value.recentPreviews = []
    }
    catch { /* ignore */ }
  }

  // ── EPG ─────────────────────────────────────────────────────────

  async function loadEpg(channelId: string): Promise<EpgProgram[]> {
    if (epgCache.value.has(channelId))
      return epgCache.value.get(channelId)!
    if (epgPendingIds.value.has(channelId))
      return []
    const pending = new Set(epgPendingIds.value)
    pending.add(channelId)
    epgPendingIds.value = pending
    try {
      const programs = await liveGetLiveEpg(channelId)
      epgCache.value.set(channelId, programs)
      triggerRef(epgCache)
      return programs
    }
    catch {
      return []
    }
    finally {
      const next = new Set(epgPendingIds.value)
      next.delete(channelId)
      epgPendingIds.value = next
    }
  }

  async function loadEpgBatch(channelIds: string[]): Promise<void> {
    const toLoad = channelIds.filter(id => !epgCache.value.has(id) && !epgPendingIds.value.has(id))
    if (toLoad.length === 0)
      return
    const pending = new Set(epgPendingIds.value)
    for (const id of toLoad) pending.add(id)
    epgPendingIds.value = pending
    try {
      const programs = await liveChannelEpgBatch(toLoad)
      for (const p of programs) {
        const existing = epgCache.value.get(p.channelId) ?? []
        existing.push(p)
        epgCache.value.set(p.channelId, existing)
      }
      triggerRef(epgCache)
    }
    catch { /* EPG unavailable */ }
    finally {
      const cleared = new Set(epgPendingIds.value)
      for (const id of toLoad) cleared.delete(id)
      epgPendingIds.value = cleared
    }
  }

  async function loadFreeEpg(epgId: string): Promise<void> {
    if (!epgId || epgCache.value.has(epgId) || epgPendingIds.value.has(epgId))
      return
    const pending = new Set(epgPendingIds.value)
    pending.add(epgId)
    epgPendingIds.value = pending
    try {
      const programs = await getFreeTvEpg(epgId)
      epgCache.value.set(epgId, programs)
      triggerRef(epgCache)
    }
    catch { /* EPG unavailable */ }
    finally {
      const cleared = new Set(epgPendingIds.value)
      cleared.delete(epgId)
      epgPendingIds.value = cleared
    }
  }

  function getEpg(channelId: string): EpgProgram[] {
    return epgCache.value.get(channelId) ?? []
  }

  // ── Player integration ──────────────────────────────────────────

  async function resolveStream(ch: LiveChannel): Promise<string> {
    const id = activeSourceId.value
    if (!id)
      return ch.streamUrl ?? ''
    const live = await liveResolveStream(id, ch.id)
    return live.streamUrl
  }

  function rememberChannel(channelId: string): void {
    lastChannel.value = channelId
    addRecent(channelId)
  }

  function showMiniPlayer(ch: LiveChannel, url: string): void {
    miniChannel.value = ch
    miniStreamUrl.value = url
    miniVisible.value = true
  }

  function hideMiniPlayer(): void {
    miniVisible.value = false
    miniChannel.value = null
    miniStreamUrl.value = null
  }

  function expandMiniPlayer(): void {
    if (miniChannel.value && miniStreamUrl.value) {
      const ch = miniChannel.value
      hideMiniPlayer()
      navigateTo({
        path: '/live-tv/watch',
        query: { url: miniStreamUrl.value, title: ch.name, logo: ch.logoUrl ?? '', id: ch.id, type: 'live', sourceId: activeSourceId.value },
      })
    }
  }

  // ── Filter setters (drive the visible-page query) ──────────────

  function setCountry(country: string): void {
    selectedCountry.value = country
    searchQuery.value = ''
    searchDebounced.value = ''
    loadVisible({ reset: true })
  }

  function setCategory(category: string): void {
    selectedCategory.value = category
    view.value = 'category'
    searchQuery.value = ''
    searchDebounced.value = ''
    loadVisible({ reset: true })
  }

  /**
   * Move the browser between its four views. A view is not a fifth filter
   * — it is the two filters a viewer actually switches (the group, and
   * "only my favourites") under one name, so the rail, the phone chips and
   * the heading cannot disagree about where they are.
   *
   * `recent` issues no query: it is the dashboard's own last-watched
   * preview list, and a query with every filter cleared would quietly
   * render the whole playlist under a heading that says Recent.
   */
  function setView(next: LiveView): void {
    view.value = next
    if (next !== 'category')
      selectedCategory.value = ''
    favoritesOnly.value = next === 'favorites'
    searchQuery.value = ''
    searchDebounced.value = ''
    if (next !== 'recent')
      loadVisible({ reset: true })
  }

  function setGroup(group: LiveGroup): void {
    selectedGroup.value = group
    searchQuery.value = ''
    searchDebounced.value = ''
    loadVisible({ reset: true })
  }

  function setLanguage(language: string): void {
    selectedLanguage.value = language
    searchQuery.value = ''
    searchDebounced.value = ''
    loadVisible({ reset: true })
  }

  function setSort(s: LiveTvSort): void {
    sort.value = s
    loadVisible({ reset: true })
  }

  function setViewMode(m: LiveTvViewMode): void {
    viewMode.value = m
  }

  function setDensity(d: LiveTvDensity): void {
    density.value = d
  }

  function setActiveTab(tab: LiveTvTab): void {
    activeTab.value = tab
    if (tab !== 'countries')
      selectedCountry.value = ''
    if (tab !== 'categories')
      selectedCategory.value = ''
    loadVisible({ reset: true })
  }

  function toggleFavoritesOnly(): void {
    favoritesOnly.value = !favoritesOnly.value
    loadVisible({ reset: true })
  }

  function clearFilters(): void {
    view.value = 'all'
    selectedCountry.value = ''
    selectedCategory.value = ''
    selectedLanguage.value = ''
    selectedGroup.value = 'All'
    favoritesOnly.value = false
    searchQuery.value = ''
    sort.value = 'recommended'
    loadVisible({ reset: true })
  }

  // ── Channel health ──────────────────────────────────────────────
  // The rules and the reasoning are in `utils/livehealth.ts`; this is
  // where the verdicts are kept. Session-only on purpose: a channel that
  // was dead this morning is a channel worth trying again tonight, and a
  // persisted verdict would hide it for good.

  const offlineIds = ref<Set<string>>(new Set())
  /**
   * Ids already sent to a probe. Not reactive — nothing renders "we are
   * currently checking", and the point of the set is that a scroll back
   * and forth across the same row does not re-probe it.
   */
  const probedIds = new Set<string>()

  function isOffline(ch: LiveChannel): boolean {
    return offlineIds.value.has(ch.id)
  }

  function markOffline(channelId: string): void {
    if (offlineIds.value.has(channelId))
      return
    // A new Set rather than a mutation: the cards read this through a
    // computed and a Set is not deeply reactive.
    offlineIds.value = new Set(offlineIds.value).add(channelId)
    probedIds.add(channelId)
  }

  /**
   * Probe the channels the grid can currently see. Called with the same
   * id batch the EPG hook gets, so "visible" is defined in one place.
   */
  async function probeIds(ids: string[]): Promise<void> {
    const targets: LiveChannel[] = []
    for (const id of ids) {
      if (probedIds.has(id))
        continue
      const ch = visibleChannels.value.find(c => c.id === id)
      const url = ch?.streamUrl
      if (!ch || !url || url === 'undefined' || url === 'null')
        continue
      // Claimed before the await, so the next scroll event does not queue
      // the same channel again while this batch is still in flight.
      probedIds.add(id)
      targets.push(ch)
    }
    if (targets.length === 0)
      return
    await pool(targets, PROBE_CONCURRENCY, async ch => {
      let proxied: string
      try {
        proxied = await proxyFreeStreamUrl(ch.streamUrl!, ch.userAgent ?? undefined, ch.referer ?? undefined)
      }
      catch {
        // No Tauri behind the page (browser dev): there is no proxy to
        // probe through, so forget the claim rather than calling every
        // channel dead.
        probedIds.delete(ch.id)
        return
      }
      if (await probeStream(proxied) === 'offline')
        markOffline(ch.id)
    })
  }

  // ── Stats (per-source, derived from the dashboard) ──────────────

  const stats = computed(() => ({
    totalChannels: totalChannels.value,
    totalCountries: countries.value.length,
    totalCategories: categories.value.length,
    favoriteCount: favKeys.value.size,
  }))

  // ── Compat computed values (for legacy components) ──────────────
  // These match the old `liveTv.foo` names that pages use. Each is
  // derived from the dashboard, the visible page, or the favourites
  // cache. They're cheap — O(dashboard size) at worst.

  /** Favourites for the active source, capped at 20. */
  const favoriteChannels = computed<LiveChannel[]>(() => {
    const id = activeSourceId.value
    if (!id)
      return []
    return [...(dashboard.value?.favoritePreviews ?? [])]
  })

  /** Recent (last watched) for the active source, capped at 20. */
  const recentChannels = computed<LiveChannel[]>(() => {
    return [...(dashboard.value?.recentPreviews ?? [])]
  })

  /** "Live now" — channels the user is currently looking at. */
  const liveNowChannels = computed<LiveChannel[]>(() => visibleChannels.value.slice(0, 30))

  /** Categories as a list shaped like `{id, name, group, country}`. */
  const liveCategoryItems = computed(() =>
    categories.value.map(c => ({
      id: c.name,
      name: c.name,
      group: null,
      country: null,
    })),
  )

  /** Filtered list of categories (apply country/group from UI prefs). */
  const filteredCategories = computed(() => liveCategoryItems.value)

  /** Map of country name → first 20 channels from the dashboard. */
  const channelsByCountry = computed(() => {
    const map = new Map<string, LiveChannel[]>()
    for (const row of dashboard.value?.countryPreviews ?? []) {
      map.set(row.name, row.channels)
    }
    return map
  })

  /** Map of category name → first 20 channels from the dashboard. */
  const channelsByCategory = computed(() => {
    const map = new Map<string, LiveChannel[]>()
    for (const row of dashboard.value?.categoryPreviews ?? []) {
      map.set(row.name, row.channels)
    }
    return map
  })

  /** Empty placeholder — language wasn't kept as a separate aggregate. */
  const languages = computed<Array<{ name: string, count: number }>>(() => [])

  /**
   * Channel-count map for category chips (server already returns this
   *  as part of the dashboard's category stats).
   */
  const categoryCounts = computed(() => {
    const m = new Map<string, number>()
    for (const c of categories.value) m.set(c.name, c.count)
    return m
  })

  /** Channel-count map for country chips. */
  const countryCounts = computed(() => {
    const m = new Map<string, number>()
    for (const c of countries.value) m.set(c.name, c.count)
    return m
  })

  /** Alias for `visibleChannels` — kept for the free page. */
  const filteredChannels = computed(() => visibleChannels.value)

  return {
    // State
    sources,
    activeSource,
    activeSourceId,
    activeSourceType,
    activeSourceStatus,
    dashboard,
    countries,
    categories,
    groups,
    totalChannels,
    visibleChannels,
    visibleTotal,
    nextCursor,
    visibleLoading,
    dashboardLoading,
    refreshing,
    m3uImporting,
    m3uProgress,
    error,
    lastSyncAt,
    lastChannel,
    miniChannel,
    miniStreamUrl,
    miniVisible,
    recentChannelIds,

    // UI prefs
    selectedCountry,
    selectedCategory,
    selectedGroup,
    selectedLanguage,
    sort,
    viewMode,
    density,
    activeTab,
    view,
    favoritesOnly,
    searchQuery,
    searchDebounced,

    // EPG
    epgCache,
    epgLoading,

    // Health
    offlineIds,
    isOffline,
    markOffline,
    probeIds,

    // Computed
    favKeys,
    stats,

    // Legacy/compat computed values
    favoriteChannels,
    recentChannels,
    liveNowChannels,
    filteredCategories,
    channelsByCountry,
    channelsByCategory,
    languages,
    categoryCounts,
    countryCounts,
    filteredChannels,

    // Source actions
    refreshSources,
    setActiveSource,
    useFreeSource,
    removeSource,

    // Dashboard
    loadDashboard,

    // Visible page
    loadVisible,
    loadMore,
    searchChannels,
    loadChannelByCountry,
    loadChannelByCategory,

    // Free TV
    cancelImport,
    refreshFreeTv,

    // Favorites / recent
    isFavorite,
    toggleFavorite,
    loadFavorites,
    addRecent,
    loadRecent,
    clearRecent,
    channelFavKey,

    // EPG
    loadEpg,
    loadEpgBatch,
    loadFreeEpg,
    getEpg,

    // Player
    resolveStream,
    rememberChannel,
    showMiniPlayer,
    hideMiniPlayer,
    expandMiniPlayer,

    // Filter setters
    setCountry,
    setCategory,
    setView,
    setGroup,
    setLanguage,
    setSort,
    setViewMode,
    setDensity,
    setActiveTab,
    toggleFavoritesOnly,
    clearFilters,
  }
})
