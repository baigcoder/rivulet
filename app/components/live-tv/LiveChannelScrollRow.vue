<script setup lang="ts">
/** A titled row of Free TV channels as full cards, with what is on now. */
import type { LiveChannel } from '~/utils/iptv'

const props = defineProps<{
  title: string
  channels: LiveChannel[]
  max?: number
}>()

const emit = defineEmits<{
  play: [channel: LiveChannel]
}>()

const liveTv = useLiveTvStore()
const visible = computed(() => props.channels.slice(0, props.max ?? 12))
</script>

<template>
  <scroll-row
    v-if="channels.length"
    :title="title"
    :count="channels.length"
  >
    <live-tv-live-channel-card
      v-for="ch in visible"
      :key="ch.id"
      :channel="ch"
      :get-epg="liveTv.getEpg"
      :is-favorite="liveTv.isFavorite"
      :is-offline="liveTv.isOffline"
      class="w-44 shrink-0 sm:w-48 lg:w-52"
      @play="emit('play', $event)"
      @toggle-favorite="liveTv.toggleFavorite($event)"
    />
  </scroll-row>
</template>
