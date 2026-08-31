<script setup lang="ts">
/**
 * The virtualized channel grid.
 *
 * A provider with five thousand channels is normal and one with twenty
 * thousand exists, so the number of mounted cards is bounded by the
 * viewport and nothing else: only the rows the virtualizer asks for are
 * in the DOM, and the array behind them is a `shallowRef` so a page
 * append does not deep-proxy several thousand objects.
 *
 * Rows are **measured**, not estimated. A card's logo box is a fixed
 * height but its name can wrap and its guide bar comes and goes, so the
 * row is still not knowable in advance — and being wrong makes
 * absolutely-positioned rows overlap. `measureElement` reads what the row
 * actually is, which is also why `content-visibility` is *not* used on
 * the cards here: it would hide the very height being measured.
 */
import type { ComponentPublicInstance } from 'vue'
import type { EpgProgram, IPTVChannel } from '~/types/premium'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { useDebounceFn, useElementSize } from '@vueuse/core'
import { computed, nextTick, ref, shallowRef, triggerRef, watch } from 'vue'

const props = defineProps<{
  channels: IPTVChannel[]
  nowNext: (id: string) => { now: EpgProgram | null, next: EpgProgram | null }
  favorite: (id: string) => boolean
  density?: 'compact' | 'comfortable'
  /** Batched now/next fetch for the rows currently on screen. */
  loadEpg?: (ids: string[]) => void
  hasMore?: boolean
  loading?: boolean
}>()

const emit = defineEmits<{
  play: [channel: IPTVChannel]
  toggleFavorite: [channel: IPTVChannel]
  loadMore: []
}>()

/** At most this many channels are asked about per EPG batch. */
const EPG_BATCH = 20

/** Debounce, so dragging a scrollbar is one fetch and not one per frame. */
const EPG_DEBOUNCE_MS = 600

const scrollRef = ref<HTMLElement>()
const { width: gridWidth } = useElementSize(scrollRef)

/**
 * Must mirror the template's breakpoint classes exactly. The virtualizer
 * groups cards into rows itself, so a `cols` that disagrees with the CSS
 * leaves empty columns or wraps a row in two.
 */
const cols = computed(() => {
  const width = gridWidth.value
  if (props.density === 'compact') {
    if (width >= 1280)
      return 8
    if (width >= 1024)
      return 6
    if (width >= 768)
      return 5
    if (width >= 640)
      return 4
    return 3
  }
  if (width >= 1280)
    return 6
  if (width >= 1024)
    return 5
  if (width >= 768)
    return 4
  if (width >= 640)
    return 3
  return 2
})

const rows = shallowRef<IPTVChannel[][]>([])

function rebuildRows(): void {
  const c = cols.value
  const ch = props.channels
  const result: IPTVChannel[][] = []
  for (let i = 0; i < ch.length; i += c)
    result.push(ch.slice(i, i + c))
  rows.value = result
  triggerRef(rows)
}

// Appending a page must not re-key every existing row: that re-mounts every
// card on screen, which is a visible flash and a fresh logo request each.
let prevLen = 0
watch(
  () => props.channels,
  next => {
    const c = cols.value
    if (next.length < prevLen) {
      prevLen = next.length
      rebuildRows()
      return
    }
    if (next.length > prevLen) {
      const result = rows.value.slice()
      const last = result[result.length - 1]
      let i = prevLen
      // The last row of the previous page is usually partial; fill it
      // before starting new rows, or the grid grows a ragged gap.
      if (last && last.length < c) {
        const tail = [...last]
        while (i < next.length && tail.length < c) {
          const item = next[i++]
          if (item)
            tail.push(item)
        }
        result[result.length - 1] = tail
      }
      for (; i < next.length; i += c)
        result.push(next.slice(i, i + c))
      prevLen = next.length
      rows.value = result
      triggerRef(rows)
    }
  },
  { immediate: true },
)

const virtualizer = useVirtualizer(computed(() => ({
  count: rows.value.length,
  getScrollElement: () => scrollRef.value ?? null,
  // Only the first paint and the scrollbar length depend on this; every
  // row is remeasured once it mounts. The numbers are the card's own
  // fixed logo box plus its text block plus the row gap — a card is a
  // fixed height now, so these are close rather than a guess.
  estimateSize: () => props.density === 'compact' ? 116 : 144,
  overscan: 4,
})))

watch(cols, () => {
  rebuildRows()
  void nextTick(() => virtualizer.value?.measure())
})

/** Vue hands a function ref the element (or a component instance). */
function measure(el: Element | ComponentPublicInstance | null): void {
  if (el instanceof HTMLElement)
    virtualizer.value?.measureElement(el)
}

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
      if (ids.length >= EPG_BATCH)
        break
      ids.push(ch.id)
    }
    if (ids.length >= EPG_BATCH)
      break
  }
  if (ids.length > 0)
    props.loadEpg(ids)
}, EPG_DEBOUNCE_MS)

// The visible *range* is what matters, not the count: scrolling inside a
// page of unchanged length still brings new channels into view.
let lastFirst = -1
let lastLast = -1
watch(
  () => virtualizer.value?.getVirtualItems(),
  items => {
    if (!items || items.length === 0)
      return
    const firstItem = items[0]
    const lastItem = items[items.length - 1]
    if (!firstItem || !lastItem)
      return
    if (firstItem.index !== lastFirst || lastItem.index !== lastLast) {
      lastFirst = firstItem.index
      lastLast = lastItem.index
      void triggerEpg()
    }
    if (props.hasMore && !props.loading && lastItem.index >= rows.value.length - 2)
      emit('loadMore')
  },
)

defineExpose({
  /** So a page can send the remote back to the top of the list. */
  scrollToTop: () => virtualizer.value?.scrollToOffset(0),
})
</script>

<template>
  <div ref="scrollRef" class="h-full overflow-y-auto" data-dpad-start>
    <div :style="{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }">
      <div
        v-for="virtualRow in virtualizer.getVirtualItems()"
        :key="virtualRow.index"
        :ref="measure"
        :data-index="virtualRow.index"
        :style="{
          position: 'absolute',
          top: 0,
          left: 0,
          right: 0,
          transform: `translateY(${virtualRow.start}px)`,
        }"
      >
        <div
          class="grid gap-3 pb-3"
          :class="{
            'grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6': density !== 'compact',
            'grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8': density === 'compact',
          }"
        >
          <premium-tv-premium-channel-card
            v-for="ch in rows[virtualRow.index]"
            :key="ch.id"
            :channel="ch"
            :now-next="nowNext"
            :favorite="favorite"
            :compact="density === 'compact'"
            @play="emit('play', $event)"
            @toggle-favorite="emit('toggleFavorite', $event)"
          />
        </div>
      </div>
    </div>
  </div>
</template>
