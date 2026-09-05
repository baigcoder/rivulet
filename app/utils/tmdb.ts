// TMDB v3 — https://developer.themoviedb.org/reference/intro/getting-started
// The token in TMDB_API is the v4 "API Read Access Token" (sent as a Bearer).

import { stripProviderPrefix } from './providerTitle'

export type MediaType = 'movie' | 'tv'

export interface TmdbItem {
  id: number
  media_type?: MediaType | 'person'
  title?: string
  name?: string
  release_date?: string
  first_air_date?: string
  poster_path?: string | null
  backdrop_path?: string | null
  overview?: string
  vote_average?: number
  vote_count?: number
  genre_ids?: number[]
  original_language?: string
}

export interface TmdbPage<T = TmdbItem> {
  page: number
  results: T[]
  total_pages: number
  total_results: number
}

export interface Genre {
  id: number
  name: string
}

/** TMDB's Animation genre, in both the movie and tv lists. */
export const ANIMATION = 16

/** Everything the UI renders, with movie/tv field differences already flattened. */
export interface Media {
  id: number
  type: MediaType
  title: string
  year: string
  /** Raw TMDB paths — size is picked at render time by posterUrl/backdropUrl. */
  poster: string | null
  backdrop: string | null
  overview: string
  rating: number
  genreIds: number[]
  /**
   * ISO 639-1 original language. Optional because a card snapshot stored before
   * it was kept has none — see `kindOf` for what stands in there.
   */
  lang?: string
  /**
   * What a source is keyed by, and the one field playback cannot start without.
   * Optional because only a detail response carries it — a card out of a TMDB
   * list has never seen one. Kept in the stored snapshot so that resuming a
   * title asks its sources straight away instead of waiting on TMDB first.
   */
  imdbId?: string | null
}

export function tmdb<T>(path: string, params?: Record<string, unknown>) {
  // The user's own token wins when they have set one — see `tmdbKey` in the
  // settings store for why that escape hatch exists.
  const key = useSettingsStore().tmdbKey || useRuntimeConfig().public.TMDB_API

  return $fetch<T>(path, {
    baseURL: 'https://api.themoviedb.org/3',
    // Titles and overviews come back in whatever the app is set to, as the
    // regional tag TMDB wants (`pt-BR` for the `pt` the URL carries) — see
    // `tmdbLanguage`. TMDB falls back to English per field, so a language it
    // has nothing in still returns a usable record.
    params: { language: tmdbLanguage(), ...params },
    headers: { Authorization: `Bearer ${key}` },
  })
}

const IMAGE_BASE = 'https://image.tmdb.org/t/p'

const POSTER_SIZES = [92, 154, 185, 342, 500, 780] as const

export type PosterSize = `w${typeof POSTER_SIZES[number]}`

export function posterUrl(path?: string | null, size: PosterSize = 'w342') {
  return path ? `${IMAGE_BASE}/${size}${path}` : null
}

/** Smallest bucket that still covers `width` device pixels — callers pass CSS px * dpr. */
export function posterFor(width: number): PosterSize {
  return `w${POSTER_SIZES.find(size => size >= width) ?? 780}`
}

export function backdropUrl(path?: string | null, size: 'w780' | 'w1280' | 'original' = 'w780') {
  return path ? `${IMAGE_BASE}/${size}${path}` : null
}

/**
 * Which picture sits behind the app — and so, with "take the colour from what's
 * on screen", which one the palette is generated from. A picture of the user's
 * own is the background the app rests on: artwork covers it only while they are
 * on a title (`artWins`), never because a browse page opened on some row.
 */
export function backdropFor(mode: 'art' | 'custom' | 'off', artPath: string | null | undefined, image: string, artWins: boolean) {
  if (mode === 'off')
    return undefined
  const url = backdropUrl(artPath, 'w1280')
  if (mode === 'art')
    return url ?? undefined
  return (artWins && url) || image || undefined
}

export function profileUrl(path?: string | null, size: 'w45' | 'w185' = 'w185') {
  return path ? `${IMAGE_BASE}/${size}${path}` : null
}

/** Episode thumbnails. */
export function stillUrl(path?: string | null, size: 'w300' | 'w780' = 'w300') {
  return path ? `${IMAGE_BASE}/${size}${path}` : null
}

/** Title treatments (transparent PNG) — used instead of text in the hero. */
export function logoUrl(path?: string | null, size: 'w300' | 'w500' = 'w500') {
  return path ? `${IMAGE_BASE}/${size}${path}` : null
}

/**
 * Route to a media detail page. Also the shape `[type]/[id].vue` validates.
 *
 * Every link helper here runs its path through `localePath`, which is the
 * identity under the app's `no_prefix` strategy — it is the one place that
 * would have to change if the language ever went back into the URL, instead of
 * the ~40 call sites.
 */
export function mediaLink(media: Pick<Media, 'id' | 'type'>) {
  return localePath(`/${media.type}/${media.id}`)
}

export function seasonLink(showId: string | number, season: number) {
  return localePath(`/tv/${showId}/season/${season}`)
}

export function episodeLink(showId: string | number, season: number, episode: number) {
  return `${seasonLink(showId, season)}/episode/${episode}`
}

export function personLink(id: string | number) {
  return localePath(`/people/${id}`)
}

export function collectionLink(id: string | number) {
  return localePath(`/collection/${id}`)
}

/**
 * Route to the player. It takes the TMDB id rather than a magnet: the source
 * lookup (utils/torrents.ts) happens there, so every Play button in the app
 * only needs what it already has on screen.
 */
export function watchLink(
  type: MediaType,
  id: string | number,
  season?: number,
  episode?: number,
  /** Extra query the player can use to skip a round trip — an IMDb id, a title. */
  extra?: Record<string, string | undefined>,
) {
  const query = new URLSearchParams({ type, id: String(id) })
  if (season && episode) {
    query.set('s', String(season))
    query.set('e', String(episode))
  }
  for (const [key, value] of Object.entries(extra ?? {})) {
    if (value)
      query.set(key, value)
  }
  return `${localePath('/watch')}?${query}`
}

/**
 * A picker-chosen URL or magnet is too long for `?url=` (and illegal inside
 * a button). Stash it here, navigate with `?pick=`, and the player takes it
 * once. Memory covers a quota miss; the query flag stops a leftover stash
 * from hijacking the ordinary Play button.
 */
const PENDING_RELEASE = 'rivulet.pendingRelease'

export interface PendingRelease {
  url?: string
  magnet?: string
}

let stagedRelease: PendingRelease | null = null

export function savePendingRelease(play: PendingRelease) {
  stagedRelease = play
  try {
    sessionStorage.setItem(PENDING_RELEASE, JSON.stringify(play))
  }
  catch { /* quota — memory still has it for this navigation */ }
}

export function takePendingRelease(): PendingRelease | null {
  const fromMem = stagedRelease
  stagedRelease = null
  let fromStore: PendingRelease | null = null
  try {
    const raw = sessionStorage.getItem(PENDING_RELEASE)
    sessionStorage.removeItem(PENDING_RELEASE)
    if (raw)
      fromStore = JSON.parse(raw) as PendingRelease
  }
  catch { /* no window / SSR */ }
  const play = fromMem || fromStore
  if (!play?.url && !play?.magnet)
    return null
  return play
}

/** 148 -> "2h 28m". */
export function runtimeText(minutes?: number) {
  if (!minutes)
    return ''
  const h = Math.floor(minutes / 60)
  return h ? $t('{hours}h {minutes}m', { hours: h, minutes: minutes % 60 }) : $t('{minutes}m', { minutes })
}

export function moneyText(amount?: number) {
  if (!amount)
    return ''
  // Currency stays USD — it is TMDB's figure, not a converted one — but the
  // grouping and the compact suffix follow the reader's language.
  return amount.toLocaleString(uiLocale(), { style: 'currency', currency: 'USD', notation: 'compact', maximumFractionDigits: 1 })
}

export function dateText(date?: string) {
  if (!date)
    return ''
  return new Date(date).toLocaleDateString(uiLocale(), { day: 'numeric', month: 'short', year: 'numeric' })
}

/**
 * `type` is the fallback for endpoints that don't return media_type
 * (everything except /search/multi and /trending/all). Returns null for
 * people, which /search/multi mixes into results.
 */
export function toMedia(item: TmdbItem, type?: MediaType): Media | null {
  const mediaType = item.media_type ?? type
  if (mediaType !== 'movie' && mediaType !== 'tv')
    return null

  return {
    id: item.id,
    type: mediaType,
    title: item.title ?? item.name ?? $t('Untitled'),
    year: (item.release_date ?? item.first_air_date ?? '').slice(0, 4),
    poster: item.poster_path ?? null,
    backdrop: item.backdrop_path ?? null,
    overview: item.overview ?? '',
    rating: item.vote_average ?? 0,
    genreIds: item.genre_ids ?? [],
    lang: item.original_language ?? '',
  }
}

export function useGenres(type: MediaType) {
  return useAsyncData(
    `genres-${type}`,
    () => tmdb<{ genres: Genre[] }>(`/genre/${type}/list`),
    { lazy: true, default: (): Genre[] => [], transform: data => data.genres },
  )
}

/**
 * Title -> IMDb id, for the two cases a library page can't cover: TMDB has no
 * IMDb id on the summary the page was built from, or there is no TMDB entry at
 * all because the user pasted a magnet and its filename is the only clue.
 *
 * Two requests, because /search hands back TMDB ids and only the detail side
 * carries external ones. It runs at most once per playback, on a path that
 * would otherwise have nothing to search a source or subtitles with.
 */
/** Strip year/quality tokens a provider puts on a VOD filename. */
function titleQuery(title: string): { query: string, year: string } {
  const year = title.match(/\(((?:19|20)\d{2})\)/)?.[1] ?? ''
  const query = title
    .replace(/\((?:19|20)\d{2}\)/g, '')
    .replace(/\[[^\]]*\]/g, '')
    .replace(/\b(?:4K|UHD|FHD|HD|SD|CAM|TS|1080p|720p|2160p|BluRay|WEB-?DL)\b/gi, '')
    .replace(/\s+/g, ' ')
    .trim()
  return { query, year }
}

function yearOf(m: TmdbItem): string {
  return (m.release_date ?? m.first_air_date ?? '').slice(0, 4)
}

function pickSearchHit(results: TmdbItem[], query: string, year: string): TmdbItem | undefined {
  if (year) {
    const y = results.find(m => yearOf(m) === year)
    if (y)
      return y
  }
  const want = query.toLowerCase()
  return results.find(m => (m.title ?? m.name ?? '').toLowerCase() === want) ?? results[0]
}

const titleMatchCache = new Map<string, number | null>()

/** First TMDB hit for a provider title — used to fill trailer/cast/reviews. */
export async function tmdbMatchByTitle(title: string, type: MediaType): Promise<number | null> {
  const { query, year } = titleQuery(title)
  if (!query)
    return null
  const cacheKey = `${type}:${query}:${year ?? ''}`
  if (titleMatchCache.has(cacheKey))
    return titleMatchCache.get(cacheKey)!
  // `EN: Dune` is unknown to TMDB. `IT: Chapter Two` is the film — try
  // the raw string first, then the prefix-stripped one.
  const stripped = stripProviderPrefix(query)
  const queries = stripped && stripped !== query ? [query, stripped] : [query]
  try {
    for (const q of queries) {
      const { results } = await tmdb<TmdbPage>(`/search/${type}`, { query: q })
      if (!results.length)
        continue
      const hit = pickSearchHit(results, q, year)
      if (!hit)
        continue
      // A prefixed query that only matched some other film's first hit
      // is noise — wait for the stripped search.
      if (q !== stripped) {
        const name = (hit.title ?? hit.name ?? '').toLowerCase()
        if (name !== q.toLowerCase() && !(year && yearOf(hit) === year))
          continue
      }
      titleMatchCache.set(cacheKey, hit.id)
      return hit.id
    }
    titleMatchCache.set(cacheKey, null)
    return null
  }
  catch {
    return null
  }
}

export async function imdbIdByTitle(title: string, series = false, year = ''): Promise<string> {
  if (!title.trim())
    return ''

  const type: MediaType = series ? 'tv' : 'movie'
  try {
    const { results } = await tmdb<TmdbPage>(`/search/${type}`, { query: title })
    // Year is a preference, not a filter: passing it to TMDB turns a wrong
    // guess into no results at all, and "Dune" only needs it to break a tie.
    const hit = (year && results.find(m => (m.release_date ?? m.first_air_date ?? '').startsWith(year)))
      || results[0]
    if (!hit)
      return ''

    const { imdb_id } = await tmdb<{ imdb_id?: string | null }>(`/${type}/${hit.id}/external_ids`)
    return imdb_id ?? ''
  }
  catch {
    return '' // offline, or TMDB has never heard of it
  }
}

// --- Detail ------------------------------------------------------------------

export interface Person {
  id: number
  name: string
  role: string
  profile: string | null
}

export interface Season {
  number: number
  name: string
  episodes: number
  year: string
  poster: string | null
  overview: string
}

export interface Episode {
  number: number
  name: string
  overview: string
  air: string
  runtime: number
  still: string | null
  rating: number
}

/** A season fetched on its own — the show's season list carries no episodes. */
export interface SeasonDetail {
  number: number
  name: string
  overview: string
  air: string
  poster: string | null
  episodes: Episode[]
}

export interface EpisodeDetail extends Episode {
  season: number
  votes: number
  /** Cast credited for this episode only — the regulars are on the show. */
  guests: Person[]
  directors: string[]
  writers: string[]
}

export interface MediaDetail extends Media {
  tagline: string
  status: string
  /** Movie length, or the average episode length for a show. */
  runtime: number
  genres: Genre[]
  homepage: string
  imdbId: string | null
  /** US age rating, when TMDB has one. */
  certification: string
  votes: number
  released: string
  /** Transparent title treatment, when TMDB has an English one. */
  logo: string | null
  /** YouTube key for the best available trailer. */
  trailer: string | null
  /** Official trailers first, then the rest — the hero walks this on a geo-block. */
  trailers: string[]
  cast: Person[]
  directors: string[]
  writers: string[]
  companies: string[]
  seasons: Season[]
  episodeCount: number
  budget: number
  revenue: number
  collection: { id: number, name: string, poster: string | null, backdrop: string | null } | null
}

// TMDB drops appends the endpoint doesn't know (content_ratings on a movie,
// release_dates on a show), so one string covers both types in one request.
// external_ids is how a show gets its IMDb id — only movies carry imdb_id
// inline, and the source protocol is keyed by that id.
//
// Images stay off the first request — that payload is every still. Credits
// ride with it so cast is on the page with the title. Videos have a
// language-wide fallback fetch when the UI-language list is empty.
const DETAIL_CORE = 'release_dates,content_ratings,external_ids,belongs_to_collection,credits'

interface RawCredit { id: number, name: string, character?: string, job?: string, profile_path?: string | null }
interface RawImage { file_path: string, iso_639_1: string | null }
interface RawVideo { key: string, site: string, type: string, official: boolean }
interface RawSeason { season_number: number, name: string, episode_count: number, air_date?: string | null, poster_path?: string | null, overview?: string }

interface RawDetail extends TmdbItem {
  tagline?: string
  status?: string
  runtime?: number
  episode_run_time?: number[]
  number_of_episodes?: number
  genres?: Genre[]
  homepage?: string
  imdb_id?: string | null
  budget?: number
  revenue?: number
  seasons?: RawSeason[]
  production_companies?: { name: string }[]
  networks?: { name: string }[]
  last_episode_to_air?: { runtime?: number } | null
  credits?: { cast: RawCredit[], crew: RawCredit[] }
  videos?: { results: RawVideo[] }
  images?: { logos: RawImage[] }
  release_dates?: { results: { iso_3166_1: string, release_dates: { certification: string }[] }[] }
  content_ratings?: { results: { iso_3166_1: string, rating: string }[] }
  external_ids?: { imdb_id?: string | null }
  belongs_to_collection?: { id: number, name: string, poster_path?: string | null, backdrop_path?: string | null } | null
}

function certificationOf(raw: RawDetail) {
  const dates = raw.release_dates?.results.find(r => r.iso_3166_1 === 'US')
  if (dates)
    return dates.release_dates.find(d => d.certification)?.certification ?? ''
  return raw.content_ratings?.results.find(r => r.iso_3166_1 === 'US')?.rating ?? ''
}

function trailersOf(raw: { videos?: { results?: RawVideo[] } }) {
  const videos = (raw.videos?.results ?? []).filter(v => v.site === 'YouTube' && v.key)
  const rank = (v: RawVideo) => {
    if (v.type === 'Trailer' && v.official)
      return 0
    if (v.type === 'Trailer')
      return 1
    if (v.type === 'Teaser')
      return 2
    return 3
  }
  return [...new Set(videos.toSorted((a, b) => rank(a) - rank(b)).map(v => v.key))]
}

function trailerOf(raw: RawDetail) {
  return trailersOf(raw)[0] ?? null
}

function toPerson(c: RawCredit): Person {
  return { id: c.id, name: c.name, role: c.character ?? c.job ?? '', profile: c.profile_path ?? null }
}

function jobs(crew: RawCredit[], names: string[]) {
  // Same person can hold the job twice (writer + screenplay); de-dupe by name.
  return [...new Set(crew.filter(c => names.includes(c.job ?? '')).map(c => c.name))]
}

function toDetail(raw: RawDetail, type: MediaType): MediaDetail {
  const base = toMedia(raw, type)!
  const crew = raw.credits?.crew ?? []

  return {
    ...base,
    tagline: raw.tagline ?? '',
    status: raw.status ?? '',
    runtime: raw.runtime ?? raw.episode_run_time?.[0] ?? raw.last_episode_to_air?.runtime ?? 0,
    genres: raw.genres ?? [],
    homepage: raw.homepage ?? '',
    imdbId: raw.imdb_id ?? raw.external_ids?.imdb_id ?? null,
    certification: certificationOf(raw),
    votes: raw.vote_count ?? 0,
    released: raw.release_date ?? raw.first_air_date ?? '',
    logo: raw.images?.logos.find(l => l.iso_639_1 === 'en')?.file_path ?? raw.images?.logos[0]?.file_path ?? null,
    trailer: trailerOf(raw),
    trailers: trailersOf(raw),
    cast: (raw.credits?.cast ?? []).slice(0, 20).map(toPerson),
    directors: jobs(crew, ['Director']),
    writers: jobs(crew, ['Writer', 'Screenplay', 'Story']),
    // A show's network is the more useful credit than its production companies.
    companies: (raw.networks ?? raw.production_companies ?? []).map(c => c.name).slice(0, 3),
    // Season 0 is specials/extras — real seasons only.
    seasons: (raw.seasons ?? [])
      .filter(s => s.season_number > 0 && s.episode_count > 0)
      .map(s => ({
        number: s.season_number,
        name: s.name,
        episodes: s.episode_count,
        year: (s.air_date ?? '').slice(0, 4),
        poster: s.poster_path ?? null,
        overview: s.overview ?? '',
      })),
    episodeCount: raw.number_of_episodes ?? 0,
    budget: raw.budget ?? 0,
    revenue: raw.revenue ?? 0,
    collection: raw.belongs_to_collection
      ? {
          id: raw.belongs_to_collection.id,
          name: raw.belongs_to_collection.name,
          poster: raw.belongs_to_collection.poster_path ?? null,
          backdrop: raw.belongs_to_collection.backdrop_path ?? null,
        }
      : null,
  }
}

export interface Review {
  id: string
  author: string
  avatar: string | null
  rating: number | null
  content: string
  created: string
}

interface RawReview {
  id: string
  author: string
  content: string
  created_at: string
  author_details?: {
    name?: string
    username?: string
    avatar_path?: string | null
    rating?: number | null
  }
}

function reviewAvatar(path?: string | null) {
  if (!path)
    return null
  // Gravatar arrives as `/https://…` rather than a TMDB file.
  if (path.startsWith('/http'))
    return path.slice(1)
  return profileUrl(path, 'w45')
}

/** Fetched when the rating menu opens, not with the title — reviews are not first paint. */
export function useReviews(type: MaybeRefOrGetter<MediaType>, id: MaybeRefOrGetter<string | number>) {
  return useAsyncData(
    () => `reviews-${toValue(type)}-${toValue(id)}`,
    () => tmdb<{ results: RawReview[] }>(`/${toValue(type)}/${toValue(id)}/reviews`),
    {
      lazy: true,
      immediate: false,
      watch: [() => toValue(type), () => toValue(id)],
      transform: (page): Review[] => (page.results ?? []).map(r => ({
        id: r.id,
        author: r.author_details?.name || r.author_details?.username || r.author,
        avatar: reviewAvatar(r.author_details?.avatar_path),
        rating: r.author_details?.rating ?? null,
        content: r.content.replace(/<[^>]+>/g, ''),
        created: r.created_at,
      })),
    },
  )
}

const coreCache = new Map<string, Promise<MediaDetail>>()
const coreSync = new Map<string, MediaDetail>()

/** Sync read of a title already prefetched or visited — first paint, no await. */
export function peekMediaDetail(type: MediaType, id: string | number) {
  return peekCore(type, id)
}

function peekCore(type: MediaType, id: string | number) {
  const key = `detail-${type}-${id}`
  const fresh = coreSync.get(key)
  if (fresh)
    return fresh
  if (import.meta.server)
    return undefined
  try {
    const stored = sessionStorage.getItem(`rivulet.${key}`)
    if (!stored)
      return undefined
    const parsed = JSON.parse(stored) as MediaDetail
    coreSync.set(key, parsed)
    return parsed
  }
  catch {
    return undefined
  }
}

function loadCore(type: MediaType, id: string | number) {
  const key = `detail-${type}-${id}`
  const hit = coreCache.get(key)
  if (hit)
    return hit
  const peeked = peekCore(type, id)
  if (peeked) {
    const cached = Promise.resolve(peeked)
    coreCache.set(key, cached)
    return cached
  }
  const pending = tmdb<RawDetail>(`/${type}/${id}`, { append_to_response: DETAIL_CORE })
    .then(raw => {
      const detail = toDetail(raw, type)
      coreSync.set(key, detail)
      if (!import.meta.server) {
        try {
          sessionStorage.setItem(`rivulet.${key}`, JSON.stringify(detail))
        }
        catch {
          // quota — the memory cache still holds it
        }
      }
      return detail
    })
    .catch(err => {
      coreCache.delete(key)
      throw err
    })
  coreCache.set(key, pending)
  return pending
}

/** Title (with credits) on press — the page should have the record before it mounts. */
export function prefetchMediaDetail(media: Pick<Media, 'id' | 'type'>) {
  void loadCore(media.type, media.id)
}

/** Same cache as the title page — the home hero should not fire a second /id. */
export function loadMediaDetail(type: MediaType, id: string | number) {
  return loadCore(type, id)
}

/** Card art survives a KeepAlive browse page rewriting `selected` on the way out. */
export function snapMedia(media: Media) {
  if (import.meta.server)
    return
  try {
    sessionStorage.setItem(`rivulet.snap.${media.type}.${media.id}`, JSON.stringify(media))
  }
  catch {
    // quota — the in-memory opening snapshot still helps on the same tick
  }
}

export function peekSnapMedia(type: MediaType, id: string | number): Media | null {
  if (!id)
    return null
  if (import.meta.server)
    return null
  try {
    const raw = sessionStorage.getItem(`rivulet.snap.${type}.${id}`)
    return raw ? JSON.parse(raw) as Media : null
  }
  catch {
    return null
  }
}

export function snapPremiumTmdb(kind: 'movie' | 'tv', providerId: string, tmdbId: number) {
  if (import.meta.server)
    return
  try {
    sessionStorage.setItem(`rivulet.premium.tmdb.${kind}.${providerId}`, String(tmdbId))
  }
  catch {
    // best-effort — the detail page still searches
  }
}

export function peekPremiumTmdb(kind: 'movie' | 'tv', providerId: string): string {
  if (import.meta.server || !providerId)
    return ''
  try {
    return sessionStorage.getItem(`rivulet.premium.tmdb.${kind}.${providerId}`) ?? ''
  }
  catch {
    return ''
  }
}

/** Never blocks navigation — the page renders its skeleton while this resolves. */
export function useMediaDetail(type: MaybeRefOrGetter<MediaType>, id: MaybeRefOrGetter<string | number>) {
  const core = useAsyncData(
    () => `detail-${toValue(type)}-${toValue(id)}`,
    () => {
      const tid = String(toValue(id) ?? '')
      if (!tid)
        return Promise.resolve(null as unknown as MediaDetail)
      return loadCore(toValue(type), tid)
    },
    {
      lazy: true,
      server: false,
      immediate: true,
      watch: [() => toValue(type), () => toValue(id)],
      getCachedData: () => {
        const tid = String(toValue(id) ?? '')
        if (!tid)
          return undefined
        return peekCore(toValue(type), tid)
      },
    },
  )
  // The first request asks for videos in the UI language. A Spanish film
  // often has none of those, so the hero never got a trailer. This pass
  // only runs when that list was empty.
  const videos = useAsyncData(
    () => `detail-videos-${toValue(type)}-${toValue(id)}`,
    () => tmdb<{ results: RawVideo[] }>(`/${toValue(type)}/${toValue(id)}/videos`, {
      include_video_language: 'en,null,es,pt,fr,de,it,ja,ko,zh,hi,ar',
    }).then(page => trailersOf({ videos: page })),
    { lazy: true, immediate: false },
  )
  watch(() => core.data.value, value => {
    if (!value)
      return
    if (!value.trailer)
      videos.execute()
  }, { immediate: true })
  const data = computed(() => {
    const a = core.data.value
    if (!a)
      return a
    const keys = videos.data.value
    if (keys?.length && !a.trailer) {
      return { ...a, trailer: keys[0] ?? null, trailers: keys }
    }
    return a
  })
  return { data, status: core.status, error: core.error, refresh: core.refresh }
}

interface RawEpisode {
  episode_number: number
  season_number?: number
  name?: string
  overview?: string
  air_date?: string | null
  runtime?: number | null
  still_path?: string | null
  vote_average?: number
  vote_count?: number
  guest_stars?: RawCredit[]
  crew?: RawCredit[]
}

function toEpisode(e: RawEpisode): Episode {
  return {
    number: e.episode_number,
    name: e.name ?? '',
    overview: e.overview ?? '',
    air: e.air_date ?? '',
    runtime: e.runtime ?? 0,
    still: e.still_path ?? null,
    rating: e.vote_average ?? 0,
  }
}

// The season endpoint returns the episodes themselves instead of a count.
interface RawSeasonDetail extends Omit<RawSeason, 'episode_count'> {
  episodes?: RawEpisode[]
}

function toSeasonDetail(raw: RawSeasonDetail): SeasonDetail {
  return {
    number: raw.season_number,
    name: raw.name,
    overview: raw.overview ?? '',
    air: raw.air_date ?? '',
    poster: raw.poster_path ?? null,
    episodes: (raw.episodes ?? []).map(toEpisode),
  }
}

const seasonCache = new Map<string, Promise<SeasonDetail>>()
const seasonSync = new Map<string, SeasonDetail>()

function peekSeason(id: string | number, season: number) {
  return seasonSync.get(`season-${id}-${season}`)
}

function loadSeason(id: string | number, season: number) {
  const key = `season-${id}-${season}`
  const hit = seasonCache.get(key)
  if (hit)
    return hit
  const peeked = peekSeason(id, season)
  if (peeked) {
    const cached = Promise.resolve(peeked)
    seasonCache.set(key, cached)
    return cached
  }
  const pending = tmdb<RawSeasonDetail>(`/tv/${id}/season/${season}`)
    .then(raw => {
      const detail = toSeasonDetail(raw)
      seasonSync.set(key, detail)
      return detail
    })
    .catch(err => {
      seasonCache.delete(key)
      throw err
    })
  seasonCache.set(key, pending)
  return pending
}

/** Card focus on the show page — episodes should be ready when the season opens. */
export function prefetchSeason(id: string | number, season: number) {
  void loadSeason(id, season)
}

const episodeCache = new Map<string, Promise<EpisodeDetail>>()
const episodeSync = new Map<string, EpisodeDetail>()

function peekEpisode(id: string | number, season: string | number, episode: string | number) {
  return episodeSync.get(`episode-${id}-${season}-${episode}`)
}

function loadEpisode(id: string | number, season: string | number, episode: string | number) {
  const key = `episode-${id}-${season}-${episode}`
  const hit = episodeCache.get(key)
  if (hit)
    return hit
  const peeked = peekEpisode(id, season, episode)
  if (peeked) {
    const cached = Promise.resolve(peeked)
    episodeCache.set(key, cached)
    return cached
  }
  const pending = tmdb<RawEpisode>(`/tv/${id}/season/${season}/episode/${episode}`)
    .then(raw => {
      const detail: EpisodeDetail = {
        ...toEpisode(raw),
        season: raw.season_number ?? Number(season),
        votes: raw.vote_count ?? 0,
        guests: (raw.guest_stars ?? []).slice(0, 20).map(toPerson),
        directors: jobs(raw.crew ?? [], ['Director']),
        writers: jobs(raw.crew ?? [], ['Writer', 'Teleplay', 'Screenplay', 'Story']),
      }
      episodeSync.set(key, detail)
      return detail
    })
    .catch(err => {
      episodeCache.delete(key)
      throw err
    })
  episodeCache.set(key, pending)
  return pending
}

/** Press on a season row — the episode page should not wait on mount. */
export function prefetchEpisode(id: string | number, season: string | number, episode: string | number) {
  void loadEpisode(id, season, episode)
}

export function useSeason(id: MaybeRefOrGetter<string | number>, season: MaybeRefOrGetter<number>) {
  return useAsyncData(
    () => `season-${toValue(id)}-${toValue(season)}`,
    () => loadSeason(toValue(id), toValue(season)),
    {
      lazy: true,
      watch: [() => toValue(id), () => toValue(season)],
      getCachedData: () => peekSeason(toValue(id), toValue(season)),
    },
  )
}

export function useEpisode(
  id: MaybeRefOrGetter<string | number>,
  season: MaybeRefOrGetter<string | number>,
  episode: MaybeRefOrGetter<string | number>,
) {
  return useAsyncData(
    () => `episode-${toValue(id)}-${toValue(season)}-${toValue(episode)}`,
    () => loadEpisode(toValue(id), toValue(season), toValue(episode)),
    {
      lazy: true,
      watch: [() => toValue(id), () => toValue(season), () => toValue(episode)],
      getCachedData: () => peekEpisode(toValue(id), toValue(season), toValue(episode)),
    },
  )
}

// --- Person ------------------------------------------------------------------

export interface PersonDetail {
  id: number
  name: string
  biography: string
  birthday: string | null
  deathday: string | null
  placeOfBirth: string
  profile: string | null
  knownForDepartment: string
  alsoKnownAs: string[]
  homepage: string | null
  imdbId: string | null
}

interface RawPerson {
  id: number
  name: string
  biography?: string
  birthday?: string | null
  deathday?: string | null
  place_of_birth?: string
  profile_path?: string | null
  known_for_department?: string
  also_known_as?: string[]
  homepage?: string | null
  imdb_id?: string | null
}

export function usePersonDetail(id: MaybeRefOrGetter<string | number>) {
  return useAsyncData(
    () => `person-${toValue(id)}`,
    () => tmdb<RawPerson>(`/person/${toValue(id)}`),
    {
      lazy: true,
      watch: [() => toValue(id)],
      transform: (raw): PersonDetail => ({
        id: raw.id,
        name: raw.name,
        biography: raw.biography ?? '',
        birthday: raw.birthday ?? null,
        deathday: raw.deathday ?? null,
        placeOfBirth: raw.place_of_birth ?? '',
        profile: raw.profile_path ?? null,
        knownForDepartment: raw.known_for_department ?? 'Acting',
        alsoKnownAs: raw.also_known_as ?? [],
        homepage: raw.homepage ?? null,
        imdbId: raw.imdb_id ?? null,
      }),
    },
  )
}

interface RawPersonCredit {
  id: number
  media_type: MediaType
  title?: string
  name?: string
  character?: string
  job?: string
  department?: string
  release_date?: string
  first_air_date?: string
  poster_path?: string | null
  backdrop_path?: string | null
  overview?: string
  vote_average?: number
  vote_count?: number
  genre_ids?: number[]
  original_language?: string
}

interface RawPersonCredits {
  cast: RawPersonCredit[]
  crew: RawPersonCredit[]
}

export interface PersonCredit {
  media: Media
  character: string
  job: string
  department: string
}

export function usePersonCredits(id: MaybeRefOrGetter<string | number>) {
  return useAsyncData(
    () => `person-credits-${toValue(id)}`,
    () => tmdb<RawPersonCredits>(`/person/${toValue(id)}/combined_credits`),
    {
      lazy: true,
      watch: [() => toValue(id)],
      transform: raw => {
        const cast: PersonCredit[] = raw.cast
          .filter(c => c.media_type === 'movie' || c.media_type === 'tv')
          .map(c => ({
            media: toMedia(c, c.media_type)!,
            character: c.character ?? '',
            job: '',
            department: 'Acting',
          }))
          .filter(c => c.media)
          .sort((a, b) => (b.media.year || '').localeCompare(a.media.year || ''))

        const crew: PersonCredit[] = raw.crew
          .filter(c => c.media_type === 'movie' || c.media_type === 'tv')
          .map(c => ({
            media: toMedia(c, c.media_type)!,
            character: '',
            job: c.job ?? '',
            department: c.department ?? '',
          }))
          .filter(c => c.media)
          .sort((a, b) => (b.media.year || '').localeCompare(a.media.year || ''))

        return { cast, crew }
      },
    },
  )
}

/** Cast press on a title page — person should not wait on mount. */
export function prefetchPerson(id: string | number) {
  void tmdb<RawPerson>(`/person/${id}`)
  void tmdb<RawPersonCredits>(`/person/${id}/combined_credits`)
}

// --- Collection --------------------------------------------------------------

export interface CollectionDetail {
  id: number
  name: string
  overview: string
  poster: string | null
  backdrop: string | null
  parts: Media[]
}

interface RawCollection {
  id: number
  name: string
  overview?: string
  poster_path?: string | null
  backdrop_path?: string | null
  parts?: TmdbItem[]
}

export function useCollectionDetail(id: MaybeRefOrGetter<string | number>) {
  return useAsyncData(
    () => `collection-${toValue(id)}`,
    () => tmdb<RawCollection>(`/collection/${toValue(id)}`),
    {
      lazy: true,
      watch: [() => toValue(id)],
      transform: (raw): CollectionDetail => ({
        id: raw.id,
        name: raw.name,
        overview: raw.overview ?? '',
        poster: raw.poster_path ?? null,
        backdrop: raw.backdrop_path ?? null,
        parts: (raw.parts ?? [])
          .map(p => toMedia({ ...p, media_type: 'movie' as const }, 'movie'))
          .filter((m): m is Media => m != null)
          .sort((a, b) => (a.year || '').localeCompare(b.year || '')),
      }),
    },
  )
}
