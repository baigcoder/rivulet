<script setup lang="ts">
/**
 * Free TV's browsing surface — the same page as Premium TV's, over the
 * bundled public playlist.
 *
 * This was a different design: a glass hero with its own gradients and
 * blurs, two rows of stat pills, and below it six country carousels and
 * eight category carousels of twelve cards each. That is a magazine cover
 * over a 2,000-channel list — the first row of actual channels sat below
 * the fold, `backdrop-filter` ran on four stacked elements at once (which
 * CLAUDE.md says is affordable once on the chrome and nowhere else), and
 * finding a channel meant scrolling past 160 cards nobody asked for.
 * Premium TV already answers all of that with one header row, a category
 * rail and one virtualized grid, so Free TV is that, and the two sections
 * of the app stop being two different apps. The rail is
 * `premium-tv-premium-sidebar` unmodified: its `CategoryCount[]` is the
 * shape `liveTv.categories` already has, and `LiveView` is the same four
 * names as `PremiumView`.
 *
 * The one thing Premium does not need: a *free* playlist is a list of
 * other people's servers, so a third of it is dead on any given day.
 * `liveTv.probeIds` rides the grid's existing visible-batch hook to dim
 * what will not open, and `watch.vue` zaps past it. See
 * `app/utils/livehealth.ts`.
 */
import type { UnlistenFn } from '@tauri-apps/api/event'
import type { LiveView } from '~/stores/liveTv'
import type { LiveChannel } from '~/utils/iptv'
import { mdiClose, mdiDeleteSweepOutline, mdiMagnify, mdiRefresh, mdiTelevisionOff, mdiTune } from '@mdi/js'
import { listen } from '@tauri-apps/api/event'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'

definePageMeta({ layout: 'default' })

const liveTv = useLiveTvStore()
const route = useRoute()
const router = useRouter()
const { mdAndUp, lgAndUp } = useDisplay()

/** `free.vue` is the parent route of `free/country|category|guide`. */
const isNestedRoute = computed(() => /\/live-tv\/free\/(?:country|category|guide)/.test(route.path))

const railPinned = computed(() => lgAndUp.value)
const sheetOpen = ref(false)
const density = computed<'compact' | 'comfortable'>(() => mdAndUp.value ? 'comfortable' : 'compact')

/**
 * `recent` is the one view with no server query behind it — the dashboard
 * already carries the last twenty, and an all-filters-cleared query would
 * render the whole playlist under a "Recently watched" heading.
 */
const channels = computed<LiveChannel[]>(() =>
  liveTv.view === 'recent' ? liveTv.recentChannels : liveTv.visibleChannels,
)
const count = computed(() =>
  liveTv.view === 'recent' ? liveTv.recentChannels.length : liveTv.visibleTotal,
)
const hasMore = computed(() => liveTv.view !== 'recent' && !!liveTv.nextCursor)

const heading = computed(() => {
  switch (liveTv.view) {
    case 'favorites':
      return $t('Favorites')
    case 'recent':
      return $t('Recently watched')
    case 'category':
      return liveTv.selectedCategory || $t('All channels')
    default:
      return $t('All channels')
  }
})

/** Amber is the state a viewer can act on: the list is still importing. */
const status = computed(() => {
  if (liveTv.m3uImporting)
    return { tone: 'bg-amber-400', label: $t('Importing the channel list…') }
  if (liveTv.totalChannels === 0)
    return { tone: 'bg-white/25', label: $t('No channels found') }
  return { tone: 'bg-emerald-400', label: $t('Ready') }
})

const showEmpty = computed(() =>
  !liveTv.visibleLoading && !liveTv.m3uImporting && channels.value.length === 0,
)

const emptyMessage = computed(() => {
  if (liveTv.searchDebounced)
    return $t('No channels match that search.')
  if (liveTv.view === 'favorites')
    return $t('Star a channel and it shows up here.')
  if (liveTv.view === 'recent')
    return $t('Channels you watch appear here.')
  if (liveTv.totalChannels === 0)
    return $t('The public playlist has not been imported yet. Refresh to fetch it.')
  return $t('Nothing in the playlist matches that filter.')
})

/**
 * EPG and the health probe ride the same hook: the grid hands over the
 * ids it just scrolled into view, debounced, capped at twenty.
 */
function loadForVisible(ids: string[]): void {
  void liveTv.loadEpgBatch(ids)
  void liveTv.probeIds(ids)
}

function pickView(view: LiveView): void {
  liveTv.setView(view)
  sheetOpen.value = false
}

function pickCategory(name: string): void {
  liveTv.setCategory(name)
  sheetOpen.value = false
}

function playChannel(ch: LiveChannel): void {
  // "undefined"/"null" are real values in a public M3U — a channel with no
  // URL would navigate to a blank player.
  if (!ch.streamUrl || ch.streamUrl === 'undefined' || ch.streamUrl === 'null')
    return
  if (!liveTv.activeSourceId)
    return
  liveTv.rememberChannel(ch.id)
  // The zap list is what is on screen: up/down on the player walks the
  // same order the viewer was just reading. Capped because it travels as
  // a query parameter.
  const zapList = channels.value
    .filter(c => c.streamUrl && c.streamUrl !== 'undefined' && c.streamUrl !== 'null')
    .slice(0, 60)
    .map(c => ({
      id: c.id,
      name: c.name,
      logoUrl: c.logoUrl,
      streamUrl: c.streamUrl,
      userAgent: c.userAgent,
      referer: c.referer,
    }))
  void router.push({
    path: localePath('/live-tv/watch'),
    query: {
      url: ch.streamUrl,
      title: ch.name,
      logo: ch.logoUrl ?? '',
      id: ch.id,
      type: 'live',
      sourceId: liveTv.activeSourceId,
      list: encodeURIComponent(JSON.stringify(zapList)),
      from: route.fullPath,
    },
  })
}

/**
 * The importer's own progress, rather than a poll for it. Rust emits
 * `m3u_progress` throughout the first-launch import and the Refresh
 * button's; the page used to re-query the dashboard every two seconds for
 * a minute instead and then give up, which is both slower to notice the
 * first batch and wrong about a slow connection.
 */
let unlisten: UnlistenFn | undefined

onMounted(async () => {
  try {
    unlisten = await listen<{ bytesDownloaded: number, totalBytes: number, stage: string, channelsImported: number }>(
      'm3u_progress',
      async ({ payload }) => {
        liveTv.m3uProgress = {
          bytesDownloaded: payload.bytesDownloaded,
          totalBytes: payload.totalBytes,
          channels: payload.channelsImported,
          stage: payload.stage,
        }
        const done = payload.stage === 'complete' || payload.stage === 'cancelled'
        liveTv.m3uImporting = !done
        if (done) {
          await liveTv.loadDashboard()
          await liveTv.loadVisible({ reset: true })
        }
      },
    )
  }
  catch { /* browser dev server: no Tauri, so no importer to hear from */ }

  // Free TV must never inherit the premium source: the two entry points
  // are separate libraries, not two views of one list.
  await liveTv.useFreeSource()
  const category = typeof route.query.category === 'string' ? route.query.category : ''
  if (category)
    liveTv.setCategory(category)
  await Promise.all([
    liveTv.loadDashboard(),
    liveTv.loadFavorites(),
    liveTv.loadVisible({ reset: true }),
  ])
  // A first launch that is still importing has nothing to show yet, and
  // the event above is the only thing that will say when it does.
  if (liveTv.totalChannels === 0)
    liveTv.m3uImporting = true
})

onUnmounted(() => unlisten?.())

// Post-mount URL changes only (browser back/forward into a ?category=).
watch(() => route.query.category, category => {
  if (typeof category === 'string' && category && category !== liveTv.selectedCategory)
    liveTv.setCategory(category)
})
</script>

<template>
  <nuxt-page v-if="isNestedRoute" />

  <div v-else class="flex h-full min-h-0 flex-col gap-3 px-4 py-4 md:px-6">
    <header class="flex flex-wrap items-center gap-x-3 gap-y-2">
      <div class="flex min-w-0 items-baseline gap-2">
        <h1 class="truncate text-title-medium font-bold">
          {{ heading }}
        </h1>
        <span class="shrink-0 text-label-small tabular-nums opacity-45">
          {{ $t('{count} channels', { count: count.toLocaleString() }) }}
        </span>
        <v-btn
          v-if="liveTv.view === 'recent' && liveTv.recentChannels.length"
          :icon="mdiDeleteSweepOutline"
          variant="text"
          size="small"
          class="shrink-0"
          :aria-label="$t('Clear recently watched')"
          :title="$t('Clear recently watched')"
          @click="liveTv.clearRecent()"
        />
        <v-btn
          v-if="liveTv.view === 'category' || liveTv.searchQuery"
          :icon="mdiClose"
          variant="text"
          size="x-small"
          class="shrink-0"
          :aria-label="$t('Clear filters')"
          @click="liveTv.clearFilters()"
        />
      </div>

      <div class="order-last w-full min-w-0 sm:order-none sm:ms-auto sm:w-64 md:w-72">
        <v-text-field
          v-model="liveTv.searchQuery"
          :label="$t('Search channels')"
          :prepend-inner-icon="mdiMagnify"
          density="compact"
          variant="outlined"
          hide-details
          clearable
          autocomplete="off"
        />
      </div>

      <div class="flex items-center gap-1.5">
        <span
          class="size-2 shrink-0 rounded-full"
          :class="status.tone"
          role="img"
          :aria-label="status.label"
          :title="status.label"
        />
        <span class="hidden max-w-40 truncate text-label-small opacity-55 lg:inline">
          {{ $t('Free TV') }}
        </span>
        <span v-if="liveTv.offlineIds.size" class="hidden text-label-small opacity-35 xl:inline">
          · {{ $t('{count} offline', { count: liveTv.offlineIds.size }) }}
        </span>

        <v-btn
          :icon="mdiRefresh"
          variant="text"
          size="small"
          :loading="liveTv.refreshing"
          :aria-label="$t('Refresh')"
          :title="$t('Refresh')"
          @click="liveTv.refreshFreeTv()"
        />
        <v-btn
          v-if="!railPinned"
          :icon="mdiTune"
          variant="text"
          size="small"
          :aria-label="$t('Categories')"
          :title="$t('Categories')"
          @click="sheetOpen = true"
        />
      </div>
    </header>

    <!-- Phone: the three fixed views as chips, so the common hop is one tap. -->
    <div v-if="!railPinned" class="-mx-1 flex gap-2 overflow-x-auto px-1 pb-1">
      <button
        v-for="v in (['all', 'favorites', 'recent'] as const)"
        :key="v"
        type="button"
        class="shrink-0 rounded-full px-3 py-1.5 text-label-medium font-medium ring-1 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :class="liveTv.view === v
          ? 'bg-primary text-on-primary ring-primary'
          : 'bg-surface-container-high text-white/70 ring-white/10 hover:text-white'"
        :aria-current="liveTv.view === v ? 'true' : undefined"
        @click="pickView(v)"
      >
        {{ v === 'all' ? $t('All channels') : v === 'favorites' ? $t('Favorites') : $t('Recently watched') }}
      </button>
      <button
        v-if="liveTv.view === 'category' && liveTv.selectedCategory"
        type="button"
        class="shrink-0 rounded-full bg-primary px-3 py-1.5 text-label-medium font-medium text-on-primary ring-1 ring-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        @click="liveTv.clearFilters()"
      >
        {{ liveTv.selectedCategory }}
        <v-icon :icon="mdiClose" size="14" class="ms-1" />
      </button>
    </div>

    <div class="flex min-h-0 flex-1 gap-4">
      <aside v-if="railPinned" class="w-52 shrink-0 xl:w-56">
        <premium-tv-premium-sidebar
          :view="liveTv.view"
          :selected-category="liveTv.selectedCategory"
          :categories="liveTv.categories"
          :total-channels="liveTv.totalChannels"
          :favorite-count="liveTv.favKeys.size"
          :recent-count="liveTv.recentChannels.length"
          @set-view="pickView"
          @set-category="liveTv.setCategory($event)"
        />
      </aside>

      <section class="flex min-h-0 flex-1 flex-col gap-2">
        <!-- Importing: an empty grid here means "not yet", not "none". -->
        <div v-if="liveTv.m3uImporting && channels.length === 0" class="grid flex-1 place-items-center text-center">
          <div class="flex flex-col items-center gap-3">
            <v-progress-circular indeterminate color="primary" size="36" />
            <p class="text-body-medium opacity-70">
              {{ $t('Importing the channel list…') }}
            </p>
            <p class="max-w-sm text-label-small opacity-45">
              {{ liveTv.m3uProgress.channels
                ? $t('{count} channels so far', { count: liveTv.m3uProgress.channels.toLocaleString() })
                : $t('This runs once and is then cached on this device.') }}
            </p>
          </div>
        </div>

        <div
          v-else-if="liveTv.visibleLoading && channels.length === 0"
          class="grid flex-1 auto-rows-max grid-cols-2 gap-3 overflow-hidden sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
          role="status"
          :aria-label="$t('Loading channels…')"
        >
          <div
            v-for="n in 24"
            :key="n"
            class="h-[132px] animate-pulse rounded-xl bg-surface-container-high/60"
          />
        </div>

        <div v-else-if="showEmpty" class="grid flex-1 place-items-center px-6 text-center">
          <div class="flex flex-col items-center gap-2">
            <v-icon :icon="mdiTelevisionOff" size="44" class="opacity-20" />
            <p class="text-body-large font-medium opacity-70">
              {{ $t('No channels found') }}
            </p>
            <p class="max-w-sm text-body-small opacity-45">
              {{ emptyMessage }}
            </p>
            <v-btn
              v-if="liveTv.totalChannels === 0"
              class="mt-2"
              color="primary"
              variant="tonal"
              :loading="liveTv.refreshing"
              @click="liveTv.refreshFreeTv()"
            >
              {{ $t('Refresh') }}
            </v-btn>
          </div>
        </div>

        <live-tv-live-channel-grid
          v-else
          :channels="channels"
          :get-epg="liveTv.getEpg"
          :is-favorite="liveTv.isFavorite"
          :is-offline="liveTv.isOffline"
          :density="density"
          :load-epg="loadForVisible"
          :has-more="hasMore"
          :loading="liveTv.visibleLoading"
          @load-more="liveTv.loadMore()"
          @play="playChannel"
          @toggle-favorite="liveTv.toggleFavorite($event)"
        />
      </section>
    </div>

    <teleport to="body">
      <transition name="fade">
        <div
          v-if="sheetOpen"
          class="fixed inset-0 z-50 flex items-end justify-end bg-black/50 md:items-stretch"
          @click.self="sheetOpen = false"
        >
          <div class="flex max-h-[80vh] w-full flex-col gap-3 rounded-t-2xl bg-surface-container p-4 md:h-full md:max-h-none md:w-80 md:rounded-none">
            <div class="flex items-center justify-between">
              <h2 class="text-title-large font-bold">
                {{ $t('Categories') }}
              </h2>
              <v-btn :icon="mdiClose" variant="text" size="small" :aria-label="$t('Close')" @click="sheetOpen = false" />
            </div>
            <div class="min-h-0 flex-1">
              <premium-tv-premium-sidebar
                :view="liveTv.view"
                :selected-category="liveTv.selectedCategory"
                :categories="liveTv.categories"
                :total-channels="liveTv.totalChannels"
                :favorite-count="liveTv.favKeys.size"
                :recent-count="liveTv.recentChannels.length"
                @set-view="pickView"
                @set-category="pickCategory"
              />
            </div>
          </div>
        </div>
      </transition>
    </teleport>
  </div>
</template>
