<script setup lang="ts">
import type { ZapChannel } from '~/stores/premiumTv'
/**
 * Premium TV player page.
 *
 * The shape is `live-tv/watch.vue`'s, because that one is proven on a TV:
 * one `<mpv-player mode="live">` filling an `absolute inset-0` box, the
 * auto-hiding `<live-tv-live-player-overlay>` over it, and a 500ms poll
 * mirroring mpv's transport state into the overlay's props. What is
 * different is everything about the *source*.
 *
 * A premium channel has no URL. `usePlaybackSource` asks the Rust side for
 * a signed redirector token good for about thirty seconds; the page plays
 * `http://127.0.0.1:3032/premium-stream/<token>` and never sees the
 * provider's host, path or credentials. That expiry is why a reconnect
 * re-mints rather than retrying the URL it has: the old token is not stale
 * data, it is expired authorization, and replaying it would 401.
 *
 * Reconnect is bounded and lives in the store (`nextReconnect`): four
 * attempts at 2s, 4s, 8s, 16s, then the next playable channel (same
 * bound as Free TV's auto-skip). One clear error only when that walk
 * is spent too. The eight-state machine in the store is what this page
 * drives — `setPlayer` on every transition, and nothing here keeps a
 * second copy of it.
 */
import type { EpgProgram, IPTVChannel } from '~/types/premium'
import { mdiCheck, mdiClose } from '@mdi/js'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { usePlaybackSource } from '~/composables/usePlaybackSource'
import { MAX_RECONNECT_ATTEMPTS } from '~/stores/premiumTv'
import { cycleAspect } from '~/utils/aspectRatio'
import { liveLocked, MAX_AUTO_SKIPS, nextPlayable } from '~/utils/livehealth'
import { readPremiumPlay } from '~/utils/liveNav'
import { friendlyPlaybackError } from '~/utils/playbackError'
import { premiumApi } from '~/utils/premiumTv'

definePageMeta({ layout: false })

/** How often mpv's transport state is mirrored for the overlay. */
const POLL_MS = 250

const route = useRoute()
const router = useRouter()
const premium = usePremiumTvStore()
const playback = usePlaybackSource()

const playKind = computed(() => {
  const k = String(route.query.kind ?? 'live')
  if (k === 'movie' || k === 'episode')
    return k
  return 'live'
})

const playId = computed(() => String(route.query.id ?? ''))
const playExt = computed(() => String(route.query.ext ?? 'mkv'))
const playTitle = computed(() => String(route.query.title ?? ''))

const isVod = computed(() => playKind.value === 'movie' || playKind.value === 'episode')
const playerMode = computed(() => isVod.value ? 'vod' : 'live')

const channelId = computed(() => isVod.value ? '' : String(route.query.id ?? ''))

/**
 * The channel being watched. Usually already in the store's page, but a
 * direct load or a reload has an empty list, so it is fetched by id — the
 * name in the top bar should not depend on how the page was reached.
 */
const channel = shallowRef<IPTVChannel | null>(null)

const playerRef = ref<{
  togglePlay: () => void
  toggleMute: () => void
  setVolume: (v: number) => void
  paused: boolean
  volume: number
  muted: boolean
  started: boolean
  buffering: boolean
  ui: boolean
  catchError?: string
  errorMsg?: string
  position?: number
  duration?: number
  videoWidth: number
  videoHeight: number
  hasAudio?: boolean
  resolutionLabel: string
  ipc: (command: unknown[]) => Promise<unknown>
  goLive: () => void | Promise<void>
  behindLive?: boolean
} | null>(null)

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
const overlayRef = ref<{ show: () => void, hide: () => void, visible: boolean } | null>(null)

const playerPlaying = ref(false)
const playerBehindLive = ref(false)
const playerVolume = ref(100)
const playerMuted = ref(false)
const playerCatchError = ref('')
const playerPosition = ref(0)
const playerDuration = ref(0)
/**
 * Mirrored from the player rather than sensed here: on X11 and Win32 mpv's
 * window is in front of the page and swallows every mousemove, so the HUD's
 * own DOM events go quiet the moment the cursor is over the picture.
 */
const playerChrome = ref(false)

const aspectRatio = ref<'contain' | 'cover' | 'fill'>('contain')
const guideLoading = ref(false)

// ── Quality variants ──────────────────────────────────────────────
const qualityVariants = ref<IPTVChannel[]>([])
const qualityLoading = ref(false)
const showQualityPicker = ref(false)

let pollHandle: ReturnType<typeof setInterval> | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null

// ── What is on screen ────────────────────────────────────────────

const staged = readPremiumPlay()
const stagedZap = ref<ZapChannel[]>(
  staged?.id && !isVod.value ? (staged.zapList ?? []) : [],
)

const channelName = computed(() => {
  if (playTitle.value)
    return playTitle.value
  if (staged?.id === channelId.value && staged.title)
    return staged.title
  return channel.value?.name ?? $t('Channel')
})

const zapList = computed(() => premium.zapList.length ? premium.zapList : stagedZap.value)
const channelIndex = computed(() => zapList.value.findIndex(c => c.id === channelId.value))
const hasPrev = computed(() => channelIndex.value > 0)
const hasNext = computed(() => channelIndex.value >= 0 && channelIndex.value < zapList.value.length - 1)

const channelLogo = computed(() => {
  if (staged?.id === channelId.value && staged.logo)
    return staged.logo
  return zapList.value[channelIndex.value]?.logoUrl ?? channel.value?.logoUrl ?? ''
})

const autoSkips = ref(0)
/**
 * Retry/Refresh mean this channel, not the next one auto-skip would pick.
 */
const holdChannel = ref(false)
const autoSkipping = computed(() =>
  !isVod.value
  && autoSkips.value > 0
  && autoSkips.value < MAX_AUTO_SKIPS
  && !playerPlaying.value
  && premium.player !== 'error')

const guide = computed<EpgProgram[]>(() => premium.guide(channelId.value))

/** The one guide line worth the top bar: what is on right now. */
const nowTitle = computed(() => {
  const now = Math.floor(Date.now() / 1000)
  return guide.value.find(p => p.start <= now && (p.stop == null || p.stop > now))?.title ?? ''
})

/**
 * The player's own status line. It sits under the title inside mpv's
 * chrome, so it says what the machine is doing and nothing else — the
 * error case is a full overlay below, not a line of small text.
 */
const statusLine = computed(() => {
  if (isVod.value) {
    switch (premium.player) {
      case 'loading': return $t('Loading…')
      case 'buffering': return $t('Buffering…')
      case 'reconnecting': return $t('Reconnecting… attempt {attempt} of {total}', {
        attempt: premium.reconnectAttempt,
        total: MAX_RECONNECT_ATTEMPTS,
      })
      default: return ''
    }
  }
  switch (premium.player) {
    case 'loading': return autoSkipping.value
      ? $t('Channel unavailable, trying next channel ({attempt} of {total})…', {
          attempt: autoSkips.value,
          total: MAX_AUTO_SKIPS,
        })
      : $t('Connecting to live stream…')
    case 'buffering': return $t('Buffering…')
    case 'reconnecting': return $t('Reconnecting… attempt {attempt} of {total}', {
      attempt: premium.reconnectAttempt,
      total: MAX_RECONNECT_ATTEMPTS,
    })
    default: return autoSkipping.value
      ? $t('Channel unavailable, trying next channel ({attempt} of {total})…', {
          attempt: autoSkips.value,
          total: MAX_AUTO_SKIPS,
        })
      : ''
  }
})

/** Spinner-worthy: something is in flight and there is no picture yet. */
const busy = computed(() =>
  premium.player === 'loading'
  || premium.player === 'reconnecting'
  || premium.player === 'buffering'
  || playback.loading.value,
)

const fatal = computed(() => premium.player === 'error')

const overlayError = computed(() => {
  // One modal. A second card on this page painted "Playback Error"
  // under the same sentence. VOD keeps its own centre overlay while
  // the file is still up; once the machine is `error` the player is
  // unmounted and this is the only place left to say so.
  // The player's catchError is not merged in: it flashed here during
  // reconnect while the overlay was already saying "Connecting…".
  if (isVod.value && premium.player !== 'error')
    return ''
  const raw = premium.player === 'error'
    ? premium.playerError
    : playback.error.value
  if (!raw)
    return ''
  return friendlyPlaybackError(raw, isVod.value ? 'vod' : 'live')
})

// ── Loading a channel ────────────────────────────────────────────

let loadTimeoutTimer: ReturnType<typeof setTimeout> | null = null

function clearLoadTimeout(): void {
  if (loadTimeoutTimer) {
    clearTimeout(loadTimeoutTimer)
    loadTimeoutTimer = null
  }
}

function clearReconnect(): void {
  clearLoadTimeout()
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
}

/**
 * Fill the zap list when the page was entered cold (a reload, or a
 * `rivulet://` style direct link). The channel's own category is the
 * list to walk — it is the one the user would have been looking at — and
 * the store's `setCategory` starts that load, whose result arrives on the
 * computeds above. Nothing awaits it: the picture must not wait for a
 * channel list.
 */
function ensureZapList(ch: IPTVChannel): void {
  if (zapList.value.length > 0)
    return
  if (ch.categoryName)
    premium.setCategory(ch.categoryName)
  else
    void premium.loadChannels({ reset: true })
}

async function resolveChannel(id: string): Promise<IPTVChannel | null> {
  const known = premium.channels.find(c => c.id === id) ?? premium.recent.find(c => c.id === id)
  if (known)
    return known
  try {
    return await premiumApi.channel(id)
  }
  catch {
    // A name is a nicety; a missing one must not stop playback.
    return null
  }
}

async function loadGuide(id: string): Promise<void> {
  guideLoading.value = true
  try {
    await premium.loadGuide(id)
  }
  finally {
    guideLoading.value = false
  }
}

async function loadQualityVariants(id: string): Promise<void> {
  qualityLoading.value = true
  qualityVariants.value = []
  try {
    const variants = await premiumApi.qualityVariants(id)
    qualityVariants.value = variants.filter(v => v.id !== id)
  }
  catch {
    qualityVariants.value = []
  }
  finally {
    qualityLoading.value = false
  }
}

function switchQuality(ch: IPTVChannel): void {
  showQualityPicker.value = false
  void router.replace({
    path: localePath('/live-tv/premium/watch'),
    query: { id: ch.id, from: String(route.query.from ?? '') },
  })
}

function connectionLimitMessage(): string {
  const a = premium.account
  return $t('Your provider is at its connection limit ({active} of {max} streams in use). Stop playback on your other devices, then try again.', {
    active: a?.activeConnections ?? 1,
    max: a?.maxConnections ?? 1,
  })
}

/**
 * Mint a source for `channelId` and start playing it. `fresh` separates a
 * new channel from a reconnect: a new channel resets the attempt counter
 * and the guide, a reconnect keeps the counter that scheduled it.
 *
 * The account probe is not on this path. It is a round trip to the panel
 * and only names a failure; awaiting it here is what made every zap wait
 * on the provider before mpv even started. A cached limit still fails
 * fast. A live probe runs after a refusal, where the count matters.
 */
async function load({ fresh } = { fresh: true }): Promise<void> {
  const id = isVod.value ? playId.value : channelId.value
  const kind = playKind.value === 'movie'
    ? 'movie' as const
    : playKind.value === 'episode'
      ? 'episode' as const
      : 'channel' as const
  clearReconnect()
  if (!id) {
    premium.setPlayer('error', isVod.value ? $t('Nothing was given to play.') : $t('No channel was given to play.'))
    return
  }
  if (fresh) {
    premium.resetPlayer()
    void premium.ensureLoaded()
  }
  premium.setPlayer(fresh ? 'loading' : 'reconnecting')
  playerCatchError.value = ''

  await playback.load(id, { kind, ext: playExt.value })
  // A zap that landed while this was in flight owns the page now.
  if (id !== (isVod.value ? playId.value : channelId.value))
    return

  if (playback.error.value || !playback.source.value) {
    premium.setPlayer('error', playback.error.value || (isVod.value
      ? $t('This title could not be opened.')
      : $t('This channel could not be opened.')))
    return
  }

  clearLoadTimeout()
  loadTimeoutTimer = setTimeout(() => {
    if (!playerPlaying.value && (premium.player === 'loading' || premium.player === 'reconnecting' || premium.player === 'buffering')) {
      void onPlaybackFailed('dead')
    }
  }, 15000)

  if (fresh && !isVod.value) {
    const i = channelIndex.value
    playback.prefetch([zapList.value[i - 1]?.id, zapList.value[i + 1]?.id])
    void resolveChannel(id).then(ch => {
      if (id !== channelId.value)
        return
      channel.value = ch
      if (ch)
        ensureZapList(ch)
    })
    void premium.addRecent(id)
    void loadGuide(id)
    void loadQualityVariants(id)
  }
}

/**
 * The stream died. Ask the store for the next backoff step; `null` means
 * the four attempts are spent, which is where the retrying stops and the
 * user gets one honest sentence instead of a spinner forever.
 *
 * `reason` is the player's reading of mpv's exit: `refused` means the
 * upstream answered 401/403 rather than going quiet. That distinction is
 * worth a provider round trip, because the commonest cause is not a dead
 * channel at all — it is the account's simultaneous-connection limit,
 * and the panel is the only thing that knows how many are in use.
 */
async function onPlaybackFailed(reason?: 'stub' | 'dead' | 'refused'): Promise<void> {
  clearReconnect()
  if (isVod.value) {
    // mpv can log a failed sub-stream while the main file keeps playing —
    // only treat it as dead when transport has actually stopped.
    if (playerPlaying.value || playerPosition.value > 0.5)
      return
    if (reason === 'refused') {
      await premium.probeAccount()
      if (premium.atConnectionLimit === true) {
        premium.setPlayer('error', connectionLimitMessage())
        return
      }
    }
    premium.setPlayer('error', reason === 'refused'
      ? $t('The provider refused this stream. Your account may be at its connection limit.')
      : $t('This title stopped responding. Try again.'))
    return
  }
  // Sound or a frame: the channel is up. A later HLS 404 must not
  // reconnect-walk away from a stream the viewer can already hear.
  if (playerPlaying.value)
    return
  if (reason === 'refused') {
    await premium.probeAccount()
    // A limit we can see is a limit worth naming: retrying cannot help
    // until a slot frees, and the viewer is the one who can free it.
    if (premium.atConnectionLimit === true) {
      premium.setPlayer('error', connectionLimitMessage())
      return
    }
  }
  const delay = premium.nextReconnect()
  if (delay === null) {
    if (autoSkip())
      return
    const current = zapList.value[channelIndex.value]
    if (current)
      premium.markOffline(current.id)
    premium.setPlayer('error', reason === 'refused'
      ? $t('The provider refused this stream. The channel may not be part of your package, or the account may be busy on another device.')
      : $t('This channel stopped responding. It may be off the air, or the provider may be busy.'))
    return
  }
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null
    void load({ fresh: false })
  }, delay)
}

// ── Transport ────────────────────────────────────────────────────

/**
 * Mirror mpv into the overlay's props, and into the store's machine.
 *
 * The machine is only written while the page is not handling a failure:
 * `reconnecting` and `error` are the page's own states and mpv, which has
 * just been torn down or never started, would otherwise overwrite them
 * with `loading` on the next tick.
 */
function syncPlayerState(): void {
  const p = playerRef.value
  if (!p)
    return
  const wasPlaying = playerPlaying.value
  const picture = typeof p.videoWidth === 'number' && p.videoWidth > 0
  const audio = asBool(p.hasAudio)
  playerPlaying.value = asBool(p.started) && !asBool(p.paused) && liveLocked(picture ? 1 : 0, audio)
  playerBehindLive.value = asBool(p.behindLive)
  playerVolume.value = typeof p.volume === 'number' ? p.volume : 100
  playerMuted.value = asBool(p.muted)
  playerChrome.value = asBool(p.ui)
  playerPosition.value = typeof p.position === 'number' ? p.position : 0
  playerDuration.value = typeof p.duration === 'number' ? p.duration : 0

  // Drop stale player errors while the clock is moving — a log line from
  // startup must not cover a title that is already playing.
  if (playerPlaying.value || playerPosition.value > 0.5)
    playerCatchError.value = ''
  else
    playerCatchError.value = asText(p.errorMsg ?? p.catchError)

  // Successful start → clear any stale errors from the previous load or
  // reconnect attempt. Otherwise a dead-token error sits forever under a
  // perfectly good picture.
  if (playerPlaying.value && !wasPlaying) {
    clearLoadTimeout()
    autoSkips.value = 0
    holdChannel.value = false
    premium.resetPlayer()
    premium.setPlayer('playing')
    playerCatchError.value = ''
    if (!isVod.value && channelId.value)
      premium.markLive(channelId.value)
    if (typeof p.catchError === 'object' && p.catchError && 'value' in p.catchError)
      (p.catchError as { value: string }).value = ''
    const i = channelIndex.value
    playback.prefetch([zapList.value[i - 1]?.id, zapList.value[i + 1]?.id])
  }

  if (premium.player === 'reconnecting' || premium.player === 'error')
    return
  if (!asBool(p.started))
    return
  if (asBool(p.buffering) || !liveLocked(picture ? 1 : 0, audio))
    premium.setPlayer('buffering')
  else if (asBool(p.paused))
    premium.setPlayer('paused')
  else
    premium.setPlayer('playing')
}

function onTogglePlay(): void {
  const p = playerRef.value
  if (!p) {
    void load({ fresh: true })
    return
  }
  p.togglePlay()
  setTimeout(syncPlayerState, 0)
}

function onGoLive(): void {
  const p = playerRef.value
  if (!p?.goLive)
    return
  void p.goLive()
  setTimeout(syncPlayerState, 0)
}

function onToggleMute(): void {
  playerRef.value?.toggleMute()
  setTimeout(syncPlayerState, 0)
}

function onSetVolume(v: number): void {
  playerRef.value?.setVolume(v)
  setTimeout(syncPlayerState, 0)
}

function onActivity(): void {
  overlayRef.value?.show()
}

function cycleAspectRatio(): void {
  aspectRatio.value = cycleAspect(aspectRatio.value)
}

function toggleFav(): void {
  const ch = channel.value
  if (ch)
    void premium.toggleFavorite(ch)
}

const isFavorite = computed(() => channel.value ? premium.isFavorite(channel.value) : false)

const isFullscreen = ref(isAndroid())
function toggleFullscreen(): void {
  isFullscreen.value = !isFullscreen.value
}

// ── Navigation ───────────────────────────────────────────────────

function goBack(): void {
  premium.resetPlayer()
  playerCatchError.value = ''
  void router.replace(localePath(liveTvFrom(String(route.query.from ?? ''), '/live-tv/premium')))
}

/**
 * Zap by index. `router.replace` rather than `push` so channel-up ten
 * times does not put ten entries between the viewer and Back, and the
 * `channelId` watcher below is what actually loads the new channel —
 * one path in, whether the zap came from a key, the arrows or the list.
 */
function zapTo(index: number): void {
  const target = zapList.value[index]
  if (!target || target.id === channelId.value)
    return
  holdChannel.value = false
  void router.replace({
    path: localePath('/live-tv/premium/watch'),
    query: {
      id: target.id,
      from: String(route.query.from ?? ''),
      title: target.name,
    },
  })
}

function zap(direction: 1 | -1): void {
  if (channelIndex.value < 0)
    return
  zapTo(channelIndex.value + direction)
}

/** From Connecting / Playback Error: leave this channel, wrap if needed. */
function skipChannel(): void {
  const current = zapList.value[channelIndex.value]
  if (current)
    premium.markOffline(current.id)
  let next = nextPlayable(zapList.value, channelIndex.value, premium.offlineIds)
  if (next < 0)
    next = nextPlayable(zapList.value, -1, premium.offlineIds)
  if (next < 0 || next === channelIndex.value)
    return
  autoSkips.value++
  zapTo(next)
}

function onNext(): void {
  if (!isVod.value && (busy.value || overlayError.value))
    skipChannel()
  else
    zap(1)
}

async function onRetry(): Promise<void> {
  holdChannel.value = true
  autoSkips.value = 0
  playerCatchError.value = ''
  if (!isVod.value && channelId.value)
    premium.markLive(channelId.value)
  await load({ fresh: true })
}

async function onRefresh(): Promise<void> {
  holdChannel.value = true
  autoSkips.value = 0
  playerCatchError.value = ''
  if (!isVod.value && channelId.value) {
    premium.markLive(channelId.value)
    playback.forget(channelId.value)
  }
  else if (isVod.value && playId.value) {
    playback.forget(`${playKind.value}:${playId.value}:${playExt.value}`)
  }
  playback.clear()
  await load({ fresh: true })
}

/**
 * Bounded walk past dead channels, same contract as Free TV. A Premium
 * lineup is the user's package, but a channel that will not open is still
 * ordinary — reconnecting the same one four times and then stopping is
 * what left the overlay on a black screen. After `MAX_AUTO_SKIPS` it
 * stops and shows the error: the list is dead, not this channel.
 */
function autoSkip(): boolean {
  if (holdChannel.value)
    return false
  if (isVod.value || channelIndex.value < 0 || autoSkips.value >= MAX_AUTO_SKIPS)
    return false
  const current = zapList.value[channelIndex.value]
  if (current)
    premium.markOffline(current.id)
  let next = nextPlayable(zapList.value, channelIndex.value, premium.offlineIds)
  if (next < 0)
    next = nextPlayable(zapList.value, -1, premium.offlineIds)
  if (next < 0 || next === channelIndex.value)
    return false
  autoSkips.value++
  zapTo(next)
  return true
}

function onKey(e: KeyboardEvent): void {
  if (e.key === 'Escape' || e.key === 'Backspace' || e.key === 'GoBack') {
    e.preventDefault()
    if (showQualityPicker.value) {
      showQualityPicker.value = false
      return
    }
    goBack()
    return
  }
  if (overlayError.value || (busy.value && !playerPlaying.value)) {
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
  window.addEventListener('keydown', onKey)
  window.addEventListener('mousemove', onActivity)
  window.addEventListener('click', onActivity)
  window.addEventListener('touchstart', onActivity, { passive: true })
  window.addEventListener('pointermove', onActivity, { passive: true })
  if (isAndroid())
    setAndroidPlayerMode(true)
  pollHandle = setInterval(syncPlayerState, POLL_MS)
})

// Immediate: the first mint and every zap. onMounted must not also
// call `load` — two mints is two mpv starts, and a 1-slot account
// answers the second with the connection-limit card.
watch([channelId, playId, playKind], () => {
  void load({ fresh: true })
}, { immediate: true })

// Release the slot so Retry is one start, not a remount of the dead
// token plus a new one.
watch(fatal, isFatal => {
  if (isFatal)
    playback.clear()
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('mousemove', onActivity)
  window.removeEventListener('click', onActivity)
  window.removeEventListener('touchstart', onActivity)
  window.removeEventListener('pointermove', onActivity)
  if (pollHandle)
    clearInterval(pollHandle)
  clearReconnect()
  playback.clear()
  premium.resetPlayer()
  if (isAndroid())
    setAndroidPlayerMode(false)
})
</script>

<template>
  <div class="relative h-screen w-screen overflow-hidden bg-black">
    <!-- Player surface. A plain `relative`/`absolute inset-0` parent, not
         flex centring: the latter collapses `<mpv-player>`'s box to zero
         height on first mount and its `waitForBox()` then refuses to
         start mpv.

         Deliberately *not* keyed on the source URL. A key change
         unmounts the old player and mounts a new one, and the old one's
         `onBeforeUnmount` fires `player_stop` without awaiting it while
         the new one is already calling `player_start` — two mpv
         processes racing over one window, and, on an account with a
         single connection slot, the second one getting a 401 for the
         slot the first has not released. Leaving the component mounted
         puts a zap and a reconnect through its own `watch(src)`, which
         awaits the stop before the start. Every mint is a distinct URL
         (the redirector token carries a `jti`), so the watcher always
         sees the change. -->
    <div
      class="absolute inset-0"
      :class="{
        '[&_video]:!object-cover': aspectRatio === 'cover',
        '[&_video]:!object-fill': aspectRatio === 'fill',
      }"
    >
      <mpv-player
        v-if="playback.source.value && !fatal"
        ref="playerRef"
        :src="playback.source.value.url"
        :status="statusLine"
        :title="channelName"
        :quality="playback.source.value?.quality"
        :mode="playerMode"
        :aspect="aspectRatio"
        :fullscreen="isFullscreen"
        :resolving="!isVod && busy"
        :user-agent="playback.source.value.userAgent"
        :referer="playback.source.value.referer"
        @failed="reason => void onPlaybackFailed(reason)"
      />
    </div>

    <!-- Live chrome. Auto-hides after 3s of stillness; the guide panel
         below follows its visibility rather than keeping its own timer.
         Stays mounted on fatal: this overlay is the only error modal. -->
    <live-tv-live-player-overlay
      ref="overlayRef"
      class="!z-40"
      :variant="isVod ? 'vod' : 'live'"
      :playing="playerPlaying"
      :behind-live="playerBehindLive"
      :volume="playerVolume"
      :muted="playerMuted"
      :has-prev="isVod ? false : hasPrev"
      :has-next="isVod ? false : hasNext"
      :busy="busy"
      :busy-text="statusLine"
      :channel-name="channelName"
      :now-playing="isVod ? '' : nowTitle"
      :channel-logo="isVod ? '' : channelLogo"
      :channel-index="isVod ? 0 : (channelIndex >= 0 ? channelIndex : 0)"
      :channel-total="isVod ? 0 : zapList.length"
      :channel-list="isVod ? [] : zapList"
      :offline-ids="premium.offlineIds"
      :position="playerPosition"
      :duration="playerDuration"
      :is-favorite="isFavorite"
      :is-fullscreen="isFullscreen"
      :chrome-up="playerChrome"
      :error="overlayError"
      :resolution-label="typeof playerRef?.resolutionLabel === 'string' ? playerRef.resolutionLabel : ''"
      :source-quality="playback.source.value?.quality ?? null"
      :quality-variants="qualityVariants"
      :quality-loading="qualityLoading"
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
      @toggle-favorite="toggleFav"
      @toggle-fullscreen="toggleFullscreen"
      @show-quality-picker="showQualityPicker = !showQualityPicker"
      @cycle-aspect-ratio="cycleAspectRatio"
    />

    <!-- Guide. Renders nothing at all when the provider has no listing for
         this channel — an empty container with headings in it is worse
         than no panel. -->
    <div
      v-if="!fatal && !isVod && (guide.length > 0 || guideLoading)"
      class="pointer-events-none absolute bottom-24 left-4 z-30 w-80 max-w-[85vw] rounded-2xl bg-black/70 p-4 text-white ring-1 ring-white/10 transition-opacity duration-300"
      :class="overlayRef?.visible ? 'opacity-100' : 'opacity-0'"
    >
      <premium-tv-premium-epg-panel :programs="guide" :loading="guideLoading" :up-next="3" />
    </div>

    <!-- Quality picker. A compact card, not a full-height drawer: the
         drawer started at the top-right of the window and sat under the
         title-bar controls and the LIVE header. `data-cut` punches the
         mpv hole; a translucent fill there would show the page's black. -->
    <div
      v-if="showQualityPicker && qualityVariants.length > 0"
      class="absolute inset-0 z-50"
      @click.self="showQualityPicker = false"
    >
      <div
        data-cut
        class="absolute end-4 top-24 flex w-80 max-w-[calc(100vw-2rem)] max-h-[min(70vh,24rem)] flex-col overflow-hidden rounded-2xl border border-white/15 bg-neutral-950 text-white shadow-2xl"
        @click.stop
      >
        <div class="flex shrink-0 items-center justify-between gap-3 border-b border-white/10 px-4 py-3">
          <h2 class="text-title-small font-bold">
            {{ $t('Quality') }}
          </h2>
          <button
            type="button"
            class="grid size-8 shrink-0 place-items-center rounded-lg text-white/70 transition-colors hover:bg-white/10 hover:text-white focus-visible:bg-white/10 focus-visible:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            :aria-label="$t('Close')"
            @click="showQualityPicker = false"
          >
            <v-icon :icon="mdiClose" size="18" />
          </button>
        </div>

        <div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-2" data-dpad-start>
          <!-- Current channel -->
          <button
            type="button"
            class="flex w-full items-center gap-3 rounded-xl bg-primary p-2.5 text-start font-bold text-on-primary"
            aria-current="true"
          >
            <span class="min-w-0 flex-1 truncate text-body-small">{{ channelName }}</span>
            <span v-if="playback.source.value?.quality" class="shrink-0 text-label-small opacity-80">
              {{ playback.source.value.quality }}
            </span>
            <v-icon :icon="mdiCheck" size="16" class="shrink-0" />
          </button>
          <!-- Variants -->
          <button
            v-for="variant in qualityVariants"
            :key="variant.id"
            type="button"
            class="flex w-full items-center gap-3 rounded-xl p-2.5 text-start text-white/80 transition-colors hover:bg-white/10 hover:text-white focus-visible:bg-white/10 focus-visible:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            @click="switchQuality(variant)"
          >
            <span class="min-w-0 flex-1 truncate text-body-small">{{ variant.name }}</span>
            <span v-if="variant.quality" class="shrink-0 text-label-small opacity-70">
              {{ variant.quality }}
            </span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
