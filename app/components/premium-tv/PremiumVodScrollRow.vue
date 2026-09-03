<script setup lang="ts">
import type { PremiumSeriesItem, PremiumVodItem } from '~/types/premium'

const props = defineProps<{
  title: string
  kind: 'movie' | 'series'
  movies?: PremiumVodItem[]
  series?: PremiumSeriesItem[]
  max?: number
}>()

const emit = defineEmits<{
  openMovie: [item: PremiumVodItem]
  openSeries: [item: PremiumSeriesItem]
}>()

const visibleMovies = computed(() => (props.movies ?? []).slice(0, props.max ?? 16))
const visibleSeries = computed(() => (props.series ?? []).slice(0, props.max ?? 16))

const count = computed(() => props.kind === 'movie' ? (props.movies?.length ?? 0) : (props.series?.length ?? 0))
</script>

<template>
  <scroll-row
    v-if="count > 0"
    :title="title"
    :count="count"
  >
    <template v-if="kind === 'movie'">
      <premium-tv-premium-vod-card
        v-for="item in visibleMovies"
        :id="item.id"
        :key="item.id"
        :name="item.name"
        :poster-url="item.posterUrl"
        :rating="item.rating"
        :category-name="item.categoryName"
        kind="movie"
        class="w-40 shrink-0 sm:w-44 lg:w-48"
        show-caption
        @open="emit('openMovie', item)"
      />
    </template>
    <template v-else>
      <premium-tv-premium-vod-card
        v-for="item in visibleSeries"
        :id="item.id"
        :key="item.id"
        :name="item.name"
        :poster-url="item.posterUrl"
        :rating="item.rating"
        kind="series"
        class="w-40 shrink-0 sm:w-44 lg:w-48"
        show-caption
        @open="emit('openSeries', item)"
      />
    </template>
  </scroll-row>
</template>
