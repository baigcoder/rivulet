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
 * Nothing here invents a number. The free playlists have no EPG yet
 * (`iptv/epg.rs`), a live window has nothing to seek over, and the player
 * reports no bitrate or frame rate — so there is no scrubber, no programme name
 * and no telemetry panel until something real can fill them.
 */
import {
  mdiAlertCircleOutline,
  mdiArrowLeft,
  mdiClose,
  mdiEyeOffOutline,
  mdiFormatListBulleted,
  mdiFullscreen,
  mdiFullscreenExit,
  mdiMagnify,
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
import { isAndroid } from '~/utils/platform'

export interface ChannelEntry {
  id: string
  name: string
  logoUrl?: string | null
  streamUrl?: string | null
  group?: string
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
    channelLogo?: string
    channelIndex?: number
    channelTotal?: number
    channelList?: ChannelEntry[]
    isFavorite?: boolean
    isFullscreen?: boolean
    error?: string
  }>(),
  {
    chromeUp: false,
    busy: false,
    channelName: '',
    channelLogo: '',
    channelIndex: 0,
    channelTotal: 0,
    channelList: () => [],
    isFavorite: false,
    isFullscreen: false,
    error: '',
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
}>()

/** Is mpv's picture in front of the page? Then holes to punch, and opaque bars. */
const overlay = hasVideoOverlay()

// ── Touch vs pointer detection (mirrors MpvPlayer's approach) ─────────
const coarsePointer = useMediaQuery('(pointer: coarse)')
const touch = computed(() => coarsePointer.value || isAndroid())
const IDLE_MS = computed(() => touch.value ? 1500 : 2800)

// ── Chrome visibility ──────────────────────────────────────────────────
const showQuickZap = ref(false)
/** The cursor is on a bar — which is a hole, so this the DOM can see. */
const onBar = ref(false)

/**
 * Pinned up while a real panel is open, the pointer is physically on the
 * bars themselves, or there is a full-screen error covering the player.
 *
 * Deliberately does NOT include `!playing` or `busy`: live backends often
 * report `paused=true` (the red Play icon) even while rendering a fresh
 * frame, and HLS live buffers every few seconds. Either would pin the HUD
 * visible permanently — which is exactly the bug. Hide-after-idle is the
 * whole point, and the user can always bring it back with a mouse move /
 * tap / keypress, so there is no such thing as "lost controls".
 */
const pinned = computed(() =>
  showQuickZap.value || onBar.value || !!props.error)

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
  show()
}
function onWindowClick() {
  show()
}
function onWindowPointerMove(e: PointerEvent) {
  // Pointer events carry a hardware-specific pointerType, so there are no
  // synthetic "touch → mouse" fallouts here like the legacy mousemove path
  // above. Real mouse moves on a phone with a Bluetooth mouse, and pen
  // input on a tablet, both deserve the same treatment as a desktop mouse.
  // Touch pointer moves are also fine — a finger dragging across the film
  // is clearly someone at the controls.
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
    <!-- TOP HEADER: channel identity. Opaque, because it sits in a hole. -->
    <header
      data-cut
      class="pointer-events-auto flex items-center justify-between px-8 py-5 transition-all duration-300"
      :class="[
        overlay ? 'hud-solid-top' : 'hud-blur-top',
        visible ? 'translate-y-0 opacity-100' : '-translate-y-full opacity-0',
      ]"
      @mouseenter="onBar = true"
      @mouseleave="onBar = false"
    >
      <!-- Left: Stream Meta Context -->
      <div class="flex items-center gap-3.5">
        <!-- Back Button -->
        <button
          type="button"
          class="glass-icon-btn group"
          :title="$t('Back')"
          @click.stop="emit('back')"
        >
          <v-icon :icon="mdiArrowLeft" size="20" class="transition-transform group-hover:-translate-x-0.5" />
        </button>

        <!-- Channel Logo -->
        <div
          v-if="channelLogo"
          class="grid size-11 shrink-0 place-items-center overflow-hidden rounded-xl bg-white/10 p-1 border border-white/15"
        >
          <img :src="channelLogo" :alt="channelName" class="size-full object-contain">
        </div>

        <!-- Stream Info Stack -->
        <div class="flex flex-col min-w-0">
          <div class="flex items-center gap-2">
            <!-- LIVE Pulse Badge -->
            <div class="flex items-center gap-1.5 px-2.5 py-0.5 rounded-full bg-red-950 border border-red-500/50">
              <span class="size-2 rounded-full bg-red-500 animate-pulse" />
              <span class="text-[10px] font-extrabold tracking-wider text-red-400 uppercase">{{ $t('LIVE') }}</span>
            </div>

            <!-- Channel Number -->
            <span v-if="channelTotal > 0" class="text-xs font-semibold text-gray-300 bg-white/10 px-2 py-0.5 rounded-md border border-white/10">
              {{ channelIndex + 1 }}/{{ channelTotal }}
            </span>
          </div>

          <!-- Channel Name -->
          <h1 class="mt-1 text-lg font-bold text-white tracking-wide truncate">
            {{ channelName }}
          </h1>
        </div>
      </div>

      <!-- Right: Quick Action Controls -->
      <div class="flex items-center gap-2.5">
        <!-- Manual Hide: eye-off — one-click instant HUD dismiss. -->
        <button
          type="button"
          class="glass-icon-btn"
          :title="$t('Hide controls')"
          aria-label="$t('Hide controls')"
          @click.stop="hide"
        >
          <v-icon :icon="mdiEyeOffOutline" size="20" />
        </button>
        <!-- Close / Exit Button -->
        <button
          type="button"
          class="glass-icon-btn hover:bg-red-600/80 hover:border-red-500 focus-visible:bg-red-600/80 focus-visible:border-red-500"
          :title="$t('Close')"
          @click.stop="emit('back')"
        >
          <v-icon :icon="mdiClose" size="20" />
        </button>
      </div>
    </header>

    <!-- Centre: tap to pause, where the page gets the tap at all. Deliberately
         *not* `data-cut`: it spans the whole picture, and a hole that size
         subtracts every pixel mpv paints. -->
    <div
      class="pointer-events-auto flex-1 grid place-items-center cursor-pointer"
      @click="triggerCenterPlayPulse"
      @touchstart.passive="show"
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
        <div class="flex flex-col items-center text-center max-w-md p-6 rounded-2xl bg-[#0F1117] border border-white/10 shadow-2xl space-y-4">
          <div class="size-14 rounded-full bg-red-950 border border-red-500/40 text-red-500 grid place-items-center">
            <v-icon :icon="mdiAlertCircleOutline" size="32" />
          </div>

          <div class="space-y-1">
            <h2 class="text-base font-bold text-white tracking-wide uppercase">
              {{ $t('Playback Error') }}
            </h2>
            <p class="text-xs text-gray-300 leading-relaxed">
              {{ error }}
            </p>
          </div>

          <div class="flex items-center gap-3 pt-2">
            <button
              type="button"
              class="px-4 py-2 rounded-xl bg-red-600 hover:bg-red-500 focus-visible:bg-red-500 text-white text-xs font-bold transition-all shadow-md flex items-center gap-1.5"
              @click.stop="emit('retry')"
            >
              <v-icon :icon="mdiReload" size="14" />
              <span>{{ $t('Retry') }}</span>
            </button>
            <button
              v-if="hasNext"
              type="button"
              class="px-4 py-2 rounded-xl bg-white/10 hover:bg-white/20 focus-visible:bg-white/20 text-white text-xs font-semibold transition-all border border-white/10"
              @click.stop="emit('next')"
            >
              {{ $t('Next channel') }}
            </button>
            <button
              type="button"
              class="px-4 py-2 rounded-xl bg-white/5 hover:bg-white/10 focus-visible:bg-white/10 text-gray-300 text-xs font-semibold transition-all border border-white/5"
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
        <div class="flex items-center justify-between pb-3 border-b border-white/10">
          <div class="flex items-center gap-2">
            <v-icon :icon="mdiFormatListBulleted" size="20" class="text-red-500" />
            <h2 class="text-sm font-bold text-white tracking-wide uppercase">
              {{ $t('Channels') }}
            </h2>
          </div>
          <button type="button" class="glass-icon-btn size-7" :title="$t('Close')" @click="showQuickZap = false">
            <v-icon :icon="mdiClose" size="14" />
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
            class="w-full flex items-center gap-3 p-2.5 rounded-xl border text-left transition-all group hover:bg-white/10 focus-visible:bg-white/10"
            :class="ch.originalIndex === channelIndex
              ? 'bg-red-600/20 border-red-500/50 text-white border-l-4 border-l-red-500'
              : 'bg-white/5 border-white/5 text-gray-300 hover:text-white focus-visible:text-white'"
            @click="handleZapSelect(ch.originalIndex)"
          >
            <span class="w-6 text-[11px] font-bold text-gray-400 text-center">
              {{ ch.originalIndex + 1 }}
            </span>
            <div v-if="ch.logoUrl" class="size-8 shrink-0 rounded-lg bg-black/40 border border-white/10 p-0.5 overflow-hidden grid place-items-center">
              <img :src="ch.logoUrl" :alt="ch.name" class="size-full object-contain">
            </div>
            <div class="min-w-0 flex-1">
              <span class="block text-xs font-semibold truncate">{{ ch.name }}</span>
              <span v-if="ch.group" class="block text-[10px] text-gray-400 truncate">{{ ch.group }}</span>
            </div>
          </button>
        </div>
      </div>
    </transition>

    <!-- BOTTOM CONTROL BAR. Opaque under an overlay player, same as the header.
         No scrubber: a live window has nothing to seek over, and the playlists
         carry no EPG to draw a programme against. -->
    <footer
      data-cut
      class="pointer-events-auto px-8 pb-7 pt-12 transition-all duration-300 flex flex-col gap-4"
      :class="[
        overlay ? 'hud-solid-bottom' : 'hud-blur-bottom',
        visible ? 'translate-y-0 opacity-100' : 'translate-y-full opacity-0',
      ]"
      @mouseenter="onBar = true"
      @mouseleave="onBar = false"
    >
      <!-- MAIN MEDIA CONTROL DOCK -->
      <div class="flex items-center justify-between">
        <!-- Left Group: Playback & Zapping Navigation -->
        <div class="flex items-center gap-3">
          <!-- Large Accent Play/Pause Glass Button -->
          <button
            type="button"
            class="size-12 rounded-full bg-red-600 hover:bg-red-500 focus-visible:bg-red-500 text-white grid place-items-center transition-all transform hover:scale-105 focus-visible:scale-105 active:scale-95"
            :title="playing ? $t('Pause') : $t('Play')"
            @click.stop="emit('togglePlay')"
          >
            <v-icon :icon="playing ? mdiPause : mdiPlay" size="26" />
          </button>

          <!-- Channel Zapping: Prev -->
          <button
            type="button"
            class="glass-icon-btn"
            :disabled="!hasPrev"
            :title="$t('Previous channel')"
            @click.stop="emit('prev')"
          >
            <v-icon :icon="mdiSkipPrevious" size="22" />
          </button>

          <!-- Channel Zapping: Next -->
          <button
            type="button"
            class="glass-icon-btn"
            :disabled="!hasNext"
            :title="$t('Next channel')"
            @click.stop="emit('next')"
          >
            <v-icon :icon="mdiSkipNext" size="22" />
          </button>

          <!-- The one piece of live state there is: is it filling the cache? -->
          <span v-if="busy" class="ml-2 text-xs font-medium text-gray-300">
            {{ $t('Buffering…') }}
          </span>
        </div>

        <!-- Right Group: Volume & Utility Overlays -->
        <div class="flex items-center gap-3">
          <!-- Dynamic Expandable Volume Module -->
          <div class="flex items-center gap-2.5">
            <button
              type="button"
              class="glass-icon-btn"
              :title="muted ? $t('Unmute') : $t('Mute')"
              @click.stop="emit('toggleMute')"
            >
              <v-icon :icon="volumeIcon()" size="20" />
            </button>
            <input
              type="range"
              min="0"
              max="100"
              :value="muted ? 0 : volume"
              class="custom-slider w-24 cursor-pointer"
              @input="onVolumeInput"
            >
            <span class="text-xs font-mono text-gray-300 w-6 text-right tabular-nums font-semibold">
              {{ muted ? 0 : volume }}
            </span>
          </div>

          <!-- QuickZap Drawer Toggle -->
          <button
            type="button"
            class="glass-icon-btn"
            :class="{ 'bg-red-600/30 border-red-500/50 text-white': showQuickZap }"
            :title="$t('Channels')"
            @click.stop="showQuickZap = !showQuickZap"
          >
            <v-icon :icon="mdiFormatListBulleted" size="20" />
          </button>

          <!-- Favorite Star Toggle -->
          <button
            type="button"
            class="glass-icon-btn"
            :class="{ 'text-amber-400': isFavorite }"
            :title="isFavorite ? $t('Remove from favourites') : $t('Add to favourites')"
            @click.stop="emit('toggleFavorite')"
          >
            <v-icon :icon="isFavorite ? mdiStar : mdiStarOutline" size="20" />
          </button>

          <!-- Fullscreen Toggle -->
          <button
            type="button"
            class="glass-icon-btn"
            :title="isFullscreen ? $t('Exit Fullscreen') : $t('Fullscreen')"
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
  transition: all 180ms ease;
  cursor: pointer;
}

/* A remote has no hover, so focus has to say the same thing. */
.glass-icon-btn:hover,
.glass-icon-btn:focus-visible {
  background: rgba(255, 255, 255, 0.18);
  border-color: rgba(255, 255, 255, 0.25);
  color: #ffffff;
  transform: translateY(-1px);
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.4);
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
