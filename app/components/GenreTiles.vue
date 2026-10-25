<script setup lang="ts">
import { mdiDramaMasks, mdiEarth, mdiEmoticonHappyOutline, mdiFire, mdiGhostOutline, mdiHandcuffs, mdiHeartOutline, mdiKnife, mdiPalette, mdiRocketLaunchOutline } from '@mdi/js'

/**
 * A row of genres to jump into, as big coloured tiles rather than another
 * strip of posters — the one row on Home that is a choice of *mood* instead of
 * a list of titles. Each opens the Movies page filtered to it (`?genre=`).
 *
 * The colours are the genres', not the theme's: they are labels, and a horror
 * tile in the same teal as a comedy one tells you nothing. They sit at partial
 * opacity over the theme's own surface, so a light theme still reads, and none
 * of it moves — a static gradient costs a TV nothing after the first paint.
 */
const GENRES = computed(() => [
  { id: 28, name: $t('Action'), icon: mdiFire, tint: 'from-orange-500/70 to-red-800/60' },
  { id: 35, name: $t('Comedy'), icon: mdiEmoticonHappyOutline, tint: 'from-amber-400/70 to-yellow-700/60' },
  { id: 18, name: $t('Drama'), icon: mdiDramaMasks, tint: 'from-violet-500/70 to-indigo-800/60' },
  { id: 27, name: $t('Horror'), icon: mdiGhostOutline, tint: 'from-zinc-500/70 to-neutral-900/70' },
  { id: 878, name: $t('Sci-Fi'), icon: mdiRocketLaunchOutline, tint: 'from-cyan-500/70 to-blue-800/60' },
  { id: 10749, name: $t('Romance'), icon: mdiHeartOutline, tint: 'from-pink-500/70 to-rose-800/60' },
  { id: 53, name: $t('Thriller'), icon: mdiKnife, tint: 'from-slate-500/70 to-slate-900/70' },
  { id: 16, name: $t('Animation'), icon: mdiPalette, tint: 'from-lime-500/70 to-emerald-800/60' },
  { id: 80, name: $t('Crime'), icon: mdiHandcuffs, tint: 'from-red-600/70 to-stone-900/70' },
  { id: 99, name: $t('Documentary'), icon: mdiEarth, tint: 'from-teal-500/70 to-green-900/60' },
])
</script>

<template>
  <scroll-row :title="$t('Browse by genre')">
    <nuxt-link
      v-for="g in GENRES"
      :key="g.id"
      :to="{ path: localePath('/movies'), query: { genre: g.id, label: g.name } }"
      class="group relative flex h-28 w-52 shrink-0 items-end overflow-hidden rounded-2xl bg-surface-container-high p-4 outline-none transition-transform duration-200 hover:scale-[1.03] focus-visible:scale-[1.03] focus-visible:ring-2 focus-visible:ring-primary md:h-32 md:w-60"
    >
      <div class="pointer-events-none absolute inset-0 bg-gradient-to-br" :class="g.tint" />
      <v-icon
        :icon="g.icon"
        size="96"
        class="pointer-events-none absolute -bottom-4 -end-3 text-white opacity-25"
      />
      <span class="relative text-title-large font-bold text-white drop-shadow-[0_2px_8px_rgba(0,0,0,0.5)]">
        {{ g.name }}
      </span>
    </nuxt-link>
  </scroll-row>
</template>
