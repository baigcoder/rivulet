// Type declarations that the Premium TV utilities depend on.
// These mirror the Rust `src-tauri/src/premium/models.rs` shapes
// over the wire; keep the field names in sync.
//
// Two shapes deliberately differ from Free TV's equivalents in
// `app/utils/iptv.ts`, and mixing them up is the one mistake this
// comment exists to prevent: a Premium `EpgProgram.start` is an
// integer number of seconds since the epoch, where Free TV's is an
// ISO string. And a Premium channel carries no `streamUrl` at all —
// the upstream URL never leaves the Rust side, so the only way to
// play one is `POST /channels/:id/play`, which answers a
// `PlaybackSource` good for thirty seconds.

export interface IPTVCategory {
  id: string
  name: string
  country?: string | null
  group?: string | null
}

export interface IPTVChannel {
  id: string
  name: string
  logoUrl?: string | null
  categoryId?: string | null
  categoryName?: string | null
  country?: string | null
  language?: string | null
  epgId?: string | null
  streamType?: string | null
  userAgent?: string | null
  referer?: string | null
  /**
   * Filled in by the repository on every channel read, so a grid can
   * draw its stars without fetching the favourite list alongside
   * every page. Absent means `false` — the server omits it rather
   * than sending `false` sixty times per page.
   */
  isFavorite?: boolean
}

export interface IPTVChannelPage {
  items: IPTVChannel[]
  total: number
  nextCursor: string | null
}

export interface EpgProgram {
  channelId: string
  title: string
  description?: string | null
  /** Unix epoch **seconds** (not milliseconds, not an ISO string). */
  start: number
  /** Unix epoch **seconds**. `null` for a programme with no listed end. */
  stop?: number | null
}

export interface PremiumAccount {
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

export interface PlaybackSource {
  url: string
  mimeType?: string | null
  /** Unix epoch milliseconds. */
  expiresAt?: number | null
  /**
   * The `User-Agent` and `Referer` the upstream expects. These are
   * props on the player, not headers on the redirect: a header set on
   * a 302 says nothing about the request the client makes next.
   */
  userAgent?: string | null
  referer?: string | null
}

export interface CategoryCount {
  name: string
  count: number
}

export interface CountryCount {
  name: string
  count: number
}

export interface PremiumChannelPreview {
  name: string
  count: number
  channels: IPTVChannel[]
}

export interface PremiumDashboard {
  sourceId: string
  totalChannels: number
  countryCount: number
  categoryCount: number
  categories: CategoryCount[]
  countries: CountryCount[]
  favoritePreviews: IPTVChannel[]
  recentPreviews: IPTVChannel[]
  countryPreviews: PremiumChannelPreview[]
  categoryPreviews: PremiumChannelPreview[]
}

/** What a catalog or EPG import ended up doing. */
export interface SyncReport {
  categories: number
  channels: number
  programs: number
  /** `false` when the provider ships no guide at all — normal, not an error. */
  epgAvailable: boolean
  /** Unix seconds. When the catalog was last written. */
  syncedAt: number
}

/**
 * The connection's freshness. An account with `channels: 0` and
 * `syncing: true` is "still importing", which is a different screen
 * from "this provider has no channels".
 */
export interface CatalogState {
  channels: number
  categories: number
  /** Unix seconds, or `null` when the catalog has never imported. */
  catalogSyncedAt?: number | null
  epgSyncedAt?: number | null
  syncing: boolean
}

/** What `GET /status` answers: who is connected, and how fresh their catalog is. */
export interface PremiumStatus {
  account: PremiumAccount | null
  /** `null` when no provider is connected, so a caller branches on one field. */
  catalog: CatalogState | null
}

/** What `/connect` and `/refresh`-with-force answer. */
export interface ConnectResult {
  account: PremiumAccount
  report: SyncReport
}
