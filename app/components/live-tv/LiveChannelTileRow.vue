<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { mdiStar, mdiTelevisionClassic } from '@mdi/js'
import { proxyLogo } from '~/utils/premiumTv'

/**
 * A titled row of channels as logo tiles — Recently watched, Favourites.
 * Shared by the Live TV landing and the top of Free TV, so a channel looks
 * the same wherever it is picked from. Playing one is the caller's: the
 * lineup it opens with is the row it was picked from.
 */
defineProps<{
  title: string
  channels: LiveChannel[]
  /** Starred channels wear the star. */
  starred?: boolean
}>()
const emit = defineEmits<{ play: [channel: LiveChannel] }>()
</script>

<template>
  <scroll-row v-if="channels.length" :title="title">
    <button
      v-for="ch in channels"
      :key="ch.id"
      type="button"
      class="group flex w-44 shrink-0 flex-col gap-2 text-start outline-none"
      @click="emit('play', ch)"
    >
      <span class="relative grid aspect-video w-full place-items-center overflow-hidden rounded-xl bg-zinc-950 ring-1 ring-white/10 transition-shadow group-hover:ring-white/30 group-focus-visible:ring-2 group-focus-visible:ring-primary">
        <img
          v-if="ch.logoUrl"
          :src="proxyLogo(ch.logoUrl)"
          alt=""
          loading="lazy"
          decoding="async"
          class="size-full object-contain p-5"
        >
        <v-icon v-else :icon="mdiTelevisionClassic" size="32" class="opacity-25" />
        <span class="absolute start-1.5 top-1.5 flex items-center gap-1 rounded bg-red-600 px-1 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-white">
          <span class="size-1.5 rounded-full bg-white" />{{ $t('LIVE') }}
        </span>
        <v-icon v-if="starred" :icon="mdiStar" size="16" class="absolute end-2 top-2 text-amber-300" />
      </span>
      <span class="truncate text-body-medium font-semibold group-hover:text-primary group-focus-visible:text-primary">{{ ch.name }}</span>
    </button>
  </scroll-row>
</template>
