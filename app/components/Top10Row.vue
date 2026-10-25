<script setup lang="ts">
import type { Media } from '~/utils/tmdb'

/**
 * Today's ten most watched, ranked — the big outlined numeral tucked behind
 * each poster is the whole idea, and it is what makes a chart read as a chart
 * rather than one more row of posters. Fed from the trending list Home has
 * already fetched for its hero, so it costs no request of its own.
 *
 * The numeral is drawn once, outlined in the theme's own outline colour and
 * filled with its background, and nothing on it moves: on a TV the GPU pays
 * for animated area, and a chart has no reason to animate.
 */
const props = defineProps<{ items: Media[] }>()
const ui = useUiStore()
const top = computed(() => props.items.slice(0, 10))

/** Two digits need more room than one, or "10" runs under its poster. */
function width(rank: number) {
  return Math.round(ui.cardWidth * (rank >= 10 ? 1.75 : 1.5))
}
</script>

<template>
  <scroll-row v-if="top.length" :title="$t('Top 10 today')">
    <nuxt-link
      v-for="(m, i) in top"
      :key="`top-${m.type}-${m.id}`"
      :to="mediaLink(m)"
      :aria-label="`${i + 1}. ${m.title}`"
      class="group relative flex shrink-0 items-end rounded-2xl outline-none focus-visible:ring-2 focus-visible:ring-primary"
      :style="{ width: `${width(i + 1)}px` }"
    >
      <span
        aria-hidden="true"
        class="pointer-events-none select-none font-black leading-[0.8] tracking-tighter"
        :style="{
          fontSize: `${Math.round(ui.cardWidth * 1.1)}px`,
          color: 'rgb(var(--v-theme-background))',
          WebkitTextStroke: '3px rgb(var(--v-theme-outline))',
        }"
      >{{ i + 1 }}</span>
      <div
        class="relative -ms-[16%] aspect-2/3 shrink-0 overflow-hidden rounded-xl shadow-xl shadow-black/40 transition-transform duration-200 group-hover:scale-[1.03] group-focus-visible:scale-[1.03]"
        :style="{ width: `${ui.cardWidth}px` }"
      >
        <media-poster :src="posterUrl(m.poster, ui.posterSize)" :alt="m.title" />
      </div>
    </nuxt-link>
  </scroll-row>
</template>
