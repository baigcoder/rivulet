<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { useDebounceFn, useElementSize } from '@vueuse/core'
import { computed, nextTick, ref, shallowRef, triggerRef, watch } from 'vue'
import { liveGridColumnCount, liveGridRowEstimate } from '~/utils/liveGridColumns'

const props = defineProps<{
  channels: LiveChannel[]
  getEpg: (id: string) => Array<{ title: string, description?: string | null, start: string, stop?: string | null }>
  isFavorite: (ch: LiveChannel) => boolean
  /** Passed straight to the card; see its own prop for why it is advisory. */
  isOffline?: (ch: LiveChannel) => boolean
  density?: 'compact' | 'comfortable'
  loadEpg?: (ids: string[]) => void
  /** True when more pages are available from the server. */
  hasMore?: boolean
  /** True while a server query is in flight. */
  loading?: boolean
}>()

const emit = defineEmits<{
  play: [channel: LiveChannel]
  toggleFavorite: [channel: LiveChannel]
  loadMore: []
}>()

const scrollRef = ref<HTMLElement>()
const { width: gridWidth } = useElementSize(scrollRef)

const cols = computed(() => liveGridColumnCount(gridWidth.value, props.density ?? 'comfortable'))

const rowStyle = computed(() => ({
  gridTemplateColumns: `repeat(${cols.value}, minmax(0, 1fr))`,
}))

const rowEstimate = computed(() =>
  liveGridRowEstimate(gridWidth.value, cols.value, props.density ?? 'comfortable', true),
)

// Slice the channels into rows of `cols` width.
// `channels` change rather than on every scroll — the array is read by the
// template, and a fresh reference on every frame re-keys the v-for and
// re-mounts every card.
const rows = shallowRef<LiveChannel[][]>([])

function rebuildRows() {
  const c = cols.value
  const ch = props.channels
  const result: LiveChannel[][] = []
  for (let i = 0; i < ch.length; i += c) {
    result.push(ch.slice(i, i + c))
  }
  rows.value = result
  triggerRef(rows)
}

// When the channels array changes: if cols is unchanged and the new array is
// a strict superset of the previous one (load-more append case), only compute
// the new rows instead of rebuilding the entire structure.
let prevChannelLen = 0
watch(
  () => props.channels,
  (next, prev) => {
    const c = cols.value
    // Full rebuild when: columns changed, array shrank, or array is a completely
    // different reference whose length didn't just grow by a multiple of c.
    if (!prev || next.length < prevChannelLen) {
      prevChannelLen = next.length
      rebuildRows()
      return
    }
    // Append-only path: only slice the new channels into additional rows.
    if (next.length > prevChannelLen) {
      const existing = rows.value
      const result = existing.slice()
      // The last existing row may be incomplete (< c items); refill it first.
      const lastRow = result[result.length - 1]
      if (lastRow && lastRow.length < c) {
        const tail = [...lastRow]
        let i = prevChannelLen
        while (i < next.length && tail.length < c) {
          const item = next[i++]
          if (item)
            tail.push(item)
        }
        result[result.length - 1] = tail
        // Then append any remaining full rows.
        for (; i < next.length; i += c) {
          result.push(next.slice(i, i + c))
        }
      }
      else {
        for (let i = prevChannelLen; i < next.length; i += c) {
          result.push(next.slice(i, i + c))
        }
      }
      prevChannelLen = next.length
      rows.value = result
      triggerRef(rows)
    }
    // Same length — nothing to do.
  },
  { immediate: true },
)

const virtualizer = useVirtualizer(computed(() => ({
  // Keep the options object reactive. Reading `.value` here captured the
  // initial zero rows before the country query returned.
  count: rows.value.length,
  getScrollElement: () => scrollRef.value ?? null,
  estimateSize: () => rowEstimate.value,
  overscan: 2,
})))

// When cols changes (window resize), always do a full rebuild.
watch(cols, () => {
  rebuildRows()
  nextTick(() => virtualizer.value?.measure())
})

// Batch EPG: debounce so a fast scroll doesn't fire a fetch per frame,
// cap at 20 channels per call so a giant grid doesn't issue 200 EPG
// requests at once.
const triggerEpg = useDebounceFn(() => {
  if (!props.loadEpg || !virtualizer.value)
    return
  const items = virtualizer.value.getVirtualItems()
  if (items.length === 0)
    return
  const ids: string[] = []
  for (const item of items) {
    const row = rows.value[item.index]
    if (!row)
      continue
    for (const ch of row) {
      ids.push(ch.id)
      if (ids.length >= 20)
        break
    }
    if (ids.length >= 20)
      break
  }
  if (ids.length > 0)
    props.loadEpg(ids)
}, 600)

// Re-trigger EPG when the visible index range changes (not just the count),
// so scrolling within a page of the same length still loads EPG data.
let lastFirstIdx = -1
let lastLastIdx = -1
function maybeLoadMore() {
  if (!props.hasMore || props.loading)
    return
  const el = scrollRef.value
  if (!el)
    return
  if (el.scrollHeight - el.scrollTop - el.clientHeight < 480)
    emit('loadMore')
}

watch(
  () => {
    const items = virtualizer.value?.getVirtualItems()
    if (!items?.length)
      return ''
    return `${items[0]?.index}:${items[items.length - 1]?.index}`
  },
  range => {
    if (!range)
      return
    const [first, last] = range.split(':').map(Number)
    if (first !== lastFirstIdx || last !== lastLastIdx) {
      lastFirstIdx = first ?? -1
      lastLastIdx = last ?? -1
      triggerEpg()
    }
    maybeLoadMore()
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
        <div
          class="grid gap-3"
          :style="rowStyle"
        >
          <live-tv-live-channel-card
            v-for="ch in rows[virtualRow.index]"
            :key="ch.id"
            :channel="ch"
            :get-epg="getEpg"
            :is-favorite="isFavorite"
            :is-offline="isOffline"
            :compact="density === 'compact'"
            @play="emit('play', $event)"
            @toggle-favorite="emit('toggleFavorite', $event)"
          />
        </div>
      </div>
    </div>
  </div>
</template>
