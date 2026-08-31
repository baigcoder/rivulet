<script setup lang="ts">
import type { LiveTvSort, LiveTvViewMode } from '~/stores/liveTv'
import { mdiFilterOutline, mdiFormatListBulleted, mdiViewGrid } from '@mdi/js'

defineProps<{
  sort: LiveTvSort
  viewMode: LiveTvViewMode
  channelCount: number
  activeFilters: string[]
}>()

const emit = defineEmits<{
  'update:sort': [sort: LiveTvSort]
  'update:viewMode': [mode: LiveTvViewMode]
  'openFilters': []
}>()

const sortOptions: Array<{ value: LiveTvSort, label: string }> = [
  { value: 'recommended', label: 'Recommended' },
  { value: 'az', label: 'A–Z' },
  { value: 'za', label: 'Z–A' },
  { value: 'favorites', label: 'Favorites' },
  { value: 'recently_watched', label: 'Recently Watched' },
]
</script>

<template>
  <div class="flex items-center gap-2 px-4 md:px-6">
    <!-- Active filters breadcrumb -->
    <div v-if="activeFilters.length" class="me-2 hidden items-center gap-1 text-body-small opacity-50 md:flex">
      <template v-for="(f, i) in activeFilters" :key="f">
        <span>{{ f }}</span>
        <span v-if="i < activeFilters.length - 1">·</span>
      </template>
    </div>

    <v-spacer />

    <!-- Sort -->
    <v-menu>
      <template #activator="{ props: menuProps }">
        <v-btn
          size="small"
          variant="tonal"
          v-bind="menuProps"
        >
          {{ sortOptions.find(s => s.value === sort)?.label }}
          <template #append>
            <v-icon icon="mdi-chevron-down" size="16" />
          </template>
        </v-btn>
      </template>
      <v-list density="compact" class="py-1">
        <v-list-item
          v-for="opt in sortOptions"
          :key="opt.value"
          :active="sort === opt.value"
          @click="emit('update:sort', opt.value)"
        >
          <v-list-item-title>{{ opt.label }}</v-list-item-title>
        </v-list-item>
      </v-list>
    </v-menu>

    <!-- View mode toggle -->
    <div class="flex overflow-hidden rounded-lg border border-white/10">
      <button
        type="button"
        class="grid size-8 place-items-center transition-colors"
        :class="viewMode === 'grid' ? 'bg-primary text-on-primary' : 'text-white/50 hover:bg-white/5'"
        @click="emit('update:viewMode', 'grid')"
      >
        <v-icon :icon="mdiViewGrid" size="18" />
      </button>
      <button
        type="button"
        class="grid size-8 place-items-center transition-colors"
        :class="viewMode === 'list' ? 'bg-primary text-on-primary' : 'text-white/50 hover:bg-white/5'"
        @click="emit('update:viewMode', 'list')"
      >
        <v-icon :icon="mdiFormatListBulleted" size="18" />
      </button>
    </div>

    <!-- Filter drawer toggle (mobile) -->
    <v-btn
      size="small"
      variant="tonal"
      class="md:hidden"
      :icon="mdiFilterOutline"
      @click="emit('openFilters')"
    />
  </div>
</template>
