<script setup lang="ts">
import type { LiveTvTab } from '~/stores/liveTv'
import { mdiClockOutline, mdiEarth, mdiFormatListBulleted, mdiMenu, mdiStar, mdiTelevision } from '@mdi/js'

const model = defineModel<LiveTvTab>()

/**
 * Tab labels as direct `$t()` calls so the i18n scanner picks them up
 * (`scripts/i18n.ts` only finds string literals, not variables).
 * The keys are added to every locale by `bun run i18n`.
 */
const tabs: Array<{ id: LiveTvTab, icon: string, label: () => string }> = [
  { id: 'favorites', icon: mdiStar, label: () => $t('Favorites') },
  { id: 'recent', icon: mdiClockOutline, label: () => $t('Recent') },
  { id: 'live', icon: mdiTelevision, label: () => $t('Live Now') },
  { id: 'countries', icon: mdiEarth, label: () => $t('Countries') },
  { id: 'categories', icon: mdiMenu, label: () => $t('Categories') },
  { id: 'all', icon: mdiFormatListBulleted, label: () => $t('All') },
]
</script>

<template>
  <nav
    class="flex items-center gap-1.5 overflow-x-auto rounded-2xl border border-white/10 bg-black/30 p-1.5 backdrop-blur-xl shadow-inner scrollbar-none"
    :aria-label="$t('Live TV')"
  >
    <button
      v-for="tab in tabs"
      :key="tab.id"
      type="button"
      class="relative flex shrink-0 appearance-none items-center gap-2 rounded-xl px-4 py-2 text-body-small font-semibold outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-primary/50"
      :class="model === tab.id
        ? 'bg-primary text-on-primary shadow-lg shadow-primary/35 ring-1 ring-white/20'
        : 'text-white/60 hover:bg-white/10 hover:text-white'"
      :aria-current="model === tab.id ? 'page' : undefined"
      @click="model = tab.id"
    >
      <v-icon :icon="tab.icon" :size="model === tab.id ? 17 : 16" />
      <span>{{ tab.label() }}</span>
    </button>
  </nav>
</template>
