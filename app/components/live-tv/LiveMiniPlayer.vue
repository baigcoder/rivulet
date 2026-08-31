<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { mdiClose, mdiFullscreen, mdiTelevision } from '@mdi/js'
import { useEventListener } from '@vueuse/core'
import { computed, ref, watch } from 'vue'

const props = defineProps<{
  channel: LiveChannel | null
  streamUrl: string | null
}>()

const emit = defineEmits<{
  close: []
  expand: []
}>()

const visible = computed(() => !!props.channel && !!props.streamUrl)
const imgError = ref(false)
const pos = ref({ x: 0, y: 0 })
const dragging = ref(false)
const size = ref({ w: 380, h: 220 })

function resetPos() {
  pos.value = {
    x: window.innerWidth - size.value.w - 20,
    y: window.innerHeight - size.value.h - 20,
  }
}

watch(visible, v => {
  if (v) {
    imgError.value = false
    resetPos()
  }
})

let startX = 0
let startY = 0
let startPosX = 0
let startPosY = 0

function onPointerDown(e: PointerEvent) {
  dragging.value = true
  startX = e.clientX
  startY = e.clientY
  startPosX = pos.value.x
  startPosY = pos.value.y
  ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
}

function onPointerMove(e: PointerEvent) {
  if (!dragging.value)
    return
  const dx = e.clientX - startX
  const dy = e.clientY - startY
  pos.value = {
    x: Math.max(0, Math.min(window.innerWidth - size.value.w, startPosX + dx)),
    y: Math.max(0, Math.min(window.innerHeight - size.value.h, startPosY + dy)),
  }
}

function onPointerUp() {
  dragging.value = false
}

useEventListener('resize', resetPos)

const epg = computed(() => {
  if (!props.channel)
    return null
  return useLiveTvStore().getEpg(props.channel.id)
})

const nowProgram = computed(() => {
  if (!epg.value?.length)
    return null
  const now = Date.now()
  return epg.value.find(p => {
    const s = new Date(p.start).getTime()
    const e = p.stop ? new Date(p.stop).getTime() : s + 3600000
    return now >= s && now <= e
  }) ?? null
})
</script>

<template>
  <transition name="mini">
    <div
      v-if="visible"
      class="fixed z-[60] overflow-hidden rounded-2xl border border-white/10 bg-surface-container shadow-2xl"
      :style="{ left: `${pos.x}px`, top: `${pos.y}px`, width: `${size.w}px`, height: `${size.h}px` }"
    >
      <!-- Drag handle / channel info -->
      <div
        class="flex items-center gap-2 bg-surface-container-high px-3 py-2"
        :class="dragging ? 'cursor-grabbing' : 'cursor-grab'"
        @pointerdown="onPointerDown"
        @pointermove="onPointerMove"
        @pointerup="onPointerUp"
      >
        <img
          v-if="channel?.logoUrl && !imgError"
          :src="channel.logoUrl"
          :alt="channel.name"
          class="size-6 rounded object-contain"
          @error="imgError = true"
        >
        <v-icon v-else :icon="mdiTelevision" size="16" class="opacity-40" />
        <span class="min-w-0 flex-1 truncate text-label-medium font-medium">
          {{ channel?.name }}
        </span>
        <v-btn
          :icon="mdiFullscreen"
          variant="text"
          size="x-small"
          :aria-label="$t('Expand')"
          @click.stop="emit('expand')"
        />
        <v-btn
          :icon="mdiClose"
          variant="text"
          size="x-small"
          :aria-label="$t('Close')"
          @click.stop="emit('close')"
        />
      </div>

      <!-- Playing indicator -->
      <div class="flex items-center gap-2 px-3 py-2">
        <div class="flex items-center gap-1.5">
          <span class="size-1.5 rounded-full bg-red-500 animate-pulse" />
          <span class="text-label-small text-red-400">{{ $t('LIVE') }}</span>
        </div>
        <span v-if="nowProgram" class="min-w-0 flex-1 truncate text-body-small opacity-60">
          {{ nowProgram.title }}
        </span>
      </div>
    </div>
  </transition>
</template>

<style scoped>
.mini-enter-active,
.mini-leave-active {
  transition: all 300ms cubic-bezier(0.16, 1, 0.3, 1);
}
.mini-enter-from,
.mini-leave-to {
  opacity: 0;
  transform: scale(0.8) translateY(20px);
}
</style>
