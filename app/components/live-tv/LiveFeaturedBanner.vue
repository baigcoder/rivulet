<script setup lang="ts">
/**
 * The channel the viewer is on, large: what is on now, how far in, what is
 * next, and a Watch button — above the grid, following focus through it.
 *
 * A grid card has room for a logo and one line; this is where a viewer reads
 * the description and decides. Up from the first row lands on Watch, which
 * plays the channel they just left, and the star here is the remote's way to
 * a favourite (the one on each card is for pointers only).
 *
 * Fixed height, so a programme with no description does not move the grid,
 * and a still tint rather than a blurred backdrop: this spans the page and
 * redraws on every focus move (check:perf).
 */
import type { FeaturedProgramme } from '~/utils/featuredChannel'
import { mdiPlay, mdiStar, mdiStarOutline } from '@mdi/js'
import { useNow } from '@vueuse/core'
import { computed, ref, watch } from 'vue'
import { channelTileStyle, isPlaceholderLogoUrl, isTinyLogo } from '~/utils/channelLogo'
import { minutesLeft } from '~/utils/epgNow'
import { proxyLogo } from '~/utils/premiumTv'

const props = defineProps<{
  id: string
  name: string
  logoUrl?: string | null
  /** Seeds the tint, the same one the channel's card uses. */
  tint: string
  quality?: string | null
  subtitle?: string
  now?: FeaturedProgramme | null
  next?: FeaturedProgramme | null
  favorite?: boolean
  offline?: boolean
}>()

const emit = defineEmits<{
  play: []
  toggleFavorite: []
}>()

const tick = useNow({ interval: 30_000 })
const imgError = ref(false)
watch(() => props.logoUrl, () => {
  imgError.value = false
})

const showLogo = computed(() => !isPlaceholderLogoUrl(props.logoUrl) && !imgError.value)
const style = computed(() => channelTileStyle(props.tint))

const end = computed(() => props.now ? props.now.stop ?? props.now.start + 3_600_000 : 0)
const started = computed(() => !!props.now && props.now.start <= tick.value.getTime())
const progress = computed(() => {
  if (!props.now || !started.value)
    return 0
  const span = end.value - props.now.start
  return span > 0 ? Math.min(1, (tick.value.getTime() - props.now.start) / span) : 0
})
const left = computed(() => started.value ? minutesLeft(end.value, tick.value.getTime()) : null)

function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}

function onLogoLoad(e: Event): void {
  if (isTinyLogo(e.target as HTMLImageElement))
    imgError.value = true
}
</script>

<template>
  <section
    class="relative hidden h-44 shrink-0 items-center gap-6 overflow-hidden rounded-2xl p-5 ring-1 ring-white/10 md:flex"
    :style="style"
    :aria-label="$t('Selected channel')"
  >
    <!-- Darkens the side the text sits on; the tint stays at the logo end. -->
    <div class="pointer-events-none absolute inset-0 bg-gradient-to-l from-black/70 via-black/40 to-black/10" />

    <div class="relative grid aspect-video h-full shrink-0 place-items-center overflow-hidden rounded-xl bg-black/35 ring-1 ring-white/10">
      <img
        v-if="showLogo"
        :key="id"
        :src="proxyLogo(logoUrl)"
        alt=""
        decoding="async"
        class="max-h-[62%] max-w-[72%] object-contain"
        @load="onLogoLoad"
        @error="imgError = true"
      >
      <span v-else class="line-clamp-2 px-3 text-center text-title-medium font-extrabold tracking-tight text-white/90">
        {{ name }}
      </span>
    </div>

    <div class="relative flex min-w-0 flex-1 flex-col justify-center gap-0.5 overflow-hidden text-white">
      <p class="flex items-center gap-2 text-label-large text-white/70">
        <span v-if="quality" class="rounded bg-black/50 px-1.5 py-0.5 text-[11px] font-semibold uppercase tracking-wide text-amber-200">{{ quality }}</span>
        <span v-if="offline" class="rounded bg-black/50 px-1.5 py-0.5 text-[11px] font-semibold uppercase tracking-wide">{{ $t('Offline') }}</span>
        <!-- With nothing on, the name is the heading below; do not say it twice. -->
        <span class="truncate">{{ [now ? name : '', subtitle].filter(Boolean).join(' · ') }}</span>
      </p>
      <template v-if="now">
        <h2 class="truncate text-headline-small font-bold tracking-tight">
          {{ now.title }}
        </h2>
        <p class="text-body-medium text-white/75">
          {{ clock(now.start) }}<template v-if="now.stop">
            – {{ clock(now.stop) }}
          </template>
          <template v-if="left">
            · {{ $t('{minutes} min left', { minutes: left }) }}
          </template>
          <template v-if="!started">
            · {{ $t('Up next') }}
          </template>
        </p>
        <div v-if="started" class="mt-1 h-1 w-full max-w-md overflow-hidden rounded-full bg-white/20">
          <div class="h-full w-full origin-left rounded-full bg-primary" :style="{ transform: `scaleX(${progress})` }" />
        </div>
        <p v-if="now.description" class="mt-1 line-clamp-1 max-w-3xl text-body-medium text-white/65">
          {{ now.description }}
        </p>
      </template>
      <h2 v-else class="truncate text-headline-small font-bold tracking-tight">
        {{ name }}
      </h2>
      <p v-if="next" class="truncate text-body-medium text-white/55">
        {{ $t('Next') }} {{ clock(next.start) }} · {{ next.title }}
      </p>
    </div>

    <div class="relative flex shrink-0 items-center gap-2">
      <button
        type="button"
        class="flex h-12 items-center gap-2 rounded-xl bg-primary px-5 text-title-small font-semibold text-on-primary transition-[filter] hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white focus-visible:ring-offset-2 focus-visible:ring-offset-black"
        @click="emit('play')"
      >
        <v-icon :icon="mdiPlay" size="22" />
        {{ $t('Watch') }}
      </button>
      <button
        type="button"
        class="grid size-12 place-items-center rounded-xl bg-white/10 text-white transition-colors hover:bg-white/20 focus-visible:bg-white/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white"
        :aria-label="favorite ? $t('Remove from favorites') : $t('Add to favorites')"
        :aria-pressed="favorite ? 'true' : 'false'"
        @click="emit('toggleFavorite')"
      >
        <v-icon :icon="favorite ? mdiStar : mdiStarOutline" size="22" :class="favorite ? 'text-amber-300' : ''" />
      </button>
    </div>
  </section>
</template>
