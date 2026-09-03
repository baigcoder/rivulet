<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'

const props = defineProps<{
  title: string
  channels: LiveChannel[]
  getEpg: (id: string) => Array<{ title: string, description?: string | null, start: string, stop?: string | null }>
  isFavorite: (ch: LiveChannel) => boolean
  isOffline?: (ch: LiveChannel) => boolean
  max?: number
  to?: string
  totalCount?: number
}>()

const emit = defineEmits<{
  play: [channel: LiveChannel]
  toggleFavorite: [channel: LiveChannel]
}>()

// Keep the landing page airy: one compact, fast-to-scan row per section.
const visible = computed(() => props.channels.slice(0, props.max ?? 12))
</script>

<template>
  <scroll-row
    v-if="channels.length"
    :title="title"
    :count="totalCount ?? channels.length"
    :to="to"
  >
    <live-tv-live-channel-card
      v-for="ch in visible"
      :key="ch.id"
      :channel="ch"
      :get-epg="getEpg"
      :is-favorite="isFavorite"
      :is-offline="isOffline"
      class="w-44 shrink-0 sm:w-48 lg:w-52"
      @play="emit('play', $event)"
      @toggle-favorite="emit('toggleFavorite', $event)"
    />
  </scroll-row>
</template>
