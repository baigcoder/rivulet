<script setup lang="ts">
/**
 * Premium TV's browsing surface: one header row, the category rail and
 * the virtualized grid.
 *
 * The header is `LiveBrowseHeader` — the same chrome as Free TV. The
 * account panel still lives in *Settings → Premium TV*; this row only
 * carries where you are, a search the remote can walk past, and a pill
 * for the provider.
 *
 * The three viewports are designed, not scaled. Desktop keeps the rail
 * open beside the grid, because a mouse can reach it and a remote can
 * walk left into it. Tablet drops to a wider grid with the rail behind a
 * button — a 260px rail on a 768px screen leaves room for three cards.
 * Phone drops the rail entirely, switches the grid to compact density and
 * puts the three fixed views on a scrollable chip row, because a rail
 * that has to be opened for every hop is the wrong control on a screen
 * you hold in one hand.
 */
import type { PremiumView } from '~/stores/premiumTv'
import type { IPTVChannel, PremiumSeriesItem, PremiumVodItem } from '~/types/premium'
import { mdiAccountCircle, mdiClose, mdiDeleteSweepOutline, mdiTelevisionOff } from '@mdi/js'
import { computed, onMounted, onUnmounted, ref } from 'vue'

/**
 * `showBack` is set by the deep-linked category route, which used to draw
 * its own back button and its own copy of the category name above this
 * component — two headings for one thing, and a row of vertical space to
 * say it twice. The button belongs on the heading that is already there.
 */
const props = defineProps<{ showBack?: boolean }>()

const premium = usePremiumTvStore()
const route = useRoute()
const router = useRouter()
const { smAndUp, mdAndUp, lgAndUp } = useDisplay()

/** The rail is only ever pinned where there is width for it. */
const railPinned = computed(() => lgAndUp.value)
const sheetOpen = ref(false)
const accountOpen = ref(false)
const busy = ref(false)

const density = computed<'compact' | 'comfortable'>(() => mdAndUp.value ? 'comfortable' : 'compact')

onMounted(async () => {
  window.addEventListener('keydown', onKey)
  await premium.ensureLoaded()
  if (premium.connected) {
    if (premium.contentSection === 'live' && premium.channels.length === 0) {
      await premium.loadChannels({ reset: true })
    }
    else if (premium.contentSection !== 'live') {
      const section = premium.contentSection
      const hasData = section === 'movies' ? premium.vodMovies.length > 0 : premium.vodSeries.length > 0
      await Promise.all([
        premium.loadVodCategories(section),
        premium.loadVod({ reset: !hasData, keepVisible: hasData }),
      ])
    }
    else if (premium.supportsVod) {
      void premium.prefetchVod()
    }
  }
})

onUnmounted(() => window.removeEventListener('keydown', onKey))

const contentTabs = computed(() => [
  { id: 'live' as const, label: $t('Live channels') },
  { id: 'movies' as const, label: $t('Movies') },
  { id: 'series' as const, label: $t('TV shows') },
])

const isLive = computed(() => premium.contentSection === 'live')
const isMovies = computed(() => premium.contentSection === 'movies')
const isSeries = computed(() => premium.contentSection === 'series')

const heading = computed(() => {
  if (isMovies.value) {
    const cat = premium.vodCategories.find(c => c.id === premium.selectedVodCategory)
    return cat?.name || $t('Movies')
  }
  if (isSeries.value) {
    const cat = premium.vodCategories.find(c => c.id === premium.selectedVodCategory)
    return cat?.name || $t('TV shows')
  }
  switch (premium.view) {
    case 'favorites':
      return $t('Favorites')
    case 'recent':
      return $t('Recently watched')
    case 'category':
      return categoryLabel(premium.selectedCategory) || $t('All channels')
    default:
      return $t('All channels')
  }
})

/**
 * `total` is the server's count for the query, which is the number worth
 * showing: "1,432 channels" while 60 are loaded is the truth about the
 * filter, and the count of what happens to be in memory is not.
 */
const count = computed(() => isLive.value ? premium.total : premium.vodTotal)

const countLine = computed(() => {
  const n = count.value.toLocaleString()
  if (isMovies.value)
    return $t('{count} movies', { count: n })
  if (isSeries.value)
    return $t('{count} TV shows', { count: n })
  return $t('{count} channels', { count: n })
})

const searchPlaceholder = computed(() => {
  if (isMovies.value)
    return $t('Search movies')
  if (isSeries.value)
    return $t('Search TV shows')
  return $t('Search channels')
})

const tuneLabel = computed(() => {
  if (isMovies.value)
    return $t('Movie categories')
  if (isSeries.value)
    return $t('TV show categories')
  return $t('Categories')
})

const sheetTitle = computed(() => tuneLabel.value)

/**
 * The provider, in one dot and one line. Primary is the catalog answering;
 * tertiary is the one state a viewer can *act* on — every connection the
 * account allows is in use, so the next channel they click will be
 * refused by the panel and not by us.
 */
const status = computed(() => {
  if (!premium.connected)
    return { tone: 'bg-outline', label: $t('Not connected') }
  if (premium.atConnectionLimit === true)
    return { tone: 'bg-tertiary', label: $t('All connections in use') }
  return { tone: 'bg-primary', label: $t('Connected') }
})

const providerLabel = computed(() =>
  premium.account?.accountName?.trim() || premium.account?.username || $t('Premium TV'),
)

const showEmpty = computed(() => {
  if (!premium.connected || premium.importing)
    return false
  if (isLive.value)
    return !premium.listLoading && premium.channels.length === 0
  if (isMovies.value)
    return !premium.vodLoading && premium.vodMovies.length === 0
  return !premium.vodLoading && premium.vodSeries.length === 0
})

const emptyMessage = computed(() => {
  if (premium.searchDebounced) {
    return isLive.value
      ? $t('No channels match that search.')
      : $t('Nothing matches that search.')
  }
  if (!isLive.value)
    return $t('This provider returned nothing for that category.')
  if (premium.view === 'favorites')
    return $t('Star a channel and it shows up here.')
  if (premium.view === 'recent')
    return $t('Channels you watch appear here.')
  return $t('This provider returned no channels for that filter.')
})

/** Favorites strip on All only — Recent has its own sidebar section. */
const resumeStrip = computed(() => {
  if (premium.contentSection !== 'live' || premium.view !== 'all' || premium.searchQuery)
    return null
  if (premium.favoriteChannels.length)
    return { title: $t('Favorites'), channels: premium.favoriteChannels }
  return null
})

function play(channel: IPTVChannel): void {
  void router.push({
    path: localePath('/live-tv/premium/watch'),
    query: { id: channel.id, from: route.fullPath },
  })
}

function openMovie(item: PremiumVodItem): void {
  void router.push({
    path: localePath(`/live-tv/premium/movie/${item.id}`),
    query: {
      from: route.fullPath,
      ext: item.containerExtension || 'mkv',
    },
  })
}

function openSeries(item: PremiumSeriesItem): void {
  void router.push({
    path: localePath(`/live-tv/premium/series/${item.id}`),
    query: { from: route.fullPath },
  })
}

function pickSection(section: 'live' | 'movies' | 'series'): void {
  premium.setContentSection(section)
  sheetOpen.value = false
}

function pickVodCategory(id: string): void {
  premium.setVodCategory(id)
  sheetOpen.value = false
}

function goBack(): void {
  void router.replace(localePath(props.showBack ? '/live-tv/premium' : '/live-tv'))
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && accountOpen.value) {
    e.preventDefault()
    accountOpen.value = false
    return
  }
  if (e.key === 'Escape' && sheetOpen.value) {
    e.preventDefault()
    sheetOpen.value = false
  }
}

function pickView(view: PremiumView): void {
  premium.setView(view)
  sheetOpen.value = false
}

function pickCategory(name: string): void {
  premium.setCategory(name)
  sheetOpen.value = false
}

async function refresh(): Promise<void> {
  busy.value = true
  try {
    await premium.refresh(true)
  }
  finally {
    busy.value = false
  }
}

async function disconnect(): Promise<void> {
  busy.value = true
  try {
    await premium.disconnect()
    await router.replace(localePath('/live-tv/premium'))
  }
  finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col gap-3">
    <live-tv-live-browse-header
      v-model:search="premium.searchQuery"
      :heading="heading"
      :count="count"
      :count-line="countLine"
      :search-placeholder="searchPlaceholder"
      :tune-label="tuneLabel"
      :status-tone="status.tone"
      :status-label="status.label"
      :status-text="status.label"
      :show-clear="(isLive && premium.view === 'category') || !!premium.searchQuery || (!isLive && !!premium.selectedVodCategory)"
      :refreshing="busy || premium.catalog?.syncing === true"
      :show-tune="!railPinned"
      @back="goBack"
      @clear="isLive ? premium.clearFilters() : premium.clearVodFilters()"
      @refresh="refresh"
      @tune="sheetOpen = true"
    >
      <button
        v-if="premium.account"
        type="button"
        class="relative grid size-11 shrink-0 place-items-center rounded-lg text-on-surface/70 transition-colors hover:bg-surface-container-highest hover:text-on-surface focus-visible:bg-surface-container-highest focus-visible:text-on-surface"
        :aria-label="$t('Account details')"
        :title="providerLabel"
        @click="accountOpen = true"
      >
        <v-icon :icon="mdiAccountCircle" size="22" />
        <span
          class="absolute end-1.5 top-1.5 size-2 rounded-full ring-2 ring-surface-container-high"
          :class="status.tone"
          aria-hidden="true"
        />
      </button>
      <button
        v-if="premium.view === 'recent' && premium.recent.length"
        type="button"
        class="grid size-11 shrink-0 place-items-center rounded-lg text-on-surface/70 transition-colors hover:bg-surface-container-highest hover:text-on-surface focus-visible:bg-surface-container-highest focus-visible:text-on-surface"
        :aria-label="$t('Clear recently watched')"
        @click="premium.clearRecent()"
      >
        <v-icon :icon="mdiDeleteSweepOutline" size="22" />
      </button>

      <template v-if="premium.supportsVod" #below>
        <div
          class="-mx-1 flex gap-1 border-t border-outline/10 px-1 pt-2"
          role="tablist"
          :aria-label="$t('Browse')"
        >
          <button
            v-for="tab in contentTabs"
            :key="tab.id"
            type="button"
            role="tab"
            class="min-h-9 flex-1 shrink-0 rounded-lg px-3 py-1.5 text-label-medium font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary sm:flex-none"
            :class="premium.contentSection === tab.id
              ? 'bg-primary text-on-primary'
              : 'text-on-surface/70 hover:bg-surface-container-high hover:text-on-surface focus-visible:bg-surface-container-high focus-visible:text-on-surface'"
            :aria-selected="premium.contentSection === tab.id ? 'true' : 'false'"
            @click="pickSection(tab.id)"
          >
            {{ tab.label }}
          </button>
        </div>
      </template>
    </live-tv-live-browse-header>

    <!-- Phone: the three fixed views as chips, so the common hop is one tap. -->
    <div v-if="!railPinned && isLive" class="flex gap-1 overflow-x-auto rounded-2xl bg-surface-container/40 p-1">
      <button
        v-for="v in (['all', 'favorites', 'recent'] as const)"
        :key="v"
        type="button"
        class="min-h-10 shrink-0 rounded-xl px-3 py-2 text-label-medium font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :class="premium.view === v
          ? 'bg-primary text-on-primary shadow-sm'
          : 'text-on-surface/70 hover:bg-surface-container-high hover:text-on-surface focus-visible:bg-surface-container-high focus-visible:text-on-surface'"
        :aria-current="premium.view === v ? 'true' : undefined"
        @click="pickView(v)"
      >
        {{ v === 'all' ? $t('All channels') : v === 'favorites' ? $t('Favorites') : $t('Recently watched') }}
      </button>
      <button
        v-if="premium.view === 'category' && premium.selectedCategory"
        type="button"
        class="shrink-0 rounded-full bg-primary px-3 py-1.5 text-label-medium font-medium text-on-primary ring-1 ring-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        @click="premium.clearFilters()"
      >
        {{ categoryLabel(premium.selectedCategory) }}
        <v-icon :icon="mdiClose" size="14" class="ms-1" />
      </button>
    </div>

    <!-- Phone: VOD category chip when a filter is active. -->
    <div v-if="!railPinned && !isLive && premium.selectedVodCategory" class="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1">
      <button
        type="button"
        class="shrink-0 rounded-full bg-primary px-3 py-1.5 text-label-medium font-medium text-on-primary ring-1 ring-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        @click="premium.clearVodFilters()"
      >
        {{ heading }}
        <v-icon :icon="mdiClose" size="14" class="ms-1" />
      </button>
    </div>

    <div class="flex min-h-0 flex-1 gap-4">
      <!-- Narrow on purpose: the rail is a list of short group names
           and a count, and every pixel it takes is a pixel the grid does
           not have for a sixth column. -->
      <aside v-if="railPinned" class="flex w-56 shrink-0 flex-col xl:w-60">
        <premium-tv-premium-sidebar
          v-if="isLive"
          :view="premium.view"
          :selected-category="premium.selectedCategory"
          :categories="premium.categoryCounts"
          :total-channels="premium.catalog?.channels ?? 0"
          :favorite-count="premium.favoriteIds.size"
          :recent-count="premium.recent.length"
          @set-view="premium.setView($event)"
          @set-category="premium.setCategory($event)"
        />
        <premium-tv-premium-vod-sidebar
          v-else
          :categories="premium.vodCategories"
          :selected-id="premium.selectedVodCategory"
          :kind="isMovies ? 'movie' : 'series'"
          @pick="premium.setVodCategory($event)"
        />
      </aside>

      <section class="flex min-h-0 flex-1 flex-col gap-2">
        <!-- Importing: an empty grid here means "not yet", not "none". -->
        <div v-if="premium.importing" class="grid flex-1 place-items-center text-center">
          <div class="flex flex-col items-center gap-3">
            <v-progress-circular indeterminate color="primary" size="36" />
            <p class="text-body-medium opacity-70">
              {{ $t('Importing the channel list…') }}
            </p>
            <p class="max-w-sm text-label-small opacity-45">
              {{ $t('This runs once and is then cached on this device.') }}
            </p>
          </div>
        </div>

        <!-- First load: cards' worth of skeleton rather than a spinner in
             a void, so the grid does not jump when the page lands. -->
        <div
          v-else-if="isLive && premium.listLoading && premium.channels.length === 0"
          class="grid flex-1 auto-rows-max grid-cols-2 gap-3 overflow-hidden sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
          role="status"
          :aria-label="$t('Loading channels…')"
        >
          <div
            v-for="n in 24"
            :key="n"
            class="h-[176px] animate-pulse rounded-lg bg-surface-container-high/60"
          />
        </div>

        <div v-else-if="showEmpty" class="grid flex-1 place-items-center px-6 text-center">
          <div class="flex flex-col items-center gap-2">
            <v-icon :icon="mdiTelevisionOff" size="44" class="opacity-20" />
            <p class="text-body-large font-medium opacity-70">
              {{ isLive ? $t('No channels found') : $t('Nothing found') }}
            </p>
            <p class="max-w-sm text-body-small opacity-45">
              {{ emptyMessage }}
            </p>
          </div>
        </div>

        <template v-else-if="isLive">
          <premium-tv-premium-channel-scroll-row
            v-if="resumeStrip"
            class="shrink-0"
            :title="resumeStrip.title"
            :channels="resumeStrip.channels"
            :now-next="premium.nowNext"
            :favorite="premium.isFavorite"
            @play="play"
            @toggle-favorite="premium.toggleFavorite($event)"
          />
          <premium-tv-premium-channel-grid
            class="min-h-0 flex-1"
            :channels="premium.channels"
            :now-next="premium.nowNext"
            :favorite="premium.isFavorite"
            :density="density"
            :load-epg="premium.loadNowNext"
            :has-more="premium.hasMore"
            :loading="premium.listLoading"
            @load-more="premium.loadMore()"
            @play="play"
            @toggle-favorite="premium.toggleFavorite($event)"
          />
        </template>

        <template v-else>
          <premium-tv-premium-vod-grid
            class="min-h-0 flex-1"
            :kind="isMovies ? 'movie' : 'series'"
            :movies="premium.vodMovies"
            :series="premium.vodSeries"
            :loading="premium.vodLoading"
            :has-more="premium.vodHasMore"
            :density="density"
            @load-more="premium.loadMoreVod()"
            @open-movie="openMovie"
            @open-series="openSeries"
          />
        </template>
      </section>
    </div>

    <v-dialog v-model="accountOpen" max-width="520" scrollable>
      <v-card class="bg-surface-container">
        <div class="flex items-center justify-between gap-3 px-4 pt-4">
          <h2 class="text-title-large font-bold">
            {{ $t('IPTV account') }}
          </h2>
          <v-btn :icon="mdiClose" variant="text" size="small" :aria-label="$t('Close')" @click="accountOpen = false" />
        </div>
        <v-card-text class="flex flex-col gap-3 !pt-2">
          <p
            v-if="premium.atConnectionLimit === true"
            class="rounded-xl bg-tertiary/15 px-3 py-2 text-body-small text-tertiary"
          >
            {{ $t('All connections are in use on this account. Stop playback on your other devices before starting a new stream.') }}
          </p>
          <premium-tv-premium-account-card
            v-if="premium.account"
            :account="premium.account"
            :catalog="premium.catalog"
            :busy="busy"
            @refresh="refresh"
            @disconnect="disconnect(); accountOpen = false"
          />
        </v-card-text>
      </v-card>
    </v-dialog>

    <v-dialog
      v-model="sheetOpen"
      scrollable
      :fullscreen="!smAndUp"
      :max-width="smAndUp ? 400 : undefined"
    >
      <v-card class="flex h-full flex-col bg-surface">
        <div
          class="flex items-center justify-between px-4 pt-4"
          :class="smAndUp ? undefined : 'pt-[max(1rem,var(--safe-top))]'"
        >
          <h2 class="text-title-large font-bold">
            {{ sheetTitle }}
          </h2>
          <v-btn :icon="mdiClose" variant="text" size="small" :aria-label="$t('Close')" @click="sheetOpen = false" />
        </div>
        <v-card-text
          class="flex min-h-0 flex-1 flex-col !pt-3"
          :style="smAndUp ? { height: 'min(80vh, 640px)' } : undefined"
        >
          <premium-tv-premium-sidebar
            v-if="isLive"
            :view="premium.view"
            :selected-category="premium.selectedCategory"
            :categories="premium.categoryCounts"
            :total-channels="premium.catalog?.channels ?? 0"
            :favorite-count="premium.favoriteIds.size"
            :recent-count="premium.recent.length"
            bare
            @set-view="pickView"
            @set-category="pickCategory"
          />
          <premium-tv-premium-vod-sidebar
            v-else
            :categories="premium.vodCategories"
            :selected-id="premium.selectedVodCategory"
            :kind="isMovies ? 'movie' : 'series'"
            bare
            @pick="pickVodCategory"
          />
        </v-card-text>
      </v-card>
    </v-dialog>
  </div>
</template>
