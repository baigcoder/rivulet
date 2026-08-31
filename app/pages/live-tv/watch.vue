<script lang="ts" setup>
/**
 * Live TV player (IPTV Smarters Pro style).
 * Features:
 *   - Auto stream protocol fallback (.m3u8 <-> .ts)
 *   - Auto failover zap on dead channels (bounded; see `autoSkip`)
 *   - Quick Channel List side drawer with instant search
 *   - TV Remote key navigation (Up/Down/Left/Right/ChannelUp/ChannelDown)
 *   - Aspect ratio mode switcher (Contain, Cover, Fill)
 */
import { iptvProxyHealth, liveResolveStream, proxyFreeStreamUrl } from '~/utils/iptv'
import { MAX_AUTO_SKIPS, nextPlayable } from '~/utils/livehealth'

definePageMeta({ layout: false })

const route = useRoute()
const router = useRouter()

/** Ref to the <mpv-player> so the overlay can call its methods. */
const playerRef = ref<{
  togglePlay: () => void
  toggleMute: () => void
  setVolume: (v: number) => void
  paused: { value: boolean }
  volume: { value: number }
  muted: { value: boolean }
  started: { value: boolean }
  /** The player's own chrome flag — fed by the native pointer poll on X11/Win32. */
  ui: { value: boolean }
  /** Current native-level error (play() failures, IPC errors) — lives on the mpv component, cleared when playback starts or a new src is loaded. */
  catchError?: { value: string }
  zapTo: () => void | Promise<void>
} | null>(null)

/** Ref to <live-tv-live-player-overlay> for `show()` on activity. */
const overlayRef = ref<{ show: () => void } | null>(null)

/** Reactive mirror of the player's state, polled every 500ms while mounted. */
const playerPlaying = ref(false)
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

function syncPlayerState() {
  const p = playerRef.value
  if (!p)
    return
  const wasPlaying = playerPlaying.value
  playerPlaying.value = !p.paused?.value && p.started?.value
  playerVolume.value = p.volume?.value ?? 100
  playerMuted.value = p.muted?.value ?? false
  playerChrome.value = p.ui?.value === true
  playerCatchError.value = p.catchError?.value ?? ''

  // If the stream just crossed from not-playing to actually playing, clear
  // any stale errors from the previous attempt. Without this an auto-skip
  // loop's last dead-channel error would linger over a perfectly good picture
  // until the user clicked something.
  if (playerPlaying.value && !wasPlaying) {
    errorMsg.value = ''
    resolveError.value = ''
    playerCatchError.value = ''
    statusLine.value = ''
    if ((p.catchError as any)?.value != null)
      p.catchError.value = ''
  }
}

// ── Channel + stream URL ──────────────────────────────────────────
const rawUrl = computed(() => {
  const v = String(route.query.url ?? '')
  if (!v || v === 'undefined' || v === 'null')
    return ''
  return v
})
const sourceId = computed(() => String(route.query.sourceId ?? ''))
/**
 * The URL the player receives. Phase 1 puts the raw upstream URL
 * here so native mpv (Linux/Windows/macOS) plays it directly with
 * ffmpeg decoders. The webview fallback (`proxiedStreamUrl`) is kept
 * for browser dev and the rare stream mpv refuses to open.
 */
const streamUrl = ref('')
/**
 * The proxy-wrapped URL — only used by the webview `<video>` path
 * when native mpv isn't available.
 */
const proxiedStreamUrl = ref('')
const userAgent = ref<string | null>(null)
const referer = ref<string | null>(null)
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

const channelName = computed(() => {
  const queryTitle = String(route.query.title ?? '').trim()
  if (queryTitle && queryTitle !== 'undefined' && queryTitle !== 'null')
    return queryTitle
  return channelList.value[channelIndex.value]?.name ?? ''
})
const channelLogo = computed(() => String(route.query.logo ?? ''))
const channelId = computed(() => String(route.query.id ?? ''))
const statusLine = ref('')

const resolving = ref(false)
const resolveError = ref('')

function loadChannelList() {
  const raw = String(route.query.list ?? '')
  if (!raw)
    return
  try {
    const parsed = JSON.parse(decodeURIComponent(raw)) as ZapEntry[]
    if (Array.isArray(parsed)) {
      channelList.value = parsed.filter(c => c.streamUrl)
      channelIndex.value = channelList.value.findIndex(c => c.id === channelId.value)
    }
  }
  catch {
    channelList.value = []
    channelIndex.value = -1
  }
}

const hasPrev = computed(() => channelIndex.value > 0)
const hasNext = computed(() => channelIndex.value >= 0 && channelIndex.value < channelList.value.length - 1)

const errorMsg = ref('')

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
const overlayError = computed(() =>
  resolveError.value
  || errorMsg.value
  || (!playerPlaying.value ? playerCatchError.value : ''))

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

async function resolveStreamUrl() {
  if (!rawUrl.value) {
    streamUrl.value = ''
    proxiedStreamUrl.value = ''
    return
  }

  resolving.value = true
  resolveError.value = ''
  errorMsg.value = ''
  streamUrl.value = ''
  proxiedStreamUrl.value = ''

  try {
    if (sourceId.value && channelId.value) {
      try {
        const resolved = await liveResolveStream(sourceId.value, channelId.value)
        if (resolved.streamUrl) {
          streamUrl.value = resolved.streamUrl
          proxiedStreamUrl.value = resolved.streamUrl
          userAgent.value = resolved.userAgent ?? null
          referer.value = resolved.referer ?? null
          return
        }
      }
      catch {
        // Fall through to explicit proxy path
      }
    }

    const current = channelList.value[channelIndex.value]
    const channelUa = current?.userAgent || undefined
    const channelReferer = current?.referer || undefined
    userAgent.value = channelUa ?? null
    referer.value = channelReferer ?? null

    try {
      const proxied = await proxyFreeStreamUrl(rawUrl.value, channelUa, channelReferer)
      streamUrl.value = rawUrl.value
      proxiedStreamUrl.value = proxied
      return
    }
    catch {
      // Browser dev mode fallback
    }

    const healthy = await iptvProxyHealth().catch(() => false)
    if (!healthy) {
      await new Promise(r => setTimeout(r, 400))
      const retry = await iptvProxyHealth().catch(() => false)
      if (!retry) {
        streamUrl.value = rawUrl.value
        proxiedStreamUrl.value = ''
        return
      }
    }
    streamUrl.value = rawUrl.value
    proxiedStreamUrl.value = await proxyFreeStreamUrl(rawUrl.value, channelUa, channelReferer).catch(() => '')
  }
  catch (e) {
    resolveError.value = String(e)
    if (rawUrl.value) {
      streamUrl.value = rawUrl.value
      proxiedStreamUrl.value = ''
    }
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
  const from = String(route.query.from ?? '')
  if (from) {
    router.replace(from)
    return
  }
  if (window.history.length > 1)
    router.back()
  else
    router.replace('/live-tv')
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
 * Switch the player to a new channel. Phase 3 calls the player's
 * own `zapTo()` so the swap is in-process — no full route
 * navigation, no component remount, no fresh layout. The previous
 * version navigated to `/live-tv/watch?url=…` which remounted the
 * `<mpv-player>` and made channel-up feel sluggish.
 */
function zapTo(index: number) {
  if (index < 0 || index >= channelList.value.length)
    return
  const ch = channelList.value[index]
  if (!ch?.streamUrl)
    return
  // Push the route query. `resolveStreamUrl` is wired as a watcher
  // on `route.query.url` via the `onMounted` hook, so the channel
  // swap goes through the same resolution pipeline as a fresh
  // navigation. The player's `:key="streamUrl"` triggers a remount
  // when the URL changes; the new mpv child window gets the
  // updated UA/Referer and the channel name from the new query.
  router.replace({
    path: '/live-tv/watch',
    query: {
      url: ch.streamUrl,
      title: ch.name,
      logo: ch.logoUrl ?? '',
      id: ch.id,
      type: 'live',
      sourceId: sourceId.value,
      list: encodeURIComponent(JSON.stringify(channelList.value)),
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
const liveTv = useLiveTvStore()
const autoSkips = ref(0)

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
  playerRef.value?.togglePlay()
  // Read the new state immediately so the overlay reflects the click
  // without waiting for the next 500ms poll.
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

function onPlaybackFailed() {
  if (!attemptedFallback.value && rawUrl.value) {
    attemptedFallback.value = true
    if (streamUrl.value.includes('.m3u8')) {
      const fallbackUrl = rawUrl.value.replace(/\.m3u8$/i, '.ts')
      if (fallbackUrl !== rawUrl.value) {
        streamUrl.value = fallbackUrl
        return
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
  pollHandle = setInterval(syncPlayerState, 500)
})

/**
 * Channel zaps push a new `?url=...` via `router.replace` instead of
 * navigating, so the page itself doesn't unmount. The route query
 * change is what triggers a fresh `liveResolveStream` for the new
 * channel and the player's `:key="streamUrl"` remounts mpv with
 * the new URL. Without this watcher, the channel-up key would land
 * on the same stream.
 */
watch(() => route.query.url, () => {
  loadChannelList()
  // Per channel, not per page: without this the first channel to fail
  // spent the one `.m3u8` → `.ts` retry for every channel after it.
  attemptedFallback.value = false
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
    <div class="absolute inset-0">
      <mpv-player
        v-if="streamUrl"
        :key="streamUrl"
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
      :volume="playerVolume"
      :muted="playerMuted"
      :has-prev="hasPrev"
      :has-next="hasNext"
      :busy="resolving"
      :channel-name="channelName"
      :channel-logo="channelLogo"
      :channel-index="channelIndex >= 0 ? channelIndex : 0"
      :channel-total="channelList.length"
      :channel-list="channelList"
      :is-favorite="isFavorite"
      :chrome-up="playerChrome"
      :is-fullscreen="isFullscreen"
      :error="overlayError"
      @back="goBack"
      @prev="zap(-1)"
      @next="zap(1)"
      @zap-to="zapTo"
      @retry="resolveStreamUrl"
      @toggle-play="onTogglePlay"
      @toggle-mute="onToggleMute"
      @set-volume="onSetVolume"
      @toggle-favorite="toggleFavorite"
      @toggle-fullscreen="toggleFullscreen"
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
