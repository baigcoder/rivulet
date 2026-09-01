<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { mdiPlay, mdiStar } from '@mdi/js'

const props = defineProps<{
  channel: LiveChannel
  getEpg: (id: string) => Array<{ title: string, description?: string | null, start: string, stop?: string | null }>
  isFavorite: (ch: LiveChannel) => boolean
  compact?: boolean
  /**
   * A probe found this channel closed. Advisory, never a gate: a public
   * playlist's servers come back, and a verdict is at most a few minutes
   * old — so the card dims and says so, and the click still works.
   */
  isOffline?: (ch: LiveChannel) => boolean
}>()

const emit = defineEmits<{
  play: [channel: LiveChannel]
  toggleFavorite: [channel: LiveChannel]
}>()

const epg = computed(() => props.getEpg(props.channel.id))
const nowProgram = computed(() => epg.value[0] ?? null)
const fav = computed(() => props.isFavorite(props.channel))
const offline = computed(() => props.isOffline?.(props.channel) === true)
const imgError = ref(false)
const imgLoaded = ref(false)

// A channel with no stream URL cannot be played.
const hasStream = computed(() => {
  const s = props.channel.streamUrl
  return !!s && s !== 'undefined' && s !== 'null'
})

// Only start the EPG timer when the card actually has EPG data to avoid
// thousands of idle intervals clogging the event loop.
const nowMs = ref(Date.now())
let progressTimer: ReturnType<typeof setInterval> | undefined
watch(nowProgram, prog => {
  if (prog && !progressTimer) {
    progressTimer = setInterval(() => {
      nowMs.value = Date.now()
    }, 60_000)
  }
  else if (!prog && progressTimer) {
    clearInterval(progressTimer)
    progressTimer = undefined
  }
}, { immediate: true })
onUnmounted(() => {
  if (progressTimer !== undefined)
    clearInterval(progressTimer)
})

const epgProgress = computed(() => {
  if (!nowProgram.value?.start)
    return 0
  const start = new Date(nowProgram.value.start).getTime()
  const end = nowProgram.value.stop ? new Date(nowProgram.value.stop).getTime() : start + 3600000
  const now = nowMs.value
  if (now < start)
    return 0
  if (now > end)
    return 100
  return Math.round(((now - start) / (end - start)) * 100)
})

const cleanedName = computed(() => {
  let name = props.channel.name || ''
  // Strip leading channel numbers like 001:, 002:, 003 -
  name = name.replace(/^\d+[:\-\s]+/, '')
  // Strip event status prefixes like "THE EVENT HAS NOT BEGUN ::"
  name = name.replace(/^(the event has not begun|event has not begun)\s*:*\s*/i, '')
  // Strip IPTV prefixes like (FLSP 002):, TR:, USA:, UK:, LV:, [1080p], (TN), etc.
  name = name.replace(/^\([\w\-\s]+\):?\s*/, '')
  name = name.replace(/^[\w\-]{2,6}:\s*/, '')
  name = name.replace(/\[[^\]]+\]/g, '')
  name = name.replace(/\([^)]+\)/g, '')
  return name.trim() || props.channel.name.trim()
})

const channelInitials = computed(() => {
  const clean = cleanedName.value.replace(/[^a-z0-9\s]/gi, ' ').trim()
  const words = clean
    .split(/\s+/)
    .filter(w => w.length > 0 && !/^(?:us|uk|ca|au|fr|de|tr|hd|sd|fhd|4k|tv|channel|live)$/i.test(w))

  const first = words[0] || clean
  const second = words[1]

  if (first && second && first[0] && second[0]) {
    return (first[0] + second[0]).toUpperCase()
  }
  const str = first || clean || 'TV'
  return str.slice(0, 2).toUpperCase()
})

const cardGradient = computed(() => {
  const gradients = [
    'from-indigo-950 via-purple-900/70 to-slate-950',
    'from-blue-950 via-cyan-900/70 to-slate-950',
    'from-emerald-950 via-teal-900/70 to-slate-950',
    'from-rose-950 via-pink-900/70 to-slate-950',
    'from-amber-950 via-orange-900/70 to-slate-950',
    'from-violet-950 via-purple-950/70 to-slate-950',
    'from-fuchsia-950 via-rose-950/70 to-slate-950',
    'from-teal-950 via-emerald-950/70 to-slate-950',
  ]
  let hash = 0
  const str = props.channel.name || props.channel.id
  for (let i = 0; i < str.length; i++) {
    hash = (hash << 5) - hash + str.charCodeAt(i)
    hash |= 0
  }
  return gradients[Math.abs(hash) % gradients.length]
})

/** Whether this channel has a usable logo URL (not empty, not a known placeholder). */
const hasLogo = computed(() => {
  const url = props.channel.logoUrl
  if (!url)
    return false
  // Filter out common empty/broken patterns
  if (url === 'null' || url === 'undefined' || url === '')
    return false
  if (url.length < 8)
    return false
  return true
})
</script>

<template>
  <!--
    A real `button`, not a div with a click handler: that is what gives a
    remote a focus ring, Enter and Space without a keydown handler, and
    `disabled` for a channel the playlist gave no URL. Every `hover:`
    below has a `focus-visible:` twin for the same reason.
  -->
  <button
    type="button"
    class="group relative flex flex-col overflow-hidden rounded-2xl bg-surface-container-high text-start ring-1 ring-white/6 transition-[transform,box-shadow,ring-color] duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    :class="[
      hasStream
        ? 'cursor-pointer hover:-translate-y-0.5 hover:ring-primary/50 hover:shadow-xl hover:shadow-primary/10 focus-visible:-translate-y-0.5'
        : 'cursor-not-allowed opacity-40',
      offline ? 'opacity-55' : '',
    ]"
    :disabled="!hasStream"
    :aria-label="cleanedName"
    @click="emit('play', channel)"
  >
    <!-- Logo viewport -->
    <div class="relative aspect-video w-full overflow-hidden bg-surface-container">
      <!-- Real channel logo image -->
      <img
        v-if="hasLogo && !imgError"
        :src="channel.logoUrl!"
        :alt="channel.name"
        loading="lazy"
        decoding="async"
        class="size-full object-contain p-3 transition-opacity duration-300"
        :class="imgLoaded ? 'opacity-100' : 'opacity-0'"
        @load="imgLoaded = true"
        @error="imgError = true"
      >

      <!-- Styled Fallback Artwork Badge (shown when no logo or logo failed) -->
      <div
        v-if="!hasLogo || imgError"
        class="relative flex size-full items-center justify-center overflow-hidden bg-gradient-to-br"
        :class="cardGradient"
      >
        <!-- Subtle radial glow background -->
        <div class="absolute inset-0 bg-[radial-gradient(circle_at_center,rgba(255,255,255,0.08),transparent_70%)] pointer-events-none" />

        <!-- Centered Channel Emblem Badge -->
        <div class="relative z-10 flex flex-col items-center justify-center text-center">
          <div class="flex size-11 items-center justify-center rounded-2xl bg-black/40 ring-1 ring-white/15">
            <span class="text-title-medium font-black tracking-widest text-white drop-shadow">
              {{ channelInitials }}
            </span>
          </div>
          <span class="mt-1.5 max-w-[90%] truncate text-[11px] font-bold text-white/85">
            {{ cleanedName }}
          </span>
        </div>
      </div>

      <!-- bottom gradient — visible on hover for text legibility -->
      <div class="absolute inset-0 bg-gradient-to-t from-black/55 via-transparent to-transparent opacity-0 transition-opacity duration-200 group-focus-visible:opacity-100 group-hover:opacity-100" />

      <!-- LIVE badge — static dot instead of animate-pulse to reduce GPU repaints -->
      <div
        class="absolute left-2 top-2 flex items-center gap-1 rounded-full px-2 py-0.5 text-[9px] font-bold tracking-widest text-white shadow-sm"
        :class="offline ? 'bg-neutral-700/90' : 'bg-red-600/90'"
      >
        <span class="size-1.5 rounded-full" :class="offline ? 'bg-white/50' : 'bg-white'" />
        {{ offline ? $t('OFFLINE') : 'LIVE' }}
      </div>

      <!-- Favorite button -->
      <!-- A `span`, because the card itself is now the button and HTML
           has no nested one. `tabindex="-1"` so the remote walks channels
           and not two stops per card; the favourite is reachable from the
           player. -->
      <span
        class="absolute right-2 top-2 grid size-7 cursor-pointer place-items-center rounded-full bg-black/60 shadow transition-colors hover:bg-black/80 group-focus-visible:opacity-100 group-hover:opacity-100"
        :class="fav ? 'opacity-100' : 'opacity-0'"
        role="button"
        :aria-label="fav ? $t('Remove from favorites') : $t('Add to favorites')"
        tabindex="-1"
        @click.stop.prevent="emit('toggleFavorite', channel)"
      >
        <v-icon :icon="mdiStar" size="15" :class="fav ? 'text-amber-400' : 'text-white/70'" />
      </span>

      <!-- Centred play button on hover -->
      <div class="absolute inset-0 grid place-items-center opacity-0 transition-opacity duration-200 group-focus-visible:opacity-100 group-hover:opacity-100">
        <div class="grid size-11 place-items-center rounded-full bg-primary shadow-lg shadow-primary/40 ring-2 ring-white/20">
          <v-icon :icon="mdiPlay" size="22" class="ml-0.5 text-on-primary" />
        </div>
      </div>
    </div>

    <!-- Info -->
    <div class="flex flex-1 flex-col gap-1 px-3 py-2.5">
      <h3 class="line-clamp-1 text-body-medium font-semibold leading-snug">
        {{ cleanedName }}
      </h3>

      <div class="flex items-center gap-1 text-label-small text-white/45">
        <span v-if="channel.countryFlag" class="shrink-0 text-sm leading-none">{{ channel.countryFlag }}</span>
        <span v-if="channel.country" class="line-clamp-1">{{ channel.country }}</span>
        <template v-if="channel.country && channel.categoryName">
          <span class="opacity-40">·</span>
        </template>
        <span v-if="channel.categoryName" class="line-clamp-1 text-white/35">{{ channel.categoryName }}</span>
      </div>

      <!-- EPG now-playing -->
      <template v-if="nowProgram">
        <p class="mt-0.5 line-clamp-1 text-label-small font-medium text-primary/80">
          {{ nowProgram.title }}
        </p>
        <div class="mt-1">
          <div class="h-0.5 w-full overflow-hidden rounded-full bg-white/8">
            <!-- scaleX, not width — see LiveChannelRow for why. -->
            <div
              class="h-full w-full origin-left rounded-full bg-primary/70 transition-transform duration-1000 ease-linear"
              :style="{ transform: `scaleX(${epgProgress / 100})` }"
            />
          </div>
        </div>
      </template>
    </div>
  </button>
</template>
