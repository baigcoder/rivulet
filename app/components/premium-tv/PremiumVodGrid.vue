<script setup lang="ts">
/**
 * Virtualized poster grid for Premium movies or series.
 *
 * Column count is derived from the scroll width and a target poster width,
 * not Tailwind breakpoints — 2:3 tiles get huge fast if the virtualizer
 * and CSS disagree, or if a wide row only gets three columns.
 */
import type { ComponentPublicInstance } from 'vue'
import type { PremiumSeriesItem, PremiumVodItem } from '~/types/premium'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { useElementSize } from '@vueuse/core'
import { computed, nextTick, ref, shallowRef, triggerRef, watch } from 'vue'

const props = defineProps<{
  kind: 'movie' | 'series'
  movies?: PremiumVodItem[]
  series?: PremiumSeriesItem[]
  loading?: boolean
  hasMore?: boolean
  density?: 'compact' | 'comfortable'
}>()

const emit = defineEmits<{
  openMovie: [item: PremiumVodItem]
  openSeries: [item: PremiumSeriesItem]
  loadMore: []
}>()

const GAP = 12

const scrollRef = ref<HTMLElement>()
const { width: gridWidth } = useElementSize(scrollRef)

const items = computed(() => props.kind === 'movie' ? (props.movies ?? []) : (props.series ?? []))

/** Target poster width in px — keeps rows dense on desktop without tiny phone tiles. */
function columnCount(width: number): number {
  if (width < 1)
    return props.density === 'compact' ? 3 : 2
  const target = props.density === 'compact' ? 108 : 124
  const min = props.density === 'compact' ? 3 : 2
  return Math.max(min, Math.min(12, Math.floor((width + GAP) / (target + GAP))))
}

const cols = computed(() => columnCount(gridWidth.value))

const rowStyle = computed(() => ({
  gridTemplateColumns: `repeat(${cols.value}, minmax(0, 1fr))`,
}))

const rowEstimate = computed(() => {
  const w = gridWidth.value
  const c = cols.value
  if (w < 1 || c < 1)
    return props.density === 'compact' ? 180 : 200
  const cardW = (w - GAP * (c - 1)) / c
  return cardW * 1.5 + GAP
})

const rows = shallowRef<(PremiumVodItem | PremiumSeriesItem)[][]>([])

function rebuildRows(): void {
  const c = cols.value
  const list = items.value
  const result: (PremiumVodItem | PremiumSeriesItem)[][] = []
  for (let i = 0; i < list.length; i += c)
    result.push(list.slice(i, i + c))
  rows.value = result
  triggerRef(rows)
}

let prevLen = 0
watch(items, next => {
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
}, { immediate: true })

const virtualizer = useVirtualizer(computed(() => ({
  count: rows.value.length,
  getScrollElement: () => scrollRef.value ?? null,
  estimateSize: () => rowEstimate.value,
  overscan: 2,
})))

watch(cols, () => {
  rebuildRows()
  void nextTick(() => virtualizer.value?.measure())
})

function measure(el: Element | ComponentPublicInstance | null): void {
  if (el instanceof HTMLElement)
    virtualizer.value?.measureElement(el)
}

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
  () => virtualizer.value?.getVirtualItems()?.length,
  () => maybeLoadMore(),
)
</script>

<template>
  <div ref="scrollRef" class="min-h-0 flex-1 overflow-y-auto pb-4" data-dpad-start>
    <div
      v-if="loading && items.length === 0"
      class="grid gap-3"
      :style="rowStyle"
      role="status"
      :aria-label="$t('Loading…')"
    >
      <div
        v-for="n in 18"
        :key="n"
        class="aspect-[2/3] animate-pulse rounded-xl bg-surface-container-high/70"
      />
    </div>

    <div
      v-else-if="items.length === 0"
      class="grid place-items-center px-6 py-16 text-center"
    >
      <p class="text-body-medium opacity-60">
        {{ $t('Nothing here yet. Try another category or search.') }}
      </p>
    </div>

    <div
      v-else
      :style="{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }"
    >
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
          :style="rowStyle"
        >
          <template v-if="kind === 'movie'">
            <premium-tv-premium-vod-card
              v-for="item in (rows[virtualRow.index] as PremiumVodItem[])"
              :id="item.id"
              :key="item.id"
              :name="item.name"
              :poster-url="item.posterUrl"
              :rating="item.rating"
              :category-name="item.categoryName"
              kind="movie"
              :compact="density === 'compact'"
              :show-caption="false"
              @open="emit('openMovie', item)"
            />
          </template>
          <template v-else>
            <premium-tv-premium-vod-card
              v-for="item in (rows[virtualRow.index] as PremiumSeriesItem[])"
              :id="item.id"
              :key="item.id"
              :name="item.name"
              :poster-url="item.posterUrl"
              :rating="item.rating"
              kind="series"
              :compact="density === 'compact'"
              :show-caption="false"
              @open="emit('openSeries', item)"
            />
          </template>
        </div>
      </div>
    </div>

    <div v-if="loading && items.length > 0" class="grid place-items-center py-6">
      <v-progress-circular indeterminate color="primary" size="28" />
    </div>
  </div>
</template>
