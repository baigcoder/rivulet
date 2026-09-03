<script setup lang="ts">
import type { EpgProgram, IPTVChannel } from '~/types/premium'

const props = defineProps<{
  title: string
  channels: IPTVChannel[]
  nowNext: (id: string) => { now: EpgProgram | null, next: EpgProgram | null }
  favorite: (id: string) => boolean
  max?: number
}>()

const emit = defineEmits<{
  play: [channel: IPTVChannel]
  toggleFavorite: [channel: IPTVChannel]
}>()

const visible = computed(() => props.channels.slice(0, props.max ?? 12))
</script>

<template>
  <scroll-row
    v-if="channels.length"
    :title="title"
    :count="channels.length"
  >
    <premium-tv-premium-channel-card
      v-for="ch in visible"
      :key="ch.id"
      :channel="ch"
      :now-next="nowNext"
      :favorite="favorite"
      class="w-44 shrink-0 sm:w-48 lg:w-52"
      @play="emit('play', $event)"
      @toggle-favorite="emit('toggleFavorite', $event)"
    />
  </scroll-row>
</template>
