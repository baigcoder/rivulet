<script setup lang="ts">
import type { EpgProgram, LiveChannel } from '~/utils/iptv'
/**
 * Traditional cable/satellite-style TV guide grid.
 *
 * Layout:
 *   - Y axis: list of channels (filtered by current country/category
 *     from the live TV store)
 *   - X axis: time, with a sticky time scale at the top
 *   - Each cell: program block (LiveGuideProgram) positioned by time
 *   - A red horizontal "now" line at the current time
 *   - Auto-scrolls on mount so the current time is at the left edge
 *
 * EPG data is loaded per visible channel via loadFreeEpg. The Rust
 * pipeline already caches it on disk (7d) and in memory (1h) so
 * reopening the guide is instant after the first visit.
 */
import { mdiClockOutline } from '@mdi/js'

const props = defineProps<{
  channels: LiveChannel[]
  /** Map of channel-id -> EPG programs (may be empty while loading).
   *  Prefer `getEpg` when available; avoids re-building a fresh Map on
   *  every cache write which forces the whole grid to see a new prop
   *  reference and re-render. */
  epgByChannel?: Map<string, EpgProgram[]>
  /** Optional direct EPG getter (same shape as all the browse-list
   *  components). When given, rows read lazily through it so a single
   *  cache write doesn't force every row to observe a new array ref. */
  getEpg?: (channelId: string) => EpgProgram[]
  /** Optional country name shown as a section header. */
  heading?: string
}>()

defineEmits<{
  /** Click on a program block. */
  program: [channel: LiveChannel, program: EpgProgram]
  /** Click on the channel name (not a program). */
  channel: [channel: LiveChannel]
}>()

function programsFor(ch: LiveChannel): EpgProgram[] {
  if (props.getEpg)
    return props.getEpg(ch.id)
  return props.epgByChannel?.get(ch.id) ?? []
}

const STICKY_COL_WIDTH = 160 // px — wide enough for logo + name + flag
const MINUTE_WIDTH = 4 // px — a 4-hour range is 960px wide
const ROW_HEIGHT = 56
const TIME_AXIS_HEIGHT = 28

const now = ref(Math.floor(Date.now() / 1000))
let nowTimer: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  // Update the "now" indicator every minute. Cheap; doesn't re-render
  // the grid, just the line position.
  nowTimer = setInterval(() => {
    now.value = Math.floor(Date.now() / 1000)
  }, 30_000)
})
onUnmounted(() => {
  if (nowTimer)
    clearInterval(nowTimer)
})

/** Round down to the start of the hour for a clean anchor. */
const anchorTime = computed(() => Math.floor(now.value / 3600) * 3600)
const totalMinutes = computed(() => 4 * 60) // 4 hours of grid

const stripWidth = computed(() => totalMinutes.value * MINUTE_WIDTH)

/** Time labels for the axis: every hour. */
const hourLabels = computed(() => {
  const labels: { minuteOffset: number, label: string }[] = []
  for (let m = 0; m <= totalMinutes.value; m += 60) {
    const t = anchorTime.value + m * 60
    const d = new Date(t * 1000)
    labels.push({
      minuteOffset: m,
      label: d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
    })
  }
  return labels
})

/** Position of the "now" line within the strip. */
const nowOffset = computed(() => {
  const min = (now.value - anchorTime.value) / 60
  return `${min * MINUTE_WIDTH}px`
})

const hasAnyEpg = computed(() => {
  for (const ch of props.channels) {
    const progs = programsFor(ch)
    if (progs && progs.length > 0)
      return true
  }
  return false
})

const scrollRef = ref<HTMLElement>()

onMounted(() => {
  // Auto-scroll so the "now" line is near the left edge.
  nextTick(() => {
    if (scrollRef.value) {
      const offset = (now.value - anchorTime.value) / 60 * MINUTE_WIDTH
      scrollRef.value.scrollLeft = Math.max(0, offset - 60)
    }
  })
})
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- Section heading -->
    <div v-if="heading" class="border-b border-white/5 px-4 py-2 md:px-6">
      <h2 class="text-title-large font-semibold">
        {{ heading }}
      </h2>
    </div>

    <!-- Empty state -->
    <div v-if="channels.length === 0" class="grid flex-1 place-items-center">
      <div class="text-center">
        <v-icon :icon="mdiClockOutline" size="48" class="mb-3 opacity-25" />
        <p class="text-body-medium opacity-60">
          {{ $t('No channels match your filters') }}
        </p>
      </div>
    </div>

    <div v-else-if="!hasAnyEpg" class="grid flex-1 place-items-center">
      <div class="text-center">
        <v-icon :icon="mdiClockOutline" size="48" class="mb-3 opacity-25" />
        <p class="text-body-medium opacity-60">
          {{ $t('No EPG data available for these channels') }}
        </p>
      </div>
    </div>

    <div v-else ref="scrollRef" class="flex-1 overflow-auto">
      <!-- Sticky time axis -->
      <div
        class="sticky top-0 z-20 flex border-b border-white/10 bg-surface"
        :style="{ height: `${TIME_AXIS_HEIGHT}px` }"
      >
        <div
          class="sticky left-0 z-10 shrink-0 border-r border-white/5 bg-surface"
          :style="{ width: `${STICKY_COL_WIDTH}px` }"
        />
        <div class="relative shrink-0" :style="{ width: `${stripWidth}px` }">
          <div
            v-for="h in hourLabels"
            :key="h.minuteOffset"
            class="absolute top-0 flex h-full items-end border-l border-white/10 pb-1 pl-1 text-[10px] text-white/50"
            :style="{ left: `${h.minuteOffset * MINUTE_WIDTH}px` }"
          >
            {{ h.label }}
          </div>
        </div>
      </div>

      <!-- Channel rows + now line -->
      <div class="relative">
        <live-tv-live-guide-row
          v-for="ch in channels"
          :key="ch.id"
          :channel="ch"
          :programs="programsFor(ch)"
          :strip-left="STICKY_COL_WIDTH"
          :minute-width="MINUTE_WIDTH"
          :anchor-time="anchorTime"
          :now="now"
          :row-height="ROW_HEIGHT"
          @program="(p) => $emit('program', ch, p)"
          @channel="$emit('channel', $event)"
        />

        <!-- "Now" line: absolutely positioned over the whole grid, not
             per-row, so it doesn't get clipped by row overflow:hidden. -->
        <div
          class="pointer-events-none absolute top-0 z-10 h-full w-0.5 bg-red-500"
          :style="{ left: `${STICKY_COL_WIDTH + parseFloat(nowOffset)}px` }"
        />
      </div>
    </div>
  </div>
</template>
