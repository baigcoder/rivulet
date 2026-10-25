<script setup lang="ts">
import type { TitleVideo } from '~/utils/tmdb'
import { mdiPlay } from '@mdi/js'

/**
 * Every trailer, teaser and featurette a title has, as thumbnails. Pressing
 * one hands its key back to the page, which plays it in the same dialog the
 * Trailer button opens — one player, however it was reached.
 */
defineProps<{ videos: TitleVideo[] }>()
const emit = defineEmits<{ play: [key: string] }>()
</script>

<template>
  <scroll-row v-if="videos.length" :title="$t('Videos')" :count="videos.length">
    <button
      v-for="v in videos"
      :key="v.key"
      type="button"
      class="group flex w-72 shrink-0 flex-col gap-2 rounded-xl text-start outline-none focus-visible:ring-2 focus-visible:ring-primary"
      @click="emit('play', v.key)"
    >
      <div class="relative aspect-video w-full overflow-hidden rounded-xl bg-surface-container-high">
        <img
          :src="`https://i.ytimg.com/vi/${v.key}/mqdefault.jpg`"
          alt=""
          loading="lazy"
          decoding="async"
          class="h-full w-full object-cover transition-transform duration-300 group-hover:scale-105 group-focus-visible:scale-105"
        >
        <div class="absolute inset-0 grid place-items-center bg-black/20">
          <span class="grid size-11 place-items-center rounded-full bg-black/70 text-white transition-[transform,background-color] group-hover:scale-110 group-hover:bg-white group-hover:text-black group-focus-visible:scale-110 group-focus-visible:bg-white group-focus-visible:text-black">
            <v-icon :icon="mdiPlay" size="24" />
          </span>
        </div>
        <span class="absolute bottom-2 start-2 rounded bg-black/75 px-1.5 py-0.5 text-label-small font-semibold uppercase tracking-wide text-white">
          {{ v.type }}
        </span>
      </div>
      <p class="line-clamp-2 text-body-medium font-medium">
        {{ v.name }}
      </p>
    </button>
  </scroll-row>
</template>
