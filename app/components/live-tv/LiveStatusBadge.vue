<script setup lang="ts">
/**
 * LIVE / Offline chip. Unknown is still LIVE — it is a live catalogue —
 * but the pulse is reserved for a channel that has actually painted a
 * frame this session. Colour is not the only signal: the word changes.
 */
import type { Health } from '~/utils/livehealth'
import { computed } from 'vue'

const props = withDefaults(defineProps<{
  health: Health
  compact?: boolean
}>(), {
  compact: false,
})

const dead = computed(() => props.health === 'offline')
const confirmed = computed(() => props.health === 'live')
</script>

<template>
  <span
    class="inline-flex items-center gap-1 rounded px-1 font-semibold uppercase tracking-wide"
    :class="[
      compact ? 'py-px text-[9px]' : 'py-0.5 text-[10px]',
      dead ? 'bg-zinc-800/90 text-white/75' : 'bg-red-600 text-white',
    ]"
  >
    <span
      v-if="!dead"
      class="size-1.5 rounded-full bg-white"
      :class="confirmed ? 'animate-pulse' : 'opacity-70'"
      aria-hidden="true"
    />
    {{ dead ? $t('Offline') : $t('LIVE') }}
  </span>
</template>
