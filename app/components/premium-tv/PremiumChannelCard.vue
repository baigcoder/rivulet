<script setup lang="ts">
/**
 * One channel in the Premium grid.
 *
 * Visual contract with `LiveChannelCard`: 16:9 art, name always visible
 * under the tile, inset ring on art at focus. Generic provider logos
 * (repeated 24/7 placeholders) fall back to coloured initials.
 */
import type { EpgProgram, IPTVChannel } from '~/types/premium'
import { mdiPlay, mdiStar } from '@mdi/js'
import { computed, onUnmounted, ref, watch } from 'vue'
import { categoryLabel } from '~/utils/categoryLabel'
import { channelTileStyle, isPlaceholderLogoUrl, isTinyLogo } from '~/utils/channelLogo'
import { channelInitials, parseChannelName } from '~/utils/channelName'
import { proxyLogo } from '~/utils/premiumTv'

const props = defineProps<{
  channel: IPTVChannel
  nowNext: (id: string) => { now: EpgProgram | null, next: EpgProgram | null }
  favorite: (id: string) => boolean
  compact?: boolean
}>()

const emit = defineEmits<{
  play: [channel: IPTVChannel]
  toggleFavorite: [channel: IPTVChannel]
}>()

const guide = computed(() => props.nowNext(props.channel.id))
const nowProgram = computed(() => guide.value.now)
const fav = computed(() => props.favorite(props.channel.id))
const imgError = ref(false)
const imgLoaded = ref(false)

const nowSecs = ref(Math.floor(Date.now() / 1000))
let progressTimer: ReturnType<typeof setInterval> | undefined
watch(nowProgram, prog => {
  if (prog && !progressTimer) {
    progressTimer = setInterval(() => {
      nowSecs.value = Math.floor(Date.now() / 1000)
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
  const prog = nowProgram.value
  if (!prog)
    return 0
  const end = prog.stop ?? prog.start + 3600
  if (end <= prog.start)
    return 0
  const pct = ((nowSecs.value - prog.start) / (end - prog.start)) * 100
  return Math.max(0, Math.min(100, Math.round(pct)))
})

const parsedName = computed(() => parseChannelName(props.channel.name || '', $t('Channel')))
const displayName = computed(() => parsedName.value.name)
const initials = computed(() => channelInitials(displayName.value))
const tileStyle = computed(() => channelTileStyle(props.channel.id || displayName.value))

const wantsLogo = computed(() => !isPlaceholderLogoUrl(props.channel.logoUrl))
const showFallback = computed(() => !wantsLogo.value || imgError.value)
const proxyLogoUrl = computed(() => proxyLogo(props.channel.logoUrl))

const subtitle = computed(() => {
  if (props.channel.country)
    return props.channel.country
  if (props.channel.categoryName)
    return categoryLabel(props.channel.categoryName)
  return ''
})

function onLogoLoad(e: Event): void {
  const img = e.target as HTMLImageElement
  if (isTinyLogo(img)) {
    imgError.value = true
    return
  }
  imgLoaded.value = true
}
</script>

<template>
  <button
    type="button"
    class="group relative flex cursor-pointer flex-col gap-1.5 text-start outline-none"
    :aria-label="displayName"
    @click="emit('play', channel)"
  >
    <div class="relative aspect-video w-full overflow-hidden rounded-xl bg-zinc-950 ring-1 ring-white/10 transition-shadow group-focus-visible:ring-2 group-focus-visible:ring-inset group-focus-visible:ring-primary">
      <div
        v-if="wantsLogo && !imgLoaded && !imgError"
        class="absolute inset-0 animate-pulse bg-surface-container-high"
      />

      <img
        v-if="wantsLogo && !imgError"
        :src="proxyLogoUrl"
        alt=""
        loading="lazy"
        decoding="async"
        class="size-full object-contain p-3 transition-opacity duration-300"
        :class="imgLoaded ? 'opacity-100' : 'opacity-0'"
        @load="onLogoLoad"
        @error="imgError = true"
      >

      <div
        v-if="showFallback"
        class="grid size-full place-items-center p-3"
        :style="tileStyle"
      >
        <span
          class="text-center font-bold leading-none text-white/90 drop-shadow-sm"
          :class="compact ? 'text-xl' : 'text-2xl'"
        >
          {{ initials }}
        </span>
      </div>

      <span
        v-if="parsedName.quality"
        class="pointer-events-none absolute start-1.5 top-1.5 z-10 rounded bg-black/60 px-1 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-amber-200"
      >
        {{ parsedName.quality }}
      </span>

      <span
        class="absolute end-1.5 top-1.5 z-10 grid size-6 cursor-pointer place-items-center rounded-full bg-black/55 opacity-0 shadow transition-opacity hover:bg-black/75 focus-visible:bg-black/75 group-hover:opacity-100 group-focus-visible:opacity-100"
        :class="fav ? '!opacity-100' : ''"
        role="button"
        tabindex="-1"
        :aria-label="fav ? $t('Remove from favorites') : $t('Add to favorites')"
        @click.stop.prevent="emit('toggleFavorite', channel)"
      >
        <v-icon :icon="mdiStar" size="14" :class="fav ? 'text-primary' : 'text-white/75'" />
      </span>

      <div class="pointer-events-none absolute inset-0 flex flex-col items-center justify-center bg-black/45 px-2 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
        <div class="grid size-10 place-items-center rounded-full bg-primary shadow-lg">
          <v-icon :icon="mdiPlay" size="20" class="ms-0.5 text-on-primary" />
        </div>
        <p
          v-if="nowProgram"
          class="mt-1.5 line-clamp-2 text-center text-[11px] leading-snug text-white"
        >
          {{ nowProgram.title }}
        </p>
      </div>

      <div
        v-if="nowProgram"
        class="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-0.5 bg-black/50"
      >
        <div
          class="h-full w-full origin-left bg-primary transition-transform duration-1000 ease-linear"
          :style="{ transform: `scaleX(${epgProgress / 100})` }"
        />
      </div>
    </div>

    <div class="min-w-0 px-0.5">
      <p
        class="font-semibold leading-snug text-on-surface group-hover:text-primary group-focus-visible:text-primary"
        :class="compact ? 'line-clamp-1 text-label-medium' : 'line-clamp-2 text-body-small'"
        :title="displayName"
      >
        {{ displayName }}
      </p>
      <p
        v-if="subtitle"
        class="line-clamp-1 text-label-small opacity-45"
      >
        {{ subtitle }}
      </p>
    </div>
  </button>
</template>
