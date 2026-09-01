<script setup lang="ts">
import type { LiveCategory } from '~/utils/iptv'
import { mdiAccountMultiple, mdiBabyFaceOutline, mdiBarn, mdiChurch, mdiFilmstripBoxMultiple, mdiMusicNote, mdiNewspaper, mdiTelevision } from '@mdi/js'

const props = defineProps<{
  categories: LiveCategory[]
  selected: string
  counts?: Map<string, number>
}>()

const emit = defineEmits<{
  select: [categoryId: string]
}>()

const CATEGORY_ICONS: Record<string, string> = {
  sports: mdiTelevision,
  news: mdiNewspaper,
  entertainment: mdiTelevision,
  kids: mdiBabyFaceOutline,
  movies: mdiFilmstripBoxMultiple,
  music: mdiMusicNote,
  documentary: mdiBarn,
  religious: mdiChurch,
  general: mdiAccountMultiple,
}

const CATEGORY_COLORS: Record<string, string> = {
  sports: '#e53935',
  news: '#1e88e5',
  entertainment: '#8e24aa',
  kids: '#43a047',
  movies: '#f4511e',
  music: '#00897b',
  documentary: '#5c6bc0',
  religious: '#fdd835',
  general: '#546e7a',
}

function getCategoryIcon(name: string): string {
  const lower = name.toLowerCase()
  for (const [key, icon] of Object.entries(CATEGORY_ICONS)) {
    if (lower.includes(key))
      return icon
  }
  return mdiTelevision
}

function getCategoryColor(name: string): string {
  const lower = name.toLowerCase()
  for (const [key, color] of Object.entries(CATEGORY_COLORS)) {
    if (lower.includes(key))
      return color
  }
  return '#546e7a'
}

function getCategoryId(cat: LiveCategory): string {
  return cat.id || cat.name
}
</script>

<template>
  <div class="px-4 md:px-6">
    <div class="mb-2 flex items-center justify-between">
      <h3 class="text-label-large font-medium opacity-80">
        {{ $t('Categories') }}
      </h3>
      <button
        v-if="selected"
        type="button"
        class="text-body-small text-primary hover:underline"
        @click="emit('select', '')"
      >
        {{ $t('Clear') }}
      </button>
    </div>

    <!-- Horizontal scroll chips -->
    <div class="flex gap-2 overflow-x-auto pb-2 scrollbar-none">
      <!-- All button -->
      <button
        type="button"
        class="flex shrink-0 items-center gap-2 rounded-full border px-4 py-2 text-body-small font-medium transition-colors"
        :class="!selected ? 'border-primary/50 bg-primary/20 text-primary font-bold shadow-md' : 'border-white/10 bg-white/5 text-white/70 hover:bg-white/10 hover:text-white'"
        @click="emit('select', '')"
      >
        {{ $t('All') }}
      </button>

      <!-- Category chips -->
      <button
        v-for="cat in categories"
        :key="getCategoryId(cat)"
        type="button"
        class="flex shrink-0 items-center gap-2 rounded-full border px-4 py-2 text-body-small font-medium transition-colors"
        :class="selected === getCategoryId(cat) ? 'border-primary/50 bg-primary/20 text-primary font-bold shadow-md' : 'border-white/10 bg-white/5 text-white/70 hover:bg-white/10 hover:text-white'"
        @click="emit('select', getCategoryId(cat) === selected ? '' : getCategoryId(cat))"
      >
        <span
          class="flex size-5 shrink-0 items-center justify-center rounded-full"
          :style="{ backgroundColor: `${getCategoryColor(cat.name)}30` }"
        >
          <v-icon :icon="getCategoryIcon(cat.name)" size="12" :color="getCategoryColor(cat.name)" />
        </span>
        <span>{{ cat.name }}</span>
        <span
          v-if="counts?.get(cat.name)"
          class="rounded-full bg-white/10 px-1.5 py-0.5 text-[10px] leading-none opacity-60"
        >
          {{ counts.get(cat.name) }}
        </span>
      </button>
    </div>
  </div>
</template>
