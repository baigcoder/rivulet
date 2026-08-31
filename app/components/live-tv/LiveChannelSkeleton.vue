<script setup lang="ts">
defineProps<{ count?: number }>()
</script>

<template>
  <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
    <div
      v-for="i in (count ?? 12)"
      :key="i"
      class="animate-pulse overflow-hidden rounded-xl border border-white/5 bg-surface-container-high"
    >
      <div class="aspect-video bg-surface-container" />
      <div class="p-3">
        <div class="mb-2 h-4 w-3/4 rounded bg-surface-container" />
        <div class="h-3 w-1/2 rounded bg-surface-container" />
      </div>
    </div>
  </div>
</template>
