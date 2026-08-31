<script setup lang="ts">
defineProps<{
  countries: string[]
  selected: string
}>()

const emit = defineEmits<{
  select: [country: string]
}>()

const scrollEl = ref<HTMLElement>()

function prev() {
  scrollEl.value?.scrollBy({ left: -200, behavior: 'smooth' })
}
function next() {
  scrollEl.value?.scrollBy({ left: 200, behavior: 'smooth' })
}
</script>

<template>
  <div class="relative">
    <div class="text-label-small mb-1.5 font-medium uppercase tracking-wider opacity-50">
      {{ $t('Countries') }}
    </div>
    <div class="group/bar relative flex items-center">
      <button
        type="button"
        class="absolute left-0 z-10 grid size-7 place-items-center rounded-full bg-surface-container-high shadow-md opacity-0 transition-opacity hover:bg-surface-container-high group-hover/bar:opacity-100"
        @click="prev"
      >
        <v-icon icon="mdi-chevron-left" size="18" />
      </button>

      <div
        ref="scrollEl"
        class="flex gap-2 overflow-x-auto px-1 py-1 scrollbar-none"
      >
        <button
          type="button"
          class="shrink-0 rounded-full border px-3 py-1 text-body-small transition-all"
          :class="selected === '' ? 'border-primary bg-primary text-on-primary' : 'border-white/10 bg-surface-container-high hover:bg-surface-container'"
          @click="emit('select', '')"
        >
          {{ $t('All') }}
        </button>
        <button
          v-for="country in countries"
          :key="country"
          type="button"
          class="shrink-0 rounded-full border px-3 py-1 text-body-small transition-all"
          :class="selected === country ? 'border-primary bg-primary text-on-primary' : 'border-white/10 bg-surface-container-high hover:bg-surface-container'"
          @click="emit('select', country)"
        >
          {{ country }}
        </button>
      </div>

      <button
        type="button"
        class="absolute right-0 z-10 grid size-7 place-items-center rounded-full bg-surface-container-high shadow-md opacity-0 transition-opacity hover:bg-surface-container-high group-hover/bar:opacity-100"
        @click="next"
      >
        <v-icon icon="mdi-chevron-right" size="18" />
      </button>
    </div>
  </div>
</template>
