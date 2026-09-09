<script lang="ts" setup>
// Seek and volume control for the player. Not a v-slider: this one has to draw
// a buffered range behind the fill, show a time bubble under the cursor, and
// tell the player when a drag starts so polling stops fighting the thumb.
const props = withDefaults(defineProps<{
  modelValue: number
  max?: number
  /** Dimmer range behind the fill — how far the stream is buffered. */
  buffered?: number
  /** Bubble text for the position under the cursor. Omit for no bubble. */
  format?: (v: number) => string
  /** Frame to show above the bubble. Whoever answers `hover` decides if there is one. */
  thumb?: string | null
  /** That frame is a neighbour's, not this exact position's — so it's faded. */
  approx?: boolean
  disabled?: boolean
  /** How far one arrow-key press moves it — seconds for seek, percent for volume. */
  step?: number
  /** Chapter markers to display on the rail. */
  chapters?: { time: number, title?: string }[]
}>(), { max: 100, buffered: 0 })

const emit = defineEmits<{
  /** Live, every frame of a drag. */
  'update:modelValue': [value: number]
  /** Pointer released — the value to actually commit. */
  'change': [value: number]
  /** True while dragging. */
  'scrub': [active: boolean]
  /** Where the cursor is, null once it leaves. Fires per move — debounce it. */
  'hover': [value: number | null]
}>()

const el = ref<HTMLElement | null>(null)
const hover = ref<number | null>(null)
const dragging = ref(false)

function valueAt(e: PointerEvent) {
  const r = el.value!.getBoundingClientRect()
  return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * props.max
}

function onDown(e: PointerEvent) {
  if (props.disabled)
    return
  // Capture so a fast drag that leaves the 6px rail keeps scrubbing.
  el.value!.setPointerCapture(e.pointerId)
  dragging.value = true
  emit('scrub', true)
  emit('update:modelValue', valueAt(e))
}

function onMove(e: PointerEvent) {
  if (props.disabled)
    return
  hover.value = valueAt(e)
  emit('hover', hover.value)
  if (dragging.value)
    emit('update:modelValue', hover.value)
}

function onLeave() {
  if (el.value?.matches(':focus'))
    return
  hover.value = null
  emit('hover', null)
}

function onUp(e: PointerEvent) {
  if (!dragging.value)
    return
  dragging.value = false
  emit('scrub', false)
  emit('change', valueAt(e))
}

/** Position along the rail, 0–1. */
function frac(v: number) {
  return Math.max(0, Math.min(1, v / (props.max || 1)))
}

function pct(v: number) {
  return `${frac(v) * 100}%`
}

// The rail is a d-pad target of its own: on a remote, left/right on the seek bar
// is the only way to scrub. preventDefault keeps the player's global seek keys
// and the d-pad's focus moves off the same press.
const KEYS: Record<string, (v: number) => number> = {
  ArrowLeft: v => v - (props.step ?? props.max / 100),
  ArrowRight: v => v + (props.step ?? props.max / 100),
  Home: () => 0,
  End: () => props.max,
}

function onKey(e: KeyboardEvent) {
  const next = KEYS[e.key]
  if (props.disabled || !next)
    return
  e.preventDefault()
  const value = Math.max(0, Math.min(props.max, next(props.modelValue)))
  hover.value = value
  emit('hover', value)
  emit('update:modelValue', value)
  emit('change', value)
}

function onFocus() {
  if (props.disabled)
    return
  hover.value = props.modelValue
  emit('hover', hover.value)
}

function onBlur() {
  if (dragging.value)
    return
  hover.value = null
  emit('hover', null)
}

const chapterTitle = computed(() => {
  const t = hover.value
  const list = props.chapters
  if (t == null || !list?.length)
    return ''
  let title = ''
  for (const ch of list) {
    if (ch.time <= t)
      title = ch.title || ''
    else break
  }
  return title
})
</script>

<template>
  <!-- h-10 is the pointer and remote target (~40px at 10 feet). The painted
       rail stays 3px; `group` thickens it and shows the knob on hover *or*
       focus, because a TV never hovers. -->
  <div
    ref="el"
    class="group relative flex items-center rounded touch-none"
    :class="[
      format ? 'h-10' : 'h-4',
      disabled ? 'pointer-events-none cursor-default opacity-40' : 'cursor-pointer',
    ]"
    role="slider"
    :tabindex="disabled ? -1 : 0"
    :aria-valuenow="Math.round(modelValue)"
    :aria-valuemin="0"
    :aria-valuemax="max"
    @keydown="onKey"
    @focus="onFocus"
    @blur="onBlur"
    @pointerdown="onDown"
    @pointermove="onMove"
    @pointerup="onUp"
    @pointercancel="onUp"
    @pointerleave="onLeave"
  >
    <div
      class="relative w-full overflow-hidden rounded-full bg-white/22 transition-[height] duration-120 group-hover:h-[5px] group-focus-within:h-[5px]"
      :class="dragging ? 'h-[5px]' : 'h-[3px]'"
    >
      <div class="absolute inset-y-0 left-0 bg-white/35" :style="{ width: pct(buffered) }" />
      <div class="absolute inset-y-0 left-0 bg-primary" :style="{ width: pct(modelValue) }" />
      <div
        v-for="(ch, i) in chapters"
        :key="i"
        class="absolute top-0 h-full w-px bg-white/40"
        :style="{ left: `calc(7px + (100% - 14px) * ${frac(ch.time)})` }"
        :title="ch.title || format?.(ch.time) || ''"
      />
    </div>

    <!-- Sized to match VSlider's thumbSize in vuetify.config.ts. Its travel is
         inset by its own radius so the ends stay inside the rail — the volume
         slider's parent clips, and half a knob is what showed at 100%. -->
    <div
      class="absolute top-1/2 size-3.5 rounded-full bg-primary transition-transform duration-120 -translate-x-1/2 -translate-y-1/2 group-hover:scale-100 group-focus-within:scale-100"
      :class="dragging ? 'scale-100' : 'scale-0'"
      :style="{ left: `calc(7px + (100% - 14px) * ${frac(modelValue)})` }"
    />

    <!-- Always `data-cut`: on X11/Win32 the card hangs into the picture, and
         a hole that waits for the JPEG never opens. Half a card is 7.5rem. -->
    <div
      v-if="format && hover !== null"
      data-cut
      class="pointer-events-none absolute bottom-5 z-10 flex w-60 flex-col overflow-hidden rounded-lg bg-black shadow-lg -translate-x-1/2"
      :style="{ left: `clamp(7.5rem, ${pct(hover)}, calc(100% - 7.5rem))` }"
    >
      <div class="relative aspect-video w-full bg-white/10">
        <img
          v-if="thumb"
          :src="thumb"
          alt=""
          class="absolute inset-0 h-full w-full object-cover transition-opacity duration-100"
          :class="approx ? 'opacity-55' : 'opacity-100'"
        >
      </div>
      <div class="px-2 py-1 text-center">
        <div class="text-label-large tabular-nums">
          {{ format(hover) }}
        </div>
        <div v-if="chapterTitle" class="truncate text-body-small opacity-70">
          {{ chapterTitle }}
        </div>
      </div>
    </div>
  </div>
</template>
