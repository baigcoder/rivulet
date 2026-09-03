<script setup lang="ts">
/**
 * The category rail.
 *
 * Counts come from `/api/premium-tv/categories/counts`, not from
 * `/categories`: a provider's declared group list routinely includes
 * groups with nothing in them, and a rail entry that leads to an empty
 * grid is worse than no entry. The counts are one cheap query against the
 * cached catalog, so the rail is right without loading a single channel.
 *
 * Selected state is the same primary treatment for All / Favorites /
 * Recent / a category — not a second colour system beside the rest of the app.
 *
 * Catch-alls (`ALL SPORTS`, `ALL MOVIES`) sit at the top with no code
 * badge, so the full name fits. Country folders keep the two-letter slot.
 */
import type { PremiumView } from '~/stores/premiumTv'
import type { CategoryCount } from '~/types/premium'
import { mdiChevronDown, mdiChevronRight, mdiHistory, mdiStar, mdiTelevisionClassic } from '@mdi/js'
import { computed, ref, watch } from 'vue'
import { categoryLabel, isBundleCategory, parseCategoryName } from '~/utils/categoryLabel'

const props = defineProps<{
  view: PremiumView
  selectedCategory: string
  categories: CategoryCount[]
  totalChannels: number
  favoriteCount: number
  recentCount: number
  /** Free TV's iptv-org list is short — keep it open so the rail matches Premium. */
  categoriesOpen?: boolean
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
const groupsOpen = ref(props.categoriesOpen === true)

watch(
  () => props.view,
  v => {
    if (v === 'category')
      groupsOpen.value = true
  },
  { immediate: true },
)

const shown = computed(() => {
  const q = filter.value.trim().toLowerCase()
  const list = q
    ? props.categories.filter(c => {
        const parsed = parseCategoryName(c.name)
        return c.name.toLowerCase().includes(q)
          || parsed.label.toLowerCase().includes(q)
          || (parsed.code?.toLowerCase().includes(q) ?? false)
      })
    : props.categories
  return [...list]
    .sort((a, b) => Number(isBundleCategory(b.name)) - Number(isBundleCategory(a.name)))
    .map(cat => ({
      cat,
      ...parseCategoryName(cat.name),
      label: categoryLabel(cat.name),
      bundle: isBundleCategory(cat.name),
    }))
})

function isActive(name: string): boolean {
  return props.view === 'category' && props.selectedCategory === name
}

function fmt(n: number): string {
  return n.toLocaleString()
}
</script>

<template>
  <nav class="flex min-h-0 flex-1 flex-col gap-3 select-none" :aria-label="$t('Channel categories')">
    <div class="flex flex-col gap-0.5">
      <button
        type="button"
        class="flex min-h-11 items-center gap-3 rounded-lg px-2.5 text-start text-body-small transition-colors"
        :class="view === 'all'
          ? 'bg-primary text-on-primary'
          : 'text-on-surface/80 hover:bg-surface-container-high focus-visible:bg-surface-container-high'"
        :aria-current="view === 'all' ? 'true' : undefined"
        @click="emit('setView', 'all')"
      >
        <v-icon :icon="mdiTelevisionClassic" size="18" class="shrink-0" />
        <span class="min-w-0 flex-1 truncate font-medium">{{ $t('All channels') }}</span>
        <span class="w-10 shrink-0 text-end text-label-small tabular-nums opacity-70">{{ fmt(totalChannels) }}</span>
      </button>

      <button
        type="button"
        class="flex min-h-11 items-center gap-3 rounded-lg px-2.5 text-start text-body-small transition-colors"
        :class="view === 'favorites'
          ? 'bg-primary text-on-primary'
          : 'text-on-surface/80 hover:bg-surface-container-high focus-visible:bg-surface-container-high'"
        :aria-current="view === 'favorites' ? 'true' : undefined"
        @click="emit('setView', 'favorites')"
      >
        <v-icon :icon="mdiStar" size="18" class="shrink-0" />
        <span class="min-w-0 flex-1 truncate font-medium">{{ $t('Favorites') }}</span>
        <span class="w-10 shrink-0 text-end text-label-small tabular-nums opacity-70">{{ fmt(favoriteCount) }}</span>
      </button>

      <button
        type="button"
        class="flex min-h-11 items-center gap-3 rounded-lg px-2.5 text-start text-body-small transition-colors"
        :class="view === 'recent'
          ? 'bg-primary text-on-primary'
          : 'text-on-surface/80 hover:bg-surface-container-high focus-visible:bg-surface-container-high'"
        :aria-current="view === 'recent' ? 'true' : undefined"
        @click="emit('setView', 'recent')"
      >
        <v-icon :icon="mdiHistory" size="18" class="shrink-0" />
        <span class="min-w-0 flex-1 truncate font-medium">{{ $t('Recently watched') }}</span>
        <span class="w-10 shrink-0 text-end text-label-small tabular-nums opacity-70">{{ fmt(recentCount) }}</span>
      </button>
    </div>

    <div v-if="categories.length > 0" class="flex min-h-0 flex-1 flex-col gap-2 border-t border-outline/20 pt-2">
      <button
        type="button"
        class="flex items-center gap-2 px-1.5 py-1 text-label-medium font-medium opacity-55 hover:opacity-100 focus-visible:opacity-100 transition-opacity"
        :aria-expanded="groupsOpen"
        @click="groupsOpen = !groupsOpen"
      >
        <v-icon :icon="groupsOpen ? mdiChevronDown : mdiChevronRight" size="16" />
        <span class="flex-1 text-start">{{ $t('Categories') }}</span>
        <span class="tabular-nums opacity-70">{{ fmt(categories.length) }}</span>
      </button>

      <template v-if="groupsOpen">
        <search-field
          v-model="filter"
          :placeholder="$t('Find a category…')"
          density="compact"
        />

        <!-- The rail scrolls on its own so the grid keeps its full height. -->
        <div class="min-h-0 flex-1 overflow-y-auto">
          <ul class="flex flex-col">
            <li
              v-for="row in shown"
              :key="row.cat.name"
              class="[content-visibility:auto] [contain-intrinsic-size:auto_44px]"
            >
              <button
                type="button"
                class="flex min-h-11 w-full items-center gap-2.5 rounded-lg px-2.5 text-start text-body-small transition-colors"
                :class="isActive(row.cat.name)
                  ? 'bg-primary text-on-primary'
                  : 'text-on-surface/80 hover:bg-surface-container-high focus-visible:bg-surface-container-high'"
                :aria-current="isActive(row.cat.name) ? 'true' : undefined"
                :title="row.cat.name"
                @click="emit('setCategory', row.cat.name)"
              >
                <span
                  v-if="!row.bundle"
                  class="grid size-8 shrink-0 place-items-center rounded-md text-label-small font-semibold tabular-nums"
                  :class="isActive(row.cat.name) ? 'bg-on-primary/20' : 'bg-surface-container-highest'"
                >
                  {{ row.code || row.label.slice(0, 2).toUpperCase() }}
                </span>
                <span class="min-w-0 flex-1 leading-snug" :class="row.bundle ? 'line-clamp-2 font-medium' : 'truncate'">{{ row.label }}</span>
                <span class="w-10 shrink-0 text-end text-label-small tabular-nums opacity-55">{{ fmt(row.cat.count) }}</span>
              </button>
            </li>
          </ul>
          <p v-if="shown.length === 0" class="px-3 py-4 text-center text-label-small opacity-50">
            {{ $t('No categories match that filter.') }}
          </p>
        </div>
      </template>
    </div>
  </nav>
</template>
