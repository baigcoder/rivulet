<script setup lang="ts">
/**
 * One program block in the TV guide grid. Positioned absolutely by
 * the parent row, colored by category, shows program title and time.
 */
const props = defineProps<{
  title: string
  start: number // Unix timestamp (seconds)
  stop: number // Unix timestamp (seconds)
  /** Category id, for color. Falls back to a default gray. */
  category?: string
  now?: number // Current time, for "live" indicator
}>()

defineEmits<{
  click: []
}>()

const CATEGORY_COLORS: Record<string, string> = {
  news: 'bg-blue-600/80 border-blue-400/30',
  sports: 'bg-red-600/80 border-red-400/30',
  movies: 'bg-purple-600/80 border-purple-400/30',
  series: 'bg-indigo-600/80 border-indigo-400/30',
  kids: 'bg-green-600/80 border-green-400/30',
  music: 'bg-pink-600/80 border-pink-400/30',
  entertainment: 'bg-orange-600/80 border-orange-400/30',
  documentary: 'bg-amber-600/80 border-amber-400/30',
  education: 'bg-teal-600/80 border-teal-400/30',
  general: 'bg-slate-600/80 border-slate-400/30',
  default: 'bg-slate-600/80 border-slate-400/30',
}

const colorClass = computed(() => {
  const cat = (props.category ?? 'default').toLowerCase()
  return CATEGORY_COLORS[cat] ?? CATEGORY_COLORS.default
})

const isLive = computed(() => {
  if (!props.now)
    return false
  return props.now >= props.start && props.now < props.stop
})

const timeRange = computed(() => {
  const fmt = (ts: number) => {
    const d = new Date(ts * 1000)
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }
  return `${fmt(props.start)} – ${fmt(props.stop)}`
})

const duration = computed(() => Math.max(15, Math.round((props.stop - props.start) / 60)))
</script>

<template>
  <button
    type="button"
    class="absolute top-0.5 h-[calc(100%-4px)] cursor-pointer overflow-hidden rounded border-l-2 px-2 py-1 text-left text-white shadow-sm transition-transform hover:scale-[1.02] hover:z-10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    :class="colorClass"
    :style="{ width: `${duration}px` }"
    :title="`${title}\n${timeRange}`"
    @click.stop="$emit('click')"
  >
    <div class="line-clamp-2 text-body-small font-medium leading-tight">
      {{ title }}
    </div>
    <div class="mt-0.5 flex items-center gap-1 text-[10px] opacity-75">
      <span
        v-if="isLive"
        class="rounded-full bg-red-500 px-1 py-px text-[9px] font-bold uppercase text-white"
      >
        LIVE
      </span>
      <span class="truncate">{{ timeRange }}</span>
    </div>
  </button>
</template>
