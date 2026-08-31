<script setup lang="ts">
/**
 * The category rail.
 *
 * Counts come from `/api/premium-tv/categories/counts`, not from
 * `/categories`: a provider's declared group list routinely includes
 * groups with nothing in them, and a rail entry that leads to an empty
 * grid is worse than no entry. The counts are one cheap query against the
 * cached catalog, so the rail is right without loading a single channel.
 */
import type { PremiumView } from '~/stores/premiumTv'
import type { CategoryCount } from '~/types/premium'
import { mdiChevronDown, mdiChevronRight, mdiHistory, mdiMagnify, mdiStar, mdiTelevisionClassic } from '@mdi/js'
import { computed, ref } from 'vue'

const props = defineProps<{
  view: PremiumView
  selectedCategory: string
  categories: CategoryCount[]
  totalChannels: number
  favoriteCount: number
  recentCount: number
}>()

const emit = defineEmits<{
  setView: [view: PremiumView]
  setCategory: [name: string]
}>()

/** Filters the rail itself, which matters at a hundred-odd groups. */
const filter = ref('')

/**
 * The provider's own groups fold away. The three fixed views are the ones
 * a viewer uses every session; a lineup's hundred-odd groups are how they
 * go looking for something, and while they are not looking the list is a
 * wall. Open by default — hiding the catalog behind a click would be the
 * opposite mistake.
 */
const groupsOpen = ref(true)

const shown = computed(() => {
  const q = filter.value.trim().toLowerCase()
  if (!q)
    return props.categories
  return props.categories.filter(c => c.name.toLowerCase().includes(q))
})

function isActive(name: string): boolean {
  return props.view === 'category' && props.selectedCategory === name
}

function fmt(n: number): string {
  return n.toLocaleString()
}
</script>

<template>
  <nav class="flex h-full min-h-0 flex-col gap-3 font-sans select-none" :aria-label="$t('Channel categories')">
    <div class="flex flex-col gap-1.5">
      <button
        type="button"
        class="flex items-center gap-3 rounded-xl px-3.5 py-2.5 text-start text-sm transition-all duration-150 border"
        :class="view === 'all'
          ? 'bg-primary/20 border-primary/40 text-primary font-bold shadow-md shadow-primary/10'
          : 'bg-white/5 border-white/5 text-gray-300 hover:bg-white/10 hover:text-white'"
        :aria-current="view === 'all' ? 'true' : undefined"
        @click="emit('setView', 'all')"
      >
        <v-icon :icon="mdiTelevisionClassic" size="18" class="shrink-0" />
        <span class="flex-1 truncate font-medium">{{ $t('All channels') }}</span>
        <span class="text-xs tabular-nums px-2 py-0.5 rounded-md bg-black/40 text-gray-400 font-mono">{{ fmt(totalChannels) }}</span>
      </button>

      <button
        type="button"
        class="flex items-center gap-3 rounded-xl px-3.5 py-2.5 text-start text-sm transition-all duration-150 border"
        :class="view === 'favorites'
          ? 'bg-amber-500/20 border-amber-500/40 text-amber-300 font-bold shadow-md shadow-amber-500/10'
          : 'bg-white/5 border-white/5 text-gray-300 hover:bg-white/10 hover:text-white'"
        :aria-current="view === 'favorites' ? 'true' : undefined"
        @click="emit('setView', 'favorites')"
      >
        <v-icon :icon="mdiStar" size="18" class="shrink-0" :class="view === 'favorites' ? 'text-amber-400' : ''" />
        <span class="flex-1 truncate font-medium">{{ $t('Favorites') }}</span>
        <span class="text-xs tabular-nums px-2 py-0.5 rounded-md bg-black/40 text-gray-400 font-mono">{{ fmt(favoriteCount) }}</span>
      </button>

      <button
        type="button"
        class="flex items-center gap-3 rounded-xl px-3.5 py-2.5 text-start text-sm transition-all duration-150 border"
        :class="view === 'recent'
          ? 'bg-cyan-500/20 border-cyan-500/40 text-cyan-300 font-bold shadow-md shadow-cyan-500/10'
          : 'bg-white/5 border-white/5 text-gray-300 hover:bg-white/10 hover:text-white'"
        :aria-current="view === 'recent' ? 'true' : undefined"
        @click="emit('setView', 'recent')"
      >
        <v-icon :icon="mdiHistory" size="18" class="shrink-0" />
        <span class="flex-1 truncate font-medium">{{ $t('Recently watched') }}</span>
        <span class="text-xs tabular-nums px-2 py-0.5 rounded-md bg-black/40 text-gray-400 font-mono">{{ fmt(recentCount) }}</span>
      </button>
    </div>

    <div v-if="categories.length > 0" class="flex min-h-0 flex-1 flex-col gap-2 pt-2 border-t border-white/10">
      <button
        type="button"
        class="flex items-center gap-2 px-1.5 py-1 text-xs font-bold uppercase tracking-wider text-gray-400 hover:text-white transition-colors"
        :aria-expanded="groupsOpen"
        @click="groupsOpen = !groupsOpen"
      >
        <v-icon :icon="groupsOpen ? mdiChevronDown : mdiChevronRight" size="16" />
        <span class="flex-1 text-start">{{ $t('Categories') }}</span>
        <span class="tabular-nums font-mono opacity-70">{{ fmt(categories.length) }}</span>
      </button>

      <template v-if="groupsOpen">
        <!-- Compact Glass Category Search Input -->
        <div class="relative">
          <v-icon :icon="mdiMagnify" size="15" class="absolute left-3 top-2.5 text-gray-400" />
          <input
            v-model="filter"
            type="text"
            :placeholder="$t('Filter categories')"
            class="w-full pl-9 pr-3 py-1.5 bg-white/5 border border-white/10 rounded-xl text-xs text-white placeholder-gray-500 outline-none focus:border-primary transition-colors"
          >
        </div>

        <!-- The rail scrolls on its own so the grid keeps its full height. -->
        <div class="min-h-0 flex-1 overflow-y-auto pr-1 space-y-1">
          <ul class="flex flex-col gap-1">
            <li v-for="cat in shown" :key="cat.name">
              <button
                type="button"
                class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-start text-xs transition-all border"
                :class="isActive(cat.name)
                  ? 'bg-primary/20 border-primary/40 text-white font-bold border-l-4 border-l-primary'
                  : 'bg-white/5 border-white/5 text-gray-300 hover:bg-white/10 hover:text-white'"
                :aria-current="isActive(cat.name) ? 'true' : undefined"
                @click="emit('setCategory', cat.name)"
              >
                <span class="flex-1 truncate">{{ cat.name }}</span>
                <span class="text-[11px] font-mono tabular-nums opacity-50">{{ fmt(cat.count) }}</span>
              </button>
            </li>
          </ul>
          <p v-if="shown.length === 0" class="px-3 py-4 text-xs opacity-50 text-center">
            {{ $t('No categories match that filter.') }}
          </p>
        </div>
      </template>
    </div>
  </nav>
</template>
