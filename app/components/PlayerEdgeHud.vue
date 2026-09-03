<script setup lang="ts">
/**
 * The chip a volume / brightness / seek swipe paints in the middle of the
 * picture. Not a control — the drag is on the film. Decorative, so it stays
 * out of the d-pad (`pointer-events-none`).
 */
import { mdiBrightness6, mdiFastForward, mdiRewind, mdiVolumeHigh, mdiVolumeLow, mdiVolumeMedium, mdiVolumeOff } from '@mdi/js'
import { computed } from 'vue'

const props = defineProps<{
  kind: 'volume' | 'brightness' | 'seek'
  level: number
  caption?: string
}>()

const icon = computed(() => {
  if (props.kind === 'seek')
    return (props.caption ?? '').startsWith('-') ? mdiRewind : mdiFastForward
  if (props.kind === 'brightness')
    return mdiBrightness6
  if (props.level <= 0)
    return mdiVolumeOff
  if (props.level < 34)
    return mdiVolumeLow
  return props.level < 67 ? mdiVolumeMedium : mdiVolumeHigh
})
</script>

<template>
  <div
    class="pointer-events-none absolute left-1/2 top-1/2 z-50 flex -translate-x-1/2 -translate-y-1/2 items-center gap-3 rounded-2xl bg-black/80 px-4 py-3 text-white"
  >
    <v-icon :icon="icon" size="22" />
    <div class="relative h-28 w-1.5 overflow-hidden rounded-full bg-white/20">
      <div
        class="absolute bottom-0 w-full rounded-full bg-white"
        :style="{ height: `${level}%` }"
      />
    </div>
    <span class="min-w-10 text-label-large tabular-nums">{{ caption ?? `${level}%` }}</span>
  </div>
</template>
