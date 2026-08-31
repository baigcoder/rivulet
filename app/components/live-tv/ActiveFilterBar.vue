<script setup lang="ts">
import { mdiClose } from '@mdi/js'

interface FilterChip {
  id: string
  label: string
}

defineProps<{
  filters: FilterChip[]
  channelCount: number
}>()

const emit = defineEmits<{
  remove: [id: string]
  clearAll: []
}>()
</script>

<template>
  <div class="flex items-center gap-2 px-4 md:px-6">
    <div class="flex flex-wrap items-center gap-1.5">
      <span
        v-for="f in filters"
        :key="f.id"
        class="flex items-center gap-1 rounded-full border border-primary/30 bg-primary/10 px-2.5 py-1 text-body-small text-primary"
      >
        {{ f.label }}
        <button
          class="grid size-4 place-items-center rounded-full hover:bg-primary/20"
          @click="emit('remove', f.id)"
        >
          <v-icon :icon="mdiClose" size="12" />
        </button>
      </span>
      <button
        v-if="filters.length > 1"
        class="text-body-small text-primary/70 hover:text-primary"
        @click="emit('clearAll')"
      >
        {{ $t('Clear all') }}
      </button>
    </div>
    <v-spacer />
    <span class="text-body-small opacity-50">
      {{ channelCount }} {{ $t('channels') }}
    </span>
  </div>
</template>
