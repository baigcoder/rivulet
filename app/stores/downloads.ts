import type { DiskSpace, EngineTorrent } from '~/utils/torrents'
import { mdiAlertCircleOutline, mdiCheckCircleOutline, mdiFormatListBulleted, mdiPauseCircleOutline, mdiTrayArrowDown } from '@mdi/js'
import { invoke } from '@tauri-apps/api/core'
import { appCacheDir, join } from '@tauri-apps/api/path'
import { sendNotification } from '@tauri-apps/plugin-notification'
import { key } from '~/brand'

export type StatusKey = 'downloading' | 'done' | 'paused' | 'error' | 'checking'
export type FilterKey = 'all' | StatusKey

/** Chip label + colour for whatever the engine says a torrent is doing. */
export const TORRENT_STATUS: Record<StatusKey, { text: () => string, color: string }> = {
  downloading: { text: () => $t('Downloading'), color: 'primary' },
  done: { text: () => $t('Completed'), color: 'success' },
  paused: { text: () => $t('Paused'), color: 'warning' },
  checking: { text: () => $t('Metadata'), color: 'info' },
  error: { text: () => $t('Error'), color: 'error' },
}

/** The left rail, qBittorrent-style: one row per state with a live count. */
// `title` is a function for the same reason SECTIONS' is: this list is built
// when the module loads, before `$t` has a locale to read — and the label has
// to change when the language does.
export const FILTERS: { value: FilterKey, title: () => string, icon: string }[] = [
  { value: 'all', title: () => $t('All'), icon: mdiFormatListBulleted },
  { value: 'downloading', title: () => $t('Downloading'), icon: mdiTrayArrowDown },
  { value: 'done', title: () => $t('Completed'), icon: mdiCheckCircleOutline },
  { value: 'paused', title: () => $t('Paused'), icon: mdiPauseCircleOutline },
  { value: 'error', title: () => $t('Errored'), icon: mdiAlertCircleOutline },
]

export function torrentStatus(t: EngineTorrent): StatusKey {
  const s = t.stats
  if (s?.error)
    return 'error'
  if (s?.state === 'paused')
    return 'paused'
  if (s?.finished)
    return 'done'
  if (!s || s.state === 'initializing')
    return 'checking'
  return 'downloading'
}

export function percentOf(t: EngineTorrent) {
  const s = t.stats
  return s?.total_bytes ? Math.min(100, (s.progress_bytes / s.total_bytes) * 100) : 0
}

export const useDownloadsStore = defineStore('downloads', () => {
  /** Rate ceilings and the storage folder are the user's, not ours. */
  const settings = useSettingsStore()
  const torrents = ref<EngineTorrent[]>([])
  /**
   * Where a download with no folder of its own actually lands — the setting
   * when there is one, else the default Rust works out. Shown on the Downloads
   * page and opened by its folder button, so "Open folder" names a real place.
   */
  const resolvedDir = ref('')
  /** Connected peers across every torrent — the usual reason a fast line looks like 3 MiB/s. */
  const peers = computed(() =>
    torrents.value.reduce((n, t) => n + (t.stats?.live?.snapshot.peer_stats.live ?? 0), 0),
  )
  const offline = ref(false)
  const loaded = ref(false)
  let lastFail = 0

  /**
   * When each torrent was last played, or first seen if it never was. Drives
   * eviction, so it outlives the session; pruned to what the engine still holds
   * so a re-added title doesn't inherit the timestamp of the copy we deleted.
   */
  const touched = useLocalStorage<Record<string, number>>(key('touched'), {})

  /**
   * Which torrent, and which file inside it, a title was last played from —
   * keyed the way watch progress is (`movie:603`, `tv:1396:2:3`). A finished
   * one plays straight off the disk: no TMDB, no sources, no peers. That is all
   * "offline" is here, so it outlives the session and is pruned to what the
   * engine still holds.
   */
  const cached = useLocalStorage<Record<string, { hash: string, file: number }>>(key('cached'), {})

  function cachedFor(keyStr: string) {
    return (keyStr && cached.value[keyStr]) || null
  }

  /**
   * The reverse of `cachedFor`: which title a torrent belongs to.
   *
   * `cached` is keyed by progress key because that is the question the
   * detail page asks ("does this film have an offline copy"). The
   * downloads page asks the opposite — it has a row and wants the title
   * it was filed under, so Play can hand the player a TMDB id instead of
   * a release filename. Without it the pause overlay falls back to plain
   * text where the title treatment belongs, and there is no artwork.
   *
   * A linear scan over a map holding one entry per cached title, called
   * once per Play — not worth a second index.
   */
  function titleFor(hash: string) {
    const want = hash.trim().toLowerCase()
    if (!want)
      return null
    const found = Object.entries(cached.value)
      .find(([, v]) => v.hash.trim().toLowerCase() === want)
    return found ? parseKey(found[0]) : null
  }

  /** User's own ceiling on the cache, in bytes. 0 = whatever the disk allows. */
  const cap = useLocalStorage(key('storageCap'), 0)

  const disk = ref<DiskSpace | null>(null)
  const used = computed(() => usedBytes(torrents.value))
  const budget = computed(() => diskBudget(disk.value, used.value, cap.value))

  /**
   * Largest single file the storage folder will take, `Infinity` when nothing
   * caps it — a FAT32 stick in a TV stops at 4 GiB (see `storageVolumes`).
   *
   * Separate from `budget` on purpose: the budget is how much of the disk the
   * cache may fill and is what eviction works against, while this is a ceiling
   * on one file that no amount of deleting will raise.
   */
  const fileLimit = ref(Number.POSITIVE_INFINITY)

  /** The torrent being watched: never evicted, and the only one downloading. */
  const focused = ref<number | null>(null)
  /** What `focus` paused, so `release` can put it back. */
  let paused: number[] = []

  /**
   * Drop what the engine no longer holds. Only ever called with a list the
   * engine actually answered with: pruning against the empty list an offline
   * engine leaves behind would throw away every play time and every cached
   * copy, so a slow-starting engine would cost the whole eviction order.
   */
  function prune() {
    const seen = torrents.value.map(t => t.info_hash)
    // Rebuilt only when it would really change — this runs every two seconds,
    // and a fresh object each tick rewrites the whole map to localStorage.
    if (seen.length !== Object.keys(touched.value).length || seen.some(hash => !touched.value[hash]))
      touched.value = Object.fromEntries(seen.map(hash => [hash, touched.value[hash] ?? Date.now()]))

    for (const [key, entry] of Object.entries(cached.value)) {
      if (!seen.includes(entry.hash))
        delete cached.value[key]
    }
  }

  /**
   * Hashes of torrents that stream-only mode added and is due to delete — kept
   * in storage so a stream the app was closed in the middle of is still
   * cleared up on the next launch rather than left on the disk for good.
   */
  const ephemeral = useLocalStorage<string[]>(key('ephemeral'), [])
  let sweptEphemeral = false

  /**
   * Per show, the torrent its last episode played from — for `startTorrent`'s
   * `pack`. Session-only: a pack worth reusing is one still in the engine.
   */
  const lastPack = new Map<string, { hash: string, bingeGroup?: string }>()

  /**
   * How long a stream-only copy outlives its player. Long enough for the next
   * episode to start from the same pack — the autoplay countdown and a click
   * on Next both land well inside it — and short enough that nothing is kept.
   */
  const EPHEMERAL_GRACE_MS = 2 * 60_000

  /**
   * Stream only's hold on the download while the buffer is full — see
   * `streamCap`. Set by the player as it plays, null otherwise; the tighter of
   * this and the user's own limit is what the engine is given.
   */
  const streamLimit = ref<number | null>(null)

  /** The reader holding the next episode's opening first in line — see `preloadNext`. */
  let preloading: { abort: () => void } | null = null

  /**
   * The credits are rolling: get the next episode ready. From the same pack
   * when it is in it — its opening fetched now, so the next Play is a read from
   * the disk — and otherwise by asking the sources early, so that Play at least
   * skips the search. Released by the next `start`, or after a minute and a half.
   */
  async function preloadNext(key: string, next: { season: number, episode: number }, imdbId?: string | null) {
    const show = key ? parseKey(key) : null
    if (!show?.season)
      return
    preloading?.abort()
    preloading = null
    const pack = lastPack.get(show.title)
    const primed = pack ? await preloadEpisode(pack.hash, next.season, next.episode).catch(() => null) : null
    if (primed) {
      preloading = primed
      setTimeout(() => {
        if (preloading === primed) {
          primed.abort()
          preloading = null
        }
      }, 90_000)
      return
    }
    if (imdbId)
      searchAhead(imdbId, next.season, next.episode)
  }
  const pendingDelete = new Map<string, ReturnType<typeof setTimeout>>()

  /** Delete what a stream-only playback left behind, unless it is playing now. */
  async function sweepEphemeral() {
    const due = new Set(ephemeral.value.map(h => h.toLowerCase()))
    if (!due.size)
      return
    for (const t of torrents.value) {
      if (due.has(t.info_hash.toLowerCase()) && t.id !== focused.value) {
        await torrentAction(t.id, 'delete').catch(() => {})
        due.delete(t.info_hash.toLowerCase())
      }
    }
    // What is left is the one playing now; anything the engine no longer
    // holds has nothing left to delete.
    ephemeral.value = [...due].filter(h => torrents.value.some(t => t.info_hash.toLowerCase() === h))
  }

  async function refresh() {
    try {
      torrents.value = await listTorrents()
      offline.value = false
      prune()
      if (!sweptEphemeral) {
        sweptEphemeral = true
        await sweepEphemeral()
      }
    }
    catch {
      offline.value = true
      lastFail = Date.now()
    }
    finally {
      loaded.value = true
    }
  }

  /**
   * Start something for a title and remember what it turned out to be, so the
   * next play can go straight to that copy. Every Play and Download button in
   * the app comes through here — `startTorrent` on its own has no idea what
   * title it is fetching.
   */
  async function start(key: string, options: Parameters<typeof startTorrent>[0]) {
    // Both ceilings are on the same quantity — the size of the file we'd fetch —
    // so the tighter one wins and `pickBest` needs to know nothing about drives.
    const maxBytes = Math.min(budget.value, fileLimit.value)
    // The torrent this show's last episode came from: the next one may be in it.
    const show = key ? parseKey(key) : null
    const pack = show?.season ? lastPack.get(show.title) ?? null : null
    // The preload's work is done once its episode starts; the player reads now.
    preloading?.abort()
    preloading = null
    const started = await startTorrent({ maxBytes, cached: cachedFor(key), pack, ...options })
    if (show?.season && started.hash)
      lastPack.set(show.title, { hash: started.hash, bingeGroup: started.torrent?.bingeGroup ?? pack?.bingeGroup })
    // Back inside the grace a stream-only copy gets on the way out: keep it.
    const waiting = started.hash && pendingDelete.get(started.hash.toLowerCase())
    if (waiting) {
      clearTimeout(waiting)
      pendingDelete.delete(started.hash.toLowerCase())
    }
    // A direct link leaves nothing on the disk to come back to, so there is no
    // offline copy to file — and filing an empty hash would shadow a real one.
    // Neither does a stream-only torrent: it is deleted when the player closes.
    if (key && started.hash && !started.ephemeral && !ephemeral.value.includes(started.hash.toLowerCase()))
      cached.value[key] = { hash: started.hash, file: started.index }
    if (started.ephemeral && started.hash)
      ephemeral.value = [...new Set([...ephemeral.value, started.hash.toLowerCase()])]
    // The add can land paused (session restore, a metered poll). Download is
    // the ask to pull bytes, so start it and refresh so the list is not a lie.
    if (started.id >= 0 && started.hash) {
      touched.value[started.hash] = Date.now()
      await torrentAction(started.id, 'start').catch(() => {})
      await waitForEngineHash(started.hash)
      for (let attempt = 0; attempt < 4; attempt++) {
        await refresh()
        if (torrents.value.some(t => t.info_hash.toLowerCase() === started.hash.toLowerCase()))
          break
        await new Promise(r => setTimeout(r, 500))
      }
    }
    return started
  }

  /**
   * Delete the least recently played torrents once the cache is over budget.
   * Runs on the poll rather than before each add: it then also covers torrents
   * resumed from a previous session, downloads that outgrew their estimate, and
   * a disk that filled up behind our back.
   */
  async function evict() {
    const drop = planEviction(torrents.value, budget.value, focused.value, touched.value)
    if (!drop.length)
      return
    // `delete` (not `forget`) — the point is the bytes.
    await Promise.all(drop.map(id => torrentAction(id, 'delete').catch(() => {})))
  }

  /**
   * Mobile data or a metered hotspot, as Android sees it (`meteredNetwork`).
   * Always false where nothing can tell, so the setting below is a no-op on
   * desktop rather than a guess.
   */
  const metered = ref(false)

  /** Torrents `meter` stopped, so Wi-Fi coming back starts exactly those. */
  let held: number[] = []

  /**
   * Keep downloads off a metered connection when the user asked for that.
   *
   * It can only run while this page can run — off screen the process
   * is frozen the moment the last download pauses (that is what stops the data
   * from flowing at all, see Downloads.kt), so a phone that reaches Wi-Fi in
   * someone's pocket resumes when the app is next opened, not before. Waking up
   * on a network change needs a JobScheduler job on the Android side, and a
   * download that waited for you to open the app has cost nobody anything.
   */
  async function meter() {
    metered.value = meteredNetwork() ?? false
    const running = torrents.value
      .filter(t => ['downloading', 'checking'].includes(torrentStatus(t)))
      .map(t => t.id)

    const plan = planNetwork(running, focused.value, held, settings.wifiOnly && metered.value)
    held = plan.held
    if (!plan.pause.length && !plan.start.length)
      return

    await Promise.all([
      ...plan.pause.map(id => torrentAction(id, 'pause').catch(() => {})),
      ...plan.start.map(id => torrentAction(id, 'start').catch(() => {})),
    ])
    await refresh()
  }

  // The poll below is throttled to about once a minute once the app is off
  // screen, which is exactly where walking out of the house lands. This event
  // fires regardless of that, so the switch to mobile data costs seconds of data
  // rather than a minute of it.
  useEventListener(() => (navigator as unknown as { connection?: EventTarget }).connection ?? null, 'change', meter)

  const visibility = useDocumentVisibility()

  /**
   * Nothing worth asking about: the window is hidden and nothing is moving.
   * Minimised or on a TV with the screen off, that's the app's whole idle cost.
   *
   * Still downloading is a different matter — eviction runs off this poll, so a
   * hidden window that stopped ticking would let a background download fill the
   * disk with nobody watching it.
   */
  function dormant() {
    return visibility.value === 'hidden'
      && !torrents.value.some(t => ['downloading', 'checking'].includes(torrentStatus(t)))
  }

  // One poll for the whole app — the toolbar badge, the sidebar counts
  // and the page all read this list. It backs off while the engine is down so a
  // browser-only dev session isn't hammering a dead port twice a second.
  useIntervalFn(
    async () => {
      if (dormant())
        return
      if (offline.value && Date.now() - lastFail <= 10_000)
        return
      await refresh()
      await evict()
      await meter()
    },
    2000,
    { immediateCallback: true },
  )

  // Free space moves on its own (other apps, other users), so it's re-read
  // rather than tracked. Failure leaves `disk` null, which means no budget and
  // no eviction — that's also the browser-only dev case, where invoke throws.
  useIntervalFn(
    // The budget has to follow the storage folder: moved to another drive, it's
    // that drive's free space that decides what gets evicted.
    async () => {
      // Same reasoning as above, and the same exception: the budget eviction
      // works from has to stay current for as long as anything is downloading.
      if (dormant())
        return
      disk.value = await invoke<DiskSpace>('disk_space', { path: settings.downloadDir || null }).catch(() => null)
      resolvedDir.value = await invoke<string>('download_dir', { path: settings.downloadDir || null }).catch(() => '')
      // Rides along with the free-space poll: the cap belongs to the drive, so
      // it has to follow the storage folder from one to the other exactly as
      // the budget does. Only Android reports one.
      const volumes = storageVolumes()
      // No folder chosen means the engine's default, which sits on the volume
      // Android lists first — built-in storage.
      const drive = settings.downloadDir
        ? volumes?.find(v => v.path === settings.downloadDir)
        : volumes?.[0]
      fileLimit.value = drive?.maxFile || Number.POSITIVE_INFINITY
    },
    10_000,
    { immediateCallback: true },
  )

  /**
   * Bumped by every `focus` and every `release`, and checked by `focus` after
   * each of its awaits.
   *
   * `focus` is four round trips long — release the last one, refresh, pause the
   * others, start this one — and the player can be gone before the last of
   * them. Leaving it called `release` while `focused` was still null, because
   * the assignment sat *after* that first await: nothing was paused, and
   * `focus` then reached its `start` and set the download going again with
   * nobody watching. That is the download that kept running after Back.
   */
  let generation = 0

  /**
   * Playback owns the downlink: every other *download* pauses so the whole pipe
   * goes to the stream, and is put back on `release`. Finished torrents keep
   * seeding — they cost no download bandwidth, and `uploadLimit` already holds
   * their upload down to a quarter of the line while anyone is watching.
   *
   * `id` is -1 while a direct link is playing. The engine has never heard of it,
   * but the downlink is just as busy, so everything else still gets out of the way.
   */
  async function focus(id: number) {
    const mine = ++generation
    // Claimed before the first await, so a `release` landing mid-flight knows
    // which torrent this call is on the hook for. Whatever was focused before
    // is let go here rather than by calling `release`, which would take the
    // claim straight back off again.
    const previous = focused.value
    const restore = paused
    focused.value = id
    paused = []
    if (previous != null && previous !== id)
      await stopFocused(previous)
    await restorePaused(restore)
    if (mine !== generation)
      return

    // Refresh first so the newly added torrent is in the local cache.
    // Without this, the find() below returns undefined and the torrent never gets started.
    await refresh()
    if (mine !== generation)
      return

    const playing = torrents.value.find(t => t.id === id)
    if (playing)
      touched.value[playing.info_hash] = Date.now()

    // A finished torrent asks the network for nothing, so nothing has to get
    // out of its way: playing off the disk leaves background downloads running.
    if (playing?.stats?.finished)
      return

    paused = torrents.value
      // 'checking' too: a torrent still pulling its metadata is competing already.
      .filter(t => t.id !== id && ['downloading', 'checking'].includes(torrentStatus(t)))
      .map(t => t.id)
    await Promise.all(paused.map(other => torrentAction(other, 'pause').catch(() => {})))
    if (mine !== generation)
      return
    // Always try to start the torrent - it may not be in local cache yet but exists in engine.
    // Calling start on already-started torrent is idempotent.
    await torrentAction(id, 'start').catch(() => {})
  }

  /**
   * Pause the torrent playback was pulling. A finished one is left seeding: it
   * costs no download bandwidth and the downloads page can't resume it (its
   * pause button is disabled once complete). Nothing to pause when a link was
   * playing — `id` is -1 and the engine has never heard of it.
   */
  async function stopFocused(id: number) {
    const own = torrents.value.find(t => t.id === id)
    if (own && !own.stats?.finished)
      await torrentAction(id, 'pause').catch(() => {})
  }

  /** Hand the downlink back to the background downloads `focus` took it from. */
  async function restorePaused(restore: number[]) {
    // On mobile data with Wi-Fi only asked for, what playback paused stays
    // paused — starting it here would spend the data the setting exists to save,
    // and `meter` would stop it again two seconds later anyway.
    if (settings.wifiOnly && metered.value)
      held = [...new Set([...held, ...restore])]
    else
      await Promise.all(restore.map(other => torrentAction(other, 'start').catch(() => {})))
  }

  /**
   * Leaving the player stops the download it started — an unwatched torrent has
   * no reason to keep pulling.
   *
   * A background download you also watched ends up paused too. One
   * click on the downloads page fixes it; if that gets annoying, `focus` takes a
   * flag for "was already running".
   *
   * The `generation` bump is what makes this reliable: a `focus` still in
   * flight for this same id would otherwise finish by starting it again.
   */
  async function release() {
    generation++
    streamLimit.value = null
    const id = focused.value
    const restore = paused
    focused.value = null
    paused = []
    if (id == null)
      return

    // Keep downloading (Settings → Sources, engine mode) leaves it pulling after
    // the player closes — as a background download from here on, so Wi-Fi only
    // still holds it on mobile data. Off, it waits for the next Play, which
    // resumes it from the pieces already on disk.
    const own = torrents.value.find(t => t.id === id)
    if (own && ephemeral.value.includes(own.info_hash.toLowerCase())) {
      // Stream-only: nothing is kept — but paused for a moment first, in case
      // the next episode is in this same pack (see `EPHEMERAL_GRACE_MS`). A
      // restart before then is cleared up by `sweepEphemeral` on the next launch.
      const hash = own.info_hash.toLowerCase()
      await torrentAction(id, 'pause').catch(() => {})
      clearTimeout(pendingDelete.get(hash))
      pendingDelete.set(hash, setTimeout(() => {
        pendingDelete.delete(hash)
        if (torrents.value.find(t => t.info_hash.toLowerCase() === hash)?.id === focused.value)
          return
        void torrentAction(id, 'delete').catch(() => {})
        ephemeral.value = ephemeral.value.filter(h => h !== hash)
      }, EPHEMERAL_GRACE_MS))
    }
    else if (!settings.keepDownloading) {
      await stopFocused(id)
    }
    await restorePaused(restore)
    await refresh()
  }

  const filter = ref<FilterKey>('all')
  const query = ref('')

  /**
   * What the Downloads page lists: everything but a Stream only copy, which is
   * a cache and not a download — it is gone when the player closes.
   */
  const kept = computed(() => {
    const streaming = new Set(ephemeral.value)
    return torrents.value.filter(t => !streaming.has(t.info_hash.toLowerCase()))
  })

  const counts = computed(() => {
    const tally = { all: kept.value.length, downloading: 0, done: 0, paused: 0, checking: 0, error: 0 }
    for (const t of kept.value)
      tally[torrentStatus(t)]++
    return tally as Record<FilterKey, number>
  })

  /** What the badge shows: anything still working, done or not. */
  const active = computed(() => counts.value.downloading + counts.value.checking)

  // --- Download completion notifications ------------------------------------
  // Track previous states so we can fire a notification on transition.
  const prevStates = ref<Record<number, StatusKey>>({})

  watch(kept, now => {
    for (const t of now) {
      const prev = prevStates.value[t.id]
      const curr = torrentStatus(t)
      if (prev && prev !== curr) {
        if (prev === 'downloading' || prev === 'checking') {
          if (curr === 'done' && settings.notifyComplete) {
            sendNotification({
              title: $t('Download complete'),
              body: t.name ?? $t('Your download is ready to watch'),
            })
          }
          else if (curr === 'error' && settings.notifyError) {
            sendNotification({
              title: $t('Download failed'),
              body: `${t.name ?? $t('Download')}: ${t.stats?.error ?? $t('Unknown error')}`,
            })
          }
        }
      }
      prevStates.value[t.id] = curr
    }
    // Prune entries for torrents the engine no longer holds.
    const ids = new Set(now.map(t => t.id))
    for (const id of Object.keys(prevStates.value).map(Number)) {
      if (!ids.has(id))
        delete prevStates.value[id]
    }
  }, { deep: true })

  const speed = computed(() => {
    const sum = (pick: (t: EngineTorrent) => number) => torrents.value.reduce((n, t) => n + pick(t), 0)
    return {
      down: sum(t => t.stats?.live?.download_speed.mbps ?? 0),
      up: sum(t => t.stats?.live?.upload_speed.mbps ?? 0),
    }
  })

  // --- Seeding -----------------------------------------------------------------
  // Downloading and never giving anything back is what gets swarms killed, so
  // finished torrents keep seeding — under a ceiling, because a saturated uplink
  // is felt everywhere else on the network.

  /** Best upload rate this line has ever managed, bytes/s. See `uploadLimit`. */
  const peakUp = useLocalStorage(key('peakUpload'), 0)

  /**
   * Seeding runs unlimited for the first few minutes of a session so there's
   * something real to measure. It's paused for playback, which is the only time
   * the ceiling actually matters.
   *
   * One probe per launch, so a line that got faster is learned on the
   * next start. If that's too slow to adapt, re-probe on a timer while idle.
   */
  const probing = ref(true)
  useTimeoutFn(() => (probing.value = false), 5 * 60_000)

  watch(() => speed.value.up, up => (peakUp.value = Math.max(peakUp.value, up * 1024 ** 2)))

  // What the settings page asked for, in bytes/s. 0 there means "automatic".
  const MB = 1024 ** 2

  watch(
    () => {
      const own = settings.downLimit > 0 ? settings.downLimit * MB : null
      const cap = streamLimit.value
      return [
        uploadLimit(peakUp.value, focused.value != null, probing.value, settings.upLimit * MB),
        own != null && cap != null ? Math.min(own, cap) : own ?? cap,
      ] as const
    },
    ([up, down]) => setLimits(up, down),
    { immediate: true },
  )

  /** Is this torrent a Stream only copy — a cache, not a download? */
  function isStreamOnly(hash?: string | null) {
    return !!hash && ephemeral.value.includes(hash.toLowerCase())
  }

  // New torrents land wherever storage says; the engine keeps the folder of the
  // ones it already holds.
  watch(() => settings.downloadDir, dir => setDownloadDir(dir), { immediate: true })

  // Searching only ever hits the sources the user configured, so the list has to
  // reach utils/torrents before the first search — hence `immediate`.
  // Only the sources switched on are searched, and each search reports back
  // so the Sources page can show which are fast, slow or failing.
  // Keyed on the list's text, not the array: every search report rewrites
  // `sourceMeta`, which rebuilds `activeSources`, and `setSources` empties the
  // search-ahead cache — so watching the array threw each answer away the
  // moment it landed, and a page that asks again on change searched forever.
  watch(() => settings.activeSources.join('\n'), () => setSources(settings.activeSources), { immediate: true })
  watch(() => settings.torrentOnly, on => setTorrentOnly(on), { immediate: true })
  // Stream only's private cache — see `setStreamDir`. Inside the download
  // folder rather than the app's own data: that sits on the system drive, which
  // is the one most likely to be full, and a stream there failed for want of
  // space with hundreds of GB free on the drive the user had chosen. A hidden
  // folder of its own keeps it out of sight and apart from what is kept.
  watch(() => settings.downloadDir || resolvedDir.value, async dir => {
    const base = dir || await appCacheDir().catch(() => '')
    if (base)
      setStreamDir(await join(base, '.rivulet-stream'))
  }, { immediate: true })
  // What each source says it covers, so a movie-only addon is not asked for an
  // episode and an IMDb-only one is not asked for an anime by Kitsu id.
  // Serialised for the same reason as above: a report's speed and time are
  // not capabilities, and must not cost the cached search.
  watch(() => JSON.stringify(Object.entries(settings.sourceMeta).map(([url, m]) => [url, m.types, m.idPrefixes])), () => setSourceCapabilities(Object.fromEntries(
    Object.entries(settings.sourceMeta).map(([url, m]) => [url, { types: m.types, idPrefixes: m.idPrefixes }]),
  )), { immediate: true })
  // A source added before manifests were read has no capabilities on file:
  // read them once, quietly, so it is filtered like the rest.
  void Promise.all(settings.sources
    .filter(url => !settings.sourceMeta[url]?.types)
    .map(url => fetchManifest(url)
      .then(m => settings.patchSource(url, { types: m.types, idPrefixes: m.idPrefixes, name: settings.sourceMeta[url]?.name ?? m.name, logo: settings.sourceMeta[url]?.logo ?? m.logo }))
      .catch(() => {})))
  // Rank releases in a language the viewer follows first: the app's own, and
  // English beside it. '' is "the system's", which the browser knows.
  watch(() => settings.locale, locale => setPreferredLanguages([locale || navigator.language, 'en']), { immediate: true })
  setSourceReporter(report => settings.noteSourceReport(report))

  /**
   * What the sidebar rail and the filter box narrow the list to. Newest first:
   * the engine hands them back in the order they were added, and the one you
   * just started is the one you came here to look at. Sorting a column in the
   * table overrides this — it sorts what it's given, via its header accessors.
   */
  const list = computed(() => {
    const q = query.value.trim().toLowerCase()
    return kept.value.filter(t =>
      (filter.value === 'all' || torrentStatus(t) === filter.value)
      && (!q || (t.name ?? t.info_hash).toLowerCase().includes(q)),
    // The id *is* the added-on order — the engine has no timestamp.
    ).sort((a, b) => b.id - a.id)
  })

  async function act(id: number, action: 'pause' | 'start' | 'forget' | 'delete') {
    await torrentAction(id, action)
    await refresh()
  }

  /**
   * Delete every torrent, files and all — the manual version of `evict`, for
   * when the disk is needed now. The refresh that follows lets `prune` drop the
   * play times and the offline copies along with them.
   */
  async function clearAll() {
    await Promise.all(torrents.value.map(t => torrentAction(t.id, 'delete').catch(() => {})))
    await refresh()
  }

  return {
    torrents,
    preloadNext,
    streamLimit,
    isStreamOnly,
    resolvedDir,
    peers,
    offline,
    loaded,
    refresh,
    filter,
    query,
    counts,
    active,
    speed,
    list,
    act,
    clearAll,
    disk,
    used,
    budget,
    fileLimit,
    cap,
    focused,
    focus,
    release,
    metered,
    cachedFor,
    titleFor,
    start,
  }
})
