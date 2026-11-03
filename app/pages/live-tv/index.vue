<script setup lang="ts">
/**
 * Live TV's front door: the two services, and what is on them right now.
 *
 * Two big tiles used to be the whole page, with two-thirds of the screen
 * empty under them. The tiles are now a compact pair, and the room goes to
 * channels — favourites and each service's busiest categories, as cards that
 * say what is on — under the same featured banner the browsers use, so the
 * page answers "what's on" before anyone picks a service.
 *
 * The rows come off the dashboards both services already compute (their top
 * categories, twenty channels each), so this page costs two reads and one
 * guide lookup per row, not a channel list.
 */
import type { IPTVChannel, PremiumChannelPreview } from '~/types/premium'
import type { LiveChannel } from '~/utils/iptv'
import { mdiArrowRight, mdiCastConnected, mdiTelevisionClassic } from '@mdi/js'
import { useNow } from '@vueuse/core'
import { featuredFromFree, featuredFromPremium } from '~/utils/featuredChannel'
import { premiumApi } from '~/utils/premiumTv'

definePageMeta({ layout: 'default' })

/** Rows per service: enough to browse, few enough that a TV mounts it fast. */
const CATEGORY_ROWS = 3

const premium = usePremiumTvStore()
const liveTv = useLiveTvStore()
const settings = useSettingsStore()
const router = useRouter()
const route = useRoute()
const { play: playFree } = useFreeChannelPlayer()

const premiumPreviews = ref<PremiumChannelPreview[]>([])

interface Row<C> { key: string, title: string, channels: C[] }

const freeRows = computed<Row<LiveChannel>[]>(() => [
  { key: 'fav', title: $t('Favourites'), channels: liveTv.favoriteChannels as LiveChannel[] },
  ...(liveTv.dashboard?.categoryPreviews ?? []).slice(0, CATEGORY_ROWS).map(p => ({
    key: `cat:${p.name}`,
    title: categoryLabel(p.name),
    channels: p.channels,
  })),
].filter(r => r.channels.length))

const premiumRows = computed<Row<IPTVChannel>[]>(() => {
  if (!premium.connected)
    return []
  // The dashboard takes no adult filter of its own, so it is applied here.
  const shown = (list: IPTVChannel[]) => settings.hideAdultChannels ? list.filter(c => !c.isAdult) : list
  return [
    { key: 'fav', title: $t('Favourites'), channels: shown(premium.favoriteChannels) },
    ...premiumPreviews.value
      .map(p => ({ key: `cat:${p.name}`, title: categoryLabel(p.name), channels: shown(p.channels) }))
      .filter(r => r.channels.length)
      .slice(0, CATEGORY_ROWS),
  ].filter(r => r.channels.length)
})

// A browser dev server has no Rust behind it, so each half simply stays away.
onMounted(async () => {
  try {
    await liveTv.useFreeSource()
    await liveTv.loadDashboard()
    void liveTv.loadEpgBatch(freeRows.value.flatMap(r => r.channels.map(c => c.id)))
  }
  catch { /* no Tauri: nothing to show */ }
})

// "Active" means a paid provider account is signed in — nothing else turns it on.
onMounted(async () => {
  try {
    await premium.loadStatus()
    if (!premium.connected)
      return
    const [dash] = await Promise.all([premiumApi.dashboard(), premium.loadFavorites()])
    premiumPreviews.value = dash.categoryPreviews
    void premium.loadNowNext(premiumRows.value.flatMap(r => r.channels.map(c => c.id)))
  }
  catch { /* the tile still says how to sign in */ }
})

// ── The banner: the card the viewer is on, else the first one on the page ──

const { featuredId, featuredSource, track } = useFeaturedChannel()

const featured = computed(() => {
  const id = featuredId.value
  const findIn = <C extends { id: string }>(rows: Row<C>[]) => rows.flatMap(r => r.channels).find(c => c.id === id)
  if (featuredSource.value !== 'free') {
    const ch = (featuredSource.value === 'premium' ? findIn(premiumRows.value) : undefined)
      ?? (featuredSource.value ? undefined : premiumRows.value[0]?.channels[0])
    if (ch)
      return { kind: 'premium' as const, ...featuredFromPremium(ch, premium.nowNext(ch.id)) }
  }
  const ch = (featuredSource.value === 'free' ? findIn(freeRows.value) : undefined) ?? freeRows.value[0]?.channels[0]
  return ch ? { kind: 'free' as const, ...featuredFromFree(ch, liveTv.getEpg(ch.id)) } : null
})

function playPremium(ch: IPTVChannel): void {
  void router.push({ path: localePath('/live-tv/premium/watch'), query: { id: ch.id, from: route.fullPath } })
}

function playFeatured(): void {
  const f = featured.value
  if (f?.kind === 'premium')
    playPremium(f.channel)
  else if (f?.kind === 'free')
    playFree(f.channel, freeRows.value.find(r => r.channels.includes(f.channel))?.channels ?? [f.channel])
}

function toggleFeaturedFavourite(): void {
  const f = featured.value
  if (f?.kind === 'premium')
    void premium.toggleFavorite(f.channel)
  else if (f?.kind === 'free')
    void liveTv.toggleFavorite(f.channel)
}

const featuredIsFavourite = computed(() => {
  const f = featured.value
  if (!f)
    return false
  return f.kind === 'premium' ? premium.isFavorite(f.channel.id) : liveTv.isFavorite(f.channel)
})

// ── The two services ───────────────────────────────────────────────────

const tiles = computed(() => [
  {
    to: localePath('/live-tv/free'),
    icon: mdiTelevisionClassic,
    title: $t('Free TV'),
    body: liveTv.totalChannels
      ? $t('{channels} channels · {categories} categories', {
          channels: liveTv.totalChannels.toLocaleString(),
          categories: liveTv.categories.length.toLocaleString(),
        })
      : $t('Public live television from around the world.'),
    status: $t('Free'),
    ready: true,
    tint: 'from-primary/25',
  },
  {
    to: localePath('/live-tv/premium'),
    icon: mdiCastConnected,
    title: $t('Premium TV'),
    body: premium.connected
      ? [
          premium.account?.accountName || premium.account?.username,
          premium.catalog?.channels ? $t('{count} channels', { count: premium.catalog.channels.toLocaleString() }) : '',
        ].filter(Boolean).join(' · ')
      : $t('Sign in with your paid IPTV account.'),
    status: premium.connected ? $t('Active') : $t('Not signed in'),
    ready: premium.connected,
    tint: 'from-tertiary/25',
  },
])

/** Across a room, the time is what a TV guide is read against. */
const now = useNow({ interval: 30_000 })
const clock = computed(() => now.value.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }))
</script>

<template>
  <div class="flex flex-col gap-6 px-4 py-5 md:px-6">
    <header class="flex shrink-0 items-end justify-between gap-4">
      <div class="min-w-0">
        <h1 class="text-headline-small font-bold tracking-tight">
          {{ $t('Live TV') }}
        </h1>
        <p class="mt-1 text-body-medium opacity-75">
          {{ $t('What is on now, free and from your provider.') }}
        </p>
      </div>
      <p class="shrink-0 text-headline-medium font-bold tabular-nums opacity-80" aria-hidden="true">
        {{ clock }}
      </p>
    </header>

    <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <nuxt-link
        v-for="tile in tiles"
        :key="tile.to"
        :to="tile.to"
        class="group relative flex items-center gap-4 overflow-hidden rounded-2xl border border-outline/20 bg-surface-container-high p-4 transition-colors hover:border-primary/50 focus-visible:border-primary/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary lg:p-5"
      >
        <!-- A static wash of the tile's colour: still, so it costs a TV's GPU
             nothing past the first paint. -->
        <div class="pointer-events-none absolute inset-0 bg-gradient-to-br to-transparent" :class="tile.tint" />
        <div class="relative grid size-14 shrink-0 place-items-center rounded-2xl bg-primary/15 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary">
          <v-icon :icon="tile.icon" size="30" />
        </div>
        <div class="relative min-w-0 flex-1">
          <h2 class="flex items-center gap-2 text-title-large font-bold tracking-tight">
            <span class="truncate">{{ tile.title }}</span>
            <v-icon :icon="mdiArrowRight" size="20" class="shrink-0 opacity-50 transition-[opacity,transform] group-hover:translate-x-1 group-hover:opacity-100 group-focus-visible:translate-x-1 group-focus-visible:opacity-100" />
          </h2>
          <p class="truncate text-body-medium opacity-80">
            {{ tile.body }}
          </p>
        </div>
        <span
          class="relative shrink-0 rounded-full px-3 py-1 text-label-large font-medium"
          :class="tile.ready ? 'bg-primary/20 text-primary' : 'bg-surface-container text-medium-emphasis'"
        >
          {{ tile.status }}
        </span>
      </nuxt-link>
    </div>

    <live-tv-live-featured-banner
      v-if="featured"
      :id="featured.channel.id"
      :name="featured.name"
      :logo-url="featured.channel.logoUrl"
      :tint="featured.tint"
      :quality="featured.quality"
      :subtitle="featured.subtitle"
      :now="featured.now"
      :next="featured.next"
      :favorite="featuredIsFavourite"
      @play="playFeatured"
      @toggle-favorite="toggleFeaturedFavourite"
    />

    <div class="-mx-4 flex flex-col gap-8 md:-mx-6" @focusin="track" @pointerover="track">
      <section v-if="premiumRows.length" data-channel-source="premium" class="flex flex-col gap-4">
        <h2 class="px-4 text-title-large font-bold md:px-6">
          {{ $t('From your provider') }}
        </h2>
        <premium-tv-premium-channel-scroll-row
          v-for="row in premiumRows"
          :key="row.key"
          :title="row.title"
          :channels="row.channels"
          :now-next="premium.nowNext"
          :favorite="premium.isFavorite"
          @play="playPremium"
          @toggle-favorite="premium.toggleFavorite($event)"
        />
      </section>

      <section v-if="freeRows.length" data-channel-source="free" class="flex flex-col gap-4">
        <h2 class="px-4 text-title-large font-bold md:px-6">
          {{ $t('Free TV') }}
        </h2>
        <live-tv-live-channel-scroll-row
          v-for="row in freeRows"
          :key="row.key"
          :title="row.title"
          :channels="row.channels"
          @play="playFree($event, row.channels)"
        />
      </section>
    </div>
  </div>
</template>
