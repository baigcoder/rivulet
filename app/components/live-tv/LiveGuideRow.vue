<script setup lang="ts">
import type { EpgProgram, LiveChannel } from '~/utils/iptv'
/**
 * One channel row in the TV guide grid. The left column is sticky
 * (channel logo + name); the right side is a relative-positioned
 * strip that holds program blocks.
 */
import { mdiPlay } from '@mdi/js'

const props = defineProps<{
  channel: LiveChannel
  programs: EpgProgram[]
  /** "left" pixel offset where the program strip starts (sticky column width). */
  stripLeft: number
  /** "width" pixel size of each minute. */
  minuteWidth: number
  /** Anchor time (Unix seconds) — the leftmost visible minute. */
  anchorTime: number
  /** Current time, for the "live" indicator. */
  now: number
  /** Pixel height of the row. */
  rowHeight: number
}>()

defineEmits<{
  /** Click on a program block. */
  program: [program: EpgProgram]
  /** Click on the channel name (not a program). */
  channel: [channel: LiveChannel]
}>()

/** Position a program on the strip. `left` is pixels from the strip's left edge. */
function programStyle(p: EpgProgram) {
  const start = new Date(p.start).getTime() / 1000
  const stop = p.stop ? new Date(p.stop).getTime() / 1000 : start + 1800
  const startMin = (start - props.anchorTime) / 60
  const widthMin = Math.max(15, (stop - start) / 60)
  return {
    left: `${startMin * props.minuteWidth}px`,
    width: `${widthMin * props.minuteWidth}px`,
  }
}
</script>

<template>
  <div
    class="relative flex border-b border-white/5 last:border-b-0"
    :style="{ height: `${rowHeight}px` }"
  >
    <!-- Sticky channel column -->
    <button
      type="button"
      class="sticky left-0 z-10 flex shrink-0 items-center gap-2 border-r border-white/5 bg-surface px-3 py-1 text-left"
      :style="{ width: `${stripLeft}px` }"
      @click="$emit('channel', channel)"
    >
      <div class="grid size-8 shrink-0 place-items-center overflow-hidden rounded bg-surface-container">
        <img
          v-if="channel.logoUrl"
          :src="channel.logoUrl"
          :alt="channel.name"
          class="size-full object-contain p-0.5"
          @error="($event.target as HTMLImageElement).style.display = 'none'"
        >
        <v-icon v-else icon="mdiTelevision" size="16" class="opacity-30" />
      </div>
      <div class="min-w-0 flex-1">
        <p class="line-clamp-1 text-body-small font-medium">
          {{ channel.name }}
        </p>
        <p v-if="channel.countryFlag || channel.country" class="line-clamp-1 text-[10px] opacity-50">
          {{ channel.countryFlag }} {{ channel.country }}
        </p>
      </div>
      <v-icon :icon="mdiPlay" size="14" class="shrink-0 opacity-30" />
    </button>

    <!-- Program strip -->
    <div class="relative flex-1 overflow-hidden">
      <live-tv-live-guide-program
        v-for="p in programs"
        :key="`${p.channelId}-${p.start}`"
        :title="p.title"
        :start="new Date(p.start).getTime() / 1000"
        :stop="p.stop ? new Date(p.stop).getTime() / 1000 : new Date(p.start).getTime() / 1000 + 1800"
        :category="channel.categoryName ?? ''"
        :now="now"
        :style="programStyle(p)"
        @click="$emit('program', p)"
      />
    </div>
  </div>
</template>
