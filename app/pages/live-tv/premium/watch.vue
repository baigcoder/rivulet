<script setup lang="ts">
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
 * attempts at 1s, 2s, 4s, 8s, then one clear final error and a stop. The
 * eight-state machine there is what this page drives — `setPlayer` on
 * every transition, and nothing on this page keeps a second copy of it.
 */
import type { EpgProgram, IPTVChannel } from '~/types/premium'
import { mdiArrowLeft, mdiCropFree, mdiFormatListBulleted, mdiMagnify, mdiReload, mdiStar, mdiStarOutline } from '@mdi/js'
import { usePlaybackSource } from '~/composables/usePlaybackSource'
import { MAX_RECONNECT_ATTEMPTS } from '~/stores/premiumTv'
import { premiumApi } from '~/utils/premiumTv'

definePageMeta({ layout: false })

/** How often mpv's transport state is mirrored for the overlay. */
const POLL_MS = 500

const route = useRoute()
const router = useRouter()
const premium = usePremiumTvStore()
const playback = usePlaybackSource()

const channelId = computed(() => String(route.query.id ?? ''))

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
  ui: { value: boolean }
  catchError?: { value: string }
} | null>(null)
const overlayRef = ref<{ show: () => void, hide: () => void, visible: boolean } | null>(null)

const playerPlaying = ref(false)
const playerVolume = ref(100)
const playerMuted = ref(false)
const playerCatchError = ref('')
/**
 * Mirrored from the player rather than sensed here: on X11 and Win32 mpv's
 * window is in front of the page and swallows every mousemove, so the HUD's
 * own DOM events go quiet the moment the cursor is over the picture.
 */
const playerChrome = ref(false)

const showChannelDrawer = ref(false)
const drawerSearch = ref('')
const aspectRatio = ref<'contain' | 'cover' | 'fill'>('contain')
const guideLoading = ref(false)

let pollHandle: ReturnType<typeof setInterval> | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null

// ── What is on screen ────────────────────────────────────────────

const channelName = computed(() => channel.value?.name ?? $t('Channel'))
const channelLogo = computed(() => channel.value?.logoUrl ?? '')

const zapList = computed(() => premium.zapList)
const channelIndex = computed(() => zapList.value.findIndex(c => c.id === channelId.value))
const hasPrev = computed(() => channelIndex.value > 0)
const hasNext = computed(() => channelIndex.value >= 0 && channelIndex.value < zapList.value.length - 1)

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
  switch (premium.player) {
    case 'loading': return $t('Connecting to live stream…')
    case 'buffering': return $t('Buffering…')
    case 'reconnecting': return $t('Reconnecting… attempt {attempt} of {total}', {
      attempt: premium.reconnectAttempt,
      total: MAX_RECONNECT_ATTEMPTS,
    })
    default: return ''
  }
})

/** Spinner-worthy: something is in flight and there is no picture yet. */
const busy = computed(() =>
  premium.player === 'loading' || premium.player === 'reconnecting' || playback.loading.value,
)

const fatal = computed(() => premium.player === 'error')

const overlayError = computed(() => {
  if (premium.player === 'error')
    return premium.playerError
  if (playback.error.value)
    return playback.error.value
  return playerCatchError.value
})

const filteredDrawerChannels = computed(() => {
  const q = drawerSearch.value.trim().toLowerCase()
  const rows = zapList.value.map((c, originalIndex) => ({ ...c, originalIndex }))
  return q ? rows.filter(c => c.name.toLowerCase().includes(q)) : rows
})

// ── Loading a channel ────────────────────────────────────────────

function clearReconnect(): void {
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

/**
 * Mint a source for `channelId` and start playing it. `fresh` separates a
 * new channel from a reconnect: a new channel resets the attempt counter
 * and the guide, a reconnect keeps the counter that scheduled it.
 */
async function load({ fresh } = { fresh: true }): Promise<void> {
  const id = channelId.value
  clearReconnect()
  if (!id) {
    premium.setPlayer('error', $t('No channel was given to play.'))
    return
  }
  if (fresh) {
    premium.resetPlayer()
    void premium.ensureLoaded()
  }
  premium.setPlayer(fresh ? 'loading' : 'reconnecting')
  playerCatchError.value = ''

  await playback.load(id)
  // A zap that landed while this was in flight owns the page now.
  if (id !== channelId.value)
    return

  if (playback.error.value || !playback.source.value) {
    // Failing to *mint* a source is not a dead stream — it is the API
    // saying no (entitlement, no provider, channel gone). Retrying it on
    // a timer would only repeat the refusal, so it is final and said once.
    premium.setPlayer('error', playback.error.value || $t('This channel could not be opened.'))
    return
  }

  if (fresh) {
    const ch = await resolveChannel(id)
    if (id !== channelId.value)
      return
    channel.value = ch
    if (ch)
      ensureZapList(ch)
    void premium.addRecent(id)
    void loadGuide(id)
  }
}

function retry(): void {
  premium.resetPlayer()
  void load({ fresh: true })
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
  if (reason === 'refused') {
    await premium.probeAccount()
    // A limit we can see is a limit worth naming: retrying cannot help
    // until a slot frees, and the viewer is the one who can free it.
    if (premium.atConnectionLimit === true) {
      const a = premium.account
      premium.setPlayer('error', $t('Your provider is at its connection limit ({active} of {max} streams in use). Stop playback on your other devices, then try again.', {
        active: a?.activeConnections ?? 1,
        max: a?.maxConnections ?? 1,
      }))
      return
    }
  }
  const delay = premium.nextReconnect()
  if (delay === null) {
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
  playerPlaying.value = p.started && !p.paused
  playerVolume.value = p.volume
  playerMuted.value = p.muted
  playerChrome.value = p.ui?.value === true
  playerCatchError.value = p.catchError?.value ?? ''

  // Successful start → clear any stale errors from the previous load or
  // reconnect attempt. Otherwise a dead-token error sits forever under a
  // perfectly good picture.
  if (playerPlaying.value && !wasPlaying) {
    premium.resetPlayer()
    premium.setPlayer('playing')
    playerCatchError.value = ''
    if ((p.catchError as any)?.value != null)
      p.catchError.value = ''
  }

  if (premium.player === 'reconnecting' || premium.player === 'error')
    return
  if (!p.started)
    return
  if (p.buffering)
    premium.setPlayer('buffering')
  else if (p.paused)
    premium.setPlayer('paused')
  else
    premium.setPlayer('playing')
}

function onTogglePlay(): void {
  playerRef.value?.togglePlay()
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
  aspectRatio.value = aspectRatio.value === 'contain'
    ? 'cover'
    : aspectRatio.value === 'cover' ? 'fill' : 'contain'
}

function toggleFav(): void {
  const ch = channel.value
  if (ch)
    void premium.toggleFavorite(ch)
}

const isFavorite = computed(() => channel.value ? premium.isFavorite(channel.value) : false)

const isFullscreen = ref(false)
function toggleFullscreen(): void {
  isFullscreen.value = !isFullscreen.value
  if (isFullscreen.value)
    document.documentElement.requestFullscreen?.()
  else
    document.exitFullscreen?.()
}

// ── Navigation ───────────────────────────────────────────────────

function goBack(): void {
  premium.resetPlayer()
  playerCatchError.value = ''
  const from = String(route.query.from ?? '')
  if (from) {
    void router.replace(from)
    return
  }
  if (window.history.length > 1)
    router.back()
  else
    void router.replace(localePath('/live-tv/premium'))
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
  showChannelDrawer.value = false
  void router.replace({
    path: localePath('/live-tv/premium/watch'),
    query: { id: target.id, from: String(route.query.from ?? '') },
  })
}

function zap(direction: 1 | -1): void {
  if (channelIndex.value < 0)
    return
  zapTo(channelIndex.value + direction)
}

function onKey(e: KeyboardEvent): void {
  if (e.key === 'Escape' || e.key === 'Backspace' || e.key === 'GoBack') {
    e.preventDefault()
    if (showChannelDrawer.value)
      showChannelDrawer.value = false
    else
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

onMounted(() => {
  window.addEventListener('keydown', onKey)
  window.addEventListener('mousemove', onActivity)
  window.addEventListener('click', onActivity)
  window.addEventListener('touchstart', onActivity, { passive: true })
  window.addEventListener('pointermove', onActivity, { passive: true })
  void load({ fresh: true })
  pollHandle = setInterval(syncPlayerState, POLL_MS)
})

// A zap only changes the query, so the page stays mounted and this is
// what starts the next channel.
watch(channelId, () => {
  void load({ fresh: true })
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
        mode="live"
        :user-agent="playback.source.value.userAgent"
        :referer="playback.source.value.referer"
        @failed="reason => void onPlaybackFailed(reason)"
      />
    </div>

    <!-- Live chrome. Auto-hides after 3s of stillness; the guide panel
         below follows its visibility rather than keeping its own timer.
         NOTE: intentionally NOT gated by !fatal — the overlay owns the
         center error modal, so when the premium state machine lands on
         "error" the overlay must still be mounted to show it. -->
    <live-tv-live-player-overlay
      v-if="playback.source.value || fatal"
      ref="overlayRef"
      class="!z-40"
      :playing="playerPlaying"
      :volume="playerVolume"
      :muted="playerMuted"
      :has-prev="hasPrev"
      :has-next="hasNext"
      :busy="busy"
      :channel-name="channelName"
      :channel-logo="channelLogo"
      :channel-index="channelIndex >= 0 ? channelIndex : 0"
      :channel-total="zapList.length"
      :channel-list="zapList"
      :is-favorite="isFavorite"
      :is-fullscreen="isFullscreen"
      :chrome-up="playerChrome"
      :error="overlayError"
      @back="goBack"
      @prev="zap(-1)"
      @next="zap(1)"
      @zap-to="zapTo"
      @retry="() => void load({ fresh: true })"
      @toggle-play="onTogglePlay"
      @toggle-mute="onToggleMute"
      @set-volume="onSetVolume"
      @toggle-favorite="toggleFav"
      @toggle-fullscreen="toggleFullscreen"
    >
      <template #info>
        <div
          v-if="channelLogo"
          class="grid size-10 shrink-0 place-items-center overflow-hidden rounded-xl bg-white/10 p-1 ring-1 ring-white/15"
        >
          <img :src="channelLogo" :alt="channelName" class="size-full object-contain" loading="lazy" decoding="async">
        </div>
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <span class="inline-flex items-center gap-1 rounded-full bg-red-600 px-2 py-0.5 text-[10px] font-bold tracking-wide text-white">
              <span class="size-1.5 rounded-full bg-white" /> {{ $t('LIVE') }}
            </span>
            <span v-if="channelIndex >= 0" class="text-label-small text-white/70 tabular-nums">
              {{ $t('CH {number} / {total}', { number: channelIndex + 1, total: zapList.length }) }}
            </span>
          </div>
          <h1 class="mt-0.5 truncate text-title-medium font-bold text-white">
            {{ channelName }}
          </h1>
          <p v-if="nowTitle" class="truncate text-body-small text-white/70">
            {{ nowTitle }}
          </p>
        </div>
      </template>
      <template #actions>
        <button
          v-if="channel"
          type="button"
          class="grid size-10 place-items-center rounded-full bg-black/40 text-white transition-colors hover:bg-black/60 focus-visible:bg-black/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          :title="premium.isFavorite(channel) ? $t('Remove from favorites') : $t('Add to favorites')"
          :aria-label="premium.isFavorite(channel) ? $t('Remove from favorites') : $t('Add to favorites')"
          :aria-pressed="premium.isFavorite(channel)"
          @click="toggleFav"
        >
          <v-icon :icon="premium.isFavorite(channel) ? mdiStar : mdiStarOutline" size="18" />
        </button>
        <button
          type="button"
          class="grid size-10 place-items-center rounded-full bg-black/40 text-white transition-colors hover:bg-black/60 focus-visible:bg-black/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          :title="$t('Aspect Ratio')"
          :aria-label="$t('Aspect Ratio')"
          @click="cycleAspectRatio"
        >
          <v-icon :icon="mdiCropFree" size="18" />
        </button>
        <button
          v-if="zapList.length > 0"
          type="button"
          class="grid size-10 place-items-center rounded-full bg-black/40 text-white transition-colors hover:bg-black/60 focus-visible:bg-black/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          :title="$t('Channels')"
          :aria-label="$t('Channels')"
          @click="showChannelDrawer = !showChannelDrawer"
        >
          <v-icon :icon="mdiFormatListBulleted" size="18" />
        </button>
      </template>
    </live-tv-live-player-overlay>

    <!-- Guide. Renders nothing at all when the provider has no listing for
         this channel — an empty container with headings in it is worse
         than no panel. Hidden while the drawer is open so the two do not
         fight over the same corner. -->
    <div
      v-if="!fatal && !showChannelDrawer && (guide.length > 0 || guideLoading)"
      class="pointer-events-none absolute bottom-24 left-4 z-30 w-80 max-w-[85vw] rounded-2xl bg-black/70 p-4 text-white ring-1 ring-white/10 transition-opacity duration-300"
      :class="overlayRef?.visible ? 'opacity-100' : 'opacity-0'"
    >
      <premium-tv-premium-epg-panel :programs="guide" :loading="guideLoading" :up-next="3" />
    </div>

    <!-- First connect, and every reconnect: one spinner, one line saying
         which of the two this is. -->
    <div
      v-if="busy && !playback.source.value && !fatal"
      class="absolute inset-0 z-40 grid place-items-center text-white"
    >
      <div class="flex flex-col items-center gap-3">
        <v-progress-circular indeterminate color="primary" size="40" width="3" />
        <p class="text-body-medium font-medium opacity-70">
          {{ statusLine || $t('Connecting to live stream…') }}
        </p>
      </div>
    </div>

    <!-- The end of the road: attempts spent, or a refusal that retrying
         cannot change. One message, and something to do about it. -->
    <div
      v-else-if="fatal"
      class="absolute inset-0 z-40 grid place-items-center bg-black/90 px-6 text-center text-white"
    >
      <div class="flex max-w-md flex-col items-center gap-4">
        <h2 class="text-title-medium font-bold">
          {{ channelName }}
        </h2>
        <p class="text-body-medium opacity-80">
          {{ premium.playerError || $t('This channel could not be opened.') }}
        </p>
        <div class="flex flex-wrap justify-center gap-3">
          <v-btn color="primary" variant="flat" :prepend-icon="mdiReload" @click="retry">
            {{ $t('Retry') }}
          </v-btn>
          <v-btn v-if="hasNext" variant="tonal" @click="zap(1)">
            {{ $t('Next channel') }}
          </v-btn>
          <v-btn variant="tonal" :prepend-icon="mdiArrowLeft" @click="goBack">
            {{ $t('Back') }}
          </v-btn>
        </div>
      </div>
    </div>

    <!-- Quick channel list. The zap list as displayed, so it matches what
         channel-up walks; searchable, because a thousand-channel provider
         makes scrolling to a name absurd. -->
    <div
      v-if="showChannelDrawer"
      class="absolute inset-y-0 right-0 z-40 flex w-80 max-w-[85vw] flex-col border-s border-white/10 bg-black/85 p-4 text-white"
    >
      <div class="flex items-center justify-between gap-2 pb-3">
        <h2 class="text-title-medium font-bold">
          {{ $t('Channels') }}
        </h2>
        <v-btn
          icon
          size="x-small"
          variant="text"
          :aria-label="$t('Close')"
          @click="showChannelDrawer = false"
        >
          <v-icon :icon="mdiArrowLeft" />
        </v-btn>
      </div>

      <div class="relative mb-3">
        <v-icon :icon="mdiMagnify" size="18" class="absolute left-3 top-2.5 text-white/50" />
        <input
          v-model="drawerSearch"
          type="text"
          :placeholder="$t('Search channels')"
          :aria-label="$t('Search channels')"
          class="w-full rounded-xl border border-white/15 bg-white/10 py-1.5 pl-9 pr-3 text-body-small text-white placeholder-white/40 outline-none focus:border-primary"
        >
      </div>

      <div class="flex-1 space-y-1 overflow-y-auto" data-dpad-start>
        <button
          v-for="ch in filteredDrawerChannels"
          :key="ch.id"
          type="button"
          class="flex w-full items-center gap-3 rounded-xl p-2.5 text-start transition-colors"
          :class="ch.originalIndex === channelIndex
            ? 'bg-primary font-bold text-on-primary'
            : 'text-white/80 hover:bg-white/10 hover:text-white focus-visible:bg-white/10 focus-visible:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary'"
          :aria-current="ch.originalIndex === channelIndex ? 'true' : undefined"
          @click="zapTo(ch.originalIndex)"
        >
          <span class="w-7 shrink-0 text-label-small tabular-nums opacity-60">
            {{ ch.originalIndex + 1 }}
          </span>
          <div v-if="ch.logoUrl" class="grid size-7 shrink-0 place-items-center overflow-hidden rounded-md bg-white/10">
            <img :src="ch.logoUrl" :alt="ch.name" class="size-full object-contain" loading="lazy" decoding="async">
          </div>
          <span class="flex-1 truncate text-body-small">{{ ch.name }}</span>
        </button>
        <p v-if="filteredDrawerChannels.length === 0" class="p-3 text-body-small opacity-60">
          {{ $t('No channels match that search.') }}
        </p>
      </div>
    </div>
  </div>
</template>
