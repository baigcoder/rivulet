<script setup lang="ts">
/**
 * One movie or series tile in the Premium on-demand grid.
 *
 * Deliberately not `MediaCard`: provider metadata, no TMDB deep link.
 * Posters are 2:3 with IPTV chrome — inset ring on art only, metadata
 * on focus/hover, caption outside the ring (same contract as live cards).
 */
import { mdiMovieOpen, mdiPlay, mdiTelevisionClassic } from '@mdi/js'
import { computed, ref } from 'vue'
import { proxyLogo } from '~/utils/premiumTv'

const props = defineProps<{
  id: string
  name: string
  posterUrl?: string | null
  rating?: string | null
  categoryName?: string | null
  kind: 'movie' | 'series'
  compact?: boolean
  /** Scroll rows show a caption; the grid hides it and uses the focus overlay. */
  showCaption?: boolean
}>()

const emit = defineEmits<{
  open: []
  prime: []
}>()

const imgError = ref(false)
const imgLoaded = ref(false)
const poster = computed(() => proxyLogo(props.posterUrl))
</script>

<template>
  <button
    type="button"
    class="group flex w-full flex-col text-start outline-none"
    :class="showCaption !== false ? 'gap-2' : 'gap-0'"
    @pointerdown="emit('prime')"
    @click="emit('open')"
  >
    <div
      class="relative aspect-[2/3] w-full overflow-hidden rounded-xl bg-surface-container-high ring-1 ring-white/8 transition-[box-shadow,transform] group-hover:ring-2 group-hover:ring-inset group-hover:ring-primary/80 group-focus-visible:ring-2 group-focus-visible:ring-inset group-focus-visible:ring-primary"
    >
      <img
        v-if="poster && !imgError"
        :src="poster"
        :alt="name"
        class="size-full object-cover transition-transform duration-300 group-hover:scale-[1.04] group-focus-visible:scale-[1.04]"
        loading="lazy"
        @load="imgLoaded = true"
        @error="imgError = true"
      >
      <div
        v-if="!poster || imgError || !imgLoaded"
        class="grid size-full place-items-center bg-gradient-to-br from-primary/15 to-surface-container-highest"
      >
        <v-icon :icon="kind === 'series' ? mdiTelevisionClassic : mdiMovieOpen" :size="compact ? 28 : 36" class="opacity-30" />
      </div>

      <div
        class="pointer-events-none absolute inset-0 flex flex-col justify-end bg-gradient-to-t from-black/85 via-black/25 to-transparent p-3 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
      >
        <span class="mb-2 grid size-10 place-items-center self-center rounded-full bg-primary text-on-primary shadow-lg">
          <v-icon :icon="mdiPlay" size="20" />
        </span>
        <p class="line-clamp-2 text-center text-label-medium font-semibold leading-snug text-white">
          {{ name }}
        </p>
        <p v-if="categoryName" class="mt-0.5 truncate text-center text-label-small text-white/65">
          {{ categoryName }}
        </p>
      </div>

      <span
        class="absolute start-2 top-2 rounded-md bg-black/60 px-1.5 py-0.5 text-label-small font-semibold uppercase tracking-wide text-white/90 ring-1 ring-white/10"
      >
        {{ kind === 'series' ? $t('TV') : $t('Movie') }}
      </span>
      <span
        v-if="rating"
        class="absolute end-2 top-2 rounded-md bg-black/55 px-1.5 py-0.5 text-label-small font-medium text-amber-200 ring-1 ring-white/10"
      >
        {{ rating }}
      </span>
    </div>
    <p
      v-if="showCaption !== false"
      class="line-clamp-2 pt-2 font-medium leading-snug text-on-surface transition-colors group-hover:text-primary group-focus-visible:text-primary"
      :class="compact ? 'text-label-medium' : 'text-body-small'"
    >
      {{ name }}
    </p>
  </button>
</template>
