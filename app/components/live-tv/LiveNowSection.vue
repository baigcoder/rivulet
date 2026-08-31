<script setup lang="ts">
/**
 * Live Now tab content. Shows channels currently airing something
 * according to cached EPG. Falls back to recent channels when EPG
 * hasn't been loaded yet (any live channel is "on", so recents is a
 * reasonable default).
 */
import type { LiveChannel } from '~/utils/iptv'

defineProps<{
  channels: LiveChannel[]
  getEpg: (id: string) => Array<{ title: string, description?: string | null, start: string, stop?: string | null }>
  isFavorite: (ch: LiveChannel) => boolean
}>()

const emit = defineEmits<{
  play: [channel: LiveChannel]
  toggleFavorite: [channel: LiveChannel]
}>()
</script>

<template>
  <div class="px-4 pt-4 md:px-6">
    <div v-if="channels.length === 0" class="py-16 text-center">
      <p class="text-body-medium opacity-60">
        {{ $t('No channels airing right now') }}
      </p>
    </div>
    <div v-else class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
      <live-tv-live-channel-card
        v-for="ch in channels"
        :key="ch.id"
        :channel="ch"
        :get-epg="getEpg"
        :is-favorite="isFavorite"
        @play="emit('play', $event)"
        @toggle-favorite="emit('toggleFavorite', $event)"
      />
    </div>
  </div>
</template>
