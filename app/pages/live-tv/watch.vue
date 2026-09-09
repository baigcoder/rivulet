<script setup lang="ts">
/**
 * Live TV player (IPTV Smarters Pro style).
 * Features:
 *   - Auto stream protocol fallback (.m3u8 <-> .ts)
 *   - Auto failover zap on dead channels (bounded; see `autoSkip`)
 *   - Quick Channel List side drawer with instant search
 *   - TV Remote key navigation (Up/Down/Left/Right/ChannelUp/ChannelDown)
 *   - Aspect ratio mode switcher (Contain, Cover, Fill)
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { cycleAspect } from '~/utils/aspectRatio'
import { liveResolveStream, wrapFreeStreamUrl } from '~/utils/iptv'
import { liveLocked, MAX_AUTO_SKIPS, nextPlayable } from '~/utils/livehealth'
import { friendlyPlaybackError } from '~/utils/playbackError'

definePageMeta({ layout: false })

const route = useRoute()
const router = useRouter()
const liveTv = useLiveTvStore()

/** `defineExpose` unwraps refs, so these are the values, not `{ value }`. */
const playerRef = ref<{
  togglePlay: () => void
  toggleMute: () => void
  setVolume: (v: number) => void
  paused: boolean
  volume: number
  muted: boolean
  started: boolean
  ui: boolean
  errorMsg?: string
  zapTo: () => void | Promise<void>
  goLive: () => void | Promise<void>
  behindLive?: boolean
  videoWidth: number
  videoHeight?: number
  hasAudio?: boolean
  resolutionLabel?: string
  ipc: (command: unknown[]) => Promise<unknown>
} | null>(null)

/** Not `flag` — that name is already `app/utils/flag.ts` and auto-import
 *  colliding with it crashed this page's setup, so Free TV never mounted. */
function asBool(v: boolean | { value?: boolean } | undefined): boolean {
  if (v && typeof v === 'object' && 'value' in v)
    return !!v.value
  return !!v
}

/** Ref to <live-tv-live-player-overlay> for `show()` on activity. */
const overlayRef = ref<{ show: () => void } | null>(null)

/** Reactive mirror of the player's state, polled every 500ms while mounted. */
const playerPlaying = ref(false)
const hasPicture = ref(false)
const hasAudio = ref(false)
const locked = computed(() => liveLocked(hasPicture.value, hasAudio.value))
const playerBehindLive = ref(false)
const playerVolume = ref(100)
const playerMuted = ref(false)
/**
 * Mirrored from the player rather than sensed here: on X11 and Win32 mpv's
 * window is in front of the page and swallows every mousemove, so the HUD's own
 * DOM events go quiet the moment the cursor is over the picture.
 */
const playerChrome = ref(false)

let pollHandle: ReturnType<typeof setInterval> | null = null

const resolving = ref(false)
const resolveError = ref('')
const errorMsg = ref('')
const autoSkips = ref(0)
const channelRetries = ref(0)
/**
 * A tap on Retry/Refresh is "this channel again". Without this, a spent
 * auto-skip budget marks it offline and the next failure walks away, so
 * Retry looked like Next.
 */
const holdChannel = ref(false)

function syncPlayerState() {
  const p = playerRef.value
  if (!p)
    return
  const wasPlaying = playerPlaying.value
  const started = asBool(p.started)
  const paused = asBool(p.paused)
  const videoW = typeof p.videoWidth === 'number' ? p.videoWidth : 0
  const pos = typeof p.position === 'number' ? p.position : 0

  // A started mpv is not a picture. Live HLS reports unpaused (and we used
  // to fake 1280p) long before a frame exists. Sound *is* a lock-on: the
  // decoder is up, and skipping here is what walked past a working channel.
  hasPicture.value = videoW > 0
  hasAudio.value = asBool(p.hasAudio)
  playerPlaying.value = started && (!paused || pos > 0) && locked.value
  playerBehindLive.value = asBool(p.behindLive)
  playerVolume.value = typeof p.volume === 'number' ? p.volume : 100
  playerMuted.value = asBool(p.muted)
  playerChrome.value = asBool(p.ui)

  if (locked.value) {
    autoSkips.value = 0
  }

  if (playerPlaying.value && !wasPlaying) {
    errorMsg.value = ''
    resolveError.value = ''
  }
}

interface ZapEntry {
  id: string
  name: string
  logoUrl?: string | null
  streamUrl?: string | null
  userAgent?: string | null
  referer?: string | null
}
const channelList = ref<ZapEntry[]>([])
const channelIndex = ref(-1)
const channelId = computed(() => String(route.query.id ?? ''))

// ── Channel + stream URL ──────────────────────────────────────────
const rawUrl = computed(() => {
  const v = String(route.query.url ?? '')
  if (v && v !== 'undefined' && v !== 'null')
    return v
  const fromList = channelList.value[channelIndex.value]?.streamUrl
  if (fromList && fromList !== 'undefined' && fromList !== 'null')
    return fromList
  const staged = readLivePlay()
  if (staged && staged.id === channelId.value && staged.streamUrl)
    return staged.streamUrl
  return ''
})
const sourceId = computed(() => String(route.query.sourceId ?? ''))
/**
 * The URL the player receives. Always the loopback proxy (`127.0.0.1:3031`)
 * so upstream UA, CORS and dead-host handling stay on the Rust side — handing
 * mpv a raw M3U link is what produced DNS errors like the wurl.tv failures.
 */
const streamUrl = ref('')
/** Same as `streamUrl` — kept for the webview `<video>` fallback path. */
const proxiedStreamUrl = ref('')
const userAgent = ref<string | null>(null)
const referer = ref<string | null>(null)

const channelName = computed(() => {
  const queryTitle = String(route.query.title ?? '').trim()
  if (queryTitle && queryTitle !== 'undefined' && queryTitle !== 'null')
    return queryTitle
  const staged = readLivePlay()
  if (staged?.id === channelId.value && staged.title)
    return staged.title
  return channelList.value[channelIndex.value]?.name ?? ''
})
const channelLogo = computed(() => {
  const staged = readLivePlay()
  if (staged?.id === channelId.value && staged.logo)
    return staged.logo
  return channelList.value[channelIndex.value]?.logoUrl ?? ''
})
const nowPlaying = computed(() => {
  const now = Date.now()
  return liveTv.getEpg(channelId.value).find(p => {
    const start = Date.parse(p.start)
    const stop = p.stop ? Date.parse(p.stop) : Number.POSITIVE_INFINITY
    return Number.isFinite(start) && start <= now && now < stop
  })?.title ?? ''
})

function loadChannelList() {
  const staged = readLivePlay()
  if (staged?.zapList?.length && !liveTv.zapList?.length)
    liveTv.setZapList(staged.zapList)
  if (liveTv.zapList?.length) {
    channelList.value = liveTv.zapList.filter(c => c.streamUrl)
    channelIndex.value = channelList.value.findIndex(c => c.id === channelId.value)
    return
  }
  const raw = String(route.query.list ?? '')
  if (!raw)
    return
  try {
    const parsed = JSON.parse(decodeURIComponent(raw)) as ZapEntry[]
    if (Array.isArray(parsed)) {
      channelList.value = parsed.filter(c => c.streamUrl)
      channelIndex.value = channelList.value.findIndex(c => c.id === channelId.value)
      liveTv.setZapList(channelList.value)
    }
  }
  catch {
    channelList.value = []
    channelIndex.value = -1
  }
}

const hasPrev = computed(() => channelIndex.value > 0)
const hasNext = computed(() => channelIndex.value >= 0 && channelIndex.value < channelList.value.length - 1)

/**
 * Aggregated error prop for the overlay's center modal.
 *
 *  1. resolveError  — liveResolveStream / URL minting failed (API level).
 *  2. errorMsg      — @failed after retries/skips are spent.
 *
 * The player's own catchError is not merged in: it flashed "Playback
 * Error" on the first remount retry, then again when skips ran out.
 */
const overlayError = computed(() => {
  const raw = resolveError.value || errorMsg.value
  if (!raw)
    return ''
  return friendlyPlaybackError(raw, 'live')
})

/**
 * Auto-skip is in progress. While true, a center-screen "trying the next
 * channel…" notice shows below the spinner so the viewer sees a reason for
 * the black screen instead of staring at nothing.
 */
const autoSkipping = computed<boolean>(() =>
  autoSkips.value > 0
  && autoSkips.value < MAX_AUTO_SKIPS
  && !locked.value
  && errorMsg.value === ''
  && resolveError.value === '')

/** One notice. Passed as `resolving` so MpvPlayer does not draw a second spinner. */
const waiting = computed(() =>
  !locked.value && (resolving.value || autoSkipping.value || (!!streamUrl.value && !overlayError.value)))

const statusLine = computed(() => {
  if (autoSkipping.value) {
    return $t('Channel unavailable, trying next channel ({attempt} of {total})…', {
      attempt: autoSkips.value,
      total: MAX_AUTO_SKIPS,
    })
  }
  if (channelRetries.value > 0)
    return $t('Reconnecting… attempt {attempt} of {total}', { attempt: channelRetries.value, total: 2 })
  return $t('Connecting to live stream…')
})

/**
 * The player must get the loopback proxy URL, not the raw M3U link.
 * Wrapping is a format string (`wrapFreeStreamUrl`) — handing mpv/VLC
 * the upstream is what made Play open a black screen. Neighbours are
 * wrapped into the same map so a zap does not redo the string.
 */
const resolvedById = new Map<string, { url: string, ua: string | null, referer: string | null }>()

function wrapChannel(url: string, ua?: string | null, ref?: string | null) {
  return wrapFreeStreamUrl(url, ua, ref)
}

function prefetchNeighbors() {
  const i = channelIndex.value
  for (const j of [i - 1, i + 1]) {
    const ch = channelList.value[j]
    if (!ch?.id || !ch.streamUrl || resolvedById.has(ch.id))
      continue
    resolvedById.set(ch.id, {
      url: wrapChannel(ch.streamUrl, ch.userAgent, ch.referer),
      ua: ch.userAgent ?? null,
      referer: ch.referer ?? null,
    })
  }
}

const CONNECT_MS = 8_000
let connectTimer: ReturnType<typeof setTimeout> | null = null

function clearConnectTimer() {
  if (!connectTimer)
    return
  clearTimeout(connectTimer)
  connectTimer = null
}

function armConnectTimer() {
  clearConnectTimer()
  connectTimer = setTimeout(() => {
    connectTimer = null
    if (!locked.value && !overlayError.value)
      void onPlaybackFailed()
  }, CONNECT_MS)
}

function playNow(url: string, ua?: string | null, ref?: string | null) {
  userAgent.value = ua ?? null
  referer.value = ref ?? null
  streamUrl.value = url
  proxiedStreamUrl.value = url
  hasPicture.value = false
  hasAudio.value = false
  playerPlaying.value = false
  resolving.value = false
  armConnectTimer()
  prefetchNeighbors()
}

/**
 * Open the staged (or cached) proxy URL on this tick so mpv starts without
 * waiting on live_resolve_stream. A public M3U already named the stream;
 * wrapping it is a format string.
 */
function playStagedNow(): boolean {
  loadChannelList()
  const id = channelId.value
  const cached = id ? resolvedById.get(id) : undefined
  if (cached) {
    playNow(cached.url, cached.ua, cached.referer)
    return true
  }
  const current = channelList.value[channelIndex.value]
  const staged = readLivePlay()
  const raw = rawUrl.value
  if (!raw)
    return false
  const ua = current?.userAgent ?? staged?.userAgent ?? null
  const ref = current?.referer ?? staged?.referer ?? null
  const proxied = wrapChannel(raw, ua, ref)
  if (!proxied)
    return false
  if (id)
    resolvedById.set(id, { url: proxied, ua, referer: ref })
  playNow(proxied, ua, ref)
  return true
}

async function resolveStreamUrl() {
  if (playStagedNow())
    return

  if (!rawUrl.value && !channelId.value) {
    streamUrl.value = ''
    proxiedStreamUrl.value = ''
    return
  }

  resolveError.value = ''
  errorMsg.value = ''
  resolving.value = true
  const id = channelId.value

  try {
    if (sourceId.value && id) {
      try {
        const resolved = await liveResolveStream(sourceId.value, id)
        if (resolved.streamUrl) {
          const next = {
            url: wrapChannel(resolved.streamUrl, resolved.userAgent, resolved.referer),
            ua: resolved.userAgent ?? null,
            referer: resolved.referer ?? null,
          }
          resolvedById.set(id, next)
          playNow(next.url, next.ua, next.referer)
          return
        }
      }
      catch {
        // Fall through to an explicit wrap of the M3U URL.
      }
    }

    if (!rawUrl.value) {
      resolveError.value = $t('This channel\'s stream is not available. Try another channel.')
      return
    }

    const current = channelList.value[channelIndex.value]
    const proxied = wrapChannel(rawUrl.value, current?.userAgent, current?.referer)
    if (proxied) {
      resolvedById.set(id, { url: proxied, ua: current?.userAgent ?? null, referer: current?.referer ?? null })
      playNow(proxied, current?.userAgent, current?.referer)
      return
    }
    resolveError.value = $t('This channel\'s stream is not available. Try another channel.')
  }
  catch (e) {
    resolveError.value = friendlyPlaybackError(e instanceof Error ? e.message : String(e), 'live')
  }
  finally {
    resolving.value = false
  }
}

function goBack() {
  resolveError.value = ''
  errorMsg.value = ''
  void router.replace(localePath(liveTvFrom(String(route.query.from ?? ''), '/live-tv/free')))
}

function zap(direction: 1 | -1) {
  if (channelList.value.length === 0)
    return
  const next = channelIndex.value + direction
  if (next < 0 || next >= channelList.value.length)
    return
  zapTo(next)
}

/** From Connecting / Playback Error: leave this channel, wrap if needed. */
function skipChannel() {
  const current = channelList.value[channelIndex.value]
  if (current)
    liveTv.markOffline(current.id)
  let next = nextPlayable(channelList.value, channelIndex.value, liveTv.offlineIds)
  if (next < 0)
    next = nextPlayable(channelList.value, -1, liveTv.offlineIds)
  if (next < 0 || next === channelIndex.value)
    return
  autoSkips.value++
  zapTo(next)
}

function onNext() {
  if (waiting.value || overlayError.value)
    skipChannel()
  else
    zap(1)
}

/**
 * Switch channel. The player stays mounted; `watch(src)` restarts it
 * with the new URL. The query is only identity (id/url/title) — the
 * lineup already lives on the store.
 */
function zapTo(index: number) {
  if (index < 0 || index >= channelList.value.length)
    return
  const ch = channelList.value[index]
  if (!ch?.streamUrl)
    return
  holdChannel.value = false
  liveTv.rememberChannel(ch.id)
  router.replace({
    path: localePath('/live-tv/watch'),
    query: {
      id: ch.id,
      title: ch.name,
      type: 'live',
      sourceId: sourceId.value || 'free:iptv-org',
      from: String(route.query.from ?? ''),
    },
  })
}

const attemptedFallback = ref(false)

/**
 * Bounded auto-failover — the other half of the health story in
 * `app/utils/livehealth.ts`.
 *
 * A free playlist is a list of other people's servers, so a channel that
 * will not open is ordinary rather than exceptional, and the header
 * comment above has claimed this feature since before it existed: what
 * actually happened was an error card with a "next channel" button, i.e.
 * the viewer doing the failover by hand. So a failure now marks the
 * channel offline (which dims its card in the grid too) and moves to the
 * next one that has not already failed.
 *
 * Bounded, because unbounded is worse than nothing: a whole dead category
 * would flash the player through twenty channels and land somewhere the
 * viewer never chose. After `MAX_AUTO_SKIPS` consecutive failures it
 * stops and shows the error, which is the honest answer — the list is
 * dead, not this channel. The counter resets as soon as one plays.
 */

watch(locked, up => {
  if (!up)
    return
  clearConnectTimer()
  autoSkips.value = 0
  channelRetries.value = 0
  holdChannel.value = false
  const id = channelId.value
  if (id)
    liveTv.markLive(id)
})

function autoSkip(): boolean {
  if (holdChannel.value)
    return false
  if (channelIndex.value < 0 || autoSkips.value >= MAX_AUTO_SKIPS)
    return false
  const current = channelList.value[channelIndex.value]
  if (current)
    liveTv.markOffline(current.id)
  let next = nextPlayable(channelList.value, channelIndex.value, liveTv.offlineIds)
  if (next < 0)
    next = nextPlayable(channelList.value, -1, liveTv.offlineIds)
  if (next < 0 || next === channelIndex.value)
    return false
  autoSkips.value++
  zapTo(next)
  return true
}

function onTogglePlay() {
  const p = playerRef.value
  if (!p) {
    void resolveStreamUrl()
    return
  }
  p.togglePlay()
  setTimeout(syncPlayerState, 0)
}

function onGoLive() {
  const p = playerRef.value
  if (!p?.goLive)
    return
  void p.goLive()
  setTimeout(syncPlayerState, 0)
}

function onToggleMute() {
  playerRef.value?.toggleMute()
  setTimeout(syncPlayerState, 0)
}

function onSetVolume(v: number) {
  playerRef.value?.setVolume(v)
  setTimeout(syncPlayerState, 0)
}

const isFavorite = computed(() => !!channelId.value && liveTv.isFavorite({ id: channelId.value }))
async function toggleFavorite() {
  if (channelId.value)
    await liveTv.toggleFavorite({ id: channelId.value })
}

const isFullscreen = ref(isAndroid())
const aspectRatio = ref<'contain' | 'cover' | 'fill'>('contain')
function cycleAspectRatio(): void {
  aspectRatio.value = cycleAspect(aspectRatio.value)
}
function toggleFullscreen() {
  isFullscreen.value = !isFullscreen.value
}

/** Re-show the overlay on any user activity (mouse move, key, click). */
function onActivity() {
  overlayRef.value?.show()
}

async function onPlaybackFailed() {
  if (locked.value)
    return
  clearConnectTimer()
  const current = channelList.value[channelIndex.value]

  if (channelRetries.value < 1) {
    channelRetries.value++
    const wrapped = wrapChannel(rawUrl.value, userAgent.value, referer.value)
    if (wrapped && wrapped !== streamUrl.value) {
      playNow(wrapped, userAgent.value, referer.value)
      return
    }
    // Remounting the same URL only painted another black window. Fall
    // through to the .ts/.m3u8 swap, then the next channel.
  }

  if (!attemptedFallback.value && rawUrl.value) {
    attemptedFallback.value = true
    const isM3u8 = streamUrl.value.includes('.m3u8') || /\.m3u8$/i.test(rawUrl.value)
    const targetAlt = isM3u8
      ? rawUrl.value.replace(/\.m3u8$/i, '.ts')
      : rawUrl.value.replace(/\.ts$/i, '.m3u8')

    if (targetAlt !== rawUrl.value) {
      const proxied = wrapChannel(
        targetAlt,
        userAgent.value ?? current?.userAgent,
        referer.value ?? current?.referer,
      )
      if (proxied && proxied !== streamUrl.value) {
        playNow(
          proxied,
          userAgent.value ?? current?.userAgent,
          referer.value ?? current?.referer,
        )
        return
      }
    }
  }

  // The protocol fallback is per channel and has now been spent, so the
  // next thing to try is a different channel.
  if (autoSkip())
    return

  if (current)
    liveTv.markOffline(current.id)

  clearConnectTimer()
  errorMsg.value = $t('This channel stopped responding. It may be off the air, or the provider may be busy.')
}

async function onRetry() {
  holdChannel.value = true
  autoSkips.value = 0
  errorMsg.value = ''
  resolveError.value = ''
  attemptedFallback.value = false
  channelRetries.value = 0
  const current = channelList.value[channelIndex.value]
  if (current)
    liveTv.markLive(current.id)
  const id = channelId.value
  if (id)
    resolvedById.delete(id)
  const prev = streamUrl.value
  await resolveStreamUrl()
  // Same URL does not fire `watch(src)`, so the player has to be kicked.
  if (streamUrl.value && streamUrl.value === prev)
    await playerRef.value?.zapTo()
}

async function onRefresh() {
  holdChannel.value = true
  autoSkips.value = 0
  errorMsg.value = ''
  resolveError.value = ''
  attemptedFallback.value = false
  channelRetries.value = 0
  const current = channelList.value[channelIndex.value]
  if (current)
    liveTv.markLive(current.id)
  const id = channelId.value
  if (id)
    resolvedById.delete(id)
  await resolveStreamUrl()
  if (streamUrl.value)
    await playerRef.value?.zapTo()
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' || e.key === 'Backspace' || e.key === 'GoBack') {
    e.preventDefault()
    goBack()
    return
  }
  // Connecting / Playback Error own left/right/up/down so the card can
  // be walked. Stealing those for a zap is why Retry looked like Next.
  if (waiting.value || overlayError.value) {
    if (e.key === 'ChannelUp' && hasNext.value) {
      e.preventDefault()
      onNext()
    }
    else if (e.key === 'ChannelDown' && hasPrev.value) {
      e.preventDefault()
      zap(-1)
    }
    return
  }
  if ((e.key === 'ArrowRight' || e.key === 'ArrowDown' || e.key === 'PageDown' || e.key === 'ChannelUp') && hasNext.value) {
    e.preventDefault()
    zap(1)
  }
  else if ((e.key === 'ArrowLeft' || e.key === 'ArrowUp' || e.key === 'PageUp' || e.key === 'ChannelDown') && hasPrev.value) {
    e.preventDefault()
    zap(-1)
  }
}

onMounted(() => {
  loadChannelList()
  playStagedNow()
  // The star has to know what was already starred, and this page can be entered
  // by a reload straight onto its URL.
  void liveTv.loadFavorites()
  window.addEventListener('keydown', onKey)
  window.addEventListener('mousemove', onActivity)
  window.addEventListener('click', onActivity)
  window.addEventListener('touchstart', onActivity, { passive: true })
  window.addEventListener('pointermove', onActivity, { passive: true })
  // The player is already a full-viewport page. Hide Android's status and
  // navigation bars now, not only after the stream URL exists — the webview
  // has no Fullscreen API, so this has to go through MainActivity.
  if (isAndroid())
    setAndroidPlayerMode(true)
  pollHandle = setInterval(syncPlayerState, 200)
  if (!streamUrl.value)
    void resolveStreamUrl()
  if (channelId.value)
    void liveTv.loadEpg(channelId.value)
})

watch(() => route.query.id, (id, prev) => {
  if (!id || id === prev)
    return
  attemptedFallback.value = false
  channelRetries.value = 0
  void liveTv.loadEpg(String(id))
  if (!playStagedNow())
    void resolveStreamUrl()
})

onUnmounted(() => {
  clearConnectTimer()
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('mousemove', onActivity)
  window.removeEventListener('click', onActivity)
  window.removeEventListener('touchstart', onActivity)
  window.removeEventListener('pointermove', onActivity)
  if (pollHandle)
    clearInterval(pollHandle)
  if (isAndroid())
    setAndroidPlayerMode(false)
})
</script>

<template>
  <div class="relative h-screen w-screen overflow-hidden bg-black">
    <!-- Player surface (full-screen). The mode="live" prop hides the VOD
         chrome (seek bar, rewind, chapters, end-of-playback, "next
         episode") and forwards the per-stream UA/Referer to native mpv.
         A plain `relative` parent (not `flex items-center justify-center`)
         is the only layout that gives `<mpv-player>`'s `h-full w-full`
         box a real size on first mount: flex centering collapses the
         child to its intrinsic height, which is 0 before the player
         reports its box, and `waitForBox()` then refuses to start mpv. -->
    <div
      class="absolute inset-0"
      :class="{
        '[&_video]:!object-cover': aspectRatio === 'cover',
        '[&_video]:!object-fill': aspectRatio === 'fill',
      }"
    >
      <mpv-player
        v-if="streamUrl"
        ref="playerRef"
        :src="streamUrl"
        :title="channelName"
        mode="live"
        :aspect="aspectRatio"
        :fullscreen="isFullscreen"
        :resolving="waiting"
        :user-agent="userAgent"
        :referer="referer"
        @failed="onPlaybackFailed"
      />
    </div>

    <!-- World-Class Live TV Player HUD Overlay -->
    <live-tv-live-player-overlay
      ref="overlayRef"
      class="!z-40"
      :playing="playerPlaying"
      :behind-live="playerBehindLive"
      :volume="playerVolume"
      :muted="playerMuted"
      :has-prev="hasPrev"
      :has-next="hasNext"
      :busy="waiting"
      :busy-text="statusLine"
      :channel-name="channelName"
      :now-playing="nowPlaying"
      :channel-logo="channelLogo"
      :channel-index="channelIndex >= 0 ? channelIndex : 0"
      :channel-total="channelList.length"
      :channel-list="channelList"
      :offline-ids="liveTv.offlineIds"
      :is-favorite="isFavorite"
      :chrome-up="playerChrome"
      :is-fullscreen="isFullscreen"
      :error="overlayError"
      :resolution-label="typeof playerRef?.resolutionLabel === 'string' ? playerRef.resolutionLabel : ''"
      :aspect-ratio="aspectRatio"
      @back="goBack"
      @prev="zap(-1)"
      @next="onNext"
      @zap-to="zapTo"
      @retry="() => void onRetry()"
      @refresh="() => void onRefresh()"
      @toggle-play="onTogglePlay"
      @go-live="onGoLive"
      @toggle-mute="onToggleMute"
      @set-volume="onSetVolume"
      @toggle-favorite="toggleFavorite"
      @toggle-fullscreen="toggleFullscreen"
      @cycle-aspect-ratio="cycleAspectRatio"
    />
  </div>
</template>
