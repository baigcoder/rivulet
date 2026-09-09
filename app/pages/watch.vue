<script lang="ts" setup>
import type { MediaType } from '~/utils/tmdb'
import type { PieceMap, Release } from '~/utils/torrents'
import {
  mdiAccountGroup,
  mdiAlertCircleOutline,
  mdiArrowLeft,
  mdiDownload,
  mdiPowerPlugOutline,
  mdiReload,
} from '@mdi/js'
import { useTitleImages } from '~/utils/titleImages'
import { canonHash, ENGINE, findReleasesFast, headBuffered, heldSrc, listTorrents, magnetForHash, NoServerStream, pickVideoFile, pieceMap, playUrl, releaseKey, releaseLangs, releaseQuality, serverCandidates, streamParts, streamUrl, torrentAction, torrentDetails, torrentHaves } from '~/utils/torrents'

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
const infoHash = computed(() => String(route.query.hash ?? ''))
const picked = ref<{ url?: string, magnet?: string } | null>(null)
watch(
  () => String(route.query.pick ?? ''),
  pick => { picked.value = pick ? takePendingRelease() : null },
  { immediate: true },
)
const magnet = computed(() => String(route.query.magnet ?? picked.value?.magnet ?? (infoHash.value ? magnetForHash(infoHash.value) : '')))
/** A release the picker resolved to a plain link — played as-is, no engine. */
const link = computed(() => String(route.query.url ?? picked.value?.url ?? ''))

/** What this playback is remembered as — no id (a bare magnet) means nothing. */
const key = computed(() => id.value ? progressKey(type.value, id.value, season.value, episode.value) : '')

/** This title has a copy in the engine — one that may well need no network. */
const downloaded = computed(() => !!downloads.cachedFor(key.value))

// TMDB is only asked for the IMDb id (what a source is keyed by) and a title
// to show while the torrent warms up.
const { data: media, error: mediaError } = useMediaDetail(type, id)

// Offline, TMDB answers nothing — but anything played before left its poster and
// title in the local library, and that is enough to draw this page and to keep
// recording progress against.
const known = computed(() => library.media[titleKey(type.value, id.value)] ?? null)
const title = computed(() => media.value ?? known.value)

/**
 * The transparent title treatment the pause overlay draws instead of plain text.
 *
 * It cannot come from `media.logo`: `DETAIL_CORE` appends credits and ratings and
 * never `images`, so that field is null for every title and the overlay fell back
 * to the text branch every time. Logos live in the separate `/images` request the
 * detail page already makes — and `useTitleImages` keys its `useAsyncData` by
 * title, so arriving from a title's own page costs nothing at all.
 */
const { data: titleArt, execute: loadTitleArt } = useTitleImages(type, id)
const logo = computed(() => media.value?.logo || titleArt.value.logo || null)

const step = ref($t('Loading title…'))
const errorMsg = ref('')
const torrent = ref<Release | null>(null)
const torrentId = ref<number | null>(null)
const src = ref('')
const resolving = ref(false)

/**
 * The logo is fetched only once the stream is up. The overlay is not on screen
 * until someone pauses, so this has no business competing with the first buffer —
 * and by the time a pause happens it has long since landed.
 */
const artFor = ref('')
watch([src, id], ([url, tid]) => {
  if (!url || !tid || artFor.value === tid)
    return
  artFor.value = tid
  // Arriving from the title's own page, the request is already in the cache.
  if (!titleArt.value.logo && !titleArt.value.stills.length)
    void loadTitleArt()
})

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
/** Stream-only mode found nothing to stream — a different message, and fix, than a plain failure. */
const noServerStream = ref(false)
const offerSources = computed(() =>
  noServerStream.value || /sources have nothing/i.test(errorMsg.value))

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

// The downloads store already polls every torrent's stats for the whole app, so
// a second poll of this one would only ask the engine the same question twice.
const stats = computed(() => downloads.torrents.find(t => t.id === torrentId.value)?.stats ?? null)

// Bumped on every start and on the way out, so a lookup that lands after you
// left the page — or jumped to another episode — doesn't reach back in and give
// the connection to something nobody is watching. (The trick useMediaFeed uses.)
let generation = 0

/**
 * Open the engine HTTP stream the moment this hash is on the list.
 * addTorrent used to wait for the file list (and the 180s POST) while
 * Downloads already showed the torrent — first Play stuck on metadata,
 * second Play streamed.
 */
async function openListedStream(hash: string, index: number | null, mine: number) {
  const deadline = Date.now() + 180_000
  while (Date.now() < deadline) {
    if (mine !== generation || src.value)
      return
    const t = downloads.torrents.find(x => canonHash(x.info_hash) === canonHash(hash))
      ?? (await listTorrents().catch(() => [])).find(x => canonHash(x.info_hash) === canonHash(hash))
      ?? null
    if (t) {
      if (mine !== generation || src.value)
        return
      torrentId.value = t.id
      step.value = $t('Buffering…')
      await torrentAction(t.id, 'start').catch(() => {})
      if (mine !== generation || src.value)
        return
      let file = index ?? 0
      const listed = t.files?.length ? t.files : (await torrentDetails(t.id).catch(() => null))?.files
      if (mine !== generation || src.value)
        return
      if (listed?.length) {
        const picked = pickVideoFile(listed, index, { season: season.value, episode: episode.value })
        if (picked != null)
          file = picked
      }
      src.value = streamUrl(t.id, file)
      resolving.value = false
      void downloads.focus(t.id)
      return
    }
    await new Promise(r => setTimeout(r, 150))
  }
}

async function start() {
  const mine = ++generation
  const startedAt = Date.now()
  errorMsg.value = ''
  noServerStream.value = false
  failoverNotice.value = ''

  // Torrent engine Play opens the stream as soon as we know the id. First
  // pieces play while the rest downloads — no TMDB, no magnet re-add, no
  // sparse file:// path. Downloads sends `src`/`tid`; a title already in
  // the engine is in the downloads store.
  const readySrc = String(route.query.src ?? '')
  const readyTid = Number(route.query.tid)
  const cached = key.value ? downloads.cachedFor(key.value) : null
  const listed = (Number.isFinite(readyTid) ? downloads.torrents.find(t => t.id === readyTid) : null)
    ?? (infoHash.value ? downloads.torrents.find(t => canonHash(t.info_hash) === canonHash(infoHash.value)) : null)
    ?? (cached ? downloads.torrents.find(t => canonHash(t.info_hash) === canonHash(cached.hash)) : null)
    ?? null
  const named = streamParts(readySrc)
  const fromDownloads = !!readySrc && (readySrc.startsWith(ENGINE) || readySrc.startsWith('file:') || readySrc.startsWith('/'))
  const engineNow = listed ?? (named ? { id: named.id } : null)
  const engineAsked = !!(magnet.value || infoHash.value || fromDownloads || settings.allowTorrents)
  if (engineAsked && engineNow && (fromDownloads || listed)) {
    const hint = fileIndex.value
      ?? named?.index
      ?? cached?.file
      ?? null
    let index = hint ?? 0
    if (listed?.files?.length && fileIndex.value == null) {
      index = pickVideoFile(listed.files, hint, { season: season.value, episode: episode.value }) ?? index
    }
    resolving.value = false
    step.value = $t('Buffering…')
    torrentId.value = engineNow.id
    // File list is not on the poll. Downloads Play of a named `file://`
    // still needs it; Title Play must not wait — that delay was the
    // black first frame.
    const detailsP = listed && !listed.files?.[index]
      ? torrentDetails(listed.id)
      : Promise.resolve(null)
    const namedDisk = readySrc.startsWith('file:') || readySrc.startsWith('/')
    // The list endpoint carries no files, and `heldSrc` cannot name a path
    // without one. A finished copy has nothing to buffer while that request
    // runs, so it is worth waiting for; anything still downloading must not,
    // because that wait was the black first frame.
    const wantDisk = namedDisk || !!listed?.stats?.finished
    // Start the torrent before mpv opens the stream. Fire-and-forget here
    // was a black first Play: the engine was still paused from the last
    // leave() while player_start already ran.
    await torrentAction(engineNow.id, 'start').catch(() => {})
    if (mine !== generation)
      return
    let files = listed?.files
    if (wantDisk && !files?.[index]) {
      const extra = await detailsP
      if (mine !== generation)
        return
      files = extra?.files ?? files
    }
    else {
      void detailsP.then(extra => {
        if (mine !== generation || !extra?.files?.[index] || !torrent.value)
          return
        torrent.value = { ...torrent.value, file: extra.files[index]!.name }
      })
    }
    const hash = listed?.info_hash ?? infoHash.value ?? cached?.hash ?? ''
    const name = listed?.name || String(route.query.title ?? '')
    const playing = playingRelease(hash, name, {
      fileIdx: index,
      file: files?.[index]?.name ?? listed?.files?.[index]?.name ?? null,
      magnet: magnet.value || (hash ? magnetForHash(hash) : ''),
    })
    torrent.value = playing
    const playingHash = canonHash(hash)
    if (!candidates.value.some(r => canonHash(r.hash) === playingHash))
      candidates.value = [playing, ...candidates.value]
    if (!candidates.value.length)
      candidates.value = [playing]
    activeCandidate.value = Math.max(0, candidates.value.findIndex(r => canonHash(r.hash) === playingHash))
    userPicked.value = false
    // Quality list on screen before mpv opens, or the first paint is a
    // blank player with no 720p/1080p/4K control.
    void loadEngineQualities(playing)
    // Title Play always opens the engine HTTP stream first. A finished
    // `file://` hung the first mpv (cover trailer still holding the
    // decoder; 10-bit HEVC VO never mapped). Back then Play worked.
    // Downloads may name a disk path via `?src=` so seeking still works
    // there; `onDiskStuck` falls back to HTTP if that copy never frames.
    const http = streamUrl(engineNow.id, index)
    const disk = listed
      ? heldSrc({ ...listed, files: files ?? listed.files }, index, files?.[index])
      : http
    // Growing copies stay on the engine HTTP stream — that is what plays
    // from the first pieces, at whatever percent is on disk. A finished
    // copy opens as a path so the seek bar has a duration. Downloads used
    // to wait on the file list before navigating; the query `src` is already
    // the stream, so reuse it until we know the file is complete.
    const nextSrc = wantDisk ? disk : (readySrc.startsWith(ENGINE) ? readySrc : http)
    // Same engine stream: only refresh the Quality list. Assigning `src`
    // again (or clearing it first) remounts mpv onto a black 0:00.
    if (src.value !== nextSrc)
      src.value = nextSrc
    // Pause siblings after the stream is already opening so the first
    // frame does not wait on the downloads poll.
    await downloads.focus(engineNow.id)
    return
  }

  torrent.value = null
  candidates.value = []
  activeCandidate.value = 0
  userPicked.value = false
  // Do not blank an engine stream already opened by `onQueued`. That
  // assignment is player_stop + a second `[player] start`, which is the
  // Buffering 0% loop in the logs.
  if (src.value && !src.value.startsWith(ENGINE))
    src.value = ''
  resolving.value = true

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
        // Nothing coming out of the library carries the param — Continue
        // watching, Resume, the next episode and an episode row all build a
        // plain link — and the wait below is the whole of the delay before a
        // direct link is even asked for. The stored snapshot of anything played
        // or favourited before answers it for nothing, offline included.
        const local = known.value?.imdbId
        if (local)
          return local
        step.value = $t('Loading title…')
        await until(() => !!media.value || !!mediaError.value).toBe(true, { timeout: 20_000 })
        return media.value?.imdbId
      },
      // Read only once the lookup above has answered, so a download the app
      // never filed under this title can still be recognised by its name.
      named: () => title.value ?? (route.query.title ? { title: String(route.query.title) } : null),
      magnet: magnet.value,
      hash: infoHash.value || undefined,
      url: link.value,
      season: season.value,
      episode: episode.value,
      fileIndex: fileIndex.value,
      // Default Play follows the toggle. A magnet (or URL) from the picker
      // is a source they named — it plays even while Play is Direct-only.
      allowTorrents: !!(magnet.value || infoHash.value || settings.allowTorrents),
      // Title Play (no `?src=`) opens the engine HTTP stream even for a
      // finished copy — the first `file://` spawn after the cover trailer
      // is the black player. Downloads names a path and keeps seeking.
      preferStream: !readySrc,
      // Race the sources: first healthy answer plays, slower ones join the
      // candidate list as they land (see below).
      fast: true,
      onAlternativesLate: late => {
        if (mine !== generation || !late.length)
          return
        if (!torrent.value)
          torrent.value = late[0]!
        mergeEngineQualities(torrent.value, late)
      },
      onQueued: ({ hash, index }) => {
        if (mine !== generation)
          return
        void openListedStream(hash, index, mine)
      },
      onStep: value => (step.value = value),
    })

    if (mine !== generation)
      return

    torrent.value = started.torrent
    // Debrid links have no engine id; torrent playback keeps the id even off disk.
    torrentId.value = started.id >= 0 ? started.id : null

    resolving.value = false
    step.value = $t('Buffering…')
    const nextSrc = playUrl(started)
    if (src.value !== nextSrc)
      src.value = nextSrc
    // Pause siblings after the stream is already opening. Awaiting focus
    // here was a black first Play: metadata had landed, Downloads showed
    // the torrent, and mpv still had not been given a URL.
    if (started.id >= 0)
      void downloads.focus(started.id)

    // Quality / source menus. Engine Play lists magnets so you can switch
    // 720p/1080p/4K without leaving; Direct Play lists server URLs.
    if (started.id >= 0) {
      // Only reached when `started.torrent` is null, so it has no name to read.
      const current = started.torrent ?? playingRelease(
        started.hash,
        listed?.name || String(route.query.title ?? ''),
      )
      torrent.value = current
      mergeEngineQualities(current, started.alternatives ?? [])
      void loadEngineQualities(current)
    }
    else {
      candidates.value = started.alternatives ?? []
      activeCandidate.value = 0
      qualityPromptPending.value = false
      if (started.url && !candidates.value.length)
        void fetchCandidates(started.url)
    }
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

function playingRelease(hash: string, name: string, extra?: Partial<Release>): Release {
  return {
    name,
    hash,
    url: extra?.url ?? '',
    fileIdx: extra?.fileIdx ?? fileIndex.value,
    file: extra?.file ?? null,
    seeders: extra?.seeders ?? 0,
    size: extra?.size ?? '',
    bytes: extra?.bytes ?? 0,
    source: extra?.source ?? '',
    quality: extra?.quality || releaseQuality({ name, file: extra?.file ?? null }),
    magnet: extra?.magnet || (hash ? magnetForHash(hash) : ''),
    via: extra?.via,
  }
}

/**
 * Fill the Quality menu for torrent engine Play. Playback already started;
 * this only lists other magnets (and Direct URLs) so a 720p/1080p/4K switch
 * is a pick, not a trip back to the title.
 */
async function loadEngineQualities(playing: Release) {
  const mine = generation
  // Always keep the playing copy in the menu, even with no 1080p token and
  // no IMDb id yet — otherwise torrent Play has no Quality control at all.
  if (!candidates.value.length)
    candidates.value = [playing]

  let imdbId = String(route.query.imdb ?? known.value?.imdbId ?? '')
  if (!imdbId && id.value) {
    await until(() => !!media.value || !!mediaError.value).toBe(true, { timeout: 8_000 }).catch(() => {})
    if (mine !== generation)
      return
    imdbId = media.value?.imdbId ?? ''
  }
  if (!imdbId)
    return

  try {
    const found = await findReleasesFast(imdbId, season.value, episode.value, {
      graceMs: 0,
      needMagnet: true,
      onLate: late => {
        if (mine !== generation)
          return
        mergeEngineQualities(playing, late)
      },
    })
    if (mine !== generation)
      return
    mergeEngineQualities(playing, found)
  }
  catch {
    // One quality from the playing name is enough; a miss leaves the menu thin.
  }
}

function mergeEngineQualities(playing: Release, more: Release[]) {
  // `ranked` drops 0-seeder stubs, which is every copy we are already
  // playing from Downloads. Keep it, then append what the sources answered.
  const pool = serverCandidates(more, undefined, !hasNativePlayer(), true)
  const seen = new Set<string>()
  const list: Release[] = []
  const add = (r: Release) => {
    const k = releaseKey(r) || `name:${r.name}`
    if (seen.has(k))
      return
    seen.add(k)
    list.push(r)
  }
  add(playing)
  for (const r of [...candidates.value, ...pool])
    add(r)
  if (!list.length)
    return
  candidates.value = list
  const current = canonHash(torrent.value?.hash || playing.hash)
  const i = list.findIndex(r => canonHash(r.hash) === current)
  if (i >= 0)
    activeCandidate.value = i
  else if (activeCandidate.value >= list.length)
    activeCandidate.value = 0
  if (new Set(list.map(qualityLabel)).size > 1)
    qualityPromptPending.value = true
}

/**
 * Throw away the copy a source switch left behind.
 *
 * Only ever an unfinished one: a part-downloaded 1080p of a film you are now
 * watching in 4K is bytes nobody will ever read, while a *finished* copy is a
 * whole film that plays offline, and deleting that because someone glanced at
 * another source would be losing something they have. `delete` takes the files
 * with it — `forget` would drop the torrent and leave them on the disk with
 * nothing tracking them, which is worse than keeping it.
 */
async function dropSwitchedAway(id: number) {
  const held = downloads.torrents.find(t => t.id === id)
  if (!held || held.stats?.finished)
    return
  await torrentAction(id, 'delete').catch(() => {})
  await downloads.refresh().catch(() => {})
}

/**
 * The playing server died (or you picked another one from the menu): move down
 * the candidate list. The player's `src` watcher restarts mpv; do not key the
 * component on `src`, or fetching the Quality list remounts a blank 0:00.
 */
async function useCandidate(index: number, manual = true) {
  const next = candidates.value[index]
  if (!next)
    return
  const pickMagnet = next.magnet || (next.hash ? magnetForHash(next.hash) : '')
  if (!next.url && !pickMagnet)
    return
  const same = index === activeCandidate.value
  if (manual)
    userPicked.value = true
  activeCandidate.value = index
  torrent.value = next
  errorMsg.value = ''

  if (pickMagnet && (settings.allowTorrents || magnet.value || infoHash.value)) {
    const mine = ++generation
    // What we were watching, so it can be thrown away once the replacement is
    // up. Read before the switch: `torrentId` is about to point at the new one.
    const previous = torrentId.value
    // Keep the current picture up while the other magnet is added.
    // `resolving` punches the native window out — that is the blank player.
    step.value = $t('Buffering…')
    // A switch is a fresh open, so let the watcher act on the new URL even
    // when it matches the one already playing (retrying the highlighted row).
    src.value = ''
    await nextTick()
    if (mine !== generation)
      return
    try {
      const started = await downloads.start(key.value, {
        magnet: pickMagnet,
        hash: next.hash || undefined,
        fileIndex: next.fileIdx,
        allowTorrents: true,
        adopt: false,
        preferStream: true,
        cached: null,
        named: () => title.value ?? (route.query.title ? { title: String(route.query.title) } : null),
        // The same handoff first Play uses: open the stream the moment the
        // engine lists the hash, rather than sitting on "Fetching metadata"
        // until the whole add resolves. That wait was most of why picking
        // another source felt like nothing had happened.
        onQueued: ({ hash, index }) => {
          if (mine === generation)
            void openListedStream(hash, index, mine)
        },
        onStep: value => (step.value = value),
      })
      if (mine !== generation)
        return
      torrentId.value = started.id >= 0 ? started.id : null
      if (started.id >= 0)
        await downloads.focus(started.id)
      if (mine !== generation)
        return
      const nextSrc = playUrl(started)
      if (src.value !== nextSrc)
        src.value = nextSrc
      // The copy we walked away from is a part-downloaded file of a version
      // nobody chose, so it is only taking up the disk the new one needs.
      // A finished copy is a whole watchable film and is left for the usual
      // eviction order to deal with.
      if (previous != null && previous !== started.id)
        void dropSwitchedAway(previous)
    }
    catch (e) {
      if (mine === generation)
        errorMsg.value = e instanceof Error ? e.message : String(e)
    }
    return
  }

  if (same && !next.url)
    return

  if (next.url) {
    torrentId.value = null
    if (src.value === next.url) {
      src.value = ''
      await nextTick()
    }
    src.value = next.url
  }
}

/** Playback of the current server failed — walk the rest of the list. */
function onPlaybackFailed() {
  const list = candidates.value
  if (!list.length)
    return
  for (let i = activeCandidate.value + 1; i < list.length; i++) {
    const next = list[i]
    if (!next || !(next.url || next.magnet))
      continue
    failoverNotice.value = `${$t('Switched to')} ${
      hostOf(next.via ?? '') || next.source || qualityLabel(next)
    }`
    void useCandidate(i, false)
    return
  }
}

/** A finished disk copy never produced a frame — play the engine stream instead. */
function onDiskStuck() {
  if (torrentId.value == null || src.value.startsWith(ENGINE))
    return
  const index = fileIndex.value ?? torrent.value?.fileIdx ?? 0
  const http = streamUrl(torrentId.value, Number(index) || 0)
  if (src.value !== http)
    src.value = http
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
      qualities.push({ index, label, detail: r.size || undefined })
    }
  }
  return { servers, qualities }
})

// Driven by the route alone — the title resolving is `start`'s business now, so
// that a downloaded film never waits on TMDB. Fires again if you jump straight
// to another episode without leaving the player.
watch(
  () => [key.value, magnet.value, link.value, fileIndex.value, infoHash.value, String(route.query.src ?? ''), String(route.query.tid ?? '')].join('|'),
  () => start(),
  { immediate: true },
)

// Leaving the player stops the download and hands the connection back to
// whatever was paused for it. Every exit route unmounts — Esc, Back, the browser
// history, switching to another title — so this is the one place it belongs.
onBeforeUnmount(() => {
  generation++
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

const speed = computed(() => stats.value?.live?.download_speed.human_readable ?? '—')
const peers = computed(() => stats.value?.live?.snapshot.peer_stats.live ?? 0)

/**
 * How much of the head of the file is in, while we are still waiting for a
 * picture. The torrent-wide percentage is the wrong number to stare at during
 * a cold start — it reads 1% while the pieces that decide when playback begins
 * are nearly all here — so this replaces it until the first frame lands.
 *
 * Polled separately from the downloads store: the bitfield is a second request
 * and only matters for these few seconds. `/haves` is a plain bitmap, not the
 * stream, so it opens no second FileStream and takes nothing from mpv.
 */
const headPct = ref<number | null>(null)
let headMap: PieceMap | null = null

watch(src, () => {
  headMap = null
  headPct.value = null
})

useIntervalFn(async () => {
  const parts = streamParts(src.value)
  if (!parts || headPct.value === 100) {
    if (!parts)
      headPct.value = null
    return
  }
  headMap ??= await pieceMap(parts.id, parts.index)
  const haves = headMap ? await torrentHaves(parts.id) : null
  if (!headMap || !haves || !streamParts(src.value))
    return
  headPct.value = Math.floor(headBuffered(headMap, haves) * 100)
}, 1500, { immediateCallback: true })

/**
 * One line for the player's "buffering" notice, where there's no room for a
 * table. Empty while a direct link plays: there is no swarm to report on, and
 * "0 peers" reads as a fault rather than as "not applicable".
 */
const statusLine = computed(() => {
  if (stats.value) {
    const done = headPct.value != null && headPct.value < 100
      ? $t('{pct}% buffered', { pct: headPct.value })
      : `${progressPct.value.toFixed(0)}%`
    return `${speed.value} · ${peers.value} peers · ${done}`
  }
  if (src.value && torrent.value && torrentId.value == null) {
    const q = qualityLabel(torrent.value)
    const host = viaHost(torrent.value) || (torrent.value.source !== 'unknown' ? torrent.value.source : '')
    return [q && q !== $t('Unknown') ? q : '', host].filter(Boolean).join(' · ')
  }
  return ''
})

const backdrop = computed(() => backdropUrl(title.value?.backdrop, 'w1280'))

// What the end-of-playback screen offers. The show's season list carries an
// episode count per season, which is all the rollover needs.
const next = computed(() => {
  if (!media.value || !season.value || !episode.value)
    return null
  const target = nextEpisode(media.value.seasons, { season: season.value, episode: episode.value, watched: true })
  if (!target)
    return null
  return {
    to: watchLink('tv', id.value, target.season, target.episode),
    label: $t('Next · S{season} E{episode}', { season: target.season, episode: target.episode }),
  }
})

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
      <!-- Always mounted — shows resolving/loading overlay while src is empty.
           Not keyed on `src`: a Quality fetch must not remount mpv to idle. -->
      <mpv-player
        :src="src"
        :resolving="resolving"
        :step="step"
        :status="statusLine"
        :media="title"
        :next="next"
        :imdb-id="media?.imdbId"
        :title="title?.title ?? String(route.query.title ?? '')"
        :year="title?.year"
        :logo="logo"
        :season="season"
        :episode="episode"
        :quality="torrent ? qualityLabel(torrent) : ''"
        :candidates="candidateMenus"
        :active-candidate="activeCandidate"
        :osd-on-start="failoverNotice"
        :auto-open-quality="qualityPromptPending && !userPicked"
        @failed="onPlaybackFailed"
        @disk-stuck="onDiskStuck"
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
            {{ noServerStream ? $t('Your sources only provide downloads') : $t('Nothing to play') }}
          </div>
          <p class="text-body-medium opacity-70">
            {{ failure }}
          </p>
          <!-- Stream-only mode's fixes: add a streaming source, or let torrents back in. -->
          <div v-if="offerSources" class="mt-2 flex flex-wrap justify-center gap-2">
            <v-btn
              variant="tonal"
              color="primary"
              :prepend-icon="mdiPowerPlugOutline"
              @click="goToSources"
            >
              {{ $t('Add a source') }}
            </v-btn>
            <v-btn
              v-if="noServerStream"
              variant="tonal"
              :prepend-icon="mdiDownload"
              @click="settings.allowTorrents = true; start()"
            >
              {{ $t('Use torrent engine') }}
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
    </v-main>
  </v-app>
</template>
