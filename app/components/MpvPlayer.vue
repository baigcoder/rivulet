<script lang="ts" setup>
import type { PlayerEngine } from '~/utils/htmlvideo'
import type { Subtitle, SubtitleFile, SubtitleLanguage } from '~/utils/subtitles'
import type { Media } from '~/utils/tmdb'
import type { EngineTorrent, PieceMap } from '~/utils/torrents'
import {
  mdiAlertCircleOutline,
  mdiAlphaA,
  mdiAutoFix,
  mdiBookOpenPageVariant,
  mdiChartTimeline,
  mdiCheck,
  mdiChevronDown,
  mdiChevronUp,
  mdiClose,
  mdiCog,
  mdiEarHearing,
  mdiFastForward10,
  mdiFullscreen,
  mdiFullscreenExit,
  mdiInformationOutline,
  mdiMinus,
  mdiPause,
  mdiPlay,
  mdiPlaySpeed,
  mdiPlus,
  mdiReload,
  mdiRepeat,
  mdiRewind10,
  mdiSkipNext,
  mdiSleep,
  mdiStepForward,
  mdiSubtitles,
  mdiSubtitlesOutline,
  mdiSurroundSound,
  mdiVolumeHigh,
  mdiVolumeLow,
  mdiVolumeMedium,
  mdiVolumeOff,
} from '@mdi/js'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

/** Remembered, so the next episode comes up in the same language — Plex-style. */
import { key } from '~/brand'
import { inCredits, inIntro, progressKey, saveCredits, saveIntro } from '~/utils/library'

// Player for the embedded native mpv engine. mpv renders into a surface that
// the Rust backend keeps glued to `boxEl` below (see `player_start` /
// `player_set_geometry`) — an X11 child window, a child HWND, or on macOS an
// NSOpenGLView libmpv hands its frames to.
//
// On the first two that surface paints ABOVE the webview, so the controls
// cannot simply be stacked over it in CSS. Instead every overlay marks itself
// `data-cut`: each frame their rectangles are measured and handed to the
// backend, which subtracts them from mpv's window. The page then shows through
// those holes — clicks included — while the video window itself never resizes,
// so nothing rescales when a bar slides in. macOS is the other way round (the
// view is *under* WebKit, as ExoPlayer's surface is under it on Android), and
// there the bars are ordinary DOM over a transparent page — see `overlay`.
//
// The native window knows nothing about page layout, so geometry is pushed every
// time the box moves *or* resizes. Position changes (window fullscreen,
// scrolling, a bar sliding) are invisible to ResizeObserver, hence the rAF loop.
const props = defineProps<{
  /** librqbit stream URL to play. Empty string = nothing selected. */
  src: string
  /** Live torrent status, shown while playback is stalled for data. */
  status?: string
  /** Hold the OS window in fullscreen for as long as this player is mounted. */
  fullscreen?: boolean
  /**
   * What is being played, for the watch history. Without it playback is not
   * tracked at all — which is right for a bare magnet, where there is no title
   * to attach progress to.
   */
  media?: Media | null
  /** Offered on the end-of-playback screen. Absent for a movie, or a last episode. */
  next?: { to: string, label: string } | null
  /** What to look external subtitles up by. Without it, only the file's own tracks are offered. */
  imdbId?: string | null
  /** Fallback for the lookup: plenty of titles have no IMDb id on TMDB. */
  title?: string
  year?: string
  season?: number
  episode?: number
  /**
   * The other server streams the sources answered with, offered as two menus:
   * `servers` names every candidate (and is the failover list), `qualities`
   * points at one candidate per resolution. Both entries carry the candidate
   * index to hand back on `use-candidate`. Absent for torrent playback, where
   * there is no other server to be had.
   */
  candidates?: {
    servers: { index: number, label: string, detail?: string }[]
    qualities: { index: number, label: string, detail?: string }[]
  } | null
  /** Which candidate is playing — the check mark in both menus. */
  activeCandidate?: number
  /**
   * One-shot notice shown as an OSD right after this mount starts playing —
   * how a failover says "Switched to …" without anyone opening a menu.
   */
  osdOnStart?: string
  /** While true, the Quality menu opens itself once two or more qualities exist. */
  autoOpenQuality?: boolean
  /** Quality label from the source, e.g. "4k DV | HDR". Used to detect HDR content. */
  quality?: string
}>()

const emit = defineEmits<{
  /** The stream died and another candidate exists to try — the parent swaps URLs. Reason flags why, for the last-candidate message. */
  failed: [reason?: 'stub' | 'dead']
  useCandidate: [index: number]
  /** The self-introducing Quality menu fired; the parent can stop offering it. */
  autoOpened: []
}>()

/**
 * Which backend is behind these controls. Where mpv can be embedded it is;
 * elsewhere ExoPlayer or the page's own `<video>` opens the same stream URL and
 * answers the same commands (see utils/htmlvideo.ts), so everything below this
 * line is written once.
 *
 * The differences the rest of the file has to know about are exactly four: the
 * native window needs geometry and cutouts pushed at it, only mpv draws its own
 * OSD and subtitles, only the desktop has an ffmpeg to auto-sync with, and only
 * a hole in a native window has to be opaque.
 */
const native = hasNativePlayer()

/**
 * Does mpv's picture paint over this page (X11, Win32) or behind it (macOS)?
 * See `hasVideoOverlay`. Only where it is over does the page have holes to
 * punch, a pointer it cannot see, and bars that have to be opaque.
 */
const overlay = hasVideoOverlay()

/**
 * Android's backend: ExoPlayer on the device's own decoders, behind the same
 * protocol (see utils/htmlvideo.ts and Player.kt). Not `native` — it draws
 * nothing but the picture, so the OSD, the cues and every control below are the
 * page's exactly as they are for the `<video>` path. What it does share with mpv
 * is painting *outside* the webview, so the page has to be transparent down to
 * the box for any of it to be visible.
 */
const exo = hasExoPlayer()

/**
 * Is the picture painted behind the webview? ExoPlayer's surface and macOS's
 * mpv view both are, and the page's side of that is identical for the two: it
 * has to stop painting over the box (`rivulet-video` below), and the taps land
 * on the page because there is no picture element under the finger.
 */
const behind = exo || (native && !overlay)

/**
 * Does play/pause live in the bottom bar rather than dead centre?
 *
 * Where the picture is a native window over the page it has to: the middle of
 * the frame can only be an opaque hole, and an opaque box over the film is a
 * blindfold, not a control. A television lands in the same place from the other
 * end — the centre cluster is where a thumb already is, and a remote hasn't got
 * one. What it has is a d-pad that arrives at the bar anyway, so putting the
 * transport there costs nothing and stops the picture being covered by the one
 * control that is up the longest, on the one screen watched from across a room.
 */
const barTransport = computed(() => overlay || isTv() === true)

/**
 * A finger, not a pointer. Controls get thumb-sized, the volume slider goes
 * away (a phone's own buttons own volume), and a tap on the picture shows the
 * chrome rather than pausing — which is what every other player on a phone does.
 */
const touch = useMediaQuery('(pointer: coarse)')

// Every overlay surface is opaque and hairline-bordered on purpose: it sits in a
// hole punched out of the native window, so what's behind it is the page, never
// the picture. A translucent fill or a box-shadow would be sliced off at the
// hole's edge, and the transitions slide rather than fade for the same reason.
// With no hole there is no edge to be sliced at, so the bars can sit over the
// picture the way every other player's do. No blur: a full-width backdrop
// filter over live video is exactly the frame budget a TV box hasn't got.
const SURFACE = computed(() => overlay ? 'bg-[#0e0f11] border-white/9' : 'bg-[#0e0f11]/85 border-white/9')

/** Square icon button in the bars and the menu head. */
const ICO = computed(() => `inline-flex items-center justify-center border-0 bg-transparent text-white opacity-86 transition-colors transition-opacity duration-120 hover:bg-white/12 hover:opacity-100 disabled:pointer-events-none disabled:opacity-30 rounded-lg ${touch.value ? 'h-11 min-w-11' : 'h-9.5 min-w-9.5'}`)

/**
 * The transport, dead centre. Play is where a remote lands and where a thumb
 * already is, so it is the biggest target on the screen rather than a 38px
 * square in a corner — and at ten feet that is the difference between pausing a
 * film and hunting for the button that would.
 */
const ROUND = 'inline-flex items-center justify-center border-0 rounded-full bg-white/10 text-white transition-colors duration-120 hover:bg-white/22 disabled:pointer-events-none disabled:opacity-30'
const SEEK_BTN = computed(() => `${ROUND} ${touch.value ? 'h-13 w-13' : 'h-12 w-12'}`)
const PLAY_BTN = computed(() => `${ROUND} ${touch.value ? 'h-17 w-17' : 'h-16 w-16'}`)

/** Filled button in the centre notice. */
const BTN = 'inline-flex items-center gap-1.5 border-0 rounded-lg bg-white/12 px-3.5 py-1.75 text-label-large transition-colors duration-120 hover:bg-white/20'

/** One choice inside the speed / audio / subtitle menu. */
const MENU_ROW = 'flex w-full items-center justify-between gap-2.5 border-0 bg-transparent rounded-lg px-2.5 py-2 text-left text-label-large transition-colors duration-100 hover:bg-white/9'

/** Section heading between groups of menu rows. */
const MENU_GROUP = 'px-2.5 pb-1 pt-2.5 text-label-small uppercase opacity-45'

/** Explanatory line where a menu group has nothing to list. */
const NOTE = 'px-2.5 py-2 text-body-small opacity-60'

const TIME = 'mx-2.5 whitespace-nowrap text-body-medium tabular-nums'

// Slide rather than fade: a fading overlay would fade against black, not against
// the video, because the video isn't behind it — it's beside it, through a hole.
const SLIDE = 'transition-transform duration-180 ease-[cubic-bezier(0.32,0.72,0,1)]'

const rootEl = ref<HTMLElement | null>(null)
const boxEl = ref<HTMLElement | null>(null)
/** Where a d-pad enters the chrome: the centre play button. */
const playBtn = ref<HTMLButtonElement | null>(null)

const videoEl = ref<HTMLVideoElement | null>(null)
let engine: PlayerEngine | null = null
/** The no-sound notice is said once per file, not every poll. See `poll`. */
let silentSaid = false

const started = ref(false)
const busy = ref(false)
const waiting = ref(false)
const paused = ref(false)
const buffering = ref(false)
const ended = ref(false)
const duration = ref(0)
const position = ref(0)
const cacheEnd = ref(0)
const volume = ref(100)
const muted = ref(false)
const speed = ref(1)
const scrubbing = ref(false)
/** While the volume slider is being dragged, the poll must not fight it back. */
const volumeHeld = ref(false)
const errorMsg = ref('')
/** The current stream was a debrid stub clip (quota/key error) — remembered for the failover verdict. */
const stubSeen = ref(false)
/** Chapter list fetched once when the file opens. */
const chapters = ref<{ time: number, title?: string }[]>([])
const currentChapter = ref(-1)

// ---------------------------------------------------------------------------
// A-B Loop
// ---------------------------------------------------------------------------
/** null = not looping, { a, b } = loop between these two points. */
const abLoop = ref<{ a: number, b: number } | null>(null)

function setLoopA() {
  if (!started.value)
    return
  abLoop.value = { a: position.value, b: abLoop.value?.b ?? position.value + 10 }
  osd($t('Loop A: {time}', { time: fmt(position.value) }))
}

function setLoopB() {
  if (!started.value || !abLoop.value)
    return
  if (position.value <= abLoop.value.a) {
    abLoop.value = null
    osd($t('Loop cancelled'))
    return
  }
  abLoop.value.b = position.value
  osd($t('Loop: {a} → {b}', { a: fmt(abLoop.value.a), b: fmt(position.value) }))
}

function clearLoop() {
  abLoop.value = null
  osd($t('Loop off'))
}

// ---------------------------------------------------------------------------
// Network Stats Overlay
// ---------------------------------------------------------------------------
/** The stream is the local engine's, rather than a link a source resolved itself. */
const fromEngine = computed(() => props.src.startsWith(ENGINE))

const showStats = ref(false)
const statsData = ref<{ download: string, upload: string, peers: number, progress: number } | null>(null)

async function pollStats() {
  if (!showStats.value || !fromEngine.value)
    return
  const [, id] = props.src.match(/\/torrents\/(\d+)\/stream/) ?? []
  if (!id)
    return
  try {
    const res = await fetch(`${ENGINE}/torrents/${id}`)
    if (!res.ok)
      return
    const t = await res.json() as EngineTorrent
    const live = t.stats?.live
    const total = t.stats?.total_bytes || 1
    statsData.value = {
      download: live?.download_speed.human_readable ?? '0 B/s',
      upload: live?.upload_speed.human_readable ?? '0 B/s',
      peers: live?.snapshot.peer_stats.live ?? 0,
      progress: Math.round((t.stats?.progress_bytes ?? 0) / total * 100),
    }
  }
  catch { /* engine offline mid-poll — ignore */ }
}

const { pause: stopStatsPoll, resume: startStatsPoll } = useIntervalFn(pollStats, 1000, { immediate: false })
watch(showStats, v => {
  if (v)
    startStatsPoll()
  else stopStatsPoll()
})

// ---------------------------------------------------------------------------
// mpv IPC
// ---------------------------------------------------------------------------
async function ipc(command: unknown[]): Promise<any> {
  if (!native)
    return engine?.command(command) ?? null
  try {
    const res = await invoke<string>('player_ipc', { command: JSON.stringify({ command }) })
    return JSON.parse(res)
  }
  catch {
    return null // player briefly unavailable (starting/stopping) — ignore
  }
}

/** Several properties over one socket round trip. Missing/failed ones read null. */
async function readProps<T = Record<string, any>>(names: string[]): Promise<T | null> {
  if (!native)
    return (engine?.props(names) ?? null) as T | null
  try {
    return JSON.parse(await invoke<string>('player_props', { names }))
  }
  catch {
    return null
  }
}

/** Cursor position in the video window, on the backends that can say. See `poll`. */
interface Pointer { x: number, y: number, over: boolean }

async function readPointer(): Promise<Pointer | null> {
  // Only a surface in front of the page can swallow the cursor. A <video>, and
  // a picture painted behind the webview, both leave the pointer events to the
  // DOM, where they arrive here like any other.
  if (!overlay)
    return null
  try {
    return await invoke<Pointer | null>('player_pointer')
  }
  catch {
    return null
  }
}

/** The `<video>` path's OSD, since there is no mpv to draw one. */
const osdText = ref('')
let osdTimer: ReturnType<typeof setTimeout> | null = null

/**
 * Text over the video without a cutout: let mpv itself render it. The trailing 0
 * is the message's minimum OSD level — without it mpv defaults to level 1 and
 * drops the message, since we launch it with --osd-level=0.
 */
function osd(text: string, ms = 1200) {
  if (!started.value || !text)
    return
  if (native)
    return ipc(['show-text', text, ms, 0])

  osdText.value = text
  if (osdTimer)
    clearTimeout(osdTimer)
  osdTimer = setTimeout(() => (osdText.value = ''), ms)
}

// ---------------------------------------------------------------------------
// Tracks: subtitles (file's own + the release's + OpenSubtitles) and audio.
//
// `sub-add` takes an http URL, so nothing is ever downloaded to disk here — the
// engine's own stream endpoint serves the release's subtitle files the same way
// it serves the video.
// ---------------------------------------------------------------------------
interface Track { id: number, type: string, lang?: string, title?: string, external?: boolean }

const tracks = ref<Track[]>([])
const sid = ref<number | 'no'>('no')
const aid = ref<number | 'no'>('no')
const externals = ref<Subtitle[]>([])
const subLoading = ref(false)
const subError = ref('')
/** Probed files per language name, filled the first time that language is used. */
const variants = ref<Record<string, SubtitleFile[]>>({})
const expanded = ref('')
const probing = ref('')
/** The downloaded file currently showing, which is what auto-sync works on. */
const activeUrl = ref('')
const subDelay = ref(0)
/** mpv's `sub-speed`. Only auto-sync ever moves it off 1. */
const subSpeed = ref(1)
const syncing = ref(false)
/** The slow second pass is running: the whole film rather than what just played. */
const syncWide = ref(false)
const syncNote = ref('')
/** The best fit of a pass that wasn't sure enough to apply itself. */
const guess = ref<Sync | null>(null)
let subsFetched = false
/** Set once mpv has the file open, which is when tracks and a duration exist. */
let loaded = false

const subLang = useLocalStorage(key('subLang'), '')

// mpv runs with --no-config, so how subtitles look is entirely ours to set.
// Pushed once the file is open and again on every edit, which is what makes the
// settings page's preview honest: change the size mid-film and it changes.
const settings = useSettingsStore()
const library = useLibraryStore()

const { height: boxHeight } = useElementSize(boxEl)
/** The track/speed panel, measured because the subtitles have to clear it. */
const menuEl = ref<HTMLElement | null>(null)
const { height: menuHeight } = useElementSize(menuEl)
/** Its own gap from the bottom of the frame — the bottom bar plus a little. */
const menuBottom = computed(() => touch.value ? 112 : 106)
/** Is the chrome up? Declared here because `subPos` measures against it. */
const ui = ref(true)
/**
 * Where the subtitle line goes. The bottom bar covers the bottom of the picture
 * and mpv draws underneath it, so while the chrome is up the subtitles move
 * above it rather than sitting behind it — and higher again for an open panel,
 * whose height is 0 with none mounted.
 */
const subPos = computed(() => subtitleLift(
  settings.subs.position,
  menuHeight.value ? menuBottom.value + menuHeight.value + 8 : ui.value ? menuBottom.value : 0,
  boxHeight.value,
))

watch(subPos, pos => {
  if (started.value && native)
    ipc(['set_property', 'sub-pos', pos])
})

function applySubtitleStyle() {
  // The other two backends draw their cues through the page, so styling is the
  // computed below and needs no pushing — it re-renders on the same edit.
  if (!started.value || !native)
    return
  for (const [name, value] of Object.entries(subtitleProps({ ...settings.subs, position: subPos.value })))
    ipc(['set_property', name, value])
}

watch(() => settings.subs, applySubtitleStyle, { deep: true })

// The subtitles the page draws itself, wherever mpv isn't drawing them. Both
// kinds arrive here: downloaded files as parsed cues, and a track muxed into the
// file as text ExoPlayer decoded and handed over. The `<video>` path has only
// the first — Chromium never hands out a muxed track, which is why its menu
// offers none.
/** Cues from a track inside the file, which only ExoPlayer can read out. */
const subText = ref('')
const cueText = computed(() => native
  ? ''
  // A muxed track is decoded by the backend that found it and handed over as
  // plain text, so both kinds are drawn right here in the user's own styling —
  // and the settings preview stays honest for either.
  // mpv shows a cue at `start * sub-speed + sub-delay`, so reading the cue list
  // at a given moment means undoing both — and the same two knobs auto-sync set.
  : captioned(subText.value || cueAt(probed(activeUrl.value)?.cues ?? [], (position.value - subDelay.value) / subSpeed.value)))

/** What `sub-filter-sdh` does for mpv, for the backends that draw their own. */
function captioned(text: string) {
  return settings.subs.hideCaptions ? stripCaptions(text) : text
}
const cueStyle = computed(() => subtitleCss(settings.subs, boxHeight.value))

/**
 * Subtitle files that came inside the torrent, one row each. `startTorrent`
 * already downloaded the ones belonging to this video (see `pickSubtitleFiles`),
 * and the stream URL is what mpv opens — no OpenSubtitles, no matching a file to
 * a cut, and it works with no network at all once the torrent is done.
 *
 * The torrent and the file are read back out of `src` rather than passed in:
 * that URL is `…/torrents/{id}/stream/{index}` and nothing else ever plays here.
 */
const release = ref<{ file: Subtitle, lang: SubtitleLanguage }[]>([])
/**
 * The video file's own name, which is the only description of *this* cut we
 * have. Handed to the subtitle search as Stremio's `filename` extra so a
 * provider that matches on a release rather than on a title can use it.
 */
const videoName = ref('')

async function loadReleaseSubs() {
  release.value = []
  videoName.value = ''
  const [, id, index] = props.src.match(/\/torrents\/(\d+)\/stream\/(\d+)/) ?? []
  if (!id)
    return
  const files = (await torrentDetails(Number(id)))?.files ?? []
  const video = files[Number(index)]?.name ?? ''
  videoName.value = video
  release.value = pickSubtitleFiles(files, Number(index)).map(i => {
    const f = files[i]!
    const lang = releaseSubtitle(f.components?.join('/') ?? f.name, video, streamUrl(Number(id), i))
    return { file: lang.files[0]!, lang }
  })
}

watch(() => props.src, loadReleaseSubs, { immediate: true })

const subLanguages = computed(() => byLanguage(externals.value))
const embedded = computed(() => tracks.value.filter(t => t.type === 'sub' && !t.external))
const audioTracks = computed(() => tracks.value.filter(t => t.type === 'audio'))
const subsOn = computed(() => sid.value !== 'no')

function trackLabel(t: Track) {
  const lang = t.lang ? langName(t.lang) : ''
  return [lang, t.title].filter(Boolean).join(' — ') || $t('Track {id}', { id: t.id })
}

async function refreshTracks() {
  const p = await readProps(['track-list', 'sid', 'aid'])
  if (!p)
    return
  tracks.value = Array.isArray(p['track-list']) ? p['track-list'] : []
  sid.value = typeof p.sid === 'number' ? p.sid : 'no'
  aid.value = typeof p.aid === 'number' ? p.aid : 'no'
}

/**
 * What to search by. A title that reached us through TMDB is already a title;
 * a magnet arrives as a raw release name, and handing
 * `House.of.the.Dragon.S01.1080p.BluRay.x265[eztv.re]` to a catalogue whole is
 * why one used to find no subtitles at all.
 *
 * The playing file is read for the episode as well as the release name, because
 * a season pack's name stops at "S01" and only the file inside it says which
 * episode is on — and for a magnet the route knows neither.
 */
const searchBy = computed(() => {
  const named = parseRelease(props.title ?? '')
  const file = parseRelease(videoName.value)
  return {
    title: named.title || (props.title ?? ''),
    year: props.year || named.year,
    season: props.season || file.season || named.season,
    episode: props.episode || file.episode,
  }
})

/** Nothing to search by at all — no id and no name. */
const unsearchable = computed(() => !props.imdbId && !props.title?.trim())

async function fetchExternals() {
  if (subsFetched || subLoading.value || unsearchable.value)
    return
  subLoading.value = true
  subError.value = ''
  try {
    const { title, year, season, episode } = searchBy.value
    const id = props.imdbId || await findImdbId(title, season > 0, year)
    if (!id)
      throw new Error($t('Couldn\'t match “{title}” to a title OpenSubtitles knows.', { title }))
    externals.value = await findSubtitles(id, season, episode, videoName.value)
    subsFetched = true
  }
  catch (e) {
    subError.value = e instanceof Error ? e.message : String(e)
  }
  finally {
    subLoading.value = false
  }
}

function setSid(id: number | 'no') {
  sid.value = id
  activeUrl.value = ''
  ipc(['set_property', 'sid', id])
}

function useTrack(t: Track) {
  setSid(t.id)
  if (t.lang)
    subLang.value = t.lang
  osd($t('Subtitles: {name}', { name: trackLabel(t) }))
}

function subsOff() {
  setSid('no')
  subLang.value = ''
  osd($t('Subtitles off'))
}

/**
 * Download this language's files so they can be told apart, once per language.
 * The video's own length goes in, because "is this file even for this video"
 * is the first thing that separates them — see `fitsRuntime`.
 */
async function probeFiles(lang: SubtitleLanguage) {
  const have = variants.value[lang.name]
  if (have)
    return have
  probing.value = lang.name
  try {
    const list = await probeLanguage(lang, duration.value)
    variants.value[lang.name] = list
    return list
  }
  finally {
    probing.value = ''
  }
}

function expand(lang: SubtitleLanguage) {
  expanded.value = expanded.value === lang.name ? '' : lang.name
  if (expanded.value)
    probeFiles(lang)
}

async function loadFile(file: Subtitle, lang: SubtitleLanguage) {
  // A file whose name named no language must not wipe the remembered one.
  if (lang.code)
    subLang.value = lang.code
  // Where the page draws the cues it has to hold them; mpv reads the URL itself
  // and only auto-sync wants a copy, which can arrive late. `probe` is cached,
  // so this costs nothing for a file the menu already listed.
  const cues = probe(file)
  if (!native)
    await cues
  // 'cached' re-selects this URL if it was added before, instead of stacking
  // a duplicate track every time the file is picked.
  await ipc(['sub-add', file.url, 'cached', lang.name, lang.code])
  activeUrl.value = file.url
  setDelay(0) // another file, its own timing
  setSubSpeed(1)
  syncNote.value = ''
  guess.value = null
  await refreshTracks()
  osd($t('Subtitles: {name}', { name: lang.name }))
}

async function useLanguage(lang: SubtitleLanguage) {
  subLang.value = lang.code
  // A track already in the file needs no download and is cut to this release.
  const own = embedded.value.find(t => t.lang && langName(t.lang) === lang.name)
  if (own)
    return useTrack(own)

  // The addon's own order is the id it matched on and nothing else: the file for
  // another cut, the hearing-impaired one and the right one arrive shuffled.
  // probeFiles reads them and puts one that covers this runtime on top.
  expanded.value = lang.name
  const list = await probeFiles(lang)
  const pick = list[0] ?? lang.files[0]
  if (pick)
    await loadFile(pick, lang)
}

/** Does this file look like it belongs to what's playing? See `fitsRuntime`. */
function fits(f: SubtitleFile) {
  return fitsRuntime(f, duration.value)
}

function setAudio(t: Track) {
  aid.value = t.id
  ipc(['set_property', 'aid', t.id])
  osd(`Audio: ${trackLabel(t)}`)
}

function toggleSubs() {
  if (subsOn.value)
    return subsOff()
  const first = embedded.value[0]
  const mine = release.value[0]
  if (first)
    useTrack(first)
  else if (mine)
    loadFile(mine.file, mine.lang)
  else
    openMenu('subs')
}

/**
 * The release's own file in a given language, if it ships one. The name is
 * matched as well as the code, because plenty spell it out: `2_English.srt`
 * gives no code to compare, only the word.
 */
function releaseSub(want: string) {
  return release.value.find(r =>
    (r.lang.code && langName(r.lang.code) === want)
    || r.lang.name.toLowerCase().includes(want.toLowerCase()))
}

/** Once per playback: honour the remembered language, file first, addon last. */
async function applyPreferredSub() {
  if (!subLang.value)
    return
  const want = langName(subLang.value)
  const own = embedded.value.find(t => t.lang && langName(t.lang) === want)
  if (own)
    return setSid(own.id)

  // Already on the disk and already cut to this encode — nothing to search for.
  const mine = releaseSub(want)
  if (mine)
    return loadFile(mine.file, mine.lang)

  await fetchExternals()
  const hit = subLanguages.value.find(l => l.name === want)
  if (hit)
    await useLanguage(hit)
}

// ---------------------------------------------------------------------------
// Timing. A file cut for another release is early or late, and one cut for
// another framerate is early or late by a little more every minute. mpv answers
// both: `sub-delay` shifts the cues and `sub-speed` multiplies their timestamps,
// so `t = cue * speed + delay`. Which pair to use is what the audio decides.
// ---------------------------------------------------------------------------
/**
 * Only a file we downloaded ourselves has cues to line up; muxed tracks are
 * already cut to the release. And only the desktop can listen to the audio at
 * all — `audio_envelope` shells out to ffmpeg, which Android has no way to run.
 */
const syncable = computed(() => native && !!probed(activeUrl.value)?.cues.length)
// Two decimals, trailing zero trimmed: the fit lands well inside a tenth of a
// second and rounding the display to one would show "+0.0s" for a real shift.
const seconds = (v: number) => `${v > 0 ? '+' : ''}${v.toFixed(2).replace(/0$/, '')}s`
const delayText = computed(() => seconds(subDelay.value))
const guessText = computed(() => seconds(guess.value?.offset ?? 0))

/** The first look. Cheap to read, and enough for most files. */
const SYNC_WINDOW = 1200

function setDelay(seconds: number) {
  subDelay.value = Math.round(seconds * 100) / 100
  ipc(['set_property', 'sub-delay', subDelay.value])
}

/** Only ever 1 or one of `RATES`; a nudge of the delay leaves it alone. */
function setSubSpeed(rate: number) {
  subSpeed.value = rate
  ipc(['set_property', 'sub-speed', rate])
}

function nudgeDelay(delta: number) {
  // Back onto the tenth-of-a-second grid: a fit leaves the delay somewhere in
  // between, and stepping from 0.14 to 0.24 reads like a stuck button.
  setDelay(Math.round((subDelay.value + delta) * 10) / 10)
  syncNote.value = ''
  guess.value = null
  osd(`Subtitle delay ${delayText.value}`)
}

/**
 * The audio behind `span` seconds of playback, clipped to what is on disk. The
 * bytes just played are certainly there; ffmpeg reading ahead of the download
 * pulls pieces away from it, or blocks on ones that never come. `onDisk` is what
 * answers that, and a plain url is always ready.
 */
async function playedSpan(span: number) {
  const from = Math.max(0, Math.min(position.value - span, (duration.value || span) - span))
  const ahead = from + span
  const to = ahead > position.value && !await onDisk(ahead) ? position.value : ahead
  return { from, to, length: to - from }
}

async function fitOver(cues: Cue[], from: number, to: number) {
  const envelope = await invoke<number[]>('audio_envelope', {
    url: props.src,
    start: from,
    duration: to - from,
  })
  return bestSync(cues, Float32Array.from(envelope), from)
}

/**
 * Both numbers are absolute — the fit is measured from the file's own
 * timestamps — so this replaces a delay set by hand rather than adding to it.
 */
function applyFit(fit: Sync) {
  const was = subDelay.value
  const wasSpeed = subSpeed.value
  setDelay(fit.offset)
  setSubSpeed(fit.speed)
  guess.value = null
  const moved = Math.abs(subDelay.value - was) >= 0.05 || fit.speed !== wasSpeed

  if (fit.speed !== 1) {
    // A rate error is global, so this one holds for the whole film rather than
    // just the scene it was measured on.
    syncNote.value = `This file was cut for a different framerate — stretched to fit, delay ${delayText.value}.`
    osd(`Subtitles synced (${delayText.value}, rate fixed)`, 2500)
  }
  else if (!moved) {
    syncNote.value = `Already in sync at ${delayText.value}.`
  }
  else {
    syncNote.value = `Delay set to ${delayText.value}.`
    osd(`Subtitles synced (${delayText.value})`, 2000)
  }
}

async function autoSync() {
  const file = probed(activeUrl.value)
  if (syncing.value || !file?.cues.length)
    return
  syncing.value = true
  syncNote.value = ''
  guess.value = null
  try {
    const near = await playedSpan(Math.min(SYNC_WINDOW, duration.value || SYNC_WINDOW))
    let fit = near.length >= SYNC_MIN_WINDOW ? await fitOver(file.cues, near.from, near.to) : null

    // Twenty minutes of an old, quiet or sparsely-spoken film can honestly mean
    // anything — and the rest of the film is more of the same signal, which is
    // exactly what a weak one needs. Reading the lot is a few seconds of ffmpeg
    // on a normal encode and half a second of arithmetic — a minute of I/O on a
    // 4K remux, which is why it waits for the cheap look to fail first, and only
    // ever covers what the download has actually reached.
    if (!fit || !synced(fit)) {
      const all = await playedSpan(duration.value || SYNC_WINDOW)
      if (all.length >= SYNC_MIN_WINDOW && all.length > near.length + 60) {
        syncWide.value = true
        const wide = await fitOver(file.cues, all.from, all.to)
        if (!fit || wide.confidence > fit.confidence)
          fit = wide
      }
    }

    if (!fit) {
      syncNote.value = `Not enough has played yet — auto-sync needs about ${Math.round(SYNC_MIN_WINDOW / 60)} minutes of audio to be sure. Nudge the delay for now.`
      return
    }

    if (!synced(fit)) {
      // A confident wrong answer is worse than none. The best it found is still
      // worth offering though: a file a minute out is unwatchable, and one wrong
      // button is cheaper than six hundred taps of the nudge.
      guess.value = fit.score > 0.05 ? fit : null
      syncNote.value = $t('Couldn\'t tell — the audio doesn\'t line up clearly with this file. Try a different subtitle file, or nudge the delay by hand.')
      return
    }

    applyFit(fit)
  }
  catch (e) {
    syncNote.value = e instanceof Error ? e.message : String(e)
  }
  finally {
    syncing.value = false
    syncWide.value = false
  }
}

// ---------------------------------------------------------------------------
// Menus. One panel, five lists — a popup would need its own cutout and a
// Vuetify overlay renders outside this root, where the tracker can't see it.
// ---------------------------------------------------------------------------
type Menu = '' | 'subs' | 'audio' | 'speed' | 'server' | 'quality' | 'sleep' | 'chapter' | 'video'
const menu = ref<Menu>('')
const MENU_TITLES: Record<Exclude<Menu, ''>, () => string> = {
  subs: () => $t('Subtitles'),
  audio: () => $t('Audio'),
  speed: () => $t('Playback speed'),
  server: () => $t('Server'),
  quality: () => $t('Quality'),
  sleep: () => $t('Sleep timer'),
  chapter: () => $t('Chapters'),
  video: () => $t('Video & Audio'),
}

// ---------------------------------------------------------------------------
// Sleep timer
// ---------------------------------------------------------------------------
const sleepTimerRemaining = ref<number | null>(null)
let sleepTimerHandle: ReturnType<typeof setTimeout> | null = null
let sleepTimerInterval: ReturnType<typeof setInterval> | null = null

const SLEEP_PRESETS = [15, 30, 45, 60] as const

function setSleepTimer(minutes: number | null) {
  clearSleepTimer()
  if (!minutes || minutes <= 0) {
    sleepTimerRemaining.value = null
    menu.value = ''
    osd($t('Sleep timer off'))
    return
  }
  sleepTimerRemaining.value = minutes * 60
  osd($t('Sleep timer: {minutes} min', { minutes }))
  menu.value = ''

  sleepTimerHandle = setTimeout(() => {
    if (started.value && !paused.value) {
      paused.value = true
      ipc(['set_property', 'pause', true])
      saveProgress()
    }
    clearSleepTimer()
    osd($t('Sleep timer: pausing'), 3000)
  }, minutes * 60 * 1000)

  sleepTimerInterval = setInterval(() => {
    if (sleepTimerRemaining.value && sleepTimerRemaining.value > 0)
      sleepTimerRemaining.value--
  }, 1000)
}

function sleepEndOfEpisode() {
  clearSleepTimer()
  osd($t('Sleep timer: end of episode'))
  menu.value = ''
  // Will be checked in the poll loop — if position is near duration, pause.
  sleepTimerRemaining.value = -1 // sentinel for "end of episode" mode
}

function clearSleepTimer() {
  if (sleepTimerHandle) {
    clearTimeout(sleepTimerHandle)
    sleepTimerHandle = null
  }
  if (sleepTimerInterval) {
    clearInterval(sleepTimerInterval)
    sleepTimerInterval = null
  }
  sleepTimerRemaining.value = null
}

function sleepTimerText(minutes: number | null) {
  if (!minutes)
    return ''
  const m = Math.floor(minutes / 60)
  const s = minutes % 60
  return m > 0 ? `${m}:${String(s).padStart(2, '0')}` : `0:${String(s).padStart(2, '0')}`
}

// ---------------------------------------------------------------------------
// Skip Intro / Credits
// ---------------------------------------------------------------------------
const skipKey = computed(() => {
  if (!props.media)
    return ''
  return progressKey(props.media.type, props.media.id, props.season ?? 0, props.episode ?? 0)
})

const introSkip = computed(() => {
  if (!skipKey.value || !started.value)
    return null
  return inIntro(skipKey.value, position.value)
})

const creditsReached = computed(() => {
  if (!skipKey.value || !started.value)
    return false
  return inCredits(skipKey.value, position.value, duration.value)
})

// ---------------------------------------------------------------------------
// Auto-play next episode countdown
// ---------------------------------------------------------------------------
const autoPlayCountdown = ref<number | null>(null)
let autoPlayHandle: ReturnType<typeof setTimeout> | null = null

function startAutoPlayCountdown() {
  if (!props.next || autoPlayCountdown.value !== null)
    return
  autoPlayCountdown.value = 15
  const tick = () => {
    if (autoPlayCountdown.value === null || autoPlayCountdown.value <= 0) {
      clearAutoPlayCountdown()
      return
    }
    autoPlayCountdown.value--
    if (autoPlayCountdown.value <= 0) {
      clearAutoPlayCountdown()
      // Navigate via router
      navigateTo(props.next!.to)
    }
    else {
      autoPlayHandle = setTimeout(tick, 1000)
    }
  }
  autoPlayHandle = setTimeout(tick, 1000)
}

function clearAutoPlayCountdown() {
  autoPlayCountdown.value = null
  if (autoPlayHandle) {
    clearTimeout(autoPlayHandle)
    autoPlayHandle = null
  }
}

// Start countdown when credits are reached (only if there's a next episode)
watch(creditsReached, v => {
  if (v && props.next)
    startAutoPlayCountdown()
})

// ---------------------------------------------------------------------------
// HDR Tone Mapping
// ---------------------------------------------------------------------------
type ToneMapping = 'auto' | 'sdr' | 'hdr-passthrough'

const toneMapping = ref<ToneMapping>('auto')

const TONE_MAPPING_PRESETS: { value: ToneMapping, label: () => string }[] = [
  { value: 'auto', label: () => $t('Auto') },
  { value: 'sdr', label: () => $t('SDR (tone-map)') },
  { value: 'hdr-passthrough', label: () => $t('HDR passthrough') },
]

/** Detect HDR / Dolby Vision from the source quality label. */
const HDR_RE = /\bhdr\b|\bdolby\s*vision\b|\bdv\b|\bhlg\b|\bpq\b/i
const isHdrContent = computed(() => HDR_RE.test(props.quality ?? ''))
/** Detect Dolby specifically (Atmos, Vision, TrueHD). */
const DOLBY_RE = /\bdolby\b|\batmos\b|\btruehd\b|\bdolby\s*digital\b/i
const isDolbyContent = computed(() => DOLBY_RE.test(props.quality ?? ''))

// Auto-apply HDR passthrough when HDR content is detected and preset is still Auto.
watch(isHdrContent, hdr => {
  if (hdr && toneMapping.value === 'auto' && native && started.value)
    applyToneMapping('hdr-passthrough')
}, { immediate: true })

function applyToneMapping(preset: ToneMapping) {
  toneMapping.value = preset
  if (!native || !started.value)
    return
  switch (preset) {
    case 'auto':
      ipc(['set_property', 'tone-mapping', 'auto'])
      ipc(['set_property', 'gamut-mapping-mode', 'auto'])
      ipc(['set_property', 'target-prim', 'auto'])
      ipc(['set_property', 'target-trc', 'auto'])
      ipc(['set_property', 'target-colorspace-hint', false])
      break
    case 'sdr':
      ipc(['set_property', 'tone-mapping', 'hable'])
      ipc(['set_property', 'gamut-mapping-mode', 'perceptual'])
      ipc(['set_property', 'target-prim', 'auto'])
      ipc(['set_property', 'target-trc', 'auto'])
      ipc(['set_property', 'target-colorspace-hint', false])
      break
    case 'hdr-passthrough':
      ipc(['set_property', 'tone-mapping', 'clip'])
      ipc(['set_property', 'gamut-mapping-mode', 'auto'])
      ipc(['set_property', 'target-prim', 'bt.2020'])
      ipc(['set_property', 'target-trc', 'pq'])
      ipc(['set_property', 'target-colorspace-hint', true])
      break
  }
  osd($t('HDR: {preset}', { preset: TONE_MAPPING_PRESETS.find(p => p.value === preset)?.label() ?? preset }))
}

// ---------------------------------------------------------------------------
// Audio Output Configuration
// ---------------------------------------------------------------------------
type AudioChannel = 'auto' | 'stereo' | 'surround' | 'passthrough'

const audioChannel = ref<AudioChannel>('auto')
const audioDeviceInfo = ref<{ ao: string, devices: string[], channels: string }>({ ao: '', devices: [], channels: '' })

const AUDIO_CHANNEL_PRESETS: { value: AudioChannel, label: () => string, desc: () => string }[] = [
  { value: 'auto', label: () => $t('Auto'), desc: () => $t('Let the system decide') },
  { value: 'stereo', label: () => $t('Stereo'), desc: () => $t('Headphones / two speakers') },
  { value: 'surround', label: () => $t('Surround'), desc: () => $t('AVR / soundbar / spatial audio') },
  { value: 'passthrough', label: () => $t('Passthrough'), desc: () => $t('Bitstream to external decoder') },
]

function applyAudioChannel(preset: AudioChannel) {
  audioChannel.value = preset
  if (!native || !started.value)
    return
  switch (preset) {
    case 'auto':
      ipc(['set_property', 'audio-channels', 'auto-safe'])
      ipc(['set_property', 'audio-passthrough', false])
      break
    case 'stereo':
      ipc(['set_property', 'audio-channels', 'stereo'])
      ipc(['set_property', 'audio-passthrough', false])
      break
    case 'surround':
      ipc(['set_property', 'audio-channels', 'auto'])
      ipc(['set_property', 'audio-passthrough', false])
      break
    case 'passthrough':
      ipc(['set_property', 'audio-channels', 'auto'])
      ipc(['set_property', 'audio-passthrough', true])
      break
  }
  osd($t('Audio output: {preset}', { preset: AUDIO_CHANNEL_PRESETS.find(p => p.value === preset)?.label() ?? preset }))
}

async function readAudioInfo() {
  if (!native)
    return
  const p = await readProps<{ 'current-ao': string, 'audio-device-list': string[], 'audio-channels': string }>(['current-ao', 'audio-device-list', 'audio-channels'])
  if (p) {
    audioDeviceInfo.value = {
      ao: p['current-ao'] ?? '',
      devices: Array.isArray(p['audio-device-list']) ? p['audio-device-list'] : [],
      channels: p['audio-channels'] ?? '',
    }
  }
}

/** Intro marking state: null = not marking, number = the recorded start time. */
const markingIntro = ref<number | null>(null)

function markIntroStart() {
  markingIntro.value = position.value
  osd($t('Intro start marked. Seek to the end of the intro and press "Mark end".'), 4000)
  menu.value = ''
}

function markIntroEnd() {
  if (markingIntro.value === null || !skipKey.value)
    return
  saveIntro(skipKey.value, markingIntro.value, position.value)
  osd($t('Intro saved. A "Skip Intro" button will appear during this section.'), 3000)
  markingIntro.value = null
}

function markCreditsStart() {
  if (!skipKey.value)
    return
  saveCredits(skipKey.value, position.value)
  osd($t('Credits start marked. The next episode will be offered from here.'), 3000)
  menu.value = ''
}

function skipIntro() {
  if (introSkip.value)
    seekTo(introSkip.value.target)
}

const menuTitle = computed(() => menu.value ? MENU_TITLES[menu.value]() : '')

/** Labelled pill for the Server / Quality selectors — text, not an icon, since "which one am I on" is their whole point. */
const PILL = computed(() => `inline-flex items-center gap-1 rounded-full border border-white/15 bg-white/6 px-3 font-medium transition-colors duration-120 hover:bg-white/14 disabled:pointer-events-none disabled:opacity-30 ${touch.value ? 'h-10 px-4' : 'h-8'} ${menu.value === 'server' || menu.value === 'quality' ? '!border-primary !text-primary' : ''}`)

const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2]

/** Server playback only — the failover list and the resolution shortcuts. */
const hasCandidates = computed(() => !!props.candidates?.servers?.length)

/** Labels for the two pills: what's playing right now, or nothing before candidates land. */
const activeQuality = computed(() => props.candidates?.qualities?.find(q => q.index === props.activeCandidate))
const activeServer = computed(() => props.candidates?.servers?.[Math.max(0, props.activeCandidate ?? 0)])

/**
 * When the parent says so, the Quality menu introduces itself the first time
 * two or more resolutions exist — "must show quality to select" made literal.
 * Once per mount; a manual close or pick ends the introduction for good.
 */
let qualityIntroduced = false
watch(
  () => [props.autoOpenQuality, props.candidates?.qualities?.length] as const,
  ([open, count]) => {
    if (open && !qualityIntroduced && (count ?? 0) > 1 && started.value) {
      menu.value = 'quality'
      qualityIntroduced = true
      emit('autoOpened')
    }
  },
)

function usePill(menuName: 'server' | 'quality') {
  openMenu(menuName)
}

function openMenu(name: Exclude<Menu, ''>) {
  menu.value = menu.value === name ? '' : name
  if (!menu.value)
    return
  refreshTracks()
  if (name === 'subs')
    fetchExternals()
}

function setSpeed(v: number) {
  speed.value = v
  ipc(['set_property', 'speed', v])
  osd(v === 1 ? $t('Normal speed') : $t('Speed {rate}×', { rate: v }))
}

function skipChapter(delta: number) {
  if (!chapters.value.length)
    return
  const idx = currentChapter.value + delta
  if (idx >= 0 && idx < chapters.value.length) {
    const ch = chapters.value[idx]!
    seekTo(ch.time)
    osd(ch.title || $t('Chapter {number}', { number: idx + 1 }))
  }
}

// ---------------------------------------------------------------------------
// Frame Advance (when paused)
// ---------------------------------------------------------------------------
function frameStep() {
  if (!started.value || !paused.value)
    return
  if (native) {
    ipc(['frame-step'])
  }
  else {
    const v = videoEl.value
    if (v)
      v.currentTime += 1 / 30 // ~30fps frame step
  }
  osd($t('Frame advance'))
}

function frameStepBack() {
  if (!started.value || !paused.value)
    return
  if (native) {
    ipc(['frame-back-step'])
  }
  else {
    const v = videoEl.value
    if (v)
      v.currentTime -= 1 / 30
  }
  osd($t('Frame back'))
}

// ---------------------------------------------------------------------------
// Subtitle Quick Adjustments (size, opacity)
// ---------------------------------------------------------------------------
function nudgeSubSize(delta: number) {
  settings.subs.size = Math.max(20, Math.min(100, settings.subs.size + delta))
  osd($t('Subtitle size: {size}', { size: settings.subs.size }))
}

function nudgeSubOpacity(delta: number) {
  settings.subs.background = Math.max(0, Math.min(1, Math.round((settings.subs.background + delta) * 100) / 100))
  osd($t('Subtitle opacity: {pct}', { pct: Math.round(settings.subs.background * 100) }))
}

// ---------------------------------------------------------------------------
// OS window fullscreen (the app window, not a CSS trick — mpv is a real window
// inside it, so faking it with fixed positioning would leave the title bar on top)
// ---------------------------------------------------------------------------
const windowFullscreen = ref(false)

async function setWindowFullscreen(on: boolean) {
  windowFullscreen.value = on

  // Android has no window to resize — the activity *is* the screen. What it
  // does have is system bars in the way and a rotation that should be locked
  // while a film is on, and the webview implements neither the Fullscreen API
  // nor screen.orientation.lock. MainActivity.kt exposes both instead.
  const android = (window as any).RivuletScreen
  if (android?.setPlayerMode) {
    android.setPlayerMode(on)
    return
  }

  try {
    await useTauriWebviewWindowGetCurrentWebviewWindow().setFullscreen(on)
  }
  catch {
    // Not running under Tauri (plain `bun dev` in a browser) — nothing to do.
  }
}

function toggleFullscreen() {
  setWindowFullscreen(!windowFullscreen.value)
}

// ---------------------------------------------------------------------------
// Geometry + cutouts
// ---------------------------------------------------------------------------
interface Rect { x: number, y: number, width: number, height: number }

/**
 * CSS px → physical px, measured rather than worked out.
 *
 * Only mpv needs this, and app scale on every target mpv runs on is the
 * webview's own page zoom (`app.vue`) — which the engines fold into
 * `devicePixelRatio` or leave beside it, and disagree about. Getting it wrong
 * parks mpv's window in the wrong place, so nothing here reasons about it: ask
 * the platform how many real pixels wide this webview is and divide by how many
 * the page thinks it is.
 *
 * Re-measured whenever the CSS viewport changes width, which is what a resize
 * and a change of zoom both do — no listener, and no round trip per frame. The
 * value it starts at is right for the ordinary case of no zoom at all, so the
 * frame or two before the first answer lands is not a jump.
 */
let pxRatio = window.devicePixelRatio || 1
let measuredAt = 0

function measurePx() {
  const css = window.innerWidth
  if (css === measuredAt || !css)
    return
  measuredAt = css
  useTauriWebviewWindowGetCurrentWebviewWindow().size().then(size => (pxRatio = size.width / css)).catch(() => {
    // No answer to be had — `devicePixelRatio` is what it keeps, which is
    // right for the only case that reaches here with mpv running: no zoom.
  })
}

/**
 * The webview viewport, in the same physical pixels the box is measured in.
 *
 * Sent alongside every geometry push for the backend that places its surface by
 * ratio rather than by scale factor (macOS — see `player_render_mac.rs`). The
 * X11 and Win32 backends are already in the units they need and ignore it.
 */
function viewport(dpr: number) {
  return {
    viewW: Math.max(1, Math.round(window.innerWidth * dpr)),
    viewH: Math.max(1, Math.round(window.innerHeight * dpr)),
  }
}

/**
 * What has to show through mpv's window. `[data-cut]` is this file's own bars;
 * the second half is a Vuetify tooltip, which teleports to the app root and so
 * would never be found by a search scoped to the player — leaving mpv painting
 * over the only label a bare icon button has.
 */
const CUT = '[data-cut], .v-tooltip > .v-overlay__content'

/** Every overlay's rectangle, clipped to the video box and in physical pixels. */
function cutouts(box: DOMRect, dpr: number): Rect[] {
  const out: Rect[] = []
  // A closed tooltip is `display: none` and measures 0x0, which the clip drops.
  for (const el of document.querySelectorAll<HTMLElement>(CUT)) {
    const r = el.getBoundingClientRect()
    const left = Math.max(r.left, box.left)
    const top = Math.max(r.top, box.top)
    const right = Math.min(r.right, box.right)
    const bottom = Math.min(r.bottom, box.bottom)
    if (right - left < 1 || bottom - top < 1)
      continue // fully outside the video (mid-slide, or off-screen)
    out.push({
      x: Math.round((left - box.left) * dpr),
      y: Math.round((top - box.top) * dpr),
      width: Math.round((right - left) * dpr),
      height: Math.round((bottom - top) * dpr),
    })
  }
  return out
}

let rafId = 0
let lastFrame = 0
let lastKey = ''

/**
 * One loop for both jobs that have to happen per frame: push geometry when it
 * actually changed (the string compare keeps it to one IPC call per real
 * change), and advance the clock so the seek bar moves at 60fps instead of
 * stepping once per poll.
 */
function frame(now: number) {
  rafId = requestAnimationFrame(frame)
  const dt = lastFrame ? Math.min(0.25, (now - lastFrame) / 1000) : 0
  lastFrame = now

  if (!started.value)
    return

  // The <video> carries the real clock and reading it costs nothing, so its
  // position is taken rather than interpolated — the cues are drawn off this,
  // and a guess between polls would leave them up to 0.4s off the picture.
  // ExoPlayer's clock is a bridge call away, which is too much per frame, so
  // that one runs forward and lets the poll correct it as mpv's does.
  const clock = exo ? undefined : videoEl.value?.currentTime
  if (clock != null) {
    if (!scrubbing.value)
      position.value = clock
  }
  else if (!paused.value && !buffering.value && !scrubbing.value && duration.value) {
    position.value = Math.min(duration.value, position.value + dt * speed.value)
  }

  // Neither shim has a surface to chase: the bars stack in CSS and the picture
  // is laid out by the page like anything else.
  if (!native)
    return

  const el = boxEl.value
  if (!el)
    return
  const r = el.getBoundingClientRect()
  measurePx()
  const dpr = pxRatio
  // Hide the native surface when the box is off-screen or not laid out —
  // otherwise it keeps painting over whatever the page scrolls under it.
  const visible = r.width >= 16 && r.height >= 16
    && r.bottom > 0 && r.top < window.innerHeight
    && r.right > 0 && r.left < window.innerWidth

  const geom = {
    ...viewport(dpr),
    x: Math.round(r.left * dpr),
    y: Math.round(r.top * dpr),
    width: Math.max(1, Math.round(r.width * dpr)),
    height: Math.max(1, Math.round(r.height * dpr)),
    visible,
    // Only a surface in front of the page needs holes cutting in it.
    cutouts: visible && overlay ? cutouts(r, dpr) : [],
  }
  const key = JSON.stringify(geom)
  if (key === lastKey)
    return
  lastKey = key
  invoke('player_set_geometry', geom).catch(() => {})
}

/**
 * mpv fails to create its video output on a 1x1 window and exits silently, so
 * never start until the box has a real size.
 */
function waitForBox(timeoutMs = 4000): Promise<DOMRect | null> {
  return new Promise(resolve => {
    const deadline = performance.now() + timeoutMs
    const check = () => {
      const r = boxEl.value?.getBoundingClientRect()
      if (r && r.width >= 16 && r.height >= 16)
        resolve(r)
      else if (performance.now() > deadline)
        resolve(null)
      else
        requestAnimationFrame(check)
    }
    check()
  })
}

// `poll` is a hoisted function declaration, so wiring the interval up here (of
// necessity, since start/stopPlayer below drive it) is safe.
const { pause: stopPoll, resume: startPoll } = useIntervalFn(poll, 200, { immediate: false })

/**
 * librqbit answers the stream endpoint with HTTP 500 for a short window after a
 * torrent is (re-)added, while it initialises. mpv does not retry — it fails the
 * open in ~7ms and exits, leaving a black box at 0:00. So poll the endpoint for
 * a real byte before launching mpv.
 *
 * A direct link gets the same probe on a much shorter leash: it is either
 * serving or it isn't, and an expired one should say so rather than spend a
 * minute looking like it's buffering.
 */
/**
 * Is this URL serving bytes right now?
 *
 * Three verdicts, because "can't check" is not "dead":
 * - `ok` — open it.
 * - `!ok` on a **local** torrent stream — keep waiting out the window; peers
 *   need time before the engine can answer.
 * - `!ok` on a **remote** link that answered HTTP ≥400 — the server itself has
 *   spoken: dead, fail over.
 * - `!ok` with `unknown` set — the fetch *threw*, which is CORS or an
 *   unroutable host. The browser refuses to say which, and media elements
 *   don't need the permission `fetch` wants — Real-Debrid links are exactly
 *   this case. Unverifiable ≠ dead: the caller opens it anyway and lets the
 *   player be the judge.
 */
async function waitForStream(url: string, timeoutMs = 60000): Promise<{ ok: boolean, status: number, unknown?: boolean, stub?: boolean }> {
  const local = url.startsWith(ENGINE)
  const deadline = Date.now() + (local ? timeoutMs : 15000)
  let status = 0
  while (Date.now() < deadline) {
    try {
      const res = await fetch(url, { headers: { Range: 'bytes=0-0' } })
      status = res.status
      // Release the connection so librqbit isn't left holding a reader.
      await res.arrayBuffer().catch(() => {})
      if (res.ok || res.status === 206) {
        // Some debrid resolvers answer a dead quota with a tiny placeholder
        // clip ("limits_exceeded.mp4") — valid video bytes, wrong movie. The
        // final URL after redirects and the full size from Content-Range give
        // it away without downloading anything.
        const finalUrl = res.url || ''
        const total = Number(res.headers.get('content-range')?.split('/')[1] ?? NaN)
        const stubLike = /limits?[_.-]?exceeded|(?:not|un)?available[_-]?clip|error[_-]?clip/i.test(finalUrl)
          || (Number.isFinite(total) && total > 0 && total < 3 * 1024 ** 2)
        return { ok: true, status, stub: stubLike }
      }
      // An explicit status from a remote host is final — waiting out a 404
      // only delays the failover. The local engine is different: statuses
      // while it warms up are just "not yet".
      if (!local && status >= 400)
        return { ok: false, status }
      if (local && status >= 400 && status < 500)
        return { ok: false, status }
    }
    catch {
      if (!local)
        return { ok: false, unknown: true, status: 0 }
      // Engine momentarily unreachable — keep waiting.
    }
    await new Promise(r => setTimeout(r, 150))
  }
  return { ok: false, status }
}

// ---------------------------------------------------------------------------
// Watch state, keyed by title rather than by stream URL — the torrent id
// changes every time the same episode is re-added. The library store owns the
// rules (what counts as watched, what's worth resuming); this only reports
// where playback got to.
// ---------------------------------------------------------------------------
function saveProgress() {
  if (props.media && duration.value)
    library.record(props.media, props.season ?? 0, props.episode ?? 0, position.value, duration.value)
}

// Pausing is the moment a resume point is worth the most — someone who walks
// away mid-film may never give the ticker another two seconds.
watch(paused, () => saveProgress())

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------
async function startPlayer() {
  if (busy.value)
    return
  busy.value = true
  errorMsg.value = ''
  ended.value = false
  try {
    if (!props.src)
      return

    if (!await waitForBox()) {
      errorMsg.value = $t('The player area never got a size, so playback was not started.')
      return
    }

    // Never hand mpv a URL that isn't serving yet — it exits instantly on a 500.
    // A torrent stream gets the patient window (peers need time to appear); a
    // direct link is a server that answers or doesn't — six seconds and the
    // candidate list moves on.
    waiting.value = true
    let probe = await waitForStream(props.src, fromEngine.value ? 60_000 : 6_000)
    waiting.value = false
    // A stub clip is a *verdict* — quota gone or key rejected. Fail over like
    // any dead server, and remember why for the last-one-standing message.
    if (probe.ok && probe.stub && !fromEngine.value) {
      const stubbed = { ...probe, ok: false, stub: true, unknown: false, status: probe.status }
      probe = stubbed
      stubSeen.value = true
    }
    // `unknown` (the probe was CORS-blocked from even asking) falls through to
    // the opener: media elements don't need the permission fetch wants, so the
    // player's own exit is the only verdict that counts for such links.
    if (!probe.ok && !probe.unknown) {
      if (fromEngine.value) {
        errorMsg.value = probe.status
          ? $t('The torrent stream isn\'t ready yet (engine replied HTTP {status}). It may still be fetching metadata from peers.', { status: probe.status })
          : $t('Could not reach the torrent engine on 127.0.0.1:3030.')
      }
      else {
        // Debrid links are minted per request and go stale; searching again is
        // what mints a fresh one, so that's what the message has to ask for —
        // unless another server is queued, in which case the message never
        // shows: the player moves to the next one instead.
        if (streamDied(probe.stub ? 'stub' : undefined))
          return
        errorMsg.value = probe.stub
          ? $t('This source answered with an error clip instead of the title — its debrid quota is exhausted or its key was rejected.')
          : probe.status
            ? $t('The link this source gave answered HTTP {status}. It may have expired — search the sources again for a fresh one.', { status: probe.status })
            : $t('The link this source gave could not be reached.')
      }
      return
    }

    if (native) {
      // Re-measure: the window may have been resized while the probe ran.
      const b = boxEl.value!.getBoundingClientRect()
      measurePx()
      const dpr = pxRatio
      await invoke('player_start', {
        url: props.src,
        ...viewport(dpr),
        x: Math.round(b.left * dpr),
        y: Math.round(b.top * dpr),
        width: Math.max(1, Math.round(b.width * dpr)),
        height: Math.max(1, Math.round(b.height * dpr)),
      })
    }
    else {
      engine ??= exoEngine() ?? videoEngine(videoEl.value!)
      await engine.start(props.src)
    }

    position.value = 0
    duration.value = 0
    cacheEnd.value = 0
    paused.value = false
    silentSaid = false
    started.value = true
    loaded = false
    tracks.value = []
    sid.value = 'no'
    aid.value = 'no'
    activeUrl.value = ''
    subText.value = ''
    subDelay.value = 0 // a fresh mpv starts at zero
    subSpeed.value = 1
    syncNote.value = ''
    guess.value = null
    lastKey = '' // force a geometry + shape push on the next frame

    // Clicks and the wheel land on the video window in front of the page, never
    // on the webview. On X11 that window is mpv's own, so it can answer them
    // itself and the poll notices what changed; on Windows mpv is handed a
    // disabled one and never sees them, and `nativeMouse` below takes over
    // instead. Where the picture is *behind* the page there is nothing to
    // arrange: the events were the document's all along.
    if (overlay) {
      ipc(['keybind', 'MBTN_LEFT', 'cycle pause'])
      ipc(['keybind', 'WHEEL_UP', 'add volume 5'])
      ipc(['keybind', 'WHEEL_DOWN', 'add volume -5'])
    }

    // Apply HDR tone mapping and read audio device info.
    applyToneMapping(toneMapping.value)
    void readAudioInfo()

    // Tracks as soon as the file is open — a dual-audio server stream should
    // show its Audio menu on the first chrome raise, not one poll later.
    void refreshTracks()

    startPoll()
  }
  catch (e) {
    started.value = false
    errorMsg.value = String(e)
    // A URL the backend refused outright is a dead server, same as a probe
    // miss — hand the failure to whoever holds the candidate list.
    if (streamDied())
      errorMsg.value = ''
  }
  finally {
    busy.value = false
  }

  // One-shot per mount: how an automatic failover announces itself.
  if (props.osdOnStart && !errorMsg.value)
    osd(props.osdOnStart, 2600)
}

/**
 * The current stream just proved unusable. With other servers queued, say so
 * and let the parent advance; its return answers whether anyone is left.
 */
function streamDied(reason?: 'stub' | 'dead') {
  if (!hasCandidates.value)
    return false
  emit('failed', reason)
  return true
}

async function stopPlayer() {
  stopPoll()
  saveProgress()
  started.value = false
  lastKey = ''
  clearSleepTimer()
  clearAutoPlayCountdown()
  chapters.value = []
  currentChapter.value = -1
  if (native)
    await invoke('player_stop').catch(() => {})
  else
    engine?.stop()
}

async function restart() {
  await stopPlayer()
  await startPlayer()
}

// ---------------------------------------------------------------------------
// Polling: playback props, plus a liveness check so a dead mpv reports itself
// instead of leaving a black rectangle behind.
// ---------------------------------------------------------------------------
const POLLED = ['pause', 'paused-for-cache', 'duration', 'time-pos', 'demuxer-cache-time', 'volume', 'mute', 'speed', 'mouse-pos', 'sub-text']

let tick = 0
let lastMouse = ''
let lastCursor = ''

async function poll() {
  if (!started.value)
    return

  // Every ~2s, confirm mpv is still alive and remember where we are.
  if (++tick % 10 === 0) {
    saveProgress()
    const st = native
      ? await invoke<{ running: boolean, log_tail: string | null }>('player_status').catch(() => null)
      : engine?.status() ?? null
    if (st && !st.running) {
      stopPoll()
      started.value = false
      // Exiting after real playback is just end-of-file, not a failure.
      if (position.value > 0 && (duration.value === 0 || position.value >= duration.value - 2)) {
        ended.value = true
        // The one unambiguous "watched" signal — mpv played the file out.
        if (props.media)
          library.finish(props.media, props.season, props.episode)
      }
      else {
        errorMsg.value = st.log_tail?.trim() || (native ? $t('mpv exited unexpectedly.') : $t('Playback stopped unexpectedly.'))
        // A server stream that stops mid-film is the server dying, not the
        // film ending — same failover as a link that never opened.
        if (!fromEngine.value)
          streamDied(stubSeen.value ? 'stub' : undefined)
      }
      return
    }

    // A picture with no sound is a codec the device lacks, and it looks exactly
    // like a muted TV until something says so. See `silent` in htmlvideo.ts —
    // only the `<video>` path can be caught out this way, since ExoPlayer says
    // outright when it has no decoder.
    if (!native && !exo && !silentSaid && position.value > 5) {
      silentSaid = !!(await readProps<{ silent: boolean }>(['silent']))?.silent
      if (silentSaid) {
        osd($t('No sound — this device can\'t decode this release\'s audio (Dolby or DTS). A release with AAC audio will play.'), 7000)
      }
    }
  }

  // Asked for together, not one after the other: they are different backends
  // and there is no reason for the cursor to wait on mpv's socket.
  const [p, cursor] = await Promise.all([readProps(POLLED), readPointer()])

  // The system's own answer to "where is the mouse", for backends that give one
  // (Windows). It is the same signal as mpv's `mouse-pos` below and is read the
  // same way, but it survives the pointer leaving the window and coming back —
  // which mpv's does not there, leaving the controls unable to un-hide again.
  if (cursor?.over) {
    noteVideoHover()
    const key = `${cursor.x},${cursor.y}`
    if (lastCursor && key !== lastCursor)
      noteActivity()
    lastCursor = key
  }

  if (!p)
    return

  if (typeof p.pause === 'boolean')
    paused.value = p.pause
  buffering.value = p['paused-for-cache'] === true
  if (typeof p.duration === 'number')
    duration.value = p.duration
  if (typeof p.volume === 'number' && !volumeHeld.value)
    volume.value = Math.round(p.volume)
  muted.value = p.mute === true
  if (typeof p.speed === 'number')
    speed.value = p.speed
  cacheEnd.value = typeof p['demuxer-cache-time'] === 'number' ? p['demuxer-cache-time'] : 0
  subText.value = typeof p['sub-text'] === 'string' ? p['sub-text'] : ''

  // The rAF loop runs the clock between polls; only correct it once it has
  // really drifted, so the bar never stutters backwards a frame.
  if (!scrubbing.value && typeof p['time-pos'] === 'number' && Math.abs(p['time-pos'] - position.value) > 0.4)
    position.value = p['time-pos']

  // Track which chapter we're in for the chapter menu highlight.
  if (chapters.value.length) {
    let ch = -1
    for (let i = chapters.value.length - 1; i >= 0; i--) {
      const chapter = chapters.value[i]
      if (chapter && position.value >= chapter.time) {
        ch = i
        break
      }
    }
    currentChapter.value = ch
  }

  // mpv's window swallows pointer events over the video, so its own cursor
  // position is the only way to notice the mouse moving there.
  const m = p['mouse-pos']
  if (m && typeof m.x === 'number') {
    // Every bar is a hole cut out of mpv's window, so a cursor mpv can see is a
    // cursor that is not on a bar. Trust that over `pointerleave`, which the
    // webview never gets for a pointer crossing into the native window.
    if (m.hover)
      noteVideoHover()
    const key = `${m.x},${m.y}`
    if (lastMouse && key !== lastMouse)
      noteActivity()
    lastMouse = key
  }

  // A-B loop: when reaching B, jump back to A.
  if (abLoop.value && position.value >= abLoop.value.b) {
    seekTo(abLoop.value.a)
  }

  // Sleep timer: end-of-episode mode — pause when we reach the end.
  if (sleepTimerRemaining.value === -1 && duration.value > 0 && position.value >= duration.value - 2) {
    paused.value = true
    ipc(['set_property', 'pause', true])
    saveProgress()
    clearSleepTimer()
    osd($t('Sleep timer: pausing'), 3000)
  }

  // Tracks only exist once mpv has the file open, and a duration is the first
  // sign of that.
  if (!loaded && duration.value > 0) {
    loaded = true
    applySubtitleStyle()
    await refreshTracks()
    applyPreferredSub()
    // Fetch chapter list once — it doesn't change during playback.
    if (native) {
      const ch = await readProps<{ 'chapter-list': { title: string, time: number }[] }>(['chapter-list'])
      chapters.value = ch?.['chapter-list'] ?? []
    }
    const saved = props.media ? library.resumeAt(props.media, props.season, props.episode) : 0
    if (saved) {
      seekTo(saved)
      osd(`Resumed at ${fmt(saved)}`, 2500)
    }
  }
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------
function togglePlay() {
  paused.value = !paused.value
  ipc(['set_property', 'pause', paused.value])
}

function seekTo(t: number) {
  position.value = Math.max(0, Math.min(duration.value || t, t))
  ipc(['set_property', 'time-pos', position.value])
}

function seekBy(delta: number) {
  seekTo(position.value + delta)
  osd(`${fmt(position.value)} / ${fmt(duration.value)}`)
}

function setVolume(v: number) {
  volume.value = Math.round(v)
  muted.value = false
  ipc(['set_property', 'mute', false])
  ipc(['set_property', 'volume', volume.value])
}

function toggleMute() {
  muted.value = !muted.value
  ipc(['set_property', 'mute', muted.value])
  osd(muted.value ? $t('Muted') : $t('Volume {level}%', { level: volume.value }))
}

const volumeIcon = computed(() => {
  if (muted.value || volume.value === 0)
    return mdiVolumeOff
  if (volume.value < 34)
    return mdiVolumeLow
  return volume.value < 67 ? mdiVolumeMedium : mdiVolumeHigh
})

// ---------------------------------------------------------------------------
// Seek previews
// ---------------------------------------------------------------------------
// The frame under the cursor on the seek bar — but only ever for a position the
// engine already holds. Decoding one it doesn't would have librqbit go and fetch
// that piece, and a hover the user never commits to would be taking bandwidth
// off the film currently playing. `haveAt` is what says no; a debrid release has
// no swarm to take anything from, so those aren't gated at all.
//
// ffmpeg does the decoding, which is the same line `syncable` draws: the <video>
// and ExoPlayer builds have no way to run it.
/**
 * Long enough to coalesce a sweep across the bar, short enough to disappear into
 * the ~75ms the decode itself costs. Nearly all of that is fixed — spawning
 * ffmpeg, opening the file, seeking — so waiting longer buys no less work.
 */
const HOVER_MS = 80
/** Frames are decoded per 5s bucket — finer than the eye wants at a film's scale. */
const BUCKET = 5
/**
 * How far off a stand-in frame may be. Generous on purpose: it goes up dimmed,
 * the time under it is exact, and the real frame replaces it a moment later. A
 * roughly right picture beats an empty box for that moment.
 */
const NEAR_S = 60
/** Where the walk starts before halving its way down to `BUCKET`. */
const COARSE = BUCKET * 128

const thumb = ref<string | null>(null)
/** The frame up is a neighbour's, not this position's. Shown faded. */
const approx = ref(false)
/** Blob URL per bucket, `''` for a position ffmpeg had no frame at. */
const thumbs = new Map<number, string>()
/** Buckets ffmpeg is busy with, so a sweep can't queue the same one twice. */
const pending = new Set<number>()
let pieces: PieceMap | null = null
let haves: Uint8Array | null = null
let havesAt = 0
let wanted = -1
let hoverTimer: ReturnType<typeof setTimeout> | undefined
/**
 * Bumped to disown every decode in flight — the frame ffmpeg is working on is of
 * a film nobody is looking at any more, or isn't playing at all. Not the cache's
 * identity: hiding the bar calls the work off without throwing the frames away.
 */
let era = 0

function cancelThumbs() {
  era++
}

function dropThumbs() {
  cancelThumbs()
  thumbs.forEach(url => url && URL.revokeObjectURL(url))
  thumbs.clear()
  pending.clear()
  thumb.value = null
  pieces = null
  haves = null
}

async function onDisk(at: number) {
  const parts = streamParts(props.src)
  if (!parts)
    return true // a plain URL: every byte is one range request away
  pieces ??= await pieceMap(parts.id, parts.index)
  // Refetched as the download grows. A stale bitfield only ever hides a frame
  // we could have shown, never invents one we haven't got.
  if (!haves || Date.now() - havesAt > 5000) {
    haves = await torrentHaves(parts.id)
    havesAt = Date.now()
  }
  return !!pieces && !!haves && haveAt(pieces, haves, at / (duration.value || 1))
}

/** Decode one bucket into the cache, unless it's there or on its way. */
async function grab(bucket: number) {
  if (thumbs.has(bucket) || pending.has(bucket))
    return
  const mine = era
  pending.add(bucket)
  try {
    // Left uncached when the bytes aren't down yet — unlike a miss, that is an
    // answer which changes as the download runs.
    if (!await onDisk(bucket))
      return
    const bytes = await invoke<ArrayBuffer>('thumbnail', { url: props.src, at: bucket }).catch(() => null)
    // Next episode may have started while ffmpeg worked, and this frame is of
    // the last one — under a bucket number the new film will read as its own.
    if (mine !== era)
      return
    // Misses are remembered too: a position ffmpeg can't decode never will.
    thumbs.set(bucket, bytes?.byteLength ? URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' })) : '')
  }
  finally {
    pending.delete(bucket)
  }
}

/**
 * Put up the best frame we have for `bucket`: its own, or a neighbour's faded
 * out. Over a film the walk has been across, that makes the bubble land filled
 * in and sharpen a moment later, rather than opening empty every time.
 */
function show(bucket: number) {
  const exact = thumbs.get(bucket)
  approx.value = !exact
  thumb.value = exact || nearestFrame(thumbs, bucket, NEAR_S)
}

function onHover(at: number | null) {
  clearTimeout(hoverTimer)
  if (at === null || !native || !duration.value) {
    thumb.value = null
    return
  }

  const bucket = Math.floor(at / BUCKET) * BUCKET
  wanted = bucket
  show(bucket)
  // Cached, or cached as a position ffmpeg gets nothing from — either way there
  // is nothing left to decode.
  if (thumbs.has(bucket))
    return

  // Only where the cursor comes to rest gets decoded, not every pixel it swept.
  hoverTimer = setTimeout(async () => {
    await grab(bucket)
    // The cursor may have moved on while ffmpeg worked.
    if (wanted === bucket)
      show(bucket)
    // A cursor that stopped is about to nudge. Both neighbours cost one ffmpeg
    // each against a wait the user would otherwise sit through twice.
    for (const near of [bucket - BUCKET, bucket + BUCKET]) {
      if (near >= 0 && near < duration.value)
        void grab(near)
    }
  }, HOVER_MS)
}

/**
 * With the bar up, fill the cache in `walkOrder`'s order so a scrub lands on a
 * frame already in hand rather than waiting on ffmpeg for one.
 *
 * The bar is the whole trigger: it means someone is at the controls, and it
 * hides 2.8s into untouched playback — so a film watched straight through never
 * warms a single frame. One at a time, and every bucket goes through `grab`, so
 * the walk thins out by itself over a part-downloaded film.
 */
let warming = false

async function warm() {
  if (warming || !native || !started.value)
    return
  const mine = era
  warming = true
  try {
    for (const at of walkOrder(duration.value, BUCKET, COARSE)) {
      // The bar hiding is a whole film's worth of work called off.
      if (mine !== era)
        return
      await grab(at)
    }
  }
  finally {
    warming = false
  }
}

// ---------------------------------------------------------------------------
// Auto-hiding chrome
// ---------------------------------------------------------------------------
/**
 * How long the chrome stays up with nobody doing anything. A remote is slower
 * than a mouse — you look at the button, move to it, then press — and the bars
 * going away mid-aim on a television is what makes one feel broken, so a set
 * gets more than twice as long.
 */
const IDLE_MS = isTv() ? 6500 : 2800
const hovering = ref(false)
/**
 * Is there a pointer that can hover at all? A television answers `hover: none`,
 * and it means it — but the webview still fires `pointerenter` at whatever turns
 * up under its idea of where a pointer last was, which is the middle of the
 * screen, which is where the transport is. One phantom enter with no leave
 * behind it, and `hovering` stayed true for the rest of the film: the bars never
 * went away once, on the one device where covering the picture matters most.
 */
const hoverable = useMediaQuery('(hover: hover)')
/** A control in the chrome holds keyboard focus — someone is driving with a remote. */
const focused = ref(false)
let hideTimer: ReturnType<typeof setTimeout> | null = null

/**
 * A mouse click focuses the button it lands on too, and nothing ever takes that
 * focus back — clicks over the video go to mpv's own window, so the webview
 * never sees one — which pinned the bars open for the rest of the film.
 * `:focus-visible` is the browser's own answer to "did a keyboard do this?"; an
 * engine too old for the selector throws, and those are the TV ones, where the
 * answer is yes.
 */
function onFocusIn(e: FocusEvent) {
  try {
    focused.value = (e.target as HTMLElement).matches(':focus-visible')
  }
  catch {
    focused.value = true
  }
}

/** mpv can only see the cursor where no bar is cut out of its window. */
function noteVideoHover() {
  hovering.value = false
}

/**
 * A touchscreen still sends the legacy mouse events after a tap — Android's
 * webview fires `mousemove` on the way to `click` — and that arrives at the
 * root just after `tapVideo` has put the bars away, which showed them straight
 * back up. A tap that visibly did nothing. Real pointer movement only.
 */
function onMouseMove() {
  if (!touch.value)
    noteActivity()
}

/**
 * Put the chrome away, and let go of whatever it had focused: a hidden button
 * holding focus leaves a remote pressing OK at nothing it can see. Dropping it
 * costs nothing, because OK on the picture brings the bars back with the play
 * button under the cursor again (see `onKey`).
 */
function hideChrome() {
  ui.value = false
  const at = document.activeElement as HTMLElement | null
  if (at && rootEl.value?.contains(at))
    at.blur()
}

function noteActivity() {
  ui.value = true
  if (hideTimer)
    clearTimeout(hideTimer)
  hideTimer = null
  // Keep them up while paused, stopped, hovered, or reading a menu — hiding only
  // makes sense mid-playback. Focus does *not* keep them up: on a TV every press
  // leaves something focused, which pinned the bars over the film for good.
  if (started.value && !paused.value && !menu.value && !(hovering.value && hoverable.value))
    hideTimer = setTimeout(hideChrome, IDLE_MS)
}

// Not `focused`: the blur `hideChrome` does would fire this straight back.
watch([started, paused, menu, hovering], noteActivity)

// The bar appearing is the only notice anyone gives that a scrub may be coming.
// `duration` is in there because it lands a beat after playback starts, and a
// walk that began without it would have had no film to walk over.
watch([ui, started, duration], ([up]) => up ? warm() : cancelThumbs())

/** Stalls flap on a torrent; wait a beat before punching a hole for the notice. */
const stalled = ref(false)
watchDebounced(() => buffering.value && started.value, v => (stalled.value = v), { debounce: 500 })

const centre = computed(() => {
  if (errorMsg.value)
    return 'error'
  if (ended.value)
    return 'ended'
  if (busy.value)
    return 'loading'
  return stalled.value && !paused.value ? 'stalled' : ''
})

// ---------------------------------------------------------------------------
// Keyboard shortcuts (ignored while typing in a field).
// ---------------------------------------------------------------------------
function isTypingTarget(t: HTMLElement | null) {
  if (!t)
    return false
  if (t.isContentEditable)
    return true
  return t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.tagName === 'INPUT'
}

function nudgeVolume(delta: number) {
  setVolume(Math.max(0, Math.min(100, volume.value + delta)))
  osd(`Volume ${volume.value}%`)
}

/** mpv reports speed as a float, so match the closest preset rather than ===. */
function speedStep(delta: number) {
  const at = SPEEDS.findIndex(s => Math.abs(s - speed.value) < 0.01)
  const next = (at < 0 ? SPEEDS.indexOf(1) : at) + delta
  return SPEEDS[Math.max(0, Math.min(SPEEDS.length - 1, next))] ?? 1
}

const KEYS: Record<string, () => void> = {
  ' ': togglePlay,
  'k': togglePlay,
  'Enter': () => focusChrome(),
  'ArrowLeft': () => seekBy(-5),
  'ArrowRight': () => seekBy(5),
  'j': () => seekBy(-10),
  'l': () => seekBy(10),
  'ArrowUp': () => nudgeVolume(5),
  'ArrowDown': () => nudgeVolume(-5),
  'm': toggleMute,
  'f': toggleFullscreen,
  'c': toggleSubs,
  's': () => openMenu('subs'),
  'z': () => nudgeDelay(-0.1),
  'Z': () => nudgeDelay(0.1),
  '[': () => setSpeed(speedStep(-1)),
  ']': () => setSpeed(speedStep(1)),
  'Home': () => seekTo(0),
  'End': () => seekTo(Math.max(0, duration.value - 5)),
  'n': () => skipChapter(1),
  'p': () => skipChapter(-1),
  // Frame advance (pause only)
  '.': frameStep,
  ',': frameStepBack,
  // A-B loop
  'i': setLoopA,
  'o': setLoopB,
  'I': clearLoop,
  // Stats toggle
  'd': () => { showStats.value = !showStats.value },
  // Video & Audio menu
  'v': () => openMenu('video'),
  // Subtitle quick adjust
  'r': () => nudgeSubSize(4),
  'R': () => nudgeSubSize(-4),
  't': () => nudgeSubOpacity(0.1),
  'T': () => nudgeSubOpacity(-0.1),
}

/**
 * Where a remote picks up: the chrome comes back and the cursor lands on play,
 * whether it got here by pressing OK on the picture or down out of it.
 */
function focusChrome() {
  noteActivity()
  nextTick(() => playBtn.value?.focus())
}

function onKey(e: KeyboardEvent) {
  if (isTypingTarget(e.target as HTMLElement | null))
    return

  // Captured before the page's own Escape handler, so a menu closes instead of
  // the whole player. preventDefault is what tells Android's back key that the
  // press was used up here (see plugins/dpad.client.ts).
  if (e.key === 'Escape' && menu.value) {
    e.preventDefault()
    e.stopPropagation()
    menu.value = ''
    return
  }

  noteActivity()
  if (!started.value)
    return

  // Remote control. Once a control has focus, the keys it would use are its
  // own: the browser clicks it on Enter or space, and the d-pad plugin does the
  // moving. The letter shortcuts below still work either way.
  if (focused.value && (e.key.startsWith('Arrow') || e.key === 'Enter' || e.key === ' ')) {
    // Except up, which hands the screen back to the film; the bar then hides on
    // its own. Inside a menu up is just the row above, so leave that alone.
    if (e.key === 'ArrowUp' && !menu.value) {
      e.preventDefault()
      ;(document.activeElement as HTMLElement | null)?.blur()
    }
    return
  }

  // Down out of the picture is how a remote reaches the bar in the first place.
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    focusChrome()
    return
  }

  // 0–9 jump to that tenth of the file.
  const digit = /^\d$/.test(e.key) && duration.value
    ? () => seekTo(duration.value * (Number(e.key) / 10))
    : null
  const run = KEYS[e.key] ?? digit
  if (!run)
    return
  e.preventDefault()
  run()
}

// ---------------------------------------------------------------------------
// Mouse on the picture
//
// The video is a native window sitting above the webview, so a click or a wheel
// notch over it is never a DOM event here. Where mpv can answer them it does
// (see `startPlayer`); where it can't — Windows, where its embedded window is
// created disabled — the Rust side reads them off its own window and reports
// them, and these are what arrive.
//
// Clicking the picture toggles playback, which is what YouTube, Netflix, Plex
// and Windows' own player all do; the click that brings the app back from
// another window is filtered out in Rust, so returning to a film doesn't pause
// it.
// ---------------------------------------------------------------------------
let nativeMouse: (() => void)[] = []
let live = true

function onNativeClick() {
  noteActivity()
  // Clicking off an open menu dismisses it rather than pausing, the same as
  // clicking outside any other popup.
  if (menu.value)
    menu.value = ''
  else if (started.value)
    togglePlay()
}

// ---------------------------------------------------------------------------
// Touch gestures on the picture
//
// What every player on a phone does: one tap works the chrome, two taps on a
// side seek that way, two taps in the middle pause. The single tap acts
// immediately rather than waiting to find out whether a second one is coming —
// holding the most-used gesture for 300ms to spare an occasional blink of the
// bars is the wrong trade, and it is the same call YouTube makes.
// ---------------------------------------------------------------------------

/** How far a double tap jumps. */
const TAP_SEEK = 10
/** Two taps further apart than this are two separate taps. */
const DOUBLE_TAP_MS = 300

/** Which side to flash an arrow on, so a seek is visibly a seek. */
const seekFlash = ref<'back' | 'forward' | ''>('')
let lastTap = 0
let flashTimer: ReturnType<typeof setTimeout> | null = null

function flashSeek(side: 'back' | 'forward') {
  seekFlash.value = side
  if (flashTimer)
    clearTimeout(flashTimer)
  flashTimer = setTimeout(() => (seekFlash.value = ''), 500)
}

/** Show the bars, or put them away — what a bare tap does. */
function toggleChrome() {
  if (ui.value) {
    hideChrome()
    if (hideTimer)
      clearTimeout(hideTimer)
    hideTimer = null
  }
  else {
    noteActivity()
  }
}

/**
 * The same press, where the picture is a DOM element rather than a window over
 * one. A mouse gets the click-to-pause every desktop player has; a finger gets
 * the gestures above.
 *
 * `.stop` in the template matters: the root's own `pointerdown` would otherwise
 * show the bars first, and `toggleChrome` would read that as "already up" and
 * hide them again — a tap that did nothing at all.
 */
function tapVideo(e: PointerEvent) {
  if (menu.value) {
    menu.value = ''
    return
  }
  if (e.pointerType === 'mouse' && !touch.value)
    return onNativeClick()

  const now = Date.now()
  // Zeroed on the second tap, so three taps are a double and then a single
  // rather than two overlapping doubles.
  const double = now - lastTap < DOUBLE_TAP_MS
  lastTap = double ? 0 : now

  if (!double)
    return toggleChrome()

  if (!started.value)
    return

  // Which third of the picture was hit. The middle stays neutral so a tap aimed
  // at the centre of the frame never scrubs it by accident.
  const box = (e.currentTarget as HTMLElement).getBoundingClientRect()
  const at = box.width ? (e.clientX - box.left) / box.width : 0.5
  if (at < 0.35) {
    seekBy(-TAP_SEEK)
    flashSeek('back')
  }
  else if (at > 0.65) {
    seekBy(TAP_SEEK)
    flashSeek('forward')
  }
  else {
    togglePlay()
  }
}

function onNativeWheel(notches: number) {
  noteActivity()
  if (started.value)
    nudgeVolume(notches * 5)
}

async function listenToNativeMouse() {
  // Nothing in front of the page, so the picture's own pointer events already
  // arrive in the DOM — see `tapVideo`.
  if (!overlay)
    return
  try {
    const off = await Promise.all([
      listen('player:click', onNativeClick),
      listen<number>('player:wheel', e => onNativeWheel(e.payload)),
    ])
    // Registration is a round trip to the backend, so the player may already
    // have been torn down by the time it lands.
    if (live)
      nativeMouse = off
    else
      off.forEach(f => f())
  }
  catch {
    // Not running under Tauri (plain `bun dev`) — there is no native surface.
  }
}

watch(() => props.src, src => {
  dropThumbs() // a different file, and the buckets meant seconds into the old one
  if (src)
    restart()
  else
    stopPlayer()
})

/**
 * ExoPlayer and macOS's mpv both paint below the webview, so while a film is up
 * the page has to stop painting over it — the whole chain from <html> down to
 * the box, which is why this is a class on the document and not something
 * scoped to this component. Every other screen keeps its own background.
 */
watch(() => behind && started.value, on => {
  document.documentElement.classList.toggle('rivulet-video', on)
})

onMounted(() => {
  window.addEventListener('keydown', onKey, true)
  // Ahead of the first geometry push rather than alongside it, so the window
  // mpv opens is already the right size at any scale but 100%.
  if (native)
    measurePx()
  rafId = requestAnimationFrame(frame)
  listenToNativeMouse()
  if (props.fullscreen)
    setWindowFullscreen(true)
  startPlayer()
})

onBeforeUnmount(() => {
  cancelAnimationFrame(rafId)
  document.documentElement.classList.remove('rivulet-video')
  window.removeEventListener('keydown', onKey, true)
  live = false
  nativeMouse.forEach(off => off())
  stopPoll()
  saveProgress()
  clearSleepTimer()
  clearAutoPlayCountdown()
  clearTimeout(hoverTimer)
  dropThumbs()
  if (osdTimer)
    clearTimeout(osdTimer)
  if (flashTimer)
    clearTimeout(flashTimer)
  if (windowFullscreen.value)
    setWindowFullscreen(false)
  if (native)
    invoke('player_stop').catch(() => {})
  else
    engine?.stop()
})

function fmt(s: number) {
  if (!Number.isFinite(s) || s < 0)
    s = 0
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = Math.floor(s % 60)
  const mm = h > 0 ? String(m).padStart(2, '0') : String(m)
  return `${h > 0 ? `${h}:` : ''}${mm}:${String(sec).padStart(2, '0')}`
}

const remaining = computed(() => duration.value ? `-${fmt((duration.value - position.value) / speed.value)}` : '')

defineExpose({ osd })
</script>

<template>
  <div
    ref="rootEl"
    data-video-hole
    class="relative h-full w-full overflow-hidden bg-black text-white"
    :class="{ 'cursor-none': !ui }"
    @mousemove="onMouseMove"
    @pointerdown="noteActivity"
    @focusin="onFocusIn"
    @focusout="focused = false"
  >
    <!-- The rectangle mpv paints into. Everything below is punched out of it —
         or, with no native window, the box the <video> fills and the bars
         simply stack over. On Android it is a hole in the page: ExoPlayer's
         SurfaceView is behind the whole webview, so the taps land here rather
         than on a picture element that isn't there. -->
    <div ref="boxEl" data-video-hole class="absolute inset-0">
      <!-- playsinline: without it iOS/Android hand the file to the system
           player full screen, taking the controls, the subtitles and the
           watch history with it. -->
      <video
        v-if="!native && !exo"
        ref="videoEl"
        class="h-full w-full bg-black"
        playsinline
        @pointerdown.stop="tapVideo"
      />
      <!-- A picture painted behind the whole webview (ExoPlayer, and mpv on
           macOS) leaves no element under the finger. This is what the taps land
           on instead — transparent, or it would be the thing covering it. -->
      <div v-else-if="behind" class="h-full w-full" @pointerdown.stop="tapVideo" />
    </div>

    <!-- Subtitles, where the page draws them itself. Pinned to `sub-pos` the
         same way mpv reads it: 100 is the bottom of the frame. -->
    <div
      v-if="cueText"
      class="pointer-events-none absolute inset-x-0 px-[6%] text-center leading-tight"
      :style="{ bottom: `${Math.max(0, 104 - subPos)}%` }"
    >
      <span class="inline-block whitespace-pre-line rounded px-1.5" :style="cueStyle">{{ cueText }}</span>
    </div>

    <!-- The same notices mpv would `show-text`, for the backend that can't. -->
    <div
      v-if="osdText"
      class="pointer-events-none absolute left-1/2 top-6 rounded-lg bg-black/72 px-3 py-1.5 text-body-medium -translate-x-1/2"
    >
      {{ osdText }}
    </div>

    <!-- A-B loop indicator -->
    <div
      v-if="abLoop && ui"
      data-cut
      class="absolute left-4 top-16 flex items-center gap-2 rounded-lg border border-primary/40 bg-black/70 px-3 py-1.5 text-label-small text-primary backdrop-blur-sm"
    >
      <v-icon :icon="mdiAlphaA" size="14" />
      <span class="tabular-nums">{{ fmt(abLoop.a) }} → {{ fmt(abLoop.b) }}</span>
    </div>

    <!-- Network stats overlay -->
    <transition
      enter-active-class="transition-opacity duration-150"
      leave-active-class="transition-opacity duration-150"
      enter-from-class="opacity-0"
      leave-to-class="opacity-0"
    >
      <div
        v-if="showStats && statsData && ui"
        data-cut
        class="absolute left-4 top-28 flex flex-col gap-1 rounded-lg border border-white/10 bg-black/75 px-3 py-2 text-label-small backdrop-blur-sm"
      >
        <div class="flex items-center gap-2 opacity-70">
          <v-icon :icon="mdiInformationOutline" size="12" />
          <span class="text-label-xs uppercase opacity-50">{{ $t('Network') }}</span>
        </div>
        <div class="flex gap-4">
          <span>↓ {{ statsData.download }}</span>
          <span>↑ {{ statsData.upload }}</span>
        </div>
        <span>{{ $t('{peers} peers', { peers: statsData.peers }) }}</span>
      </div>
    </transition>

    <!-- Double-tap seek, made visible. Only ever on the <video> path, which is
         the only one whose picture is a DOM element the taps can land on, so
         there is no cutout to punch for it. -->
    <transition
      enter-active-class="transition-opacity duration-100"
      leave-active-class="transition-opacity duration-300"
      enter-from-class="opacity-0"
      leave-to-class="opacity-0"
    >
      <div
        v-if="seekFlash"
        class="pointer-events-none absolute top-1/2 flex flex-col items-center gap-1 rounded-full bg-black/55 px-5 py-4 -translate-y-1/2"
        :class="seekFlash === 'back' ? 'left-[8%]' : 'right-[8%]'"
      >
        <v-icon :icon="seekFlash === 'back' ? mdiRewind10 : mdiFastForward10" size="34" />
        <span class="text-label-medium tabular-nums">{{ TAP_SEEK }}s</span>
      </div>
    </transition>

    <transition
      :enter-active-class="SLIDE"
      :leave-active-class="SLIDE"
      enter-from-class="-translate-y-full"
      leave-to-class="-translate-y-full"
    >
      <header
        v-show="ui"
        data-cut
        class="absolute inset-x-0 top-0 h-14 flex items-center gap-2 border-b px-3"
        :class="SURFACE"
        @pointerenter="hovering = true"
        @pointerleave="hovering = false"
      >
        <slot name="start" />
        <div class="min-w-0 flex-1">
          <slot name="info" />
        </div>
        <!-- HDR / Dolby badges, auto-detected from the source quality label. -->
        <div v-if="isHdrContent" class="flex shrink-0 items-center gap-1.5">
          <span class="rounded bg-amber-500/20 px-1.5 py-0.5 text-label-small font-bold text-amber-400">
            HDR
          </span>
          <span v-if="isDolbyContent" class="rounded bg-sky-500/20 px-1.5 py-0.5 text-label-small font-bold text-sky-400">
            DV
          </span>
        </div>
      </header>
    </transition>

    <!-- The transport, dead centre, up with the rest of the chrome: where a
         d-pad lands (see `focusChrome`) and where a thumb already is, so a tap
         in the middle of the picture with the bars up pauses rather than doing
         nothing. Hidden behind the centre notices below, which own the same
         patch of screen.

         Only where the page draws over the picture and there is a thumb to draw
         it for — see `barTransport` for the two that keep it in the bottom bar
         instead. No transition for the same reason the bars slide rather than
         fade: there is nothing behind this to fade against. -->
    <div
      v-if="!barTransport"
      v-show="ui && started && !centre"
      class="absolute left-1/2 top-1/2 flex items-center gap-3 rounded-full border border-white/9 bg-[#0e0f11]/70 px-3 py-3 -translate-x-1/2 -translate-y-1/2"
      @pointerenter="hovering = true"
      @pointerleave="hovering = false"
    >
      <button v-tooltip:top="$t('Back 10s (j)')" :class="SEEK_BTN" :disabled="!started" @click="seekBy(-10)">
        <v-icon :icon="mdiRewind10" size="26" />
      </button>
      <button
        ref="playBtn"
        v-tooltip:top="paused ? $t('Play (space)') : $t('Pause (space)')"
        :class="PLAY_BTN"
        :disabled="!started"
        @click="togglePlay"
      >
        <v-icon :icon="paused ? mdiPlay : mdiPause" size="38" />
      </button>
      <button v-tooltip:top="$t('Forward 10s (l)')" :class="SEEK_BTN" :disabled="!started" @click="seekBy(10)">
        <v-icon :icon="mdiFastForward10" size="26" />
      </button>
    </div>

    <!-- Status, dead centre. A cutout, so it works while mpv is on screen too. -->
    <div
      v-if="centre"
      :key="centre"
      data-cut
      class="absolute left-1/2 top-1/2 flex items-center gap-2.5 border -translate-x-1/2 -translate-y-1/2"
      :class="[
        SURFACE,
        centre === 'stalled'
          ? 'rounded-full px-4 py-2.5 text-body-medium'
          : 'max-w-[min(520px,80%)] flex-col rounded-2xl px-6.5 py-5.5 text-center',
      ]"
    >
      <template v-if="centre === 'error'">
        <v-icon :icon="mdiAlertCircleOutline" size="30" color="error" />
        <div class="text-title-small">
          {{ $t('Playback failed') }}
        </div>
        <pre class="max-h-32.5 max-w-full overflow-auto whitespace-pre-wrap text-left text-label-small font-mono opacity-70">{{ errorMsg }}</pre>
        <button :class="BTN" :disabled="busy" @click="startPlayer">
          <v-icon :icon="mdiReload" size="18" /> {{ $t('Retry') }}
        </button>
      </template>

      <template v-else-if="centre === 'ended'">
        <div class="text-title-small">
          {{ $t('Playback finished') }}
        </div>
        <div v-if="autoPlayCountdown !== null" class="text-body-small opacity-70">
          {{ $t('Next episode in {seconds}s', { seconds: autoPlayCountdown }) }}
        </div>
        <div class="flex gap-2">
          <nuxt-link v-if="next" :class="BTN" :to="next.to" @click="clearAutoPlayCountdown">
            <v-icon :icon="mdiSkipNext" size="18" /> {{ next.label }}
          </nuxt-link>
          <button :class="BTN" :disabled="busy" @click="clearAutoPlayCountdown(); startPlayer()">
            <v-icon :icon="mdiReload" size="18" /> {{ $t('Play again') }}
          </button>
          <button
            v-if="autoPlayCountdown !== null"
            :class="BTN"
            @click="clearAutoPlayCountdown"
          >
            {{ $t('Cancel') }}
          </button>
        </div>
      </template>

      <template v-else-if="centre === 'loading'">
        <v-progress-circular indeterminate color="primary" size="28" width="3" />
        <div class="text-title-small">
          {{ waiting ? $t('Waiting for the torrent stream…') : native ? $t('Starting mpv…') : $t('Opening the stream…') }}
        </div>
        <div v-if="status" class="text-body-small opacity-60">
          {{ status }}
        </div>
      </template>

      <template v-else>
        <v-progress-circular indeterminate color="primary" size="20" width="2" />
        <span>{{ $t('Buffering') }}<template v-if="status"> · {{ status }}</template></span>
      </template>
    </div>

    <transition
      :enter-active-class="SLIDE"
      :leave-active-class="SLIDE"
      enter-from-class="translate-y-[115%]"
      leave-to-class="translate-y-[115%]"
    >
      <!-- bottom: the bottom bar (h-24) plus a 10px gap. Bound rather than a
           class because `subPos` measures the subtitles against it. -->
      <section
        v-if="menu"
        ref="menuEl"
        data-cut
        class="absolute right-4 max-h-[44vh] w-75 flex flex-col overflow-hidden border rounded-xl"
        :class="SURFACE"
        :style="{ bottom: `${menuBottom}px` }"
        @pointerenter="hovering = true"
        @pointerleave="hovering = false"
      >
        <header class="flex items-center justify-between border-b border-white/9 py-2 pl-3.5 pr-2 text-title-small">
          <span>{{ menuTitle }}</span>
          <button v-tooltip:top="$t('Close')" class="!h-7 !min-w-7" :class="ICO" @click="menu = ''">
            <v-icon :icon="mdiClose" size="16" />
          </button>
        </header>

        <div class="overflow-y-auto p-1.5">
          <template v-if="menu === 'speed'">
            <button v-for="v in SPEEDS" :key="v" :class="[MENU_ROW, speed === v && 'text-primary']" @click="setSpeed(v)">
              <span>{{ v === 1 ? $t('Normal') : `${v}×` }}</span>
              <v-icon v-if="speed === v" :icon="mdiCheck" size="16" />
            </button>
          </template>

          <template v-else-if="menu === 'audio'">
            <button
              v-for="t in audioTracks"
              :key="t.id"
              :class="[MENU_ROW, aid === t.id && 'text-primary']"
              @click="setAudio(t)"
            >
              <span class="truncate">{{ trackLabel(t) }}</span>
              <v-icon v-if="aid === t.id" :icon="mdiCheck" size="16" />
            </button>
            <p v-if="!audioTracks.length" :class="NOTE">
              {{ $t('This file has one audio track.') }}
            </p>
          </template>

          <!-- The servers this title was found on, best first. Picking one is
               also how a dead stream is recovered by hand; a dying one is left
               for on its own (see `streamDied`). -->
          <template v-else-if="menu === 'server'">
            <p :class="MENU_GROUP">
              {{ $t('Source') }}
            </p>
            <button
              v-for="(s, i) in props.candidates?.servers ?? []"
              :key="s.index"
              :class="[MENU_ROW, i === activeCandidate && 'text-primary']"
              @click="emit('useCandidate', s.index); menu = ''"
            >
              <span class="min-w-0 flex-1">
                <span class="block truncate font-medium">{{ s.label }}</span>
                <span v-if="s.detail" class="block truncate text-label-small opacity-50">{{ s.detail }}</span>
              </span>
              <v-icon v-if="i === activeCandidate" :icon="mdiCheck" size="16" />
            </button>
            <p v-if="!(props.candidates?.servers?.length)" :class="NOTE">
              {{ $t('No other servers available.') }}
            </p>
          </template>

          <!-- One entry per resolution the servers carry: switching quality is
               switching to that server's copy, which every backend can do. -->
          <template v-else-if="menu === 'quality'">
            <p :class="MENU_GROUP">
              {{ $t('Quality') }}
            </p>
            <button
              v-for="q in props.candidates?.qualities ?? []"
              :key="q.index"
              :class="[MENU_ROW, q.index === activeCandidate && 'text-primary']"
              @click="emit('useCandidate', q.index); menu = ''"
            >
              <span class="min-w-0 flex-1">
                <span class="block truncate font-medium">{{ q.label }}</span>
                <span v-if="q.detail" class="block truncate text-label-small opacity-50">{{ q.detail }}</span>
              </span>
              <v-icon v-if="q.index === activeCandidate" :icon="mdiCheck" size="16" />
            </button>
            <p v-if="(props.candidates?.qualities?.length ?? 0) < 2" :class="NOTE">
              {{ $t('Your sources offer one quality for this title.') }}
            </p>
          </template>

          <template v-else-if="menu === 'sleep'">
            <button
              v-for="m in SLEEP_PRESETS"
              :key="m"
              :class="[MENU_ROW, sleepTimerRemaining && sleepTimerRemaining > 0 && Math.abs(sleepTimerRemaining - m * 60) < 30 && 'text-primary']"
              @click="setSleepTimer(m)"
            >
              <span>{{ $t('{minutes} minutes', { minutes: m }) }}</span>
              <v-icon v-if="sleepTimerRemaining && sleepTimerRemaining > 0 && Math.abs(sleepTimerRemaining - m * 60) < 30" :icon="mdiCheck" size="16" />
            </button>
            <button
              :class="[MENU_ROW, sleepTimerRemaining === -1 && 'text-primary']"
              @click="sleepEndOfEpisode"
            >
              <span>{{ $t('End of episode') }}</span>
              <v-icon v-if="sleepTimerRemaining === -1" :icon="mdiCheck" size="16" />
            </button>
            <button
              v-if="sleepTimerRemaining"
              :class="MENU_ROW"
              @click="setSleepTimer(null)"
            >
              <span class="text-error">{{ $t('Cancel timer') }}</span>
            </button>
            <div class="border-t border-white/9 mt-1 pt-1">
              <button v-if="!markingIntro" :class="MENU_ROW" @click="markIntroStart">
                <span>{{ $t('Mark intro start') }}</span>
              </button>
              <button v-else :class="MENU_ROW" @click="markIntroEnd">
                <span class="text-primary">{{ $t('Mark intro end') }}</span>
              </button>
              <button :class="MENU_ROW" @click="markCreditsStart">
                <span>{{ $t('Mark credits start') }}</span>
              </button>
            </div>
          </template>

          <template v-else-if="menu === 'chapter'">
            <button
              v-for="(ch, i) in chapters"
              :key="i"
              :class="[MENU_ROW, currentChapter === i && 'text-primary']"
              @click="seekTo(ch.time); menu = ''"
            >
              <span class="truncate">{{ ch.title || $t('Chapter {number}', { number: i + 1 }) }}</span>
              <span class="text-label-small opacity-50 tabular-nums">{{ fmt(ch.time) }}</span>
            </button>
            <p v-if="!chapters.length" :class="NOTE">
              {{ $t('No chapters found in this file.') }}
            </p>
          </template>

          <template v-else-if="menu === 'video'">
            <template v-if="isHdrContent || isDolbyContent">
              <div class="flex items-center gap-2 px-2.5 py-2">
                <span v-if="isHdrContent" class="rounded bg-amber-500/20 px-1.5 py-0.5 text-label-small font-bold text-amber-400">
                  HDR
                </span>
                <span v-if="isDolbyContent" class="rounded bg-sky-500/20 px-1.5 py-0.5 text-label-small font-bold text-sky-400">
                  Dolby
                </span>
                <span v-if="quality" class="truncate text-label-small opacity-50">{{ quality }}</span>
              </div>
            </template>

            <template v-if="native">
              <p :class="MENU_GROUP">
                {{ $t('HDR Tone Mapping') }}
              </p>
              <button
                v-for="p in TONE_MAPPING_PRESETS"
                :key="p.value"
                :class="[MENU_ROW, toneMapping === p.value && 'text-primary']"
                @click="applyToneMapping(p.value)"
              >
                <span>{{ p.label() }}</span>
                <v-icon v-if="toneMapping === p.value" :icon="mdiCheck" size="16" />
              </button>
            </template>
            <p v-else :class="NOTE">
              {{ $t('HDR tone mapping is only available with mpv.') }}
            </p>

            <template v-if="native">
              <p :class="MENU_GROUP">
                {{ $t('Audio Output') }}
              </p>
              <button
                v-for="p in AUDIO_CHANNEL_PRESETS"
                :key="p.value"
                :class="[MENU_ROW, audioChannel === p.value && 'text-primary']"
                @click="applyAudioChannel(p.value)"
              >
                <span class="min-w-0 flex-1">
                  <span class="block">{{ p.label() }}</span>
                  <span class="block text-label-small opacity-45">{{ p.desc() }}</span>
                </span>
                <v-icon v-if="audioChannel === p.value" :icon="mdiCheck" size="16" />
              </button>

              <p v-if="audioDeviceInfo.ao || audioDeviceInfo.channels" :class="NOTE">
                <template v-if="audioDeviceInfo.ao">
                  {{ $t('Output: {driver}', { driver: audioDeviceInfo.ao }) }}
                </template>
                <template v-if="audioDeviceInfo.ao && audioDeviceInfo.channels">
                  ·
                </template>
                <template v-if="audioDeviceInfo.channels">
                  {{ $t('Channels: {layout}', { layout: audioDeviceInfo.channels }) }}
                </template>
              </p>
            </template>
          </template>

          <template v-else>
            <button :class="[MENU_ROW, !subsOn && 'text-primary']" @click="subsOff">
              <span>{{ $t('Off') }}</span>
              <v-icon v-if="!subsOn" :icon="mdiCheck" size="16" />
            </button>

            <template v-if="embedded.length">
              <p :class="MENU_GROUP">
                {{ $t('In this file') }}
              </p>
              <button
                v-for="t in embedded"
                :key="t.id"
                :class="[MENU_ROW, sid === t.id && 'text-primary']"
                @click="useTrack(t)"
              >
                <span class="truncate">{{ trackLabel(t) }}</span>
                <v-icon v-if="sid === t.id" :icon="mdiCheck" size="16" />
              </button>
            </template>

            <!-- Shipped inside the torrent, so they need no lookup and no net. -->
            <template v-if="release.length">
              <p :class="MENU_GROUP">
                {{ $t('In this release') }}
              </p>
              <button
                v-for="r in release"
                :key="r.file.url"
                :class="[MENU_ROW, activeUrl === r.file.url && 'text-primary']"
                @click="loadFile(r.file, r.lang)"
              >
                <span class="min-w-0 flex-1">
                  <span class="block truncate">{{ r.lang.name }}</span>
                  <!-- The file's own name, since here we actually have one. -->
                  <span class="block truncate text-label-small opacity-45">{{ r.file.name }}</span>
                </span>
                <v-icon v-if="activeUrl === r.file.url" class="shrink-0" :icon="mdiCheck" size="16" />
              </button>
            </template>

            <p :class="MENU_GROUP">
              OpenSubtitles
            </p>
            <p v-if="subLoading" :class="NOTE">
              {{ $t('Searching…') }}
            </p>
            <p v-else-if="subError" class="text-error !opacity-100" :class="NOTE">
              {{ subError }}
            </p>
            <p v-else-if="unsearchable" :class="NOTE">
              {{ $t('No title or IMDb id for this stream, so nothing to search by.') }}
            </p>
            <p v-else-if="!subLanguages.length" :class="NOTE">
              {{ $t('No subtitles found.') }}
            </p>

            <!-- The name plays the best file; the chevron shows the rest of them. -->
            <template v-for="l in subLanguages" :key="l.name">
              <div class="flex items-center gap-0.5">
                <button
                  class="min-w-0 flex-1"
                  :class="[MENU_ROW, l.files.some(f => f.url === activeUrl) && 'text-primary']"
                  @click="useLanguage(l)"
                >
                  <span class="truncate">{{ l.name }}</span>
                  <v-progress-circular v-if="probing === l.name" indeterminate size="13" width="2" />
                  <span v-else class="text-label-small opacity-45">{{ l.files.length }}</span>
                </button>
                <button
                  v-if="l.files.length > 1"
                  v-tooltip:top="expanded === l.name ? $t('Hide versions') : $t('Show all versions')"
                  class="!h-8 !min-w-8" :class="ICO"
                  @click="expand(l)"
                >
                  <v-icon :icon="expanded === l.name ? mdiChevronUp : mdiChevronDown" size="16" />
                </button>
              </div>

              <!-- The listing itself says nothing about a file, so every row
                   here is read out of the downloaded cues: what it runs to, how
                   many lines it has, and whether that is this video at all. -->
              <template v-if="expanded === l.name">
                <button
                  v-for="f in variants[l.name] ?? []"
                  :key="f.url"
                  class="pl-6" :class="[MENU_ROW, activeUrl === f.url && 'text-primary']"
                  @click="loadFile(f, l)"
                >
                  <span class="min-w-0 flex-1">
                    <span class="block truncate opacity-80">{{ fileLabel(f) }}</span>
                    <span class="block truncate text-label-small opacity-45">{{ fileNote(f) }}</span>
                    <span v-if="!fits(f)" class="block truncate text-label-small text-error opacity-90">
                      {{ $t('Doesn\'t match this video\'s length') }}
                    </span>
                  </span>
                  <v-icon v-if="activeUrl === f.url" class="shrink-0" :icon="mdiCheck" size="16" />
                </button>
                <p v-if="probing === l.name" :class="NOTE">
                  {{ $t('Reading the files…') }}
                </p>
              </template>
            </template>

            <template v-if="subsOn">
              <p :class="MENU_GROUP">
                {{ $t('Text') }}
              </p>
              <button :class="MENU_ROW" @click="settings.subs.hideCaptions = !settings.subs.hideCaptions">
                <span class="flex items-center gap-2">
                  <v-icon :icon="mdiEarHearing" size="16" /> {{ $t('Hide sound descriptions') }}
                </span>
                <v-icon v-if="settings.subs.hideCaptions" :icon="mdiCheck" size="16" />
              </button>
              <p :class="NOTE">
                {{ $t('Drops “(electricity buzzing)” and “MAN:” from subtitles written for the hard of hearing.') }}
              </p>

              <p :class="MENU_GROUP">
                {{ $t('Appearance') }}
              </p>
              <div class="flex items-center justify-between px-2.5 py-1">
                <span class="text-label-large opacity-70">{{ $t('Size') }}</span>
                <div class="flex items-center gap-0.5">
                  <button class="!h-7 !min-w-7" :class="ICO" @click="nudgeSubSize(-4)">
                    <v-icon :icon="mdiMinus" size="14" />
                  </button>
                  <span class="w-10 text-center text-label-large tabular-nums">{{ settings.subs.size }}</span>
                  <button class="!h-7 !min-w-7" :class="ICO" @click="nudgeSubSize(4)">
                    <v-icon :icon="mdiPlus" size="14" />
                  </button>
                </div>
              </div>
              <div class="flex items-center justify-between px-2.5 py-1">
                <span class="text-label-large opacity-70">{{ $t('Background') }}</span>
                <div class="flex items-center gap-0.5">
                  <button class="!h-7 !min-w-7" :class="ICO" @click="nudgeSubOpacity(-0.1)">
                    <v-icon :icon="mdiMinus" size="14" />
                  </button>
                  <span class="w-10 text-center text-label-large tabular-nums">{{ Math.round(settings.subs.background * 100) }}%</span>
                  <button class="!h-7 !min-w-7" :class="ICO" @click="nudgeSubOpacity(0.1)">
                    <v-icon :icon="mdiPlus" size="14" />
                  </button>
                </div>
              </div>

              <p :class="MENU_GROUP">
                {{ $t('Timing') }}
              </p>
              <div class="flex items-center justify-between px-2.5 py-1">
                <span class="text-label-large opacity-70">{{ $t('Delay') }}</span>
                <div class="flex items-center gap-0.5">
                  <button v-tooltip:top="$t('Earlier (z)')" class="!h-7 !min-w-7" :class="ICO" @click="nudgeDelay(-0.1)">
                    <v-icon :icon="mdiMinus" size="14" />
                  </button>
                  <span class="w-16 text-center text-label-large tabular-nums">{{ delayText }}</span>
                  <button v-tooltip:top="$t('Later (Z)')" class="!h-7 !min-w-7" :class="ICO" @click="nudgeDelay(0.1)">
                    <v-icon :icon="mdiPlus" size="14" />
                  </button>
                </div>
              </div>
              <button
                class="disabled:pointer-events-none disabled:opacity-40"
                :class="MENU_ROW"
                :disabled="!syncable || syncing"
                @click="autoSync"
              >
                <span class="flex items-center gap-2">
                  <v-icon :icon="mdiAutoFix" size="16" /> {{ $t('Sync to the audio') }}
                </span>
                <v-progress-circular v-if="syncing" indeterminate size="13" width="2" />
              </button>
              <p v-if="syncing" :class="NOTE">
                {{ syncWide ? $t('Nothing certain in the last twenty minutes — listening to the whole film…') : $t('Listening to what has played…') }}
              </p>
              <template v-else-if="syncNote">
                <p :class="NOTE">
                  {{ syncNote }}
                </p>
                <button v-if="guess" :class="MENU_ROW" @click="applyFit(guess)">
                  <span class="flex items-center gap-2">
                    <v-icon :icon="mdiAutoFix" size="16" /> {{ $t('Shift by {offset} anyway', { offset: guessText }) }}
                  </span>
                </button>
              </template>
              <p v-else-if="!native" :class="NOTE">
                {{ $t('Auto-sync listens to the audio with ffmpeg, which this build can\'t run. Nudge the delay above instead.') }}
              </p>
              <p v-else-if="!syncable" :class="NOTE">
                {{ $t('Only downloaded subtitles can be synced; the file\'s own tracks already match it.') }}
              </p>
            </template>
          </template>
        </div>
      </section>
    </transition>

    <!-- Skip Intro button — appears when the playhead is inside the marked intro. -->
    <button
      v-if="introSkip && ui"
      data-cut
      class="absolute right-6 bottom-28 z-10 rounded-lg border border-white/20 bg-white/12 px-4 py-2 text-label-large backdrop-blur-sm transition-colors hover:bg-white/20"
      @click="skipIntro"
    >
      {{ $t('Skip Intro') }}
    </button>

    <transition
      :enter-active-class="SLIDE"
      :leave-active-class="SLIDE"
      enter-from-class="translate-y-[115%]"
      leave-to-class="translate-y-[115%]"
    >
      <!-- h-24 = 22 (top pad) + 16 (seek) + 8 (gap) + 38 (controls) + 12, and
           h-25.5 the same sum with the 44px touch buttons. -->
      <footer
        v-show="ui"
        data-cut
        class="absolute inset-x-0 bottom-0 border-t px-5 pb-3 pt-5.5"
        :class="[SURFACE, touch ? 'h-25.5' : 'h-24']"
        @pointerenter="hovering = true"
        @pointerleave="hovering = false"
      >
        <player-slider
          :model-value="position"
          :max="duration || 1"
          :buffered="cacheEnd"
          :format="fmt"
          :thumb="thumb"
          :approx="approx"
          :step="10"
          :chapters="chapters"
          :disabled="!started || !duration"
          @update:model-value="position = $event"
          @scrub="scrubbing = $event"
          @hover="onHover"
          @change="seekTo"
        />

        <div class="mt-2 flex items-center gap-0.5">
          <!-- The transport, where the middle of the picture is no place for it
               (see `barTransport`). Everywhere else it is the centre cluster
               above, and this row is left as the clock and the menus. -->
          <template v-if="barTransport">
            <button
              ref="playBtn"
              v-tooltip:top="paused ? $t('Play (space)') : $t('Pause (space)')"
              :class="ICO"
              :disabled="!started"
              @click="togglePlay"
            >
              <v-icon :icon="paused ? mdiPlay : mdiPause" size="26" />
            </button>
            <button v-tooltip:top="$t('Back 10s (j)')" :class="ICO" :disabled="!started" @click="seekBy(-10)">
              <v-icon :icon="mdiRewind10" size="22" />
            </button>
            <button v-tooltip:top="$t('Forward 10s (l)')" :class="ICO" :disabled="!started" @click="seekBy(10)">
              <v-icon :icon="mdiFastForward10" size="22" />
            </button>
            <button
              v-if="paused"
              v-tooltip:top="$t('Frame forward (.)')"
              :class="ICO"
              :disabled="!started"
              @click="frameStep"
            >
              <v-icon :icon="mdiStepForward" size="22" />
            </button>
          </template>

          <!-- The slider only unrolls while the group is hovered, so the bar
               stays quiet the rest of the time. Nothing hovers on a phone or a
               TV, and both have a volume rocker of their own. -->
          <div v-if="!touch" class="group/volume flex items-center">
            <button v-tooltip:top="muted ? $t('Unmute (m)') : $t('Mute (m)')" :class="ICO" :disabled="!started" @click="toggleMute">
              <v-icon :icon="volumeIcon" size="22" />
            </button>
            <player-slider
              class="w-0 flex-none overflow-hidden opacity-0 transition-[width,margin,opacity] duration-160 group-hover/volume:mx-1.5 group-hover/volume:w-19 group-hover/volume:opacity-100"
              :model-value="muted ? 0 : volume"
              :max="100"
              :disabled="!started"
              @update:model-value="setVolume"
              @scrub="volumeHeld = $event"
            />
          </div>

          <span :class="TIME">{{ fmt(position) }} <i class="not-italic opacity-45">/</i> {{ fmt(duration) }}</span>

          <div class="flex-1" />

          <span v-if="remaining" class="opacity-55" :class="TIME">{{ remaining }}</span>

          <!-- Server / Quality as labelled pills: "which one am I on" is their
               whole point, so the current pick is the button text. -->
          <button
            v-if="activeQuality"
            v-tooltip:top="$t('Quality')"
            :class="PILL"
            @click="usePill('quality')"
          >
            {{ activeQuality.label }}
            <v-icon :icon="mdiChevronDown" size="14" />
          </button>
          <button
            v-if="activeServer"
            v-tooltip:top="$t('Source')"
            class="max-w-44"
            :class="PILL"
            @click="usePill('server')"
          >
            <span class="truncate">{{ activeServer.label }}</span>
            <v-icon :icon="mdiChevronDown" size="14" class="shrink-0" />
          </button>
          <button
            v-tooltip:top="$t('Playback speed ([ / ])')"
            class="px-2" :class="[ICO, menu === 'speed' && '!text-primary !opacity-100']"
            :disabled="!started"
            @click="openMenu('speed')"
          >
            <v-icon v-if="speed === 1" :icon="mdiPlaySpeed" size="22" />
            <span v-else class="text-label-large tabular-nums">{{ speed }}×</span>
          </button>
          <button
            v-if="audioTracks.length > 1"
            v-tooltip:top="$t('Audio track')"
            :class="[ICO, menu === 'audio' && '!text-primary !opacity-100']"
            @click="openMenu('audio')"
          >
            <v-icon :icon="mdiSurroundSound" size="22" />
          </button>
          <button
            v-if="chapters.length"
            v-tooltip:top="$t('Chapters')"
            :class="[ICO, menu === 'chapter' && '!text-primary !opacity-100']"
            :disabled="!started"
            @click="openMenu('chapter')"
          >
            <v-icon :icon="mdiBookOpenPageVariant" size="22" />
          </button>
          <button
            v-tooltip:top="$t('Subtitles (s)')"
            :class="[ICO, menu === 'subs' && '!text-primary !opacity-100']"
            :disabled="!started"
            @click="openMenu('subs')"
          >
            <v-icon :icon="subsOn ? mdiSubtitles : mdiSubtitlesOutline" size="22" />
          </button>
          <button
            v-tooltip:top="sleepTimerRemaining ? $t('Sleep timer: {time}', { time: sleepTimerText(sleepTimerRemaining) }) : $t('Sleep timer')"
            :class="[ICO, menu === 'sleep' && '!text-primary !opacity-100', sleepTimerRemaining && sleepTimerRemaining > 0 && '!text-primary']"
            :disabled="!started"
            @click="openMenu('sleep')"
          >
            <v-icon :icon="mdiSleep" size="22" />
          </button>
          <button
            v-tooltip:top="$t('Video & Audio')"
            :class="[ICO, menu === 'video' && '!text-primary !opacity-100', (toneMapping !== 'auto' || audioChannel !== 'auto') && '!text-primary']"
            :disabled="!started"
            @click="openMenu('video')"
          >
            <v-icon :icon="mdiCog" size="22" />
          </button>
          <button
            v-tooltip:top="windowFullscreen ? $t('Exit fullscreen (f)') : $t('Fullscreen (f)')"
            :class="ICO"
            @click="toggleFullscreen"
          >
            <v-icon :icon="windowFullscreen ? mdiFullscreenExit : mdiFullscreen" size="22" />
          </button>
          <button
            v-tooltip:top="$t('A-B loop (i/o/I)')"
            :class="[ICO, abLoop && '!text-primary']"
            :disabled="!started"
            @click="abLoop ? clearLoop() : setLoopA()"
          >
            <v-icon :icon="mdiRepeat" size="22" />
          </button>
          <button
            v-if="!touch"
            v-tooltip:top="$t('Network stats (d)')"
            :class="[ICO, showStats && '!text-primary']"
            :disabled="!fromEngine"
            @click="showStats = !showStats"
          >
            <v-icon :icon="mdiChartTimeline" size="22" />
          </button>
        </div>
      </footer>
    </transition>
  </div>
</template>

<style>
/* ExoPlayer's picture is a SurfaceView behind the whole webview (Player.kt), so
   while a film is up every layer between it and the eye has to be see-through.
   The chain starts at <html>, well above anything this component renders, hence
   a document class rather than scoped styles — and hence `!important`, since
   what it overrides are utility classes on the elements themselves.

   `:has` is what makes it exact: the box marks itself, and every ancestor of it
   is by definition in the way. Naming them instead (#__nuxt, .v-application,
   .v-main, …) would be a guess about someone else's markup that fails as a
   black screen with working controls over it. */
html.rivulet-video,
html.rivulet-video :has([data-video-hole]),
html.rivulet-video [data-video-hole] {
  background: transparent !important;
}
</style>
