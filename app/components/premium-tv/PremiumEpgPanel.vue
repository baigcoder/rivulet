<script setup lang="ts">
/**
 * The guide for one channel: what is on, how far through it is, and what
 * follows.
 *
 * Degrading is the design, not an afterthought. A provider with no XMLTV,
 * a channel absent from the guide, and a guide whose ids do not match the
 * playlist are all common — so with no programmes this renders **nothing
 * at all** rather than an empty container with headings in it. The spec's
 * "do not show broken or empty guide containers" is that, literally.
 */
import type { EpgProgram } from '~/types/premium'
import { computed, onUnmounted, ref } from 'vue'

const props = defineProps<{
  programs: EpgProgram[]
  /** Shown while the first fetch for this channel is in flight. */
  loading?: boolean
  /** How many "up next" rows to list under the current programme. */
  upNext?: number
}>()

/** Ticks once a minute — the resolution a guide needs and no more. */
const nowSecs = ref(Math.floor(Date.now() / 1000))
const timer = setInterval(() => {
  nowSecs.value = Math.floor(Date.now() / 1000)
}, 60_000)
onUnmounted(() => clearInterval(timer))

/** Assumed length of a programme the provider gave no end time for. */
const DEFAULT_DURATION_SECS = 3600

function endOf(p: EpgProgram): number {
  return p.stop ?? p.start + DEFAULT_DURATION_SECS
}

const sorted = computed(() => [...props.programs].sort((a, b) => a.start - b.start))

const current = computed(() =>
  sorted.value.find(p => p.start <= nowSecs.value && endOf(p) > nowSecs.value) ?? null,
)

const upcoming = computed(() => {
  const limit = props.upNext ?? 4
  return sorted.value.filter(p => p.start > nowSecs.value).slice(0, limit)
})

const progress = computed(() => {
  const p = current.value
  if (!p)
    return 0
  const end = endOf(p)
  if (end <= p.start)
    return 0
  return Math.max(0, Math.min(100, Math.round(((nowSecs.value - p.start) / (end - p.start)) * 100)))
})

const remaining = computed(() => {
  const p = current.value
  if (!p)
    return 0
  return Math.max(0, Math.round((endOf(p) - nowSecs.value) / 60))
})

function clock(secs: number): string {
  return new Date(secs * 1000).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

/** Nothing to show and nothing in flight: render nothing. */
const empty = computed(() => !props.loading && !current.value && upcoming.value.length === 0)
</script>

<template>
  <div v-if="!empty" class="flex flex-col gap-3">
    <div v-if="loading && !current" class="flex items-center gap-2 text-label-medium opacity-60">
      <v-progress-circular indeterminate size="14" width="2" />
      {{ $t('Loading guide…') }}
    </div>

    <div v-if="current" class="flex flex-col gap-1.5">
      <div class="flex items-baseline justify-between gap-3">
        <h3 class="line-clamp-2 text-title-small font-semibold">
          {{ current.title }}
        </h3>
        <span class="shrink-0 text-label-small tabular-nums opacity-60">
          {{ clock(current.start) }}–{{ clock(current.stop ?? current.start + 3600) }}
        </span>
      </div>

      <div
        class="h-1 w-full overflow-hidden rounded-full bg-white/10"
        role="progressbar"
        :aria-valuenow="progress"
        aria-valuemin="0"
        aria-valuemax="100"
        :aria-label="$t('Programme progress')"
      >
        <div class="h-full rounded-full bg-primary" :style="{ width: `${progress}%` }" />
      </div>

      <p class="text-label-small opacity-55">
        {{ progress }}% · {{ $t('{minutes} min left', { minutes: remaining }) }}
      </p>

      <p v-if="current.description" class="line-clamp-3 text-body-small opacity-70">
        {{ current.description }}
      </p>
    </div>

    <div v-if="upcoming.length > 0" class="flex flex-col gap-1">
      <p class="text-label-small font-semibold uppercase tracking-wide opacity-45">
        {{ $t('Up next') }}
      </p>
      <ul class="flex flex-col gap-1">
        <li
          v-for="p in upcoming"
          :key="`${p.channelId}-${p.start}`"
          class="flex items-baseline gap-2 text-body-small"
        >
          <span class="shrink-0 tabular-nums opacity-50">{{ clock(p.start) }}</span>
          <span class="line-clamp-1 opacity-80">{{ p.title }}</span>
        </li>
      </ul>
    </div>
  </div>
</template>
