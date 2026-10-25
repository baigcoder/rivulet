<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { mdiArrowRight, mdiCastConnected, mdiTelevisionClassic } from '@mdi/js'

definePageMeta({ layout: 'default' })

const settings = useSettingsStore()
const liveTv = useLiveTvStore()
const { play } = useFreeChannelPlayer()

// What you were watching, and what you starred, one press from the door —
// the two tiles alone left most of this screen empty. Free TV's own library;
// a browser dev server has no Rust behind it, so the rows simply stay away.
onMounted(async () => {
  try {
    await liveTv.useFreeSource()
    await liveTv.loadDashboard()
  }
  catch { /* no Tauri: nothing to show */ }
})

const rows = computed(() => [
  { key: 'recent', title: $t('Recently watched'), channels: liveTv.recentChannels as LiveChannel[] },
  { key: 'favorites', title: $t('Favourites'), channels: liveTv.favoriteChannels as LiveChannel[] },
].filter(r => r.channels.length))

// Each tile carries its own colour and a status, so the two read as two
// different things at a glance — the old pair were the same grey slab, each a
// full half of the screen with an icon and a sentence in the middle of it.
const tiles = computed(() => [
  {
    to: localePath('/live-tv/free'),
    icon: mdiTelevisionClassic,
    title: $t('Free TV'),
    body: $t('Public live television from around the world.'),
    tags: [$t('No account needed'), $t('TV guide'), $t('Favourites')],
    status: $t('Free'),
    ready: true,
    tint: 'from-primary/25',
  },
  {
    to: localePath('/live-tv/premium'),
    icon: mdiCastConnected,
    title: $t('Premium TV'),
    body: settings.isPremium
      ? $t('Your IPTV provider — live, movies and shows.')
      : $t('Requires subscription'),
    tags: [$t('Xtream & M3U'), $t('Movies & series')],
    status: settings.isPremium ? $t('Active') : $t('Not set up'),
    ready: settings.isPremium,
    tint: 'from-tertiary/25',
  },
])
</script>

<template>
  <div class="flex flex-col px-4 py-5 md:px-6">
    <header class="shrink-0">
      <h1 class="text-headline-small font-bold tracking-tight">
        {{ $t('Live TV') }}
      </h1>
      <p class="mt-1 text-body-medium opacity-75">
        {{ $t('Browse free channels or sign in to your IPTV provider.') }}
      </p>
    </header>

    <div class="mt-5 grid grid-cols-1 gap-4 lg:grid-cols-2">
      <nuxt-link
        v-for="tile in tiles"
        :key="tile.to"
        :to="tile.to"
        class="group relative flex min-h-56 flex-col justify-between gap-6 overflow-hidden rounded-3xl border border-outline/20 bg-surface-container-high p-6 transition-colors hover:border-primary/50 focus-visible:border-primary/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary lg:p-8"
      >
        <!-- A static wash of the tile's colour: still, so it costs a TV's GPU
             nothing past the first paint. -->
        <div class="pointer-events-none absolute inset-0 bg-gradient-to-br to-transparent" :class="tile.tint" />
        <v-icon
          :icon="tile.icon"
          size="200"
          class="pointer-events-none absolute -bottom-10 -end-8 opacity-[0.08]"
        />

        <div class="relative flex items-start justify-between gap-4">
          <div class="grid size-14 shrink-0 place-items-center rounded-2xl bg-primary/15 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary lg:size-16">
            <v-icon :icon="tile.icon" size="30" />
          </div>
          <span
            class="rounded-full px-3 py-1 text-label-large font-medium"
            :class="tile.ready ? 'bg-primary/20 text-primary' : 'bg-surface-container text-medium-emphasis'"
          >
            {{ tile.status }}
          </span>
        </div>

        <div class="relative">
          <h2 class="flex items-center gap-2 text-headline-small font-bold tracking-tight lg:text-headline-medium">
            <span class="truncate">{{ tile.title }}</span>
            <v-icon :icon="mdiArrowRight" size="24" class="shrink-0 opacity-50 transition-[opacity,transform] group-hover:translate-x-1 group-hover:opacity-100 group-focus-visible:translate-x-1 group-focus-visible:opacity-100" />
          </h2>
          <p class="mt-1 max-w-md text-title-medium opacity-80">
            {{ tile.body }}
          </p>
          <ul class="mt-4 flex list-none flex-wrap gap-2 ps-0 text-label-large">
            <li
              v-for="tag in tile.tags"
              :key="tag"
              class="rounded-full bg-surface-container/80 px-3 py-1.5"
            >
              {{ tag }}
            </li>
          </ul>
        </div>
      </nuxt-link>
    </div>

    <div v-for="row in rows" :key="row.key" class="mt-8 -mx-4 md:-mx-6">
      <live-tv-live-channel-tile-row
        :title="row.title"
        :channels="row.channels"
        :starred="row.key === 'favorites'"
        @play="play($event, row.channels)"
      />
    </div>
  </div>
</template>
