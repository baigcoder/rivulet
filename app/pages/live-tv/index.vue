<script setup lang="ts">
/**
 * Live TV's front door: the two services and nothing else. Channels belong
 * to each service's own page, where they can be filtered and checked; here
 * the choice is only which one — the free public list, or the paid provider
 * account the user signed in with.
 */
import { mdiArrowRight, mdiCastConnected, mdiTelevisionClassic } from '@mdi/js'

definePageMeta({ layout: 'default' })

const premium = usePremiumTvStore()
const liveTv = useLiveTvStore()

// Counts only. A browser dev server has no Rust behind it, so they stay away.
onMounted(async () => {
  try {
    await liveTv.useFreeSource()
    await liveTv.loadDashboard()
  }
  catch { /* no Tauri: the tile falls back to its description */ }
})

// "Active" means a paid provider account is signed in — nothing else turns it on.
onMounted(() => {
  void premium.loadStatus().catch(() => {})
})

const tiles = computed(() => [
  {
    to: localePath('/live-tv/free'),
    icon: mdiTelevisionClassic,
    title: $t('Free TV'),
    body: $t('Public live television from around the world. No account needed.'),
    stat: liveTv.totalChannels
      ? $t('{channels} channels · {categories} categories', {
          channels: liveTv.totalChannels.toLocaleString(),
          categories: liveTv.categories.length.toLocaleString(),
        })
      : '',
    status: $t('Free'),
    ready: true,
    tint: 'from-primary/30',
  },
  {
    to: localePath('/live-tv/premium'),
    icon: mdiCastConnected,
    title: $t('Premium TV'),
    body: premium.connected
      ? $t('Your IPTV provider — live, movies and shows.')
      : $t('Sign in with your paid IPTV account.'),
    stat: premium.connected
      ? [
          premium.account?.accountName || premium.account?.username,
          premium.catalog?.channels ? $t('{count} channels', { count: premium.catalog.channels.toLocaleString() }) : '',
        ].filter(Boolean).join(' · ')
      : '',
    status: premium.connected ? $t('Active') : premium.expired ? $t('Expired') : $t('Not signed in'),
    ready: premium.connected,
    tint: 'from-tertiary/30',
  },
])
</script>

<template>
  <div class="flex min-h-full flex-col px-4 py-5 md:px-6">
    <header class="shrink-0">
      <h1 class="text-headline-small font-bold tracking-tight">
        {{ $t('Live TV') }}
      </h1>
      <p class="mt-1 text-body-medium opacity-75">
        {{ $t('Browse free channels or sign in to your IPTV provider.') }}
      </p>
    </header>

    <div class="mt-6 grid flex-1 grid-cols-1 gap-5 lg:max-h-[30rem] lg:grid-cols-2">
      <nuxt-link
        v-for="tile in tiles"
        :key="tile.to"
        :to="tile.to"
        class="group relative flex min-h-64 flex-col justify-between gap-8 overflow-hidden rounded-3xl border border-outline/20 bg-surface-container-high p-6 transition-colors hover:border-primary/50 focus-visible:border-primary/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary lg:p-10"
      >
        <!-- A static wash of the tile's colour: still, so it costs a TV's GPU
             nothing past the first paint. -->
        <div class="pointer-events-none absolute inset-0 bg-gradient-to-br to-transparent" :class="tile.tint" />
        <v-icon
          :icon="tile.icon"
          size="260"
          class="pointer-events-none absolute -bottom-12 -end-10 opacity-[0.07]"
        />

        <div class="relative flex items-start justify-between gap-4">
          <div class="grid size-16 shrink-0 place-items-center rounded-2xl bg-primary/15 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary lg:size-20">
            <v-icon :icon="tile.icon" size="38" />
          </div>
          <span
            class="rounded-full px-3 py-1 text-label-large font-medium"
            :class="tile.ready ? 'bg-primary/20 text-primary' : 'bg-surface-container text-medium-emphasis'"
          >
            {{ tile.status }}
          </span>
        </div>

        <div class="relative">
          <h2 class="flex items-center gap-2 text-headline-medium font-bold tracking-tight lg:text-display-small">
            <span class="truncate">{{ tile.title }}</span>
            <v-icon :icon="mdiArrowRight" size="28" class="shrink-0 opacity-50 transition-[opacity,transform] group-hover:translate-x-1 group-hover:opacity-100 group-focus-visible:translate-x-1 group-focus-visible:opacity-100" />
          </h2>
          <p class="mt-2 max-w-md text-title-medium opacity-80">
            {{ tile.body }}
          </p>
          <p v-if="tile.stat" class="mt-4 text-body-large font-medium opacity-70">
            {{ tile.stat }}
          </p>
        </div>
      </nuxt-link>
    </div>
  </div>
</template>
