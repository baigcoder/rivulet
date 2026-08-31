<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { mdiClockOutline, mdiPlay, mdiTelevision } from '@mdi/js'

defineProps<{
  channels: LiveChannel[]
  getEpg: (id: string) => Array<{ title: string, description?: string | null, start: string, stop?: string | null }>
}>()

const emit = defineEmits<{
  play: [channel: LiveChannel]
}>()
</script>

<template>
  <div v-if="channels.length" class="px-4 md:px-6">
    <h3 class="mb-2 flex items-center gap-1.5 text-title-small font-medium opacity-70">
      <v-icon :icon="mdiClockOutline" size="16" />
      {{ $t('Recently Watched') }}
    </h3>
    <div class="flex gap-3 overflow-x-auto pb-2 scrollbar-none">
      <button
        v-for="ch in channels"
        :key="ch.id"
        type="button"
        class="group flex w-28 shrink-0 cursor-pointer flex-col overflow-hidden rounded-xl border border-white/5 bg-surface-container-high transition-all hover:border-primary/40 hover:shadow-lg hover:shadow-primary/10 focus-visible:border-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary sm:w-32"
        @click="emit('play', ch)"
      >
        <div class="relative aspect-square w-full overflow-hidden bg-surface-container">
          <img
            v-if="ch.logoUrl"
            :src="ch.logoUrl"
            :alt="ch.name"
            loading="lazy"
            decoding="async"
            class="size-full object-contain p-2"
          >
          <div v-else class="grid size-full place-items-center">
            <v-icon :icon="mdiTelevision" size="24" class="opacity-15" />
          </div>
          <div class="absolute inset-0 grid place-items-center bg-black/0 opacity-0 transition-all group-hover:bg-black/30 group-hover:opacity-100">
            <div class="size-8 place-items-center rounded-full bg-primary/90 text-white shadow-lg grid opacity-0 group-hover:grid group-hover:opacity-100">
              <v-icon :icon="mdiPlay" size="16" class="ml-0.5" />
            </div>
          </div>
        </div>
        <div class="p-2">
          <p class="line-clamp-1 text-label-small font-medium">
            {{ ch.name }}
          </p>
        </div>
      </button>
    </div>
  </div>
</template>
