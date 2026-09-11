<script setup lang="ts">
/**
 * One channel as a guide row: its number, logo, name, and what is on now.
 *
 * The browse grids were logo tiles only — on a TV that is six channels a
 * screen and nothing about any of them but the artwork. A row carries what a
 * viewer actually chooses by, the programme and how far into it they would
 * arrive, and twice as many fit where the tiles did. Free and Premium TV
 * share it; each grid maps its own channel type onto these props.
 *
 * Fixed at 72px on purpose: Free TV's virtualizer places rows from an
 * estimate rather than a measurement, so a row that grew would overlap the
 * next one.
 */
import { mdiStar } from '@mdi/js'
import { useNow } from '@vueuse/core'
import { computed, ref } from 'vue'
import { channelTileStyle, isPlaceholderLogoUrl, isTinyLogo } from '~/utils/channelLogo'
import { channelInitials, parseChannelName } from '~/utils/channelName'
import { proxyLogo } from '~/utils/premiumTv'

/** What is on now, with times in epoch milliseconds. */
export interface StripProgramme {
  title: string
  start: number
  stop?: number | null
}

const props = defineProps<{
  /** Position in the list as the viewer reads it, from 1. */
  number: number
  id: string
  name: string
  logoUrl?: string | null
  subtitle?: string
  now?: StripProgramme | null
  offline?: boolean
  favorite?: boolean
  disabled?: boolean
}>()

const emit = defineEmits<{
  play: []
  toggleFavorite: []
}>()

// A minute is the finest grain a guide listing is written in.
const clock = useNow({ interval: 60_000 })

/** Provider prefixes ("AF: ", "[VIP]", "(24/7)") say nothing a row has room for. */
const cleaned = computed(() => {
  let name = props.name || ''
  name = name.replace(/^\d+[:\-\s]+/, '')
  name = name.replace(/^(the event has not begun|event has not begun)\s*:*\s*/i, '')
  name = name.replace(/^\([\w\-\s]+\):?\s*/, '')
  name = name.replace(/^[\w\-]{2,6}:\s*/, '')
  name = name.replace(/\[[^\]]+\]/g, '')
  name = name.replace(/\([^)]+\)/g, '')
  return name.trim() || props.name.trim()
})

const parsed = computed(() => parseChannelName(cleaned.value, props.name.trim() || $t('Channel')))
const displayName = computed(() => parsed.value.name)
const initials = computed(() => channelInitials(displayName.value))
const tileStyle = computed(() => channelTileStyle(props.id || displayName.value))
const numberLabel = computed(() => String(props.number).padStart(3, '0'))

const imgError = ref(false)
const wantsLogo = computed(() => !isPlaceholderLogoUrl(props.logoUrl))
const showFallback = computed(() => !wantsLogo.value || imgError.value)

function onLogoLoad(e: Event): void {
  if (isTinyLogo(e.target as HTMLImageElement))
    imgError.value = true
}

function hhmm(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}

const programme = computed(() => {
  const p = props.now
  if (!p)
    return null
  const end = p.stop ?? p.start + 3_600_000
  const pct = end > p.start
    ? Math.max(0, Math.min(100, ((clock.value.getTime() - p.start) / (end - p.start)) * 100))
    : 0
  return { title: p.title, times: `${hhmm(p.start)}–${hhmm(end)}`, pct }
})

const label = computed(() => [
  numberLabel.value,
  displayName.value,
  props.offline ? $t('Offline') : '',
  programme.value?.title ?? '',
].filter(Boolean).join(', '))
</script>

<template>
  <button
    type="button"
    class="group grid h-[72px] w-full grid-cols-[2.75rem_52px_minmax(0,1fr)] items-center gap-3 rounded-xl px-2 text-start outline-none transition-colors hover:bg-surface-container-high focus-visible:bg-surface-container-high focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary sm:px-3 md:grid-cols-[3.5rem_52px_minmax(0,1.1fr)_minmax(0,1.5fr)_6.5rem] md:gap-4"
    :class="disabled ? 'cursor-not-allowed opacity-40' : 'cursor-pointer'"
    :disabled="disabled"
    :aria-label="label"
    @click="emit('play')"
  >
    <span class="font-mono text-title-large tabular-nums tracking-tight text-on-surface/35 transition-colors group-hover:text-primary group-focus-visible:text-primary">
      {{ numberLabel }}
    </span>

    <span
      class="grid size-[52px] place-items-center overflow-hidden rounded-lg bg-zinc-950 ring-1 ring-white/10"
      :class="offline ? 'opacity-45 grayscale' : ''"
    >
      <img
        v-if="wantsLogo && !imgError"
        :src="proxyLogo(logoUrl)"
        alt=""
        loading="lazy"
        decoding="async"
        class="size-full object-contain p-1.5"
        @load="onLogoLoad"
        @error="imgError = true"
      >
      <span
        v-if="showFallback"
        class="grid size-full place-items-center text-label-large font-bold text-white/90"
        :style="tileStyle"
      >
        {{ initials }}
      </span>
    </span>

    <span class="min-w-0">
      <span
        class="block truncate text-body-large font-semibold text-on-surface transition-colors group-hover:text-primary group-focus-visible:text-primary"
        :class="offline ? 'opacity-55' : ''"
      >
        {{ displayName }}
      </span>
      <span class="flex min-w-0 items-center gap-2 text-label-medium text-on-surface/50">
        <span
          v-if="parsed.quality"
          class="shrink-0 rounded bg-tertiary/12 px-1.5 text-[10px] font-bold uppercase leading-4 tracking-wider text-tertiary ring-1 ring-tertiary/30 md:hidden"
        >
          {{ parsed.quality }}
        </span>
        <span class="truncate">{{ offline ? $t('Offline') : subtitle }}</span>
      </span>
    </span>

    <span class="hidden min-w-0 gap-1.5 md:grid">
      <template v-if="programme">
        <span class="flex min-w-0 items-baseline gap-2">
          <span class="shrink-0 font-mono text-label-small tabular-nums text-on-surface/45">{{ programme.times }}</span>
          <span class="truncate text-body-medium text-on-surface/85">{{ programme.title }}</span>
        </span>
        <span class="block h-[3px] overflow-hidden rounded-full bg-on-surface/10">
          <span class="block h-full rounded-full bg-on-surface/60" :style="{ width: `${programme.pct}%` }" />
        </span>
      </template>
      <span v-else class="truncate text-body-small text-on-surface/35">
        {{ $t('No guide for this channel') }}
      </span>
    </span>

    <span class="hidden items-center justify-end gap-1.5 md:flex">
      <span
        v-if="parsed.quality"
        class="rounded bg-tertiary/12 px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wider text-tertiary ring-1 ring-tertiary/30"
      >
        {{ parsed.quality }}
      </span>
      <!-- A pointer's shortcut. The row is the remote's target, so the star
           stays out of the tab order or it would swallow every step down. -->
      <span
        class="grid size-9 place-items-center rounded-full transition-opacity hover:bg-on-surface/10 focus-visible:bg-on-surface/10"
        :class="favorite ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100'"
        role="button"
        tabindex="-1"
        :aria-label="favorite ? $t('Remove from favorites') : $t('Add to favorites')"
        @click.stop.prevent="emit('toggleFavorite')"
      >
        <v-icon :icon="mdiStar" size="18" :class="favorite ? 'text-primary' : 'text-on-surface/50'" />
      </span>
    </span>
  </button>
</template>
