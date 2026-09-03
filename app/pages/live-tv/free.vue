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
 * A free playlist is other people's servers, so some of it is dead.
 * The browse grid does not probe them — that froze the page. The
 * player marks a channel offline when it fails and zaps past it.
 */
import type { UnlistenFn } from '@tauri-apps/api/event'
import type { LiveView } from '~/stores/liveTv'
import type { LiveChannel } from '~/utils/iptv'
import { mdiClose, mdiDeleteSweepOutline, mdiTelevisionOff } from '@mdi/js'
import { listen } from '@tauri-apps/api/event'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'

definePageMeta({ layout: 'default' })

const liveTv = useLiveTvStore()
const route = useRoute()
const router = useRouter()
const { mdAndUp, lgAndUp } = useDisplay()

/** Category from `/free/category/:category` or `?category=`. The parent
 *  route's `params` is a union of every nested page, so `params.category`
 *  is not always a field. A computed, not a helper called from `watch`:
 *  WebKitGTK throws "Can't find variable" when setup looks up a function
 *  binding that the compiler has not assigned yet. */
const liveCategory = computed(() => {
  const param = (route.params as Record<string, unknown>).category
  if (typeof param === 'string' && param)
    return decodeURIComponent(param)
  const query = route.query.category
  return typeof query === 'string' ? query : ''
})

/** `free.vue` is the parent route of `free/country|guide`. Category
 *  deep-links stay on this shell — same as Premium's category route. */
const isNestedRoute = computed(() => /\/live-tv\/free\/(?:country|guide)/.test(route.path))

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
      return categoryLabel(liveTv.selectedCategory) || $t('All channels')
    default:
      return $t('All channels')
  }
})

/** Primary when the list is usable; tertiary while it is still arriving. */
const status = computed(() => {
  if (liveTv.m3uImporting)
    return { tone: 'bg-tertiary', label: $t('Importing the channel list…') }
  if (liveTv.totalChannels === 0)
    return { tone: 'bg-outline', label: $t('No channels found') }
  return { tone: 'bg-primary', label: $t('Ready') }
})

/** Favorites strip on All only — Recent has its own sidebar section. */
const resumeStrip = computed(() => {
  if (liveTv.view !== 'all' || liveTv.searchQuery)
    return null
  if (liveTv.favoriteChannels.length)
    return { title: $t('Favorites'), channels: liveTv.favoriteChannels }
  return null
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

function goHub(): void {
  void router.replace(localePath('/live-tv'))
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
  liveTv.rememberChannel(ch.id)
  // The zap list is what is on screen: up/down on the player walks the
  // same order the viewer was just reading. It lives on the store so a
  // zap is a small query change, not a 60-channel JSON rewrite.
  const zapList = channels.value
    .filter(c => c.streamUrl && c.streamUrl !== 'undefined' && c.streamUrl !== 'null')
    .slice(0, 200)
    .map(c => ({
      id: c.id,
      name: c.name,
      logoUrl: c.logoUrl,
      streamUrl: c.streamUrl,
      userAgent: c.userAgent,
      referer: c.referer,
    }))
  liveTv.setZapList(zapList)
  saveLivePlay({
    id: ch.id,
    title: ch.name,
    logo: ch.logoUrl ?? '',
    sourceId: liveTv.activeSourceId || 'free:iptv-org',
    streamUrl: ch.streamUrl,
    userAgent: ch.userAgent,
    referer: ch.referer,
    zapList,
  })
  // Identity only — the stream URL stays on the zap list. Putting it in
  // the query broke on `&` in the path and blew the address bar up.
  void router.push({
    path: localePath('/live-tv/watch'),
    query: {
      id: ch.id,
      title: ch.name,
      logo: ch.logoUrl ?? '',
      type: 'live',
      sourceId: liveTv.activeSourceId || 'free:iptv-org',
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
  const category = liveCategory.value
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
  window.addEventListener('keydown', onKey)
})

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && sheetOpen.value) {
    e.preventDefault()
    sheetOpen.value = false
  }
}

onUnmounted(() => {
  unlisten?.()
  window.removeEventListener('keydown', onKey)
})

// Post-mount URL changes only (browser back/forward into a category).
watch(liveCategory, name => {
  if (name && name !== liveTv.selectedCategory)
    liveTv.setCategory(name)
})
</script>

<template>
  <nuxt-page v-if="isNestedRoute" />

  <div v-else class="flex h-full min-h-0 flex-col gap-3 px-4 py-4 md:px-6">
    <live-tv-live-browse-header
      v-model:search="liveTv.searchQuery"
      :heading="heading"
      :count="count"
      :status-tone="status.tone"
      :status-label="status.label"
      :status-text="$t('Free TV')"
      :status-meta="liveTv.offlineIds.size ? $t('{count} offline', { count: liveTv.offlineIds.size }) : undefined"
      :show-clear="liveTv.view === 'category' || !!liveTv.searchQuery"
      :refreshing="liveTv.refreshing"
      :show-tune="!railPinned"
      @back="goHub"
      @clear="liveTv.clearFilters()"
      @refresh="liveTv.refreshFreeTv()"
      @tune="sheetOpen = true"
    >
      <button
        v-if="liveTv.view === 'recent' && liveTv.recentChannels.length"
        type="button"
        class="grid size-11 shrink-0 place-items-center rounded-lg text-on-surface/70 transition-colors hover:bg-surface-container-highest hover:text-on-surface focus-visible:bg-surface-container-highest focus-visible:text-on-surface"
        :aria-label="$t('Clear recently watched')"
        @click="liveTv.clearRecent()"
      >
        <v-icon :icon="mdiDeleteSweepOutline" size="22" />
      </button>

      <template v-if="!railPinned" #below>
        <div
          class="-mx-1 flex gap-1 overflow-x-auto border-t border-outline/10 px-1 pt-2"
          role="tablist"
          :aria-label="$t('Browse')"
        >
          <button
            v-for="v in (['all', 'favorites', 'recent'] as const)"
            :key="v"
            type="button"
            role="tab"
            class="min-h-9 shrink-0 rounded-lg px-3 py-1.5 text-label-medium font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            :class="liveTv.view === v
              ? 'bg-primary text-on-primary'
              : 'text-on-surface/70 hover:bg-surface-container-high hover:text-on-surface focus-visible:bg-surface-container-high focus-visible:text-on-surface'"
            :aria-selected="liveTv.view === v ? 'true' : 'false'"
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
            {{ categoryLabel(liveTv.selectedCategory) }}
            <v-icon :icon="mdiClose" size="14" class="ms-1" />
          </button>
        </div>
      </template>
    </live-tv-live-browse-header>

    <div class="flex min-h-0 flex-1 gap-4">
      <aside v-if="railPinned" class="w-56 shrink-0 xl:w-60">
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
            class="h-[176px] animate-pulse rounded-lg bg-surface-container-high/60"
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

        <template v-else>
          <live-tv-live-channel-scroll-row
            v-if="resumeStrip"
            class="shrink-0"
            :title="resumeStrip.title"
            :channels="resumeStrip.channels"
            :get-epg="liveTv.getEpg"
            :is-favorite="liveTv.isFavorite"
            :is-offline="liveTv.isOffline"
            @play="playChannel"
            @toggle-favorite="liveTv.toggleFavorite($event)"
          />
          <live-tv-live-channel-grid
            class="min-h-0 flex-1"
            :channels="channels"
            :get-epg="liveTv.getEpg"
            :is-favorite="liveTv.isFavorite"
            :is-offline="liveTv.isOffline"
            :density="density"
            :has-more="hasMore"
            :loading="liveTv.visibleLoading"
            @load-more="liveTv.loadMore()"
            @play="playChannel"
            @toggle-favorite="liveTv.toggleFavorite($event)"
          />
        </template>
      </section>
    </div>

    <v-dialog v-model="sheetOpen" max-width="320" scrollable>
      <v-card class="bg-surface-container">
        <div class="flex items-center justify-between px-4 pt-4">
          <h2 class="text-title-large font-bold">
            {{ $t('Categories') }}
          </h2>
          <v-btn :icon="mdiClose" variant="text" size="small" :aria-label="$t('Close')" @click="sheetOpen = false" />
        </div>
        <v-card-text class="flex min-h-0 flex-1 flex-col !pt-2" style="height: min(80vh, 640px)">
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
        </v-card-text>
      </v-card>
    </v-dialog>
  </div>
</template>
