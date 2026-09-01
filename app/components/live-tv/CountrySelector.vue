<script setup lang="ts">
import { mdiEarth } from '@mdi/js'

const props = defineProps<{
  countries: Array<{ name: string, count: number }>
  selected: string
}>()

const emit = defineEmits<{
  select: [country: string]
}>()
</script>

<template>
  <div class="px-4 md:px-6">
    <div class="mb-2 flex items-center justify-between">
      <h3 class="text-label-large font-medium opacity-80">
        {{ $t('Countries') }}
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
        :class="!selected ? 'border-primary bg-primary/15 text-primary' : 'border-white/10 bg-surface-container-high text-white/70 hover:bg-surface-container'"
        @click="emit('select', '')"
      >
        <v-icon :icon="mdiEarth" size="14" />
        {{ $t('All') }}
      </button>

      <!-- Country chips -->
      <button
        v-for="c in countries"
        :key="c.name"
        type="button"
        class="flex shrink-0 items-center gap-2 rounded-full border px-4 py-2 text-body-small font-medium transition-colors"
        :class="selected === c.name ? 'border-primary bg-primary/15 text-primary' : 'border-white/10 bg-surface-container-high text-white/70 hover:bg-surface-container'"
        @click="emit('select', c.name === selected ? '' : c.name)"
      >
        <span>{{ c.name }}</span>
        <span class="rounded-full bg-white/10 px-1.5 py-0.5 text-[10px] leading-none opacity-60">
          {{ c.count }}
        </span>
      </button>
    </div>
  </div>
</template>
