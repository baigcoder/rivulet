<script lang="ts" setup>
import type { MediaType } from '~/utils/tmdb'
import type { Priming, Release } from '~/utils/torrents'
import {
  mdiAccountGroup,
  mdiAlertCircleOutline,
  mdiArrowLeft,
  mdiDownload,
  mdiPlay,
  mdiPowerPlugOutline,
  mdiReload,
} from '@mdi/js'
import { useTitleImages } from '~/utils/titleImages'
import { findReleasesFast, NoServerStream, releaseKey, releaseLangs, releaseQuality, serverCandidates } from '~/utils/torrents'

// The player owns the whole window: no app bar, no drawer, no page scroll.
definePageMeta({ layout: false })

const route = useRoute()
const router = useRouter()
const downloads = useDownloadsStore()
const library = useLibraryStore()

const type = computed<MediaType>(() => route.query.type === 'tv' ? 'tv' : 'movie')
const id = computed(() => String(route.query.id ?? ''))
const season = computed(() => Number(route.query.s) || 0)
const episode = computed(() => Number(route.query.e) || 0)
// The downloads page knows exactly which file in a pack it wants played.
const fileIndex = computed(() => route.query.file == null ? null : Number(route.query.file))
const picked = ref<{ url?: string, magnet?: string } | null>(null)
watch(
  () => String(route.query.pick ?? ''),
  pick => { picked.value = pick ? takePendingRelease() : null },
  { immediate: true },
)
const magnet = computed(() => String(route.query.magnet ?? picked.value?.magnet ?? ''))
/** A release the picker resolved to a plain link — played as-is, no engine. */
const link = computed(() => String(route.query.url ?? picked.value?.url ?? ''))

/** What this playback is remembered as — no id (a bare magnet) means nothing. */
const key = computed(() => id.value ? progressKey(type.value, id.value, season.value, episode.value) : '')

/** This title has a copy in the engine — one that may well need no network. */
const downloaded = computed(() => !!downloads.cachedFor(key.value))

// TMDB is only asked for the IMDb id (what a source is keyed by) and a title
// to show while the torrent warms up.
const { data: media, error: mediaError } = useMediaDetail(type, id)

// The pause overlay draws the title's logo treatment instead of its name in
// text. `media.logo` only fills in when the detail request appended `images`,
// and `DETAIL_CORE` deliberately never does — that payload is every still of
// the title — so the logo is asked for on its own, exactly as the detail page
// does. Without this the overlay had nothing to draw and fell back to plain
// text on every pause.
const { data: titleArt, execute: loadTitleArt } = useTitleImages(type, id, () => media.value?.lang)
watch(id, value => {
  if (value && !import.meta.server)
    void loadTitleArt()
}, { immediate: true })
const titleLogo = computed(() => media.value?.logo || titleArt.value?.logo || null)

// Offline, TMDB answers nothing — but anything played before left its poster and
// title in the local library, and that is enough to draw this page and to keep
// recording progress against.
const known = computed(() => library.media[titleKey(type.value, id.value)] ?? null)
const title = computed(() => media.value ?? known.value)

const step = ref($t('Loading title…'))
const errorMsg = ref('')
const torrent = ref<Release | null>(null)
const torrentId = ref<number | null>(null)
const src = ref('')
const resolving = ref(false)

/**
 * The server streams the sources answered with, best first, and which one is
 * playing. Only direct-link playback has them: a torrent has no "other server"
 * to fail over to. The player lists them (server menu, quality menu) and asks
 * for a different index when one dies or you pick another copy.
 */
const candidates = ref<Release[]>([])
const activeCandidate = ref(0)

/** False until you pick a server by hand — the automatic pick reads "Auto" in the menu. */
const userPicked = ref(false)
/**
 * How many servers may fail before the answer is that the title is not playable.
 *
 * Unbounded, this walked the whole list: a title with twenty dead debrid links
 * spent twelve seconds on each of them, four minutes of a black rectangle
 * silently hopping between hosts with nothing on screen to say so. Bounded, the
 * fourth failure says what happened and leaves the menu open, which is both
 * sooner and honest — the same trade Free TV's auto-skip makes, for the same
 * reason. The count is per attempt: Retry, another quality, or another title
 * all start it again.
 */
const MAX_FAILOVERS = 4
const failovers = ref(0)

/** Shown as an OSD toast by the freshly mounted player after an auto-failover. */
const failoverNotice = ref('')

/** The playing server, for the info bar — the host you added it by. */
function viaHost(r: Release | null) {
  return r?.via ? hostOf(r.via) : ''
}

/**
 * The Quality menu introduces itself once per title when two or more
 * resolutions are on offer — cleared by the player the moment it has.
 */
const qualityPromptPending = ref(false)

/** Stream-only mode found nothing to stream — a different message, and fix, than a plain failure. */
const noServerStream = ref(false)

/**
 * Every direct link turned out to be the cloud service's error clip. The fix
 * is the torrent, for this title, without touching How Play works — the
 * cloud may be back tomorrow.
 */
const cloudDown = ref(false)
/** Set by "Play from the torrent instead"; read by `start`, cleared after. */
const forceTorrent = ref(false)

const settings = useSettingsStore()

// Flipping How Play works mid-playback re-resolves at once: Direct play
// picks up server streams, Torrent engine lets magnets back in — no
// re-entering the title, no hunting for a refresh.
watch(() => settings.allowTorrents, (now, before) => {
  // A source chosen in the picker is the stream they asked for — flipping
  // Direct/Best must not throw that pick away and search again.
  if (now !== before && startedOnce() && !magnet.value && !link.value)
    start()
})

/** Did this page already attempt playback? Guards the toggle watcher above. */
function startedOnce() {
  return !!(src.value || errorMsg.value || noServerStream.value)
}

/** The engine readers this page is holding, so that leaving lets go of them. */
let head: Priming | null = null
let tail: AbortController | null = null

/**
 * Tell the engine which bytes the player is about to want, in the order it
 * wants them.
 *
 * Both readers exist to make the engine's picker prefer the pieces mpv is
 * about to ask for, and they are deliberately not concurrent: the picker takes
 * one piece per open reader in turn, so a tail opened beside the head halves
 * the rate of the only bytes that can start the film. Worse, the tail used to
 * be opened *here* — before the component had mounted, before the layout wait
 * and before the readiness probe — which left the end of the film as the whole
 * priority list for several seconds while the player waited on byte zero. That
 * is the download that reached two percent with nothing to show for it.
 *
 * So: the head first, the tail once there is enough to open the file with, and
 * the head let go once the index is in — by then mpv's own reader is the one
 * the engine should be following.
 */
function prime(id: number, index: number) {
  releasePriming()
  const reader = primeHead(id, index)
  const stop = new AbortController()
  head = reader
  tail = stop
  void reader.ready.then(async ok => {
    if (!ok || stop.signal.aborted)
      return
    await primeTail(id, index, stop.signal)
    if (head === reader)
      reader.abort()
  })
}

/** Let go of both readers: another start, or leaving the page. */
function releasePriming() {
  head?.abort()
  head = null
  tail?.abort()
  tail = null
}

// The downloads store already polls every torrent's stats for the whole app, so
// a second poll of this one would only ask the engine the same question twice.
const stats = computed(() => downloads.torrents.find(t => t.id === torrentId.value)?.stats ?? null)

// Bumped on every start and on the way out, so a lookup that lands after you
// left the page — or jumped to another episode — doesn't reach back in and give
// the connection to something nobody is watching. (The trick useMediaFeed uses.)
let generation = 0

// --- Stream only with nothing instant ---------------------------------------
// A cloud (debrid) source hands back a link that plays at once; a torrent-only
// source can't, whatever the mode says. So instead of quietly streaming the
// torrent, Play asks — once, if "Always do this" is ticked — whether to keep
// the film while it plays or stream it from a temporary cache.
type Fallback = 'download' | 'stream' | 'cancel'
const fallbackOpen = ref(false)
const fallbackAlways = ref(false)
let fallbackAnswer: ((choice: Fallback) => void) | null = null

function askFallback() {
  fallbackAlways.value = false
  fallbackOpen.value = true
  return new Promise<Fallback>(resolve => (fallbackAnswer = resolve))
}

function answerFallback(choice: Fallback) {
  fallbackOpen.value = false
  if (fallbackAlways.value && choice === 'download')
    settings.allowTorrents = true
  if (fallbackAlways.value && choice === 'stream')
    settings.askBeforeTorrentStream = false
  fallbackAnswer?.(choice)
  fallbackAnswer = null
}

/** Where a download made from the question lands, shown in it. */
const fallbackFolder = computed(() => settings.downloadDir || downloads.resolvedDir)

async function start() {
  const mine = ++generation
  const startedAt = Date.now()
  releasePriming()
  errorMsg.value = ''
  noServerStream.value = false
  cloudDown.value = false
  src.value = ''
  resolving.value = true
  torrent.value = null
  candidates.value = []
  activeCandidate.value = 0
  userPicked.value = false
  failovers.value = 0
  failoverNotice.value = ''

  // Stream only, no instant source, nothing hand-picked: ask first.
  let allowTorrents = !!(magnet.value || settings.allowTorrents || forceTorrent.value)
  forceTorrent.value = false
  if (!allowTorrents && !link.value && settings.sources.length && !settings.hasInstantSource && settings.askBeforeTorrentStream) {
    const choice = await askFallback()
    if (mine !== generation)
      return
    if (choice === 'cancel') {
      resolving.value = false
      leave()
      return
    }
    allowTorrents = choice === 'download'
  }

  try {
    // ?magnet=… hand-picks the release and skips the lookup — that's how the
    // downloads page replays something already in the engine, and the only
    // path that works with no sources configured.
    const started = await downloads.start(key.value, {
      // The detail page already knows who this is: when it hands the lookup
      // over on the link, the sources are asked without any TMDB round trip.
      // Waited for only if that param is missing.
      imdbId: async () => {
        if (route.query.imdb)
          return String(route.query.imdb)
        step.value = $t('Loading title…')
        await until(() => !!media.value || !!mediaError.value).toBe(true, { timeout: 20_000 })
        return media.value?.imdbId
      },
      // An anime is also known to Kitsu, which some addons index it by — see
      // utils/kitsu. Looked up only if the IMDb id is missing or finds nothing.
      altId: async () => {
        const m = media.value
        if (!m || kindOf(m) !== 'anime')
          return null
        return kitsuIdFor(m.title, { movie: m.type === 'movie', season: season.value || 1, year: Number(m.year) || undefined })
      },
      // Read only once the lookup above has answered, so a download the app
      // never filed under this title can still be recognised by its name.
      named: () => title.value,
      magnet: magnet.value,
      url: link.value,
      season: season.value,
      episode: episode.value,
      fileIndex: fileIndex.value,
      // Default Play follows the toggle. A magnet (or URL) from the picker
      // is a source they named — it plays even while Play is Direct-only.
      allowTorrents,
      // Race the sources: first healthy answer plays, slower ones join the
      // candidate list as they land (see below).
      fast: true,
      onAlternativesLate: late => {
        if (mine !== generation)
          return
        const known = new Set(candidates.value.map(releaseKey))
        candidates.value = [...candidates.value, ...late.filter(r => !known.has(releaseKey(r)))]
        // Re-ranking may shuffle indexes; the playing copy keeps its place. Ranked
        // with torrents allowed whenever this playback allows them — ranked as
        // links-only, a late answer used to drop every torrent runner-up at once.
        candidates.value = serverCandidates(candidates.value, undefined, undefined, !!(magnet.value || settings.allowTorrents))
        activeCandidate.value = playingIndex()
      },
      onStep: value => (step.value = value),
    })

    if (mine !== generation)
      return

    torrent.value = started.torrent
    // A direct link has no torrent behind it, so there are no stats to read.
    torrentId.value = started.url ? null : started.id

    // Pause everything else before the stream starts, so the first buffer gets
    // the whole connection. Nothing to pause for a finished torrent — see `focus`.
    // Non-blocking: the player starts immediately while focus catches up.
    void downloads.focus(started.id)
    // Point the swarm at the opening bytes, then at the index — see `prime`.
    if (!started.url && started.id >= 0)
      prime(started.id, started.index)

    resolving.value = false
    step.value = $t('Buffering…')
    src.value = started.url ? linkWithHeaders(started.url, started.torrent) : streamUrl(started.id, started.index)

    // Both kinds carry their runners-up now: a server stream its other servers,
    // a torrent the other copies the sources answered with. The player's menus
    // and the failover below walk one list either way.
    // A torrent's runners-up arrive without the copy that is playing; it goes
    // first, so the list reads "this one, then these" for both kinds. Without
    // it, the playing torrent's slot was taken by its best runner-up — which a
    // failure then skipped, and the Server menu showed as the one playing.
    const others = started.alternatives ?? []
    candidates.value = started.torrent && !started.url && !others.some(r => releaseKey(r) === releaseKey(started.torrent!))
      ? [started.torrent, ...others]
      : others
    activeCandidate.value = playingIndex()
    // `serverCandidates` already ranks 1080p first (ahead of 4K, which is
    // slower to start and more likely to buffer). Keep that automatic choice
    // visible in the player pill, rather than opening a quality menu over the
    // movie and preventing the desktop controls from fading away.
    qualityPromptPending.value = false

    // A hand-picked link (the release picker's play button) arrives without its
    // siblings: the picker navigated straight here, so no ranking ever ran. Ask
    // the sources once more, quietly, so the Server and Quality menus still have
    // something to list — and a dead link still has somewhere to fail over to.
    if (started.url && !candidates.value.length)
      void fetchCandidates(started.url)
  }
  catch (e) {
    if (mine !== generation)
      return
    noServerStream.value = e instanceof NoServerStream
    errorMsg.value = e instanceof Error ? e.message : String(e)
    // Keep the loading spinner visible for at least 400ms so the user sees
    // that something was attempted, rather than a flash of spinner → error
    // that looks like a crash. If the search already took longer, clear
    // immediately.
    const elapsed = Date.now() - startedAt
    const minDisplay = 400
    if (elapsed < minDisplay)
      await new Promise(r => setTimeout(r, minDisplay - elapsed))
    if (mine !== generation)
      return
    resolving.value = false
  }
}

/**
 * The playing server died (or you picked another one from the menu): move down
 * the candidate list and remount the player on that URL. The `:key="src"` on
 * `<mpv-player>` makes the swap a fresh start, and progress already recorded by
 * the old mount is what the new one resumes from — so a film continues where it
 * stopped, on a different server.
 */
function useCandidate(index: number, manual = true) {
  const next = candidates.value[index]
  if (!next || index === activeCandidate.value)
    return
  if (manual) {
    userPicked.value = true
    failovers.value = 0
  }
  activeCandidate.value = index
  torrent.value = next
  errorMsg.value = ''

  // A server stream is a URL swap and nothing else: the player remounts on
  // `:key="src"` and plays it. A torrent has to be handed to the engine first,
  // and until this it could not be a candidate at all — so a swarm that never
  // woke up was the end of the attempt, with three more copies of the same
  // film sitting unused in the list the picker had already ranked.
  if (!next.url && next.magnet) {
    void playMagnet(next.magnet)
    return
  }
  if (!next.url)
    return
  torrentId.value = null
  src.value = linkWithHeaders(next.url, next)
}

/**
 * Start one of the other copies through the engine, keeping the rest of the
 * list intact so a second failure moves on again rather than starting over.
 */
async function playMagnet(magnetUrl: string) {
  const mine = ++generation
  releasePriming()
  src.value = ''
  torrentId.value = null
  resolving.value = true
  try {
    const started = await downloads.start(key.value, {
      magnet: magnetUrl,
      season: season.value,
      episode: episode.value,
      allowTorrents: true,
      onStep: value => (step.value = value),
    })
    if (mine !== generation)
      return
    torrentId.value = started.url ? null : started.id
    void downloads.focus(started.id)
    if (!started.url && started.id >= 0)
      prime(started.id, started.index)
    resolving.value = false
    step.value = $t('Buffering…')
    src.value = started.url || streamUrl(started.id, started.index)
  }
  catch (e) {
    if (mine !== generation)
      return
    resolving.value = false
    errorMsg.value = e instanceof Error ? e.message : String(e)
  }
}

/** Where the playing copy sits in `candidates`: by release when there is one, else by URL. */
function playingIndex() {
  const playing = torrent.value
  const at = playing
    ? candidates.value.findIndex(r => releaseKey(r) === releaseKey(playing))
    : candidates.value.findIndex(r => r.url === src.value)
  return Math.max(0, at)
}

/** Playback of the current server failed — silently move to the next one, if any. */
function onPlaybackFailed(reason?: 'stub' | 'dead' | 'refused') {
  /** Out of links to try, and the last one was an error card: say so, offer the torrent. */
  const cardsOnly = () => {
    if (reason !== 'stub')
      return false
    cloudDown.value = true
    src.value = ''
    errorMsg.value = $t('Your cloud (debrid) service sent its "unavailable" clip instead of the film. It is down or refusing this title right now.')
    return true
  }
  if (!candidates.value.length) {
    cardsOnly()
    return
  }
  let following = activeCandidate.value + 1
  // An error card speaks for the whole service: every other link the same
  // source gave answers with the same clip, so they are passed over rather
  // than played one by one for half a minute each.
  if (reason === 'stub') {
    const via = candidates.value[activeCandidate.value]?.via
    if (via) {
      const other = candidates.value.findIndex((c, i) => i > activeCandidate.value && c.via !== via)
      following = other === -1 ? candidates.value.length : other
    }
  }
  const next = candidates.value[following]
  if (!next) {
    if (cardsOnly())
      return
    errorMsg.value = $t('None of the {count} servers for this title would play. Try another quality, or another release.', { count: candidates.value.length })
    return
  }
  if (failovers.value >= MAX_FAILOVERS) {
    if (cardsOnly())
      return
    errorMsg.value = $t('Tried {count} servers for this title and none of them played. Pick one by hand from the server menu, or try another release.', { count: failovers.value })
    return
  }
  failovers.value++
  // The swap itself is silent; the new player mount announces it (osd-on-start).
  failoverNotice.value = `${$t('Switched to')} ${hostOf(next.via ?? '') || next.source}`
  useCandidate(following, false)
}

/**
 * The other direct links for this title, for a playback that started without
 * them. Runs only after the player already has its stream — a miss changes
 * nothing on screen, it just leaves the menus thinner than they might have been.
 */
async function fetchCandidates(playingUrl: string) {
  const mine = generation
  try {
    const imdbId = route.query.imdb
      ? String(route.query.imdb)
      : (await until(() => !!media.value || !!mediaError.value).toBe(true, { timeout: 20_000 }), media.value?.imdbId)
    if (!imdbId || mine !== generation || candidates.value.length)
      return

    const found = await findReleasesFast(imdbId, season.value, episode.value, { graceMs: 0 })
    const rest = serverCandidates(found).filter(r => r.url !== playingUrl)
    if (mine !== generation || !rest.length || candidates.value.length)
      return

    // The playing link sits at [0] even though it arrived from outside this
    // search — every menu and the failover walk indexes into one list.
    const current = torrent.value?.url === playingUrl
      ? torrent.value!
      : { name: '', hash: '', url: playingUrl, fileIdx: null, file: null, seeders: 0, size: '', bytes: 0, source: '', quality: '', magnet: '' }
    if (!current.quality)
      current.quality = releaseQuality({ name: current.name, title: current.name })
    candidates.value = [current, ...rest]
    activeCandidate.value = 0
    qualityPromptPending.value = false
  }
  catch {
    // Thinner menus are the whole cost; playback itself is already running.
  }
}

function goToSources() {
  leave()
  navigateTo(localePath('/settings/sources'))
}

/** Host of a source base URL — "https://addon.example/manifest…" → "addon.example". */
function hostOf(via: string) {
  try {
    return new URL(via).host
  }
  catch {
    return via
  }
}

const RESOLUTION = /\b(2160p|1440p|4k|2k|1080p|720p|480p)\b/i

function qualityLabel(r: Release) {
  const q = releaseQuality(r)
  const m = (q.match(RESOLUTION) ?? r.name.match(RESOLUTION))?.[1]?.toLowerCase() ?? ''
  if (m === '2160p' || m === '4k')
    return '4K'
  if (m === '1440p' || m === '2k')
    return '2K'
  if (m === '1080p')
    return '1080P'
  if (m === '720p')
    return '720P'
  if (m === '480p')
    return '480P'
  return (q || '').toUpperCase() || $t('Unknown')
}

/** One server row — quality and size, not the same hostname five times. */
function serverLabel(r: Release, index: number) {
  const q = qualityLabel(r)
  const known = q !== $t('Unknown')
  const parts = [known ? q : '', r.size].filter(Boolean)
  if (parts.length)
    return parts.join(' · ')
  const short = r.name.length > 44 ? `${r.name.slice(0, 41)}…` : r.name
  return short || (r.source !== 'unknown' ? r.source : '') || hostOf(r.via ?? '') || $t('Server {n}', { n: index + 1 })
}

function serverDetail(r: Release) {
  const host = r.source !== 'unknown' ? r.source : hostOf(r.via ?? '')
  const name = r.name && r.name !== host ? r.name : ''
  return [host, name].filter(Boolean).join(' · ')
}

/**
 * What the player's two menus show. Servers list everything; qualities list the
 * first candidate per resolution, since five copies of 1080p are one choice.
 */
const candidateMenus = computed(() => {
  if (!candidates.value.length)
    return null
  const detail = (r: typeof candidates.value[number]) => serverDetail(r)
  const servers = candidates.value.map((r, index) => ({
    index,
    label: serverLabel(r, index),
    quality: qualityLabel(r),
    langs: releaseLangs(`${r.name} ${r.file ?? ''} ${r.quality}`),
    detail: detail(r),
  }))
  const seen = new Map<string, number>()
  const qualities: { index: number, label: string, detail?: string }[] = []
  for (const [index, r] of candidates.value.entries()) {
    const label = qualityLabel(r)
    if (!seen.has(label)) {
      seen.set(label, index)
      // Still offered — it may be the only copy there is — but a phone with no
      // hardware 4K decoder plays one as a frozen picture over audio that keeps
      // going, and the menu is the one place to say so before it happens.
      const decodes = label === '4K' ? uhdPlayable(`${r.name} ${r.quality}`) : true
      qualities.push(decodes === false
        ? { index, label, detail: $t('This device can\'t decode 4K smoothly') }
        : { index, label })
    }
  }
  return { servers, qualities }
})

// Driven by the route alone — the title resolving is `start`'s business now, so
// that a downloaded film never waits on TMDB. Fires again if you jump straight
// to another episode without leaving the player.
watch(
  () => [key.value, magnet.value, link.value, fileIndex.value].join('|'),
  () => start(),
  { immediate: true },
)

// Leaving the player stops the download and hands the connection back to
// whatever was paused for it. Every exit route unmounts — Esc, Back, the browser
// history, switching to another title — so this is the one place it belongs.
onBeforeUnmount(() => {
  generation++
  releasePriming()
  downloads.release()
})

// A magnet, a link and a copy on disk all need no TMDB, so a failed lookup is
// only a failure to play when the sources were the plan.
const failure = computed(() => errorMsg.value
  || (mediaError.value && !magnet.value && !link.value && !downloaded.value
    ? $t('Couldn\'t load this title from TMDB.')
    : ''))

const heading = computed(() => {
  const name = title.value?.title ?? (route.query.title as string) ?? $t('Loading…')
  return season.value && episode.value ? `${name} · S${season.value}E${episode.value}` : name
})

const progressPct = computed(() => {
  const s = stats.value
  return s?.total_bytes ? Math.min(100, (s.progress_bytes / s.total_bytes) * 100) : 0
})

/**
 * The engine's own verdict, if it has one.
 *
 * It polls this torrent already, and `state: 'error'` with a sentence under it
 * has been in every one of those answers all along — unread. A download the
 * engine had given up on showed "Buffering · — · 0 peers · 0%" until someone
 * pressed Back, because every reading the player has is a reading of a swarm,
 * and there was no swarm: a full disk had stopped it before the first piece.
 */
const fault = computed(() => engineFault(stats.value))

const speed = computed(() => stats.value?.live?.download_speed.human_readable ?? '—')
const peers = computed(() => stats.value?.live?.snapshot.peer_stats.live ?? 0)
/**
 * One line for the player's "buffering" notice, where there's no room for a
 * table. Empty while a direct link plays: there is no swarm to report on, and
 * "0 peers" reads as a fault rather than as "not applicable".
 */
const statusLine = computed(() =>
  stats.value ? `${speed.value} · ${peers.value} peers · ${progressPct.value.toFixed(0)}%` : '')

const backdrop = computed(() => backdropUrl(title.value?.backdrop, 'w1280'))

// What the end-of-playback screen offers. The show's season list carries an
// episode count per season, which is all the rollover needs.
/** Playing a Stream only copy — the player then fetches as it plays, not to the end. */
const streamOnly = computed(() => downloads.isStreamOnly(downloads.torrents.find(t => t.id === torrentId.value)?.info_hash))

/** The episode after this one, if the show has one. */
const nextTarget = computed(() => {
  if (!media.value || !season.value || !episode.value)
    return null
  return nextEpisode(media.value.seasons, { season: season.value, episode: episode.value, watched: true })
})

const next = computed(() => {
  const target = nextTarget.value
  if (!target)
    return null
  return {
    to: watchLink('tv', id.value, target.season, target.episode),
    label: $t('Next · S{season} E{episode}', { season: target.season, episode: target.episode }),
  }
})

// This episode's name and synopsis for the pause screen — a series pauses on
// the episode it is, not on the show's blurb. Nothing for a film.
const episodeInfo = ref<{ name: string, overview: string, runtime: number } | null>(null)
watch(() => [id.value, season.value, episode.value] as const, async ([showId, s, e]) => {
  episodeInfo.value = null
  if (!showId || !s || !e)
    return
  const got = await loadEpisode(showId, s, e).catch(() => null)
  if (got && showId === id.value && s === season.value && e === episode.value)
    episodeInfo.value = { name: got.name, overview: got.overview, runtime: got.runtime }
}, { immediate: true })

/** Seconds the title really runs, for spotting a source's error clip. 0 when unknown. */
const expectedSeconds = computed(() => ((episodeInfo.value?.runtime || media.value?.runtime) ?? 0) * 60)

/** The credits are rolling: get the next episode ready — see `preloadNext`. */
function onNearEnd() {
  const target = nextTarget.value
  if (target)
    void downloads.preloadNext(key.value, target, String(route.query.imdb ?? '') || media.value?.imdbId)
}

function leave() {
  if (router.options.history.state.back)
    router.back()
  else
    navigateTo(localePath('/'))
}

// preventDefault marks the press as used up, which is how the remote's back key
// knows it doesn't also have to go back a page (see plugins/dpad.client.ts).
useEventListener(window, 'keydown', (e: KeyboardEvent) => {
  if (e.key === 'Escape') {
    e.preventDefault()
    leave()
  }
})
</script>

<template>
  <v-app>
    <v-main class="h-dvh overflow-hidden bg-black text-white">
      <!-- Always mounted — shows resolving/loading overlay while src is empty. -->
      <mpv-player
        :key="src || 'idle'"
        :src="src"
        :resolving="resolving"
        :step="step"
        :status="statusLine"
        :fault="fault"
        :media="title"
        :next="next"
        :episode-title="episodeInfo?.name"
        :stream-only="streamOnly"
        :episode-overview="episodeInfo?.overview"
        :imdb-id="media?.imdbId"
        :title="title?.title ?? String(route.query.title ?? '')"
        :year="title?.year"
        :logo="titleLogo"
        :season="season"
        :episode="episode"
        :quality="torrent?.quality"
        :candidates="candidateMenus"
        :active-candidate="activeCandidate"
        :osd-on-start="failoverNotice"
        :auto-open-quality="qualityPromptPending && !userPicked"
        :expected-seconds="expectedSeconds"
        @failed="onPlaybackFailed"
        @near-end="onNearEnd"
        @use-candidate="(i: number) => useCandidate(i)"
        @auto-opened="qualityPromptPending = false"
        @back="leave"
      >
        <template #start>
          <v-btn icon variant="text" density="comfortable" :title="$t('Back (Esc)')" @click="leave">
            <v-icon :icon="mdiArrowLeft" />
          </v-btn>
        </template>

        <template #info>
          <div class="flex min-w-0 items-center gap-4">
            <div class="min-w-0">
              <div class="truncate text-title-medium">
                {{ heading }}
              </div>
              <div v-if="torrent" class="truncate text-body-small opacity-50">
                {{ torrent.quality }} · {{ torrent.size }} · {{ torrent.source }}
                <template v-if="viaHost(torrent)">
                  · {{ viaHost(torrent) }}
                </template>
              </div>
            </div>

            <v-spacer />

            <!-- Swarm figures, so only while a torrent is what's playing. -->
            <div v-if="stats" class="flex shrink-0 items-center gap-3 text-body-small opacity-70">
              <span class="flex items-center gap-1" :title="$t('Download speed')">
                <v-icon :icon="mdiDownload" size="14" />{{ speed }}
              </span>
              <span class="flex items-center gap-1" :title="$t('Connected peers')">
                <v-icon :icon="mdiAccountGroup" size="14" />{{ peers }}
              </span>
              <span class="tabular-nums" :title="$t('Downloaded')">{{ progressPct.toFixed(0) }}%</span>
              <span class="hidden opacity-50 xl:inline">
                {{ bytesText(stats.progress_bytes) }} / {{ bytesText(stats.total_bytes) }}
              </span>
            </div>
          </div>
        </template>
      </mpv-player>

      <!-- Error overlay: sits on top of the player when source resolution failed.
           The `!resolving` guard keeps it hidden while sources are still being
           searched — the player's own loading overlay is what the user should
           see during that window. -->
      <v-overlay
        v-if="failure && !src && !resolving"
        class="place-items-center"
        persistent
      >
        <img
          v-if="backdrop"
          :src="backdrop"
          alt=""
          class="absolute inset-0 h-full w-full object-cover opacity-20 blur-2xl"
        >

        <div class="relative flex max-w-xl flex-col items-center gap-3 px-6 text-center">
          <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
          <div class="text-title-large">
            {{ cloudDown ? $t('Your cloud service is unavailable') : noServerStream ? $t('Your sources only provide downloads') : $t('Nothing to play') }}
          </div>
          <p class="text-body-medium opacity-70">
            {{ failure }}
          </p>
          <!-- Stream-only mode's fixes: add a streaming source, or let torrents back in. -->
          <div v-if="noServerStream" class="mt-2 flex flex-wrap justify-center gap-2">
            <v-btn
              variant="tonal"
              color="primary"
              :prepend-icon="mdiPowerPlugOutline"
              @click="goToSources"
            >
              {{ $t('Add a source') }}
            </v-btn>
            <v-btn
              variant="tonal"
              :prepend-icon="mdiDownload"
              @click="settings.allowTorrents = true; start()"
            >
              {{ $t('Use torrent engine') }}
            </v-btn>
            <v-btn variant="text" :prepend-icon="mdiArrowLeft" @click="leave">
              {{ $t('Back') }}
            </v-btn>
          </div>
          <div v-else-if="cloudDown" class="mt-2 flex flex-wrap justify-center gap-2">
            <v-btn color="primary" variant="flat" :prepend-icon="mdiDownload" @click="forceTorrent = true; start()">
              {{ $t('Play from the torrent instead') }}
            </v-btn>
            <v-btn variant="tonal" :prepend-icon="mdiReload" @click="start">
              {{ $t('Try again') }}
            </v-btn>
            <v-btn variant="text" :prepend-icon="mdiArrowLeft" @click="leave">
              {{ $t('Back') }}
            </v-btn>
          </div>
          <div v-else class="mt-2 flex gap-2">
            <v-btn variant="tonal" :prepend-icon="mdiReload" @click="start">
              {{ $t('Try again') }}
            </v-btn>
            <v-btn variant="text" :prepend-icon="mdiArrowLeft" @click="leave">
              {{ $t('Back') }}
            </v-btn>
          </div>
        </div>
      </v-overlay>

      <!-- Stream only, and no source can stream this instantly: how to play it.
           Real buttons throughout, so a remote answers it as easily as a mouse;
           Back is Cancel. -->
      <v-dialog
        :model-value="fallbackOpen"
        max-width="620"
        persistent
        @update:model-value="open => !open && fallbackOpen && answerFallback('cancel')"
      >
        <v-card rounded="xl" class="pa-2">
          <v-card-title class="text-title-large font-bold">
            {{ $t('No instant source for this') }}
          </v-card-title>
          <v-card-text class="flex flex-col gap-4">
            <p class="text-body-medium opacity-85">
              {{ $t('Your sources answer with torrents only, so this can\'t start instantly the way a cloud (debrid) source does. Choose how to play it:') }}
            </p>

            <div class="flex flex-col gap-3 rounded-xl border border-outline-variant p-4">
              <div class="flex flex-wrap items-center gap-3">
                <v-btn :prepend-icon="mdiDownload" color="primary" size="large" @click="answerFallback('download')">
                  {{ $t('Play while downloading') }}
                </v-btn>
                <v-chip size="small" color="primary" variant="tonal" tabindex="-1">
                  {{ $t('Recommended') }}
                </v-chip>
              </div>
              <p class="text-body-medium opacity-80">
                {{ $t('Starts as the first pieces arrive, and keeps the whole file in Downloads.') }}
              </p>
              <p v-if="fallbackFolder" class="break-all text-body-small opacity-70">
                {{ $t('Saving to: {folder}', { folder: fallbackFolder }) }}
              </p>
              <drive-picker />
            </div>

            <div class="flex flex-col gap-3 rounded-xl border border-outline-variant p-4">
              <v-btn :prepend-icon="mdiPlay" variant="tonal" size="large" class="self-start" @click="answerFallback('stream')">
                {{ $t('Stream with a temporary cache') }}
              </v-btn>
              <p class="text-body-medium opacity-80">
                {{ $t('Fetches a few minutes ahead of the picture and deletes it when you leave. Nothing is kept.') }}
              </p>
            </div>

            <v-checkbox v-model="fallbackAlways" density="compact" hide-details :label="$t('Always do this')" />
          </v-card-text>
          <v-card-actions>
            <v-spacer />
            <v-btn variant="text" @click="answerFallback('cancel')">
              {{ $t('Cancel') }}
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-dialog>
    </v-main>
  </v-app>
</template>
