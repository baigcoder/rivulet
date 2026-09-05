<script setup lang="ts">
import type { ComponentPublicInstance } from 'vue'
import type { PremiumEpisode } from '~/types/premium'
import { mdiPlay } from '@mdi/js'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { useResizeObserver } from '@vueuse/core'

const props = defineProps<{ episodes: PremiumEpisode[] }>()

const emit = defineEmits<{ play: [ep: PremiumEpisode] }>()

const scrollRef = inject<Ref<HTMLElement | undefined>>('detailScroller')
const anchorRef = ref<HTMLElement>()
const margin = ref(0)

function measureMargin() {
  const scroller = scrollRef?.value
  const anchor = anchorRef.value
  margin.value = scroller && anchor
    ? anchor.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop
    : 0
}

useResizeObserver(anchorRef, measureMargin)
watch(() => props.episodes.length, () => nextTick(measureMargin), { immediate: true })

const virtualizer = useVirtualizer(computed(() => ({
  count: props.episodes.length,
  getScrollElement: () => scrollRef?.value ?? null,
  scrollMargin: margin.value,
  estimateSize: () => 76,
  overscan: 8,
})))

function measure(el: Element | ComponentPublicInstance | null): void {
  if (el instanceof HTMLElement)
    virtualizer.value?.measureElement(el)
}
</script>

<template>
  <div ref="anchorRef">
    <div
      v-if="episodes.length"
      :style="{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }"
    >
      <div
        v-for="virtualRow in virtualizer.getVirtualItems()"
        :key="virtualRow.index"
        :ref="measure"
        class="pb-1"
        :style="{
          position: 'absolute',
          top: 0,
          left: 0,
          right: 0,
          transform: `translateY(${virtualRow.start - margin}px)`,
        }"
      >
        <button
          v-if="episodes[virtualRow.index]"
          type="button"
          class="flex w-full items-center gap-3 rounded-xl px-3 py-3 text-start transition-colors hover:bg-surface-container-high focus-visible:bg-surface-container-high focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          @click="emit('play', episodes[virtualRow.index]!)"
        >
          <span class="grid size-11 shrink-0 place-items-center rounded-full bg-primary/15 text-primary">
            <v-icon :icon="mdiPlay" size="20" />
          </span>
          <span class="min-w-0 flex-1">
            <span class="block text-body-medium font-semibold">
              {{ $t('Episode {n}', { n: episodes[virtualRow.index]!.episode }) }}
              <template v-if="episodes[virtualRow.index]!.title">
                — {{ episodes[virtualRow.index]!.title }}
              </template>
            </span>
            <span v-if="episodes[virtualRow.index]!.plot" class="line-clamp-2 text-body-small opacity-55">
              {{ episodes[virtualRow.index]!.plot }}
            </span>
          </span>
        </button>
      </div>
    </div>
  </div>
</template>
