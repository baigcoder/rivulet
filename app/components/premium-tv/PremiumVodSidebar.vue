<script setup lang="ts">
/**
 * Category rail for Premium on-demand — movies or TV shows.
 * Styled like `PremiumSidebar`: primary selected rows, d-pad search field.
 */
import type { VodCategory } from '~/types/premium'
import { mdiChevronDown, mdiChevronRight, mdiMovieOpen, mdiTelevisionClassic } from '@mdi/js'
import { computed, ref } from 'vue'

const props = defineProps<{
  categories: VodCategory[]
  selectedId: string
  kind: 'movie' | 'series'
}>()

const emit = defineEmits<{
  pick: [id: string]
}>()

const filter = ref('')
const groupsOpen = ref(false)

const shown = computed(() => {
  const q = filter.value.trim().toLowerCase()
  if (!q)
    return props.categories
  return props.categories.filter(c => c.name.toLowerCase().includes(q))
})
</script>

<template>
  <nav class="flex min-h-0 flex-1 flex-col gap-3 select-none" :aria-label="kind === 'movie' ? $t('Movie categories') : $t('TV show categories')">
    <div class="flex flex-col gap-0.5">
      <button
        type="button"
        class="flex min-h-11 items-center gap-3 rounded-lg px-2.5 text-start text-body-small transition-colors"
        :class="!selectedId
          ? 'bg-primary text-on-primary'
          : 'text-on-surface/80 hover:bg-surface-container-high focus-visible:bg-surface-container-high'"
        :aria-current="!selectedId ? 'true' : undefined"
        @click="emit('pick', '')"
      >
        <v-icon :icon="kind === 'movie' ? mdiMovieOpen : mdiTelevisionClassic" size="18" class="shrink-0" />
        <span class="min-w-0 flex-1 truncate font-medium">
          {{ kind === 'movie' ? $t('All movies') : $t('All TV shows') }}
        </span>
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
        <span class="tabular-nums opacity-70">{{ categories.length }}</span>
      </button>

      <template v-if="groupsOpen">
        <search-field
          v-model="filter"
          :placeholder="$t('Find a category…')"
          density="compact"
        />

        <div class="min-h-0 flex-1 overflow-y-auto">
          <ul class="flex flex-col">
            <li
              v-for="cat in shown"
              :key="cat.id"
              class="[content-visibility:auto] [contain-intrinsic-size:auto_44px]"
            >
              <button
                type="button"
                class="flex min-h-11 w-full items-center gap-2.5 rounded-lg px-2.5 text-start text-body-small transition-colors"
                :class="selectedId === cat.id
                  ? 'bg-primary text-on-primary'
                  : 'text-on-surface/80 hover:bg-surface-container-high focus-visible:bg-surface-container-high'"
                :aria-current="selectedId === cat.id ? 'true' : undefined"
                :title="cat.name"
                @click="emit('pick', cat.id)"
              >
                <span class="min-w-0 flex-1 leading-snug line-clamp-2">{{ cat.name }}</span>
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
