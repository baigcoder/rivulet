import { invoke } from '@tauri-apps/api/core'
import { hasVlcPlayer, SOFT_DECODE_MAX_HEIGHT } from '~/utils/htmlvideo'

// ── Types ──────────────────────────────────────────────────────────

/**
 * Free TV is the only `kind` that lands in the same iptv_sources
 * table as before. Premium M3U and Premium Xtream have moved to the
 * `premium/` module and a separate `iptv_premium.db`; nothing in
 * this file touches that path. The union is kept so a free-source
 * row that the old build left behind still type-checks.
 */
export type LiveTvSourceKind = 'free-m3u' | 'premium-m3u' | 'premium-xtream'
export type LiveTvSourceStatus = 'active' | 'staging' | 'failed' | 'superseded'

export interface LiveTvSource {
  id: string
  kind: LiveTvSourceKind
  displayName: string
  status: LiveTvSourceStatus
  configJson: string
  insertedAt: number
  activatedAt: number | null
  channelCount: number
  countryCount: number
  categoryCount: number
}

export interface LiveChannel {
  id: string
  name: string
  logoUrl?: string | null
  streamUrl?: string | null
  categoryId?: string | null
  categoryName?: string | null
  /**
   * Provider-specific group (Xtream). May be the same as categoryName
   * for M3U sources, where `group-title` doubles as both.
   */
  groupName?: string | null
  country?: string | null
  language?: string | null
  epgId?: string | null
  streamType?: string | null
  /** ISO 3166-1 alpha-2 code from the iptv-org country API. */
  countryCode?: string | null
  /** Flag emoji from the iptv-org country API. */
  countryFlag?: string | null
  /** From #EXTVLCOPT:http-user-agent=. Forwarded to the proxy. */
  userAgent?: string | null
  /** From #EXTVLCOPT:http-referrer=. Forwarded to the proxy. */
  referer?: string | null
}

export interface LiveCategory {
  id: string
  name: string
  parentId?: string | null
  country?: string | null
  group?: string | null
}

export interface EpgProgram {
  channelId: string
  title: string
  description?: string | null
  start: string
  stop?: string | null
}

/**
 * Kept because dashboard pages still reference it. The Rust side
 * no longer produces one of these — `liveGetIptvStatus` is gone —
 * and the free-TV page never sees one. Premium TV has its own
 * `PremiumAccount` shape under `app/utils/premiumTv.ts`.
 */
export interface IptvAccount {
  providerType: string
  serverUrl: string
  username: string
  status: string
  accountName?: string | null
  expiresAt?: string | null
  isTrial?: boolean | null
  activeConnections?: number | null
  maxConnections?: number | null
}

export interface LiveStream {
  id: string
  name: string
  logoUrl: string | null
  /**
   * Proxy-wrapped URL (`http://127.0.0.1:3031/stream?...`). The
   * webview `<video>` fallback loads this when native mpv isn't
   * available (browser dev, Android webview path) and the URL has
   * been mangled to `.m3u8` for browser-decodable HLS.
   */
  streamUrl: string
  userAgent?: string | null
  referer?: string | null
  categoryName?: string | null
  country?: string | null
  epgId?: string | null
}

export interface CountryCount { name: string, count: number }
export interface CategoryCount { name: string, count: number }
export interface GroupCount { name: string, count: number }

export interface ChannelPage {
  items: LiveChannel[]
  total: number
  nextCursor: string | null
}

export interface LiveCountryPreview {
  name: string
  count: number
  channels: LiveChannel[]
}

export interface LiveCategoryPreview {
  name: string
  count: number
  channels: LiveChannel[]
}

export interface LiveDashboard {
  sourceId: string
  totalChannels: number
  countryCount: number
  categoryCount: number
  countries: CountryCount[]
  categories: CategoryCount[]
  groups: GroupCount[]
  favoritePreviews: LiveChannel[]
  recentPreviews: LiveChannel[]
  countryPreviews: LiveCountryPreview[]
  categoryPreviews: LiveCategoryPreview[]
}

export interface IptvCountry {
  name: string
  code: string
  languages: string[]
  flag: string
}

export interface IptvCategory {
  id: string
  name: string
  description: string
}

// ── Source registry (Free TV only) ────────────────────────────────

export function liveListSources(): Promise<LiveTvSource[]> {
  return invoke<LiveTvSource[]>('live_list_sources')
}

export function liveActiveSource(): Promise<LiveTvSource | null> {
  return invoke<LiveTvSource | null>('live_active_source')
}

export function liveSetActive(id: string): Promise<void> {
  return invoke<void>('live_set_active', { id })
}

export function liveRemoveSource(id: string): Promise<void> {
  return invoke<void>('live_remove_source', { id })
}

// ── Dashboard / queries ────────────────────────────────────────────

export function liveDashboard(sourceId: string): Promise<LiveDashboard> {
  return invoke<LiveDashboard>('live_dashboard', { sourceId })
}

export interface LiveQueryArgs {
  sourceId: string
  country?: string
  category?: string
  group?: string
  language?: string
  quality?: string
  search?: string
  favoritesOnly?: boolean
  sort?: string
  cursor?: string
  limit?: number
}

export function liveQueryChannels(args: LiveQueryArgs): Promise<ChannelPage> {
  return invoke<ChannelPage>('live_query_channels', {
    sourceId: args.sourceId,
    country: args.country ?? null,
    category: args.category ?? null,
    group: args.group ?? null,
    language: args.language ?? null,
    quality: args.quality ?? null,
    search: args.search ?? null,
    favoritesOnly: args.favoritesOnly ?? false,
    sort: args.sort ?? null,
    cursor: args.cursor ?? null,
    limit: args.limit ?? 60,
  })
}

export function liveSearchChannels(
  sourceId: string,
  query: string,
  cursor?: string,
  limit = 60,
): Promise<ChannelPage> {
  return invoke<ChannelPage>('live_search_channels', {
    sourceId,
    query,
    cursor: cursor ?? null,
    limit,
  })
}

export function liveCountryChannels(
  sourceId: string,
  country: string,
  cursor?: string,
  limit = 60,
): Promise<ChannelPage> {
  return invoke<ChannelPage>('live_country_channels', {
    sourceId,
    country,
    cursor: cursor ?? null,
    limit,
  })
}

export function liveCategoryChannels(
  sourceId: string,
  category: string,
  cursor?: string,
  limit = 60,
): Promise<ChannelPage> {
  return invoke<ChannelPage>('live_category_channels', {
    sourceId,
    category,
    cursor: cursor ?? null,
    limit,
  })
}

export function liveGroupChannels(
  sourceId: string,
  group: string,
  cursor?: string,
  limit = 60,
): Promise<ChannelPage> {
  return invoke<ChannelPage>('live_group_channels', {
    sourceId,
    group,
    cursor: cursor ?? null,
    limit,
  })
}

export function liveCountryStats(sourceId: string, limit = 200): Promise<CountryCount[]> {
  return invoke<CountryCount[]>('live_country_stats', { sourceId, limit })
}

export function liveCategoryStats(sourceId: string, limit = 200): Promise<CategoryCount[]> {
  return invoke<CategoryCount[]>('live_category_stats', { sourceId, limit })
}

export function liveGroupStats(sourceId: string, limit = 200): Promise<GroupCount[]> {
  return invoke<GroupCount[]>('live_group_stats', { sourceId, limit })
}

// ── Player-side (Free TV) ──────────────────────────────────────────

/**
 * Free TV was opened: have Rust check which channels answer, unless it did
 * within the last half hour. Verdicts arrive as `live_health` events.
 */
export function liveCheckHealth(): Promise<void> {
  return invoke<void>('live_check_health')
}

/** What one `live_health` event carries. */
export interface LiveHealthProgress {
  checked: number
  total: number
  offline: number
  done: boolean
  /** Channels found answering / dead since the last event. */
  alive?: string[]
  dead?: string[]
}

/** Channels the background health check is leaving out of the lists. */
export function liveOfflineCount(sourceId: string): Promise<number> {
  return invoke<number>('live_offline_count', { sourceId })
}

export function liveResolveStream(
  sourceId: string,
  channelId: string,
): Promise<LiveStream> {
  return invoke<LiveStream>('live_resolve_stream', {
    sourceId,
    channelId,
  })
}

// ── Favorites / recent ─────────────────────────────────────────────

export function liveToggleFavorite(sourceId: string, channelId: string): Promise<boolean> {
  return invoke<boolean>('live_toggle_favorite', { sourceId, channelId })
}

export function liveFavorites(sourceId: string, limit = 60): Promise<LiveChannel[]> {
  return invoke<LiveChannel[]>('live_favorites', { sourceId, limit })
}

export function liveRecent(sourceId: string, limit = 60): Promise<LiveChannel[]> {
  return invoke<LiveChannel[]>('live_recent', { sourceId, limit })
}

export function liveAddRecent(sourceId: string, channelId: string): Promise<void> {
  return invoke<void>('live_add_recent', { sourceId, channelId })
}

export function liveClearRecent(sourceId: string): Promise<void> {
  return invoke<void>('live_clear_recent', { sourceId })
}

// ── EPG ────────────────────────────────────────────────────────────

/**
 * A channel's guide from the user's own XMLTV addresses, keyed by channel id
 * (see `guides.rs`). Empty until they add one under Live TV settings.
 */
export function liveGetLiveEpg(channelId: string): Promise<EpgProgram[]> {
  return invoke<EpgProgram[]>('live_get_live_epg', { channelId })
}

export function liveChannelEpgBatch(channelIds: string[]): Promise<EpgProgram[]> {
  return invoke<EpgProgram[]>('live_channel_epg_batch', { channelIds })
}

// ── Free TV guide (the user's own XMLTV addresses) ─────────────────

export function guideUrls(): Promise<string[]> {
  return invoke<string[]>('live_guide_urls')
}

/** Resolves to the list as stored: only http(s), each once, at most ten. */
export function setGuideUrls(urls: string[]): Promise<string[]> {
  return invoke<string[]>('live_set_guide_urls', { urls })
}

export interface GuideReport { programs: number, channels: number, failed: string[] }

export function importGuides(): Promise<GuideReport> {
  return invoke<GuideReport>('live_import_guides')
}

/** Minutes added to every guide time, for a guide that runs early or late. */
export function guideOffset(): Promise<number> {
  return invoke<number>('live_guide_offset')
}

/** Resolves to the value stored, clamped to ±720. */
export function setGuideOffset(minutes: number): Promise<number> {
  return invoke<number>('live_set_guide_offset', { minutes })
}

// ── Free TV refresh ────────────────────────────────────────────────

export function liveRefreshFreeTv(): Promise<LiveTvSource> {
  return invoke<LiveTvSource>('live_refresh_free_tv')
}

export function liveCancelImport(): Promise<void> {
  return invoke<void>('live_cancel_import')
}

// ── iptv-org reference data ────────────────────────────────────────

export function getIptvCountries(): Promise<IptvCountry[]> {
  return invoke<IptvCountry[]>('get_iptv_countries')
}

export function getIptvCategories(): Promise<IptvCategory[]> {
  return invoke<IptvCategory[]>('get_iptv_categories')
}

export function getFreeTvEpgChannelMapping(): Promise<Record<string, string>> {
  return invoke<Record<string, string>>('get_free_tv_epg_channel_mapping')
}

export function getFreeTvEpg(tvgId: string): Promise<EpgProgram[]> {
  return invoke<EpgProgram[]>('get_free_tv_epg', { tvgId })
}

// ── CORS proxy (Free TV only) ──────────────────────────────────────

/**
 * Same string Rust's `proxy_free_stream_url` returns. Built here so Play
 * does not wait on an IPC round-trip before mpv starts — the command is
 * a format string, not a network call.
 */
export function wrapFreeStreamUrl(
  url: string,
  userAgent?: string | null,
  referer?: string | null,
): string {
  const src = url.trim()
  if (!src || src === 'undefined' || src === 'null')
    return ''
  if (/^https?:\/\/127\.0\.0\.1:3031\//i.test(src))
    return src
  let qs = `url=${encodeURIComponent(src)}`
  const ua = userAgent
    || (/\/(?:live|timeshift|movie|series)\//i.test(src) ? 'VLC/3.0.18 LibVLC/3.0.18' : undefined)
  if (ua)
    qs += `&X-Rivulet-Ua=${encodeURIComponent(ua)}`
  if (referer)
    qs += `&X-Rivulet-Referer=${encodeURIComponent(referer)}`
  // This — not `playUrl` — is the builder Free TV plays through, so the
  // Android ceiling has to be here too. See `SOFT_DECODE_MAX_HEIGHT`.
  if (hasVlcPlayer())
    qs += `&max_height=${SOFT_DECODE_MAX_HEIGHT}`
  return `http://127.0.0.1:3031/stream?${qs}`
}

export function proxyFreeStreamUrl(
  url: string,
  userAgent?: string,
  referer?: string,
): Promise<string> {
  return Promise.resolve(wrapFreeStreamUrl(url, userAgent, referer))
}

export function iptvProxyHealth(): Promise<boolean> {
  return invoke<boolean>('iptv_proxy_health')
}
