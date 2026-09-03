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
import { applyAspect, cycleAspect } from '~/utils/aspectRatio'
import { iptvProxyHealth, liveResolveStream, proxyFreeStreamUrl } from '~/utils/iptv'
import { MAX_AUTO_SKIPS, nextPlayable } from '~/utils/livehealth'
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
  catchError?: string
  errorMsg?: string
  zapTo: () => void | Promise<void>
  goLive: () => void | Promise<void>
  behindLive?: boolean
  ipc: (command: unknown[]) => Promise<unknown>
} | null>(null)

/** Not `flag` — that name is already `app/utils/flag.ts` and auto-import
 *  colliding with it crashed this page's setup, so Free TV never mounted. */
function asBool(v: boolean | { value?: boolean } | undefined): boolean {
  if (v && typeof v === 'object' && 'value' in v)
    return !!v.value
  return !!v
}

function asText(v: string | { value?: string } | undefined): string {
  if (typeof v === 'string')
    return v
  if (v && typeof v === 'object' && 'value' in v)
    return v.value ?? ''
  return ''
}

/** Ref to <live-tv-live-player-overlay> for `show()` on activity. */
const overlayRef = ref<{ show: () => void } | null>(null)

/** Reactive mirror of the player's state, polled every 500ms while mounted. */
const playerPlaying = ref(false)
const playerBehindLive = ref(false)
const playerVolume = ref(100)
const playerMuted = ref(false)
/**
 * Mirrored from the player rather than sensed here: on X11 and Win32 mpv's
 * window is in front of the page and swallows every mousemove, so the HUD's own
 * DOM events go quiet the moment the cursor is over the picture.
 */
const playerChrome = ref(false)
/** Live mirror of MpvPlayer's own catchError — NOT merged into the explicit
 *  error refs, because catchError can be transient (a brief open-failure that
 *  mpv itself recovers from). Aggregated in overlayError below. */
const playerCatchError = ref('')

let pollHandle: ReturnType<typeof setInterval> | null = null

const statusLine = ref('')
const resolving = ref(false)
const resolveError = ref('')
const errorMsg = ref('')

function syncPlayerState() {
  const p = playerRef.value
  if (!p)
    return
  const wasPlaying = playerPlaying.value
  playerPlaying.value = asBool(p.started) && !asBool(p.paused)
  playerBehindLive.value = asBool(p.behindLive)
  playerVolume.value = typeof p.volume === 'number' ? p.volume : 100
  playerMuted.value = asBool(p.muted)
  playerChrome.value = asBool(p.ui)
  playerCatchError.value = asText(p.errorMsg ?? p.catchError)

  // If the stream just crossed from not-playing to actually playing, clear
  // any stale errors from the previous attempt. Without this an auto-skip
  // loop's last dead-channel error would linger over a perfectly good picture
  // until the user clicked something.
  if (playerPlaying.value && !wasPlaying) {
    errorMsg.value = ''
    resolveError.value = ''
    playerCatchError.value = ''
    statusLine.value = ''
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
const channelTotal = computed(() => channelList.value.length)

const channelName = computed(() => {
  const queryTitle = String(route.query.title ?? '').trim()
  if (queryTitle && queryTitle !== 'undefined' && queryTitle !== 'null')
    return queryTitle
  return channelList.value[channelIndex.value]?.name ?? ''
})
const channelLogo = computed(() => String(route.query.logo ?? ''))
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
 *  2. errorMsg      — @failed on the <mpv-player>, or the MAX_AUTO_SKIPS
 *                     final failure message.
 *  3. playerCatchError — transient but *current* catchError from the
 *                     player's own ref. Only surfaced while the stream has
 *                     not actually started playing — so a brief open-error
 *                     that mpv itself recovers from (and flips started=true)
 *                     vanishes of its own accord instead of getting stuck.
 */
const overlayError = computed(() => {
  const raw = resolveError.value
    || errorMsg.value
    || (!playerPlaying.value ? playerCatchError.value : '')
  if (!raw)
    return ''
  return friendlyPlaybackError(raw)
})

const autoSkips = ref(0)
/**
 * Auto-skip is in progress. While true, a center-screen "trying the next
 * channel…" notice shows below the spinner so the viewer sees a reason for
 * the black screen instead of staring at nothing.
 */
const autoSkipping = computed<boolean>(() =>
  autoSkips.value > 0
  && autoSkips.value < MAX_AUTO_SKIPS
  && errorMsg.value === ''
  && resolveError.value === '')

/**
 * The player must get the loopback proxy URL, not the raw M3U link.
 * `liveResolveStream` wraps the channel through 127.0.0.1:3031 and
 * rewrites a `.ts` into `.m3u8` — handing mpv/VLC the upstream is what
 * made Play open a black screen. Results are cached so a zap does not
 * wait on the same IPC again.
 */
const resolvedById = new Map<string, { url: string, ua: string | null, referer: string | null }>()

function playNow(url: string, ua?: string | null, ref?: string | null) {
  userAgent.value = ua ?? null
  referer.value = ref ?? null
  streamUrl.value = url
  proxiedStreamUrl.value = url
  resolving.value = false
}

async function resolveStreamUrl() {
  if (!rawUrl.value && !channelId.value) {
    streamUrl.value = ''
    proxiedStreamUrl.value = ''
    return
  }

  const id = channelId.value
  const cached = id ? resolvedById.get(id) : undefined
  if (cached) {
    playNow(cached.url, cached.ua, cached.referer)
    return
  }

  resolveError.value = ''
  errorMsg.value = ''
  resolving.value = true
  const current = channelList.value[channelIndex.value]
  const channelUa = current?.userAgent ?? null
  const channelReferer = current?.referer ?? null

  try {
    if (sourceId.value && id) {
      try {
        const resolved = await liveResolveStream(sourceId.value, id)
        if (resolved.streamUrl) {
          const next = {
            url: resolved.streamUrl,
            ua: resolved.userAgent ?? null,
            referer: resolved.referer ?? null,
          }
          resolvedById.set(id, next)
          playNow(next.url, next.ua, next.referer)
          return
        }
      }
      catch {
        // Fall through to an explicit proxy of the M3U URL.
      }
    }

    if (!rawUrl.value) {
      resolveError.value = $t('This channel\'s stream is not available. Try another channel.')
      return
    }

    try {
      const proxied = await proxyFreeStreamUrl(rawUrl.value, channelUa ?? undefined, channelReferer ?? undefined)
      if (proxied) {
        if (id)
          resolvedById.set(id, { url: proxied, ua: channelUa, referer: channelReferer })
        playNow(proxied, channelUa, channelReferer)
        return
      }
    }
    catch {
      // Browser / proxy-down fallback below.
    }

    const healthy = await iptvProxyHealth().catch(() => false)
    if (!healthy) {
      await new Promise(r => setTimeout(r, 400))
      await iptvProxyHealth().catch(() => false)
    }
    const proxied = await proxyFreeStreamUrl(rawUrl.value, channelUa ?? undefined, channelReferer ?? undefined).catch(() => '')
    if (proxied) {
      if (id)
        resolvedById.set(id, { url: proxied, ua: channelUa, referer: channelReferer })
      playNow(proxied, channelUa, channelReferer)
      return
    }
    resolveError.value = $t('This channel\'s stream is not available. Try another channel.')
  }
  catch (e) {
    resolveError.value = friendlyPlaybackError(e instanceof Error ? e.message : String(e))
  }
  finally {
    resolving.value = false
  }
}

function goBack() {
  resolveError.value = ''
  errorMsg.value = ''
  playerCatchError.value = ''
  statusLine.value = ''
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
  liveTv.rememberChannel(ch.id)
  router.replace({
    path: localePath('/live-tv/watch'),
    query: {
      id: ch.id,
      title: ch.name,
      logo: ch.logoUrl ?? '',
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

watch(playerPlaying, playing => {
  if (playing)
    autoSkips.value = 0
})

function autoSkip(): boolean {
  if (channelIndex.value < 0 || autoSkips.value >= MAX_AUTO_SKIPS)
    return false
  const current = channelList.value[channelIndex.value]
  if (current)
    liveTv.markOffline(current.id)
  const next = nextPlayable(channelList.value, channelIndex.value, liveTv.offlineIds)
  if (next < 0)
    return false
  autoSkips.value++
  statusLine.value = $t('Channel unavailable — trying the next one…')
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

const isFullscreen = ref(false)
const aspectRatio = ref<'contain' | 'cover' | 'fill'>('contain')
function cycleAspectRatio(): void {
  aspectRatio.value = cycleAspect(aspectRatio.value)
  applyAspect(playerRef.value, aspectRatio.value)
}
watch(aspectRatio, mode => applyAspect(playerRef.value, mode))
watch(playerRef, player => applyAspect(player, aspectRatio.value))
function toggleFullscreen() {
  isFullscreen.value = !isFullscreen.value
  if (isFullscreen.value) {
    document.documentElement.requestFullscreen?.()
  }
  else {
    document.exitFullscreen?.()
  }
}

/** Re-show the overlay on any user activity (mouse move, key, click). */
function onActivity() {
  overlayRef.value?.show()
}

async function onPlaybackFailed() {
  if (!attemptedFallback.value && rawUrl.value) {
    attemptedFallback.value = true
    if (streamUrl.value.includes('.m3u8') || /\.m3u8$/i.test(rawUrl.value)) {
      const tsUrl = rawUrl.value.replace(/\.m3u8$/i, '.ts')
      if (tsUrl !== rawUrl.value) {
        const current = channelList.value[channelIndex.value]
        try {
          const proxied = await proxyFreeStreamUrl(
            tsUrl,
            userAgent.value ?? current?.userAgent ?? undefined,
            referer.value ?? current?.referer ?? undefined,
          )
          if (proxied) {
            streamUrl.value = proxied
            proxiedStreamUrl.value = proxied
            return
          }
        }
        catch { /* auto-skip below */ }
      }
    }
  }

  // The protocol fallback is per channel and has now been spent, so the
  // next thing to try is a different channel.
  if (autoSkip())
    return

  errorMsg.value = $t('Stream playback failed. The channel may be offline or temporarily unavailable.')
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' || e.key === 'Backspace' || e.key === 'GoBack') {
    e.preventDefault()
    goBack()
  }
  else if ((e.key === 'ArrowRight' || e.key === 'ArrowDown' || e.key === 'PageDown' || e.key === 'ChannelUp') && hasNext.value) {
    e.preventDefault()
    zap(1)
  }
  else if ((e.key === 'ArrowLeft' || e.key === 'ArrowUp' || e.key === 'PageUp' || e.key === 'ChannelDown') && hasPrev.value) {
    e.preventDefault()
    zap(-1)
  }
}

onMounted(async () => {
  loadChannelList()
  // The star has to know what was already starred, and this page can be entered
  // by a reload straight onto its URL.
  void liveTv.loadFavorites()
  window.addEventListener('keydown', onKey)
  window.addEventListener('mousemove', onActivity)
  window.addEventListener('click', onActivity)
  window.addEventListener('touchstart', onActivity, { passive: true })
  window.addEventListener('pointermove', onActivity, { passive: true })
  await resolveStreamUrl()
  // Poll the player state every 500ms so the overlay's playing/volume
  // bound values stay current. mpv's IPC properties are already polled
  // inside the player, so this is just mirroring into the overlay's
  // local reactive state.
  if (channelId.value)
    void liveTv.loadEpg(channelId.value)
  pollHandle = setInterval(syncPlayerState, 250)
})

watch(() => route.query.id, (id, prev) => {
  if (!id || id === prev)
    return
  loadChannelList()
  // Per channel, not per page: without this the first channel to fail
  // spent the one `.m3u8` → `.ts` retry for every channel after it.
  attemptedFallback.value = false
  void liveTv.loadEpg(String(id))
  void resolveStreamUrl()
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('mousemove', onActivity)
  window.removeEventListener('click', onActivity)
  window.removeEventListener('touchstart', onActivity)
  window.removeEventListener('pointermove', onActivity)
  if (pollHandle)
    clearInterval(pollHandle)
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
        :status="statusLine"
        :title="channelName"
        mode="live"
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
      :busy="resolving"
      :channel-name="channelName"
      :now-playing="nowPlaying"
      :channel-logo="channelLogo"
      :channel-index="channelIndex >= 0 ? channelIndex : 0"
      :channel-total="channelList.length"
      :channel-list="channelList"
      :is-favorite="isFavorite"
      :chrome-up="playerChrome"
      :is-fullscreen="isFullscreen"
      :error="overlayError"
      :aspect-ratio="aspectRatio"
      @back="goBack"
      @prev="zap(-1)"
      @next="zap(1)"
      @zap-to="zapTo"
      @retry="resolveStreamUrl"
      @toggle-play="onTogglePlay"
      @go-live="onGoLive"
      @toggle-mute="onToggleMute"
      @set-volume="onSetVolume"
      @toggle-favorite="toggleFavorite"
      @toggle-fullscreen="toggleFullscreen"
      @cycle-aspect-ratio="cycleAspectRatio"
    />

    <!-- Resolving spinner + auto-skip notice — sits above overlay so it
         beats the HUD chrome to the eye and is visible even in live mode
         (where <mpv-player> hides its own status bar). -->
    <transition
      enter-active-class="transition ease-out duration-150"
      enter-from-class="opacity-0 scale-95"
      enter-to-class="opacity-100 scale-100"
      leave-active-class="transition ease-in duration-100"
      leave-from-class="opacity-100 scale-100"
      leave-to-class="opacity-0 scale-95"
    >
      <div
        v-if="resolving || autoSkipping"
        class="pointer-events-none absolute inset-0 !z-50 grid size-full place-items-center text-white"
      >
        <div class="flex flex-col items-center gap-3 max-w-md px-4 text-center">
          <v-progress-circular indeterminate color="primary" size="40" width="3" />
          <p class="text-body-medium font-medium opacity-90">
            {{ autoSkipping
              ? $t('Channel unavailable — trying the next one…')
              : $t('Connecting to live stream…') }}
          </p>
          <p v-if="autoSkipping && channelTotal > 0" class="text-label-small opacity-60 tabular-nums">
            {{ $t('Attempt {current} of {total}', { current: autoSkips + 1, total: MAX_AUTO_SKIPS }) }}
          </p>
        </div>
      </div>
    </transition>
  </div>
</template>
