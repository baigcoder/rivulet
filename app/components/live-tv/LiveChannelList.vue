<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { useDebounceFn } from '@vueuse/core'
import { computed, ref, watch } from 'vue'

const props = defineProps<{
  channels: LiveChannel[]
  getEpg: (id: string) => Array<{ title: string, description?: string | null, start: string, stop?: string | null }>
  isFavorite: (ch: LiveChannel) => boolean
  isOffline?: (ch: LiveChannel) => boolean
  loadEpg?: (ids: string[]) => void
  hasMore?: boolean
  loading?: boolean
}>()

const emit = defineEmits<{
  play: [channel: LiveChannel]
  toggleFavorite: [channel: LiveChannel]
  loadMore: []
}>()

const scrollRef = ref<HTMLElement>()

const virtualizer = useVirtualizer(computed(() => ({
  // `channels` is populated asynchronously by the country/category pages.
  count: props.channels.length,
  getScrollElement: () => scrollRef.value ?? null,
  estimateSize: () => 56,
  overscan: 2,
})))

// Debounce so a fast scroll doesn't fire a fetch per frame, cap at 20
// channels per call so a giant list doesn't issue 200 EPG requests at once.
const triggerEpg = useDebounceFn(() => {
  if (!props.loadEpg || !virtualizer.value)
    return
  const items = virtualizer.value.getVirtualItems()
  if (items.length === 0)
    return
  const ids = items
    .map(i => props.channels[i.index]?.id)
    .filter(Boolean) as string[]
  if (ids.length > 0)
    props.loadEpg(ids.slice(0, 20))
}, 600)

watch(
  () => virtualizer.value?.getVirtualItems().length,
  () => triggerEpg(),
)

watch(
  () => virtualizer.value?.getVirtualItems().at(-1)?.index ?? -1,
  last => {
    if (!props.hasMore || props.loading || last < 0)
      return
    const el = scrollRef.value
    if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 240)
      emit('loadMore')
  },
)
</script>

<template>
  <div
    ref="scrollRef"
    class="h-full overflow-y-auto"
    data-dpad-start
  >
    <div
      :style="{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }"
    >
      <div
        v-for="virtualRow in virtualizer.getVirtualItems()"
        :key="virtualRow.index"
        :style="{
          position: 'absolute',
          top: 0,
          left: 0,
          right: 0,
          transform: `translateY(${virtualRow.start}px)`,
        }"
      >
        <live-tv-live-channel-row
          v-if="channels[virtualRow.index]"
          :channel="channels[virtualRow.index]!"
          :get-epg="getEpg"
          :is-favorite="isFavorite"
          :is-offline="isOffline"
          class="mb-1"
          @play="emit('play', $event)"
          @toggle-favorite="emit('toggleFavorite', $event)"
        />
      </div>
    </div>
  </div>
</template>
