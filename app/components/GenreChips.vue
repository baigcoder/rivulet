<script setup lang="ts">
/**
 * Genres as a row of one-press pills, "All" first. A dropdown hid the choice
 * behind a click and a scroll; this puts every genre on screen, and a remote
 * walks it left and right like any other row.
 */
defineProps<{ genres: { id: number, name: string }[] }>()
const model = defineModel<number | null>({ default: null })
</script>

<template>
  <div class="-mx-1 flex gap-2 overflow-x-auto px-1 py-1 [scrollbar-width:none]" role="radiogroup" :aria-label="$t('Genre')">
    <button
      type="button"
      role="radio"
      class="shrink-0 rounded-full border px-4 py-1.5 text-label-large font-semibold outline-none transition-colors focus-visible:ring-2 focus-visible:ring-primary"
      :class="model === null ? 'border-transparent bg-on-surface text-surface' : 'border-outline/40 hover:border-on-surface/60 hover:bg-on-surface/8 focus-visible:bg-on-surface/8'"
      :aria-checked="model === null"
      @click="model = null"
    >
      {{ $t('All') }}
    </button>
    <button
      v-for="g in genres"
      :key="g.id"
      type="button"
      role="radio"
      class="shrink-0 rounded-full border px-4 py-1.5 text-label-large font-semibold outline-none transition-colors focus-visible:ring-2 focus-visible:ring-primary"
      :class="model === g.id ? 'border-transparent bg-on-surface text-surface' : 'border-outline/40 hover:border-on-surface/60 hover:bg-on-surface/8 focus-visible:bg-on-surface/8'"
      :aria-checked="model === g.id"
      @click="model = model === g.id ? null : g.id"
    >
      {{ g.name }}
    </button>
  </div>
</template>
