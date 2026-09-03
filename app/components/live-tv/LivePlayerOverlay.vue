<script setup lang="ts">
/**
 * The Live TV player's chrome: channel identity, transport, and the zap lineup.
 *
 * Deliberately not part of `MpvPlayer.vue`. Live needs channel-up/down and a
 * lineup drawer a film has no use for, and `mode="live"` there hides the VOD bar
 * to leave room for this one.
 *
 * Two rules come from the platform rather than from taste. On Linux and Windows
 * mpv paints into a child window *in front of* the webview:
 *
 *   - Every surface that has to be seen carries `data-cut`, and its rectangle is
 *     subtracted from mpv's window (`set_shape` in `player.rs`). So a cut surface
 *     must be **opaque**: a translucent fill over a hole shows the page's own
 *     black rather than the picture, and `backdrop-filter` has nothing to read.
 *     And `data-cut` never goes on a transparent or full-screen box — that hole
 *     swallows the whole picture, which is exactly how this HUD came to play
 *     sound over a black screen with no controls in sight.
 *   - No mousemove reaches this page while the cursor is over the picture, so the
 *     auto-hide cannot be driven from DOM events alone. `chromeUp` is
 *     `MpvPlayer`'s own `ui` flag, fed by the native pointer poll
 *     (`player_pointer`), the keyboard, and the fallback for compositors that
 *     refuse a cursor query outright.
 *
 * A live window has nothing to seek over, so there is no scrubber. The
 * programme line is whatever the watch page already knows (`nowPlaying`).
 */
import {
  mdiAlertCircleOutline,
  mdiArrowLeft,
  mdiClose,
  mdiCropFree,
  mdiEyeOffOutline,
  mdiFormatListBulleted,
  mdiFullscreen,
  mdiFullscreenExit,
  mdiMagnify,
  mdiMonitor,
  mdiPause,
  mdiPlay,
  mdiReload,
  mdiSkipNext,
  mdiSkipPrevious,
  mdiStar,
  mdiStarOutline,
  mdiVolumeHigh,
  mdiVolumeLow,
  mdiVolumeMedium,
  mdiVolumeOff,
} from '@mdi/js'
import { invoke } from '@tauri-apps/api/core'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { isAndroid, isTv } from '~/utils/platform'
import { fmtHudTime, friendlyPlaybackError } from '~/utils/playbackError'
import { proxyLogo } from '~/utils/premiumTv'

export interface ChannelEntry {
  id: string
  name: string
  logoUrl?: string | null
  streamUrl?: string | null
  group?: string
  quality?: string | null
}

const props = withDefaults(
  defineProps<{
    playing: boolean
    volume: number
    muted: boolean
    hasPrev: boolean
    hasNext: boolean
    /** `MpvPlayer`'s own chrome flag — the only reveal signal the overlay path has. */
    chromeUp?: boolean
    busy?: boolean
    channelName?: string
    /** What's on right now, when the page has a guide for this channel. */
    nowPlaying?: string
    channelLogo?: string
    channelIndex?: number
    channelTotal?: number
    channelList?: ChannelEntry[]
    isFavorite?: boolean
    isFullscreen?: boolean
    error?: string
    /** Decoded video resolution label from mpv (e.g. "4K UHD", "1080p"). */
    resolutionLabel?: string
    /** Quality label from the channel name (e.g. "4K", "FHD"). */
    sourceQuality?: string | null
    /** Available quality variants for the current channel. */
    qualityVariants?: ChannelEntry[]
    /** True while quality variants are being fetched. */
    qualityLoading?: boolean
    /** Current aspect-ratio mode: contain / cover / fill. */
    aspectRatio?: 'contain' | 'cover' | 'fill'
    /** Live channel zap UI, or on-demand (movies/series) with a progress clock. */
    variant?: 'live' | 'vod'
    /** VOD: elapsed seconds from the player. */
    position?: number
    /** VOD: total seconds from the player. */
    duration?: number
    /**
     * Playing from behind the live edge after a user pause/resume.
     * Shows the jump-to-live control. Not derived from `playing` — live
     * backends lie about pause.
     */
    behindLive?: boolean
  }>(),
  {
    chromeUp: false,
    busy: false,
    channelName: '',
    nowPlaying: '',
    channelLogo: '',
    channelIndex: 0,
    channelTotal: 0,
    channelList: () => [],
    isFavorite: false,
    isFullscreen: false,
    error: '',
    resolutionLabel: '',
    sourceQuality: null,
    qualityVariants: () => [],
    qualityLoading: false,
    aspectRatio: 'contain' as const,
    variant: 'live' as const,
    position: 0,
    duration: 0,
    behindLive: false,
  },
)

const emit = defineEmits<{
  togglePlay: []
  toggleMute: []
  setVolume: [volume: number]
  prev: []
  next: []
  back: []
  retry: []
  zapTo: [index: number]
  toggleFavorite: []
  toggleFullscreen: []
  showQualityPicker: []
  cycleAspectRatio: []
  goLive: []
}>()

/** Is mpv's picture in front of the page? Then holes to punch, and opaque bars. */
const overlay = hasVideoOverlay()

// ── Quality display ──────────────────────────────────────────────────
/** The decoded resolution from mpv, or the source quality label, for the badge. */
const qualityBadge = computed(() => props.resolutionLabel || props.sourceQuality || '')
/** Whether there are alternative quality variants to offer. */
const hasQualityVariants = computed(() => props.qualityVariants.length > 0)
/** User-facing label for the current aspect-ratio mode. */
const aspectLabel = computed(() =>
  props.aspectRatio === 'contain'
    ? $t('Fit')
    : props.aspectRatio === 'cover' ? $t('Center') : $t('Stretch'))

// ── Touch vs pointer detection (mirrors MpvPlayer's approach) ─────────
const coarsePointer = useMediaQuery('(pointer: coarse)')
const touch = computed(() => coarsePointer.value || isAndroid())
const isLiveVariant = computed(() => props.variant === 'live')
const friendlyErrorText = computed(() => friendlyPlaybackError(props.error))
const timeLine = computed(() => {
  if (isLiveVariant.value || !props.duration)
    return ''
  return `${fmtHudTime(props.position)} / ${fmtHudTime(props.duration)}`
})
const IDLE_MS = computed(() => touch.value ? 1500 : 2800)

/** Left-edge vertical drag = volume, right-edge = brightness. Touch only. */
const {
  hud: edgeHud,
  swiping: edgeSwiping,
  onDown: onEdgeDown,
  onMove: onEdgeMove,
  onUp: onEdgeUp,
  onTouchStart: onEdgeTouchStart,
} = usePlayerEdgeSwipe({
  enabled: () => touch.value && isTv() !== true,
  volume: () => props.volume,
  setVolume: n => emit('setVolume', n),
})

let skipCentreClick = false

// ── Chrome visibility ──────────────────────────────────────────────────
const showQuickZap = ref(false)
/** The cursor is on a bar — which is a hole, so this the DOM can see. */
const onBar = ref(false)

/**
 * Pinned up while a real panel is open, the pointer is physically on the
 * bars themselves, there is a full-screen error, or the user is behind
 * live and the jump control is up. Not `!playing` / `busy`: live backends
 * lie about pause, and pinning on those left the HUD up for the whole
 * channel.
 */
const pinned = computed(() =>
  showQuickZap.value || onBar.value || !!props.error || props.behindLive)

/**
 * A DOM event said the user is here. Worth keeping alongside `chromeUp`: where
 * the picture is *behind* the page (macOS, Android, a browser) mousemove lands
 * on this overlay and never on the player's own root, so its flag goes stale.
 */
const nudged = ref(true)
/**
 * One-shot manual hide via the eye icon. Any subsequent user activity
 * (mousemove, keypress, tap, click, the native pointer poll seeing a move,
 * or the eye button itself) clears this flag and returns to auto-hide.
 *
 * A short grace period defeats the race where the *same tick* that clicked
 * the eye icon also fires the window-level click listener or a mousemove —
 * both of which would otherwise call `show()` and instantly undo the hide.
 */
const forceHidden = ref(false)
let forceHiddenGraceUntil = 0
let hideTimer: ReturnType<typeof setTimeout> | null = null

const HIDE_GRACE_MS = 400

function show() {
  // A volume / brightness drag is not "the user is here, show the bars".
  if (edgeSwiping.value)
    return
  // If the user explicitly clicked the eye button very recently, swallow
  // this call. The real user action is "close the HUD"; spurious same-tick
  // window events and stale chromeUp transitions must not undo it.
  if (forceHidden.value && performance.now() < forceHiddenGraceUntil)
    return
  forceHidden.value = false
  nudged.value = true
  if (hideTimer)
    clearTimeout(hideTimer)
  hideTimer = null
  if (!pinned.value)
    hideTimer = setTimeout(() => (nudged.value = false), IDLE_MS.value)
}

function hide() {
  forceHidden.value = true
  forceHiddenGraceUntil = performance.now() + HIDE_GRACE_MS
  nudged.value = false
  if (hideTimer)
    clearTimeout(hideTimer)
  hideTimer = null
}

/**
 * visible = (something that genuinely must pin HUD up and cannot be
 * dismissed, because there is no other way to close it) OR
 * ((a soft pin / nudge / MpvPlayer activity flag says to show) AND the
 * user has not just clicked the eye icon).
 *
 * Hard pins (drawer open / error modal) ALWAYS win over forceHidden
 * because an open drawer with no chrome to close it would be a stuck
 * state. Soft pins (mouse just happens to be on a bar, recent DOM nudge,
 * MpvPlayer's mirrored chromeUp flag) all respect forceHidden.
 */
const drawerOrError = computed(() =>
  showQuickZap.value || !!props.error)

const softShow = computed(() =>
  onBar.value || nudged.value || props.chromeUp)

const visible = computed(() =>
  drawerOrError.value || (softShow.value && !forceHidden.value))

watch(
  () => props.chromeUp,
  (up, wasUp) => {
    if (up && !wasUp && !forceHidden.value)
      show()
  },
)

watch(
  () => props.behindLive,
  () => {
    show()
  },
)

watch(edgeSwiping, on => {
  if (!on)
    return
  nudged.value = false
  if (hideTimer)
    clearTimeout(hideTimer)
  hideTimer = null
})

// ── Native pointer poll for overlay platforms (Linux/Win32) ────────────
// On the platforms where mpv paints ABOVE the page, the DOM sees no
// mousemove/click/pointer events anywhere over the picture — it is a
// separate OS child window in front of the webview. The only way to learn
// that the user moved the cursor (so we can re-show the HUD) is to call
// the same Rust-side `player_pointer` getter `MpvPlayer.vue` uses and
// diff its coordinates ourselves. `chromeUp` is useless for this because
// the 16ms safety net in `poll()` permanently pins the parent's `ui=true`.
//
// Poll interval mirrors `MpvPlayer.vue`'s 200ms for `readPointer` — fast
// enough to catch every wiggle, slow enough not to tax the IPC.
let pointerPollHandle: ReturnType<typeof setInterval> | null = null
let lastPointerKey: string | null = null

async function pollNativePointer() {
  if (!overlay)
    return
  try {
    const pointer = await invoke<{ x: number, y: number, over: boolean } | null>('player_pointer')
    if (!pointer)
      return
    const key = `${pointer.x},${pointer.y},${pointer.over ? 1 : 0}`
    if (lastPointerKey != null && key !== lastPointerKey)
      show()
    lastPointerKey = key
  }
  catch {
    // Native bridge briefly unavailable or not a Tauri build — ignore.
  }
}

// ── Center Screen Canvas Transport Flash ─────────────────────────────
const showPlayFlash = ref(false)
let flashTimeout: ReturnType<typeof setTimeout> | null = null

function triggerCenterPlayPulse() {
  // On touch devices, a tap should first show the chrome if it is hidden,
  // matching the behaviour every other mobile player uses. Only toggle
  // play when the chrome is already visible (a second tap, or the initial
  // 1500ms window after playback starts).
  if (touch.value && !visible.value) {
    show()
    return
  }
  show()
  showPlayFlash.value = true
  if (flashTimeout)
    clearTimeout(flashTimeout)
  flashTimeout = setTimeout(() => {
    showPlayFlash.value = false
  }, 500)
  emit('togglePlay')
}

function onCentrePointerDown(e: PointerEvent) {
  skipCentreClick = false
  onEdgeDown(e)
}

function onCentrePointerMove(e: PointerEvent) {
  onEdgeMove(e)
}

function onCentrePointerUp(e: PointerEvent) {
  if (onEdgeUp(e))
    skipCentreClick = true
}

function onCentreClick() {
  if (skipCentreClick) {
    skipCentreClick = false
    return
  }
  if (isLiveVariant.value)
    triggerCenterPlayPulse()
}

function onCentreTouchStart(e: TouchEvent) {
  onEdgeTouchStart(e)
}

// ── Window-level activity tracking ────────────────────────────────────
// The overlay root has `pointer-events-none` (it spans the picture while
// the bars sit in `data-cut` holes), so a DOM `@mousemove` on it never
// fires. Instead we hook the window for every pointer-kind of activity
// and route it through `show()`. This way the overlay is self-contained
// and does not rely on the enclosing page adding its own listeners.
let lastMouseKey = ''
function onWindowMouseMove(e: MouseEvent) {
  if (touch.value)
    return
  const key = `${e.clientX},${e.clientY}`
  if (key !== lastMouseKey)
    show()
  lastMouseKey = key
}
function onWindowTouchStart() {
  if (touch.value)
    return
  show()
}
function onWindowClick() {
  show()
}
function onWindowPointerMove() {
  // Phones (and Android WebView reporting a finger as a mouse) must not
  // treat every move as "show the bars" — that is the volume / brightness swipe.
  if (touch.value)
    return
  show()
}

// ── QuickZap Drawer Logic ─────────────────────────────────────────────
const drawerSearch = ref('')

const filteredChannels = computed(() => {
  let list = props.channelList.map((ch, idx) => ({ ...ch, originalIndex: idx }))
  if (drawerSearch.value.trim()) {
    const q = drawerSearch.value.trim().toLowerCase()
    list = list.filter(ch => ch.name.toLowerCase().includes(q))
  }
  return list
})

function handleZapSelect(originalIndex: number) {
  emit('zapTo', originalIndex)
  showQuickZap.value = false
}

// ── Volume Helpers ───────────────────────────────────────────────────
function volumeIcon() {
  if (props.muted || props.volume === 0)
    return mdiVolumeOff
  if (props.volume < 33)
    return mdiVolumeLow
  if (props.volume < 67)
    return mdiVolumeMedium
  return mdiVolumeHigh
}

function onVolumeInput(e: Event) {
  const v = Number((e.target as HTMLInputElement).value)
  emit('setVolume', v)
}

// ── Keyboard Shortcuts ───────────────────────────────────────────────
// Escape/Backspace and the channel arrows belong to the page (`watch.vue`), so
// all this owns is the lineup drawer.
function onKeyDown(e: KeyboardEvent) {
  if (['INPUT', 'TEXTAREA'].includes((e.target as HTMLElement)?.tagName))
    return

  show()

  if (e.key.toLowerCase() === 'z')
    showQuickZap.value = !showQuickZap.value
  else if (e.key === 'Escape' && showQuickZap.value)
    showQuickZap.value = false
}

onMounted(() => {
  show()
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('mousemove', onWindowMouseMove)
  window.addEventListener('touchstart', onWindowTouchStart, { passive: true })
  window.addEventListener('pointermove', onWindowPointerMove, { passive: true })
  window.addEventListener('click', onWindowClick)
  if (overlay) {
    void pollNativePointer()
    pointerPollHandle = setInterval(pollNativePointer, 200)
  }
})
onUnmounted(() => {
  if (hideTimer)
    clearTimeout(hideTimer)
  if (flashTimeout)
    clearTimeout(flashTimeout)
  if (pointerPollHandle)
    clearInterval(pointerPollHandle)
  pointerPollHandle = null
  lastPointerKey = null
  window.removeEventListener('keydown', onKeyDown)
  window.removeEventListener('mousemove', onWindowMouseMove)
  window.removeEventListener('touchstart', onWindowTouchStart)
  window.removeEventListener('pointermove', onWindowPointerMove)
  window.removeEventListener('click', onWindowClick)
})

defineExpose({ show, hide, visible })
</script>

<template>
  <div
    class="pointer-events-none absolute inset-0 z-20 flex flex-col justify-between overflow-hidden font-sans select-none"
  >
    <!-- TOP HEADER: identity only. One way out (Back), one name, one quiet
         meta line. A second Close next to Hide duplicated Back and sat
         under the window controls. -->
    <header
      data-cut
      class="pointer-events-auto flex items-center gap-3 px-5 py-3 transition-transform duration-300 sm:px-6 sm:py-3.5"
      :class="[
        overlay ? 'hud-solid-top' : 'hud-blur-top',
        visible ? 'translate-y-0 opacity-100' : '-translate-y-full opacity-0',
      ]"
      @mouseenter="onBar = true"
      @mouseleave="onBar = false"
    >
      <button
        type="button"
        class="glass-icon-btn group shrink-0"
        :title="$t('Back')"
        :aria-label="$t('Back')"
        @click.stop="emit('back')"
      >
        <v-icon :icon="mdiArrowLeft" size="20" class="transition-transform group-hover:-translate-x-0.5 group-focus-visible:-translate-x-0.5" />
      </button>

      <div
        v-if="channelLogo"
        class="grid size-10 shrink-0 place-items-center overflow-hidden rounded-lg bg-white/10"
      >
        <img :src="proxyLogo(channelLogo)" alt="" class="size-full object-contain">
      </div>

      <div class="min-w-0 flex-1">
        <h1 class="truncate text-title-medium font-semibold tracking-tight text-white">
          {{ channelName }}
        </h1>
        <p class="mt-0.5 flex min-w-0 items-center gap-2 text-label-small text-white/55">
          <template v-if="isLiveVariant">
            <button
              v-if="behindLive"
              type="button"
              class="live-jump inline-flex shrink-0 items-center gap-1.5 rounded-md px-2 py-0.5 font-bold tracking-wide"
              :title="$t('Go live')"
              :aria-label="$t('Go live')"
              tabindex="-1"
              @click.stop="emit('goLive')"
            >
              <span class="size-1.5 animate-pulse rounded-full bg-white" aria-hidden="true" />
              {{ $t('LIVE') }}
            </button>
            <span
              v-else
              class="inline-flex shrink-0 items-center gap-1.5 font-semibold tracking-wide text-red-400"
            >
              <span class="size-1.5 rounded-full bg-red-500" aria-hidden="true" />
              {{ $t('LIVE') }}
            </span>
            <span v-if="qualityBadge" class="shrink-0 tabular-nums text-white/45">{{ qualityBadge }}</span>
            <span v-if="channelTotal > 0" class="shrink-0 tabular-nums text-white/45">{{ channelIndex + 1 }}/{{ channelTotal }}</span>
            <span v-if="nowPlaying" class="min-w-0 truncate text-white/55">{{ nowPlaying }}</span>
          </template>
          <span v-else-if="timeLine" class="shrink-0 font-mono tabular-nums text-white/70">{{ timeLine }}</span>
        </p>
      </div>

      <button
        type="button"
        class="glass-icon-btn shrink-0"
        :title="$t('Hide controls')"
        :aria-label="$t('Hide controls')"
        @click.stop="hide"
      >
        <v-icon :icon="mdiEyeOffOutline" size="20" />
      </button>
    </header>

    <!-- Centre: tap to pause, where the page gets the tap at all. Deliberately
         *not* `data-cut`: it spans the whole picture, and a hole that size
         subtracts every pixel mpv paints. -->
    <div
      class="flex-1 grid place-items-center touch-none bg-black/[0.01]"
      :class="isLiveVariant ? 'pointer-events-auto cursor-pointer' : 'pointer-events-none'"
      @click="onCentreClick"
      @pointerdown="onCentrePointerDown"
      @pointermove="onCentrePointerMove"
      @pointerup="onCentrePointerUp"
      @pointercancel="onCentrePointerUp"
      @touchstart.stop="onCentreTouchStart"
    >
      <transition
        enter-active-class="transition duration-200 ease-out transform scale-50 opacity-0"
        enter-to-class="scale-100 opacity-100"
        leave-active-class="transition duration-300 ease-in transform scale-100 opacity-100"
        leave-to-class="scale-125 opacity-0"
      >
        <div
          v-if="showPlayFlash"
          class="size-20 rounded-full bg-black/70 border border-white/25 text-white grid place-items-center backdrop-blur-md shadow-2xl shadow-black/80"
        >
          <v-icon :icon="playing ? mdiPause : mdiPlay" size="44" />
        </div>
      </transition>
    </div>

    <player-edge-hud v-if="edgeHud" :kind="edgeHud.kind" :level="edgeHud.level" :caption="edgeHud.caption" />

    <!-- CENTER PLAYER ERROR MODAL -->
    <transition
      enter-active-class="transition ease-out duration-200"
      enter-from-class="opacity-0 scale-95"
      enter-to-class="opacity-100 scale-100"
      leave-active-class="transition ease-in duration-150"
      leave-from-class="opacity-100 scale-100"
      leave-to-class="opacity-0 scale-95"
    >
      <div
        v-if="error"
        data-cut
        class="pointer-events-auto absolute inset-0 z-30 grid place-items-center p-6"
        :class="overlay ? 'bg-black' : 'bg-black/85 backdrop-blur-xl'"
      >
        <div class="flex max-w-md flex-col items-center space-y-4 p-6 text-center">
          <div class="grid size-14 place-items-center rounded-full bg-red-950 text-red-400">
            <v-icon :icon="mdiAlertCircleOutline" size="32" />
          </div>

          <div class="space-y-1">
            <h2 class="text-title-small font-semibold text-white">
              {{ $t('Playback Error') }}
            </h2>
            <p class="text-body-small leading-relaxed text-white/70">
              {{ friendlyErrorText }}
            </p>
          </div>

          <div class="flex flex-wrap items-center justify-center gap-2 pt-1">
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-xl bg-primary px-4 py-2.5 text-body-small font-semibold text-on-primary transition-colors hover:brightness-110 focus-visible:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white"
              @click.stop="emit('retry')"
            >
              <v-icon :icon="mdiReload" size="16" />
              <span>{{ $t('Retry') }}</span>
            </button>
            <button
              v-if="hasNext && isLiveVariant"
              type="button"
              class="rounded-xl bg-white/10 px-4 py-2.5 text-body-small font-semibold text-white transition-colors hover:bg-white/16 focus-visible:bg-white/16 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white"
              @click.stop="emit('next')"
            >
              {{ $t('Next channel') }}
            </button>
            <button
              type="button"
              class="rounded-xl bg-white/10 px-4 py-2.5 text-body-small font-semibold text-white/80 transition-colors hover:bg-white/16 hover:text-white focus-visible:bg-white/16 focus-visible:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white"
              @click.stop="emit('back')"
            >
              {{ $t('Back') }}
            </button>
          </div>
        </div>
      </div>
    </transition>

    <!-- QUICKZAP SIDE DRAWER (SLIDE-IN FROM LEFT) -->
    <transition
      enter-active-class="transition transform duration-300 ease-out"
      enter-from-class="-translate-x-full"
      enter-to-class="translate-x-0"
      leave-active-class="transition transform duration-250 ease-in"
      leave-from-class="translate-x-0"
      leave-to-class="-translate-x-full"
    >
      <div
        v-if="showQuickZap"
        data-cut
        class="pointer-events-auto absolute inset-y-0 left-0 z-40 w-80 max-w-[85vw] flex flex-col border-r border-white/10 shadow-2xl p-4"
        :class="overlay ? 'bg-[#0F1117]' : 'bg-[#0F1117]/92 backdrop-blur-2xl'"
      >
        <!-- Header -->
        <div class="flex items-center justify-between border-b border-white/10 pb-3">
          <h2 class="text-title-small font-semibold text-white">
            {{ $t('Channels') }}
          </h2>
          <button type="button" class="glass-icon-btn !size-8" :title="$t('Close')" :aria-label="$t('Close')" @click="showQuickZap = false">
            <v-icon :icon="mdiClose" size="16" />
          </button>
        </div>

        <!-- Search Bar -->
        <div class="relative my-3">
          <v-icon :icon="mdiMagnify" size="16" class="absolute left-3 top-2.5 text-gray-400" />
          <input
            v-model="drawerSearch"
            type="text"
            :placeholder="$t('Search channels')"
            class="w-full pl-9 pr-3 py-1.5 bg-white/5 border border-white/10 rounded-xl text-xs text-white placeholder-gray-500 outline-none focus:border-red-500 transition-colors"
          >
        </div>

        <!-- Channel Scroll List -->
        <div class="flex-1 overflow-y-auto space-y-1.5 pr-1">
          <button
            v-for="ch in filteredChannels"
            :key="ch.id"
            type="button"
            class="w-full flex items-center gap-3 p-2.5 rounded-xl border text-left transition-colors group hover:bg-white/10 focus-visible:bg-white/10"
            :class="ch.originalIndex === channelIndex
              ? 'bg-red-600/20 border-red-500/50 text-white border-l-4 border-l-red-500'
              : 'bg-white/5 border-white/5 text-gray-300 hover:text-white focus-visible:text-white'"
            @click="handleZapSelect(ch.originalIndex)"
          >
            <span class="w-6 text-[11px] font-bold text-gray-400 text-center">
              {{ ch.originalIndex + 1 }}
            </span>
            <div v-if="ch.logoUrl" class="size-8 shrink-0 rounded-lg bg-black/40 border border-white/10 p-0.5 overflow-hidden grid place-items-center">
              <img :src="proxyLogo(ch.logoUrl)" :alt="ch.name" class="size-full object-contain">
            </div>
            <div class="min-w-0 flex-1">
              <span class="block text-xs font-semibold truncate">{{ ch.name }}</span>
              <span v-if="ch.group" class="block text-[10px] text-gray-400 truncate">{{ ch.group }}</span>
            </div>
          </button>
        </div>
      </div>
    </transition>

    <!-- BOTTOM CONTROL BAR — live channels only. VOD uses MpvPlayer's seek bar. -->
    <footer
      v-if="isLiveVariant"
      data-cut
      class="pointer-events-auto px-5 pb-5 pt-8 transition-transform duration-300 sm:px-6 sm:pb-6"
      :class="[
        overlay ? 'hud-solid-bottom' : 'hud-blur-bottom',
        visible ? 'translate-y-0 opacity-100' : 'translate-y-full opacity-0',
      ]"
      @mouseenter="onBar = true"
      @mouseleave="onBar = false"
    >
      <div class="flex items-center justify-between gap-4">
        <div class="flex items-center gap-2" data-dpad-start>
          <button
            type="button"
            class="glass-icon-btn"
            :disabled="!hasPrev"
            :title="$t('Previous channel')"
            :aria-label="$t('Previous channel')"
            @click.stop="emit('prev')"
          >
            <v-icon :icon="mdiSkipPrevious" size="22" />
          </button>

          <button
            type="button"
            class="grid size-12 shrink-0 place-items-center rounded-full bg-primary text-on-primary transition-colors hover:brightness-110 focus-visible:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white"
            :title="playing ? $t('Pause') : $t('Play')"
            :aria-label="playing ? $t('Pause') : $t('Play')"
            @click.stop="emit('togglePlay')"
          >
            <v-icon :icon="playing ? mdiPause : mdiPlay" size="26" />
          </button>

          <button
            type="button"
            class="glass-icon-btn"
            :disabled="!hasNext"
            :title="$t('Next channel')"
            :aria-label="$t('Next channel')"
            @click.stop="emit('next')"
          >
            <v-icon :icon="mdiSkipNext" size="22" />
          </button>

          <button
            v-if="behindLive"
            type="button"
            class="live-jump inline-flex h-12 shrink-0 items-center gap-1.5 rounded-full px-3.5 text-label-medium font-bold tracking-wide"
            :title="$t('Go live')"
            :aria-label="$t('Go live')"
            @click.stop="emit('goLive')"
          >
            <span class="size-2 animate-pulse rounded-full bg-white" aria-hidden="true" />
            {{ $t('LIVE') }}
          </button>

          <span v-if="busy" class="ms-1 text-label-small font-medium text-white/50">
            {{ $t('Buffering…') }}
          </span>
        </div>

        <div class="flex items-center gap-1.5 sm:gap-2">
          <div class="flex items-center gap-1.5">
            <button
              type="button"
              class="glass-icon-btn"
              :title="muted ? $t('Unmute') : $t('Mute')"
              :aria-label="muted ? $t('Unmute') : $t('Mute')"
              @click.stop="emit('toggleMute')"
            >
              <v-icon :icon="volumeIcon()" size="20" />
            </button>
            <template v-if="!touch">
              <input
                type="range"
                min="0"
                max="100"
                :value="muted ? 0 : volume"
                class="custom-slider w-20 cursor-pointer sm:w-24"
                @input="onVolumeInput"
              >
              <span class="w-7 text-end font-mono text-label-small tabular-nums text-white/50">
                {{ muted ? 0 : volume }}
              </span>
            </template>
          </div>

          <span class="mx-1 hidden h-5 w-px bg-white/12 sm:block" aria-hidden="true" />

          <button
            type="button"
            class="glass-icon-btn"
            :class="{ 'border-primary/50 bg-primary/25 text-white': showQuickZap }"
            :title="$t('Channels')"
            :aria-label="$t('Channels')"
            @click.stop="showQuickZap = !showQuickZap"
          >
            <v-icon :icon="mdiFormatListBulleted" size="20" />
          </button>

          <button
            v-if="hasQualityVariants"
            type="button"
            class="glass-icon-btn"
            :title="$t('Quality')"
            :aria-label="$t('Quality')"
            @click.stop="emit('showQualityPicker')"
          >
            <v-icon :icon="mdiMonitor" size="20" />
          </button>

          <button
            type="button"
            class="glass-icon-btn"
            :class="{ 'text-amber-400': isFavorite }"
            :title="isFavorite ? $t('Remove from favourites') : $t('Add to favourites')"
            :aria-label="isFavorite ? $t('Remove from favourites') : $t('Add to favourites')"
            @click.stop="emit('toggleFavorite')"
          >
            <v-icon :icon="isFavorite ? mdiStar : mdiStarOutline" size="20" />
          </button>

          <button
            type="button"
            class="glass-icon-btn"
            :title="aspectLabel"
            :aria-label="aspectLabel"
            @click.stop="emit('cycleAspectRatio')"
          >
            <v-icon :icon="mdiCropFree" size="20" />
          </button>

          <button
            type="button"
            class="glass-icon-btn"
            :title="isFullscreen ? $t('Exit Fullscreen') : $t('Fullscreen')"
            :aria-label="isFullscreen ? $t('Exit Fullscreen') : $t('Fullscreen')"
            @click.stop="emit('toggleFullscreen')"
          >
            <v-icon :icon="isFullscreen ? mdiFullscreenExit : mdiFullscreen" size="20" />
          </button>
        </div>
      </div>
    </footer>
  </div>
</template>

<style scoped>
/*
 * Two variants of each bar, because these are `data-cut` surfaces. Where mpv
 * paints over the page the bar sits in a hole punched out of mpv's window:
 * a gradient to `transparent` would show the page's black there, and
 * `backdrop-filter` would have nothing behind it to read. So `overlay` gets a
 * flat opaque fill, and only the composited platforms get the glass.
 */
.hud-solid-top,
.hud-solid-bottom {
  background: #0f1117;
}

.hud-blur-top {
  background: linear-gradient(180deg, rgba(15, 17, 23, 0.95) 0%, rgba(15, 17, 23, 0.6) 70%, transparent 100%);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
}

.hud-blur-bottom {
  background: linear-gradient(0deg, rgba(15, 17, 23, 0.95) 0%, rgba(15, 17, 23, 0.6) 75%, transparent 100%);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
}

.glass-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 42px;
  height: 42px;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.12);
  color: #f3f4f6;
  transition: transform 180ms ease, opacity 180ms ease;
  cursor: pointer;
}

/* A remote has no hover, so focus has to say the same thing. */
.glass-icon-btn:hover,
.glass-icon-btn:focus-visible {
  background: rgba(255, 255, 255, 0.16);
  border-color: rgba(255, 255, 255, 0.22);
  color: #ffffff;
}

.glass-icon-btn:focus-visible {
  outline: 2px solid #ffffff;
  outline-offset: 2px;
}

.glass-icon-btn:active {
  transform: scale(0.95);
}

.glass-icon-btn:disabled {
  opacity: 0.35;
  pointer-events: none;
}

.live-jump {
  background: #dc2626;
  color: #fff;
  cursor: pointer;
}

.live-jump:hover,
.live-jump:focus-visible {
  background: #ef4444;
}

.live-jump:focus-visible {
  outline: 2px solid #ffffff;
  outline-offset: 2px;
}

/* Custom Volume Slider Styling */
input[type="range"].custom-slider {
  -webkit-appearance: none;
  appearance: none;
  background: rgba(255, 255, 255, 0.2);
  border-radius: 9999px;
  height: 4px;
  outline: none;
}

input[type="range"].custom-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: #E50914;
  cursor: pointer;
  transition: transform 150ms ease;
}

input[type="range"].custom-slider::-webkit-slider-thumb:hover {
  transform: scale(1.25);
}

input[type="range"].custom-slider:focus-visible {
  outline: 2px solid #ffffff;
  outline-offset: 2px;
}
</style>
