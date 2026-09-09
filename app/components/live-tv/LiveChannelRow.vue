<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { mdiPlay, mdiStar, mdiTelevision } from '@mdi/js'
import { proxyLogo } from '~/utils/premiumTv'

const props = defineProps<{
  channel: LiveChannel
  getEpg: (id: string) => Array<{ title: string, description?: string | null, start: string, stop?: string | null }>
  isFavorite: (ch: LiveChannel) => boolean
  isOffline?: (ch: LiveChannel) => boolean
}>()

const emit = defineEmits<{
  play: [channel: LiveChannel]
  toggleFavorite: [channel: LiveChannel]
}>()

const epg = computed(() => props.getEpg(props.channel.id))
const nowProgram = computed(() => epg.value[0] ?? null)
const fav = computed(() => props.isFavorite(props.channel))
const dead = computed(() => {
  const s = props.channel.streamUrl
  if (!s || s === 'undefined' || s === 'null')
    return true
  return props.isOffline?.(props.channel) === true
})
const imgError = ref(false)
const proxyLogoUrl = computed(() => proxyLogo(props.channel.logoUrl))

const epgProgress = computed(() => {
  if (!nowProgram.value?.start)
    return 0
  const start = new Date(nowProgram.value.start).getTime()
  const end = nowProgram.value.stop ? new Date(nowProgram.value.stop).getTime() : start + 3600000
  const now = Date.now()
  if (now < start)
    return 0
  if (now > end)
    return 100
  return Math.round(((now - start) / (end - start)) * 100)
})
</script>

<template>
  <div
    class="group flex h-14 cursor-pointer items-center gap-3 rounded-lg border border-white/5 bg-surface-container-high px-3 transition-colors duration-200 hover:border-primary/40 hover:bg-surface-container-highest focus-visible:border-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    tabindex="0"
    role="button"
    @click="emit('play', channel)"
    @keydown.enter="emit('play', channel)"
  >
    <div class="grid size-10 shrink-0 place-items-center overflow-hidden rounded bg-surface-container">
      <img
        v-if="channel.logoUrl && !imgError"
        :src="proxyLogoUrl"
        :alt="channel.name"
        loading="lazy"
        decoding="async"
        class="size-full object-contain p-1"
        @error="imgError = true"
      >
      <v-icon v-else :icon="mdiTelevision" size="20" class="opacity-20" />
    </div>

    <div class="min-w-0 flex-1">
      <div class="flex min-w-0 items-center gap-1.5">
        <span
          class="pointer-events-none shrink-0 rounded px-1 py-px text-[9px] font-semibold uppercase tracking-wide"
          :class="dead ? 'bg-zinc-800 text-white/70' : 'bg-red-600 text-white'"
        >
          {{ dead ? $t('Offline') : $t('LIVE') }}
        </span>
        <h3 class="truncate text-body-medium font-medium">
          {{ channel.name }}
        </h3>
      </div>
      <div class="flex items-center gap-1.5 text-body-small opacity-50">
        <span v-if="channel.country" class="truncate">{{ channel.country }}</span>
        <span v-if="channel.country && channel.categoryName">·</span>
        <span v-if="channel.categoryName" class="truncate">{{ channel.categoryName }}</span>
        <span v-if="nowProgram" class="truncate">· {{ nowProgram.title }}</span>
      </div>
    </div>

    <div v-if="nowProgram" class="hidden w-24 shrink-0 sm:block">
      <div class="h-1 w-full overflow-hidden rounded-full bg-white/10">
        <!-- scaleX, not width: the EPG bar crawls once a second per visible row,
             and a width tween is layout — every card on the list relayouts with
             it. Scale is the compositor's, and the pill looks the same. -->
        <div
          class="h-full w-full origin-left rounded-full bg-primary transition-transform duration-1000 ease-linear"
          :style="{ transform: `scaleX(${epgProgress / 100})` }"
        />
      </div>
    </div>

    <button
      class="grid size-8 shrink-0 place-items-center rounded-full text-white/60 opacity-0 transition-opacity hover:text-white focus-visible:opacity-100 group-hover:opacity-100"
      :aria-label="fav ? 'Remove from favorites' : 'Add to favorites'"
      tabindex="-1"
      @click.stop.prevent="emit('toggleFavorite', channel)"
    >
      <v-icon :icon="mdiStar" :class="fav ? 'text-amber-400' : ''" size="16" />
    </button>

    <v-btn
      icon
      size="small"
      variant="tonal"
      color="primary"
      class="shrink-0 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
      tabindex="-1"
      @click.stop="emit('play', channel)"
    >
      <v-icon :icon="mdiPlay" size="18" class="ml-0.5" />
    </v-btn>
  </div>
</template>
