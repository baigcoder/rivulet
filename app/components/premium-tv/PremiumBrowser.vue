<script setup lang="ts">
/**
 * Premium TV's browsing surface: one header row, the category rail and
 * the virtualized grid.
 *
 * The header is deliberately *one row*. What was here before opened with
 * the full account panel — provider, expiry, trial, connection count,
 * catalog and guide ages — which is a page about the subscription sitting
 * on top of a page about channels, and it pushed the first row of cards
 * off a 768px-tall screen. The account panel still exists, in
 * *Settings → Premium TV*, which is where an account is managed. What a
 * viewer needs while browsing is where they are, how many channels that
 * is, a box to search it, and one dot saying the provider is still
 * answering; that is what the row carries.
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
import type { IPTVChannel } from '~/types/premium'
import { mdiArrowLeft, mdiClose, mdiDeleteSweepOutline, mdiLogout, mdiMagnify, mdiRefresh, mdiTelevisionOff, mdiTune } from '@mdi/js'
import { computed, onMounted, ref } from 'vue'

/**
 * `showBack` is set by the deep-linked category route, which used to draw
 * its own back button and its own copy of the category name above this
 * component — two headings for one thing, and a row of vertical space to
 * say it twice. The button belongs on the heading that is already there.
 */
const props = defineProps<{ showBack?: boolean }>()

const premium = usePremiumTvStore()
const router = useRouter()
const { mdAndUp, lgAndUp } = useDisplay()

/** The rail is only ever pinned where there is width for it. */
const railPinned = computed(() => lgAndUp.value)
const sheetOpen = ref(false)
const busy = ref(false)

const density = computed<'compact' | 'comfortable'>(() => mdAndUp.value ? 'comfortable' : 'compact')

onMounted(async () => {
  await premium.ensureLoaded()
  if (premium.connected && premium.channels.length === 0)
    await premium.loadChannels({ reset: true })
})

const heading = computed(() => {
  switch (premium.view) {
    case 'favorites':
      return $t('Favorites')
    case 'recent':
      return $t('Recently watched')
    case 'category':
      return premium.selectedCategory || $t('All channels')
    default:
      return $t('All channels')
  }
})

/**
 * `total` is the server's count for the query, which is the number worth
 * showing: "1,432 channels" while 60 are loaded is the truth about the
 * filter, and the count of what happens to be in memory is not.
 */
const count = computed(() => premium.total)

/**
 * The provider, in one dot and one line. Green is not decoration here:
 * amber is the one state a viewer can *act* on — every connection the
 * account allows is in use, so the next channel they click will be
 * refused by the panel and not by us.
 */
const status = computed(() => {
  if (!premium.connected)
    return { tone: 'bg-white/25', label: $t('Not connected') }
  if (premium.atConnectionLimit === true)
    return { tone: 'bg-amber-400', label: $t('All connections in use') }
  return { tone: 'bg-emerald-400', label: $t('Connected') }
})

const providerLabel = computed(() =>
  premium.account?.accountName?.trim() || premium.account?.username || $t('Premium TV'),
)

/** How stale the on-disk catalog is; the only account fact worth a header. */
const syncedAgo = computed(() => {
  const secs = premium.catalog?.catalogSyncedAt
  if (!secs)
    return ''
  const mins = Math.floor((Date.now() / 1000 - secs) / 60)
  if (mins < 1)
    return $t('just now')
  if (mins < 60)
    return $t('{minutes} min ago', { minutes: mins })
  const hours = Math.floor(mins / 60)
  if (hours < 24)
    return $t('{hours} h ago', { hours })
  return $t('{days} d ago', { days: Math.floor(hours / 24) })
})

const showEmpty = computed(() =>
  premium.connected
  && !premium.listLoading
  && !premium.importing
  && premium.channels.length === 0,
)

const emptyMessage = computed(() => {
  if (premium.searchDebounced)
    return $t('No channels match that search.')
  if (premium.view === 'favorites')
    return $t('Star a channel and it shows up here.')
  if (premium.view === 'recent')
    return $t('Channels you watch appear here.')
  return $t('This provider returned no channels for that filter.')
})

function play(channel: IPTVChannel): void {
  void router.push({
    path: localePath('/live-tv/premium/watch'),
    query: { id: channel.id },
  })
}

function goBack(): void {
  void router.replace(localePath('/live-tv/premium'))
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
    <!--
      One row: where you are, how many that is, the search box, the
      provider dot and the two controls that belong to the catalog. It
      wraps rather than scrolls, so a 1366px screen loses nothing and a
      phone stacks the search under the heading.
    -->
    <header class="flex flex-wrap items-center gap-x-3 gap-y-2">
      <v-btn
        v-if="props.showBack"
        :icon="mdiArrowLeft"
        variant="text"
        size="small"
        class="shrink-0"
        :aria-label="$t('Back')"
        @click="goBack"
      />

      <div class="flex min-w-0 items-baseline gap-2">
        <h1 class="truncate text-title-medium font-bold">
          {{ heading }}
        </h1>
        <span class="shrink-0 text-label-small tabular-nums opacity-45">
          {{ $t('{count} channels', { count: count.toLocaleString() }) }}
        </span>
        <!-- The heading *is* the active filter, so the way out of it sits
             on the heading. Only rendered when there is one to clear. -->
        <v-btn
          v-if="premium.view === 'category' || premium.searchQuery"
          :icon="mdiClose"
          variant="text"
          size="x-small"
          class="shrink-0"
          :aria-label="$t('Clear filters')"
          @click="premium.clearFilters()"
        />
      </div>

      <!-- `order-last` on a phone: the heading reads first and the box it
           searches sits under it, full width. -->
      <div class="order-last w-full min-w-0 sm:order-none sm:ms-auto sm:w-64 md:w-72">
        <v-text-field
          v-model="premium.searchQuery"
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
        <!-- Dot plus a title, not a pill of text: the state is almost
             always "fine", and "fine" should cost one glyph. -->
        <span
          class="size-2 shrink-0 rounded-full"
          :class="status.tone"
          role="img"
          :aria-label="status.label"
          :title="status.label"
        />
        <span class="hidden max-w-40 truncate text-label-small opacity-55 lg:inline">
          {{ providerLabel }}
        </span>
        <span v-if="syncedAgo" class="hidden text-label-small opacity-35 xl:inline">
          · {{ syncedAgo }}
        </span>

        <v-btn
          v-if="premium.view === 'recent' && premium.recent.length"
          :icon="mdiDeleteSweepOutline"
          variant="text"
          size="small"
          :aria-label="$t('Clear recently watched')"
          :title="$t('Clear recently watched')"
          @click="premium.clearRecent()"
        />
        <v-btn
          :icon="mdiRefresh"
          variant="text"
          size="small"
          :loading="busy || premium.catalog?.syncing === true"
          :aria-label="$t('Refresh')"
          :title="$t('Refresh')"
          @click="refresh"
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
        <v-btn
          :icon="mdiLogout"
          variant="text"
          size="small"
          color="error"
          :disabled="busy"
          :aria-label="$t('Disconnect')"
          :title="$t('Disconnect')"
          @click="disconnect"
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
        :class="premium.view === v
          ? 'bg-primary text-on-primary ring-primary'
          : 'bg-surface-container-high text-white/70 ring-white/10 hover:text-white'"
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
        {{ premium.selectedCategory }}
        <v-icon :icon="mdiClose" size="14" class="ms-1" />
      </button>
    </div>

    <div class="flex min-h-0 flex-1 gap-4">
      <!-- Narrow on purpose: the rail is a list of short group names
           and a count, and every pixel it takes is a pixel the grid does
           not have for a sixth column. -->
      <aside v-if="railPinned" class="w-52 shrink-0 xl:w-56">
        <premium-tv-premium-sidebar
          :view="premium.view"
          :selected-category="premium.selectedCategory"
          :categories="premium.categoryCounts"
          :total-channels="premium.catalog?.channels ?? 0"
          :favorite-count="premium.favoriteIds.size"
          :recent-count="premium.recent.length"
          @set-view="premium.setView($event)"
          @set-category="premium.setCategory($event)"
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
          v-else-if="premium.listLoading && premium.channels.length === 0"
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
          </div>
        </div>

        <premium-tv-premium-channel-grid
          v-else
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
      </section>
    </div>

    <!-- Tablet / phone category sheet. Teleported so the page's own
         scrollers cannot clip it. -->
    <teleport to="body">
      <transition name="fade">
        <div
          v-if="sheetOpen"
          class="fixed inset-0 z-50 flex items-end justify-end bg-black/50 md:items-stretch"
          @click.self="sheetOpen = false"
        >
          <div class="flex max-h-[80vh] w-full flex-col gap-3 rounded-t-2xl bg-surface-container p-4 md:max-h-none md:h-full md:w-80 md:rounded-none">
            <div class="flex items-center justify-between">
              <h2 class="text-title-large font-bold">
                {{ $t('Categories') }}
              </h2>
              <v-btn :icon="mdiClose" variant="text" size="small" :aria-label="$t('Close')" @click="sheetOpen = false" />
            </div>
            <div class="min-h-0 flex-1">
              <premium-tv-premium-sidebar
                :view="premium.view"
                :selected-category="premium.selectedCategory"
                :categories="premium.categoryCounts"
                :total-channels="premium.catalog?.channels ?? 0"
                :favorite-count="premium.favoriteIds.size"
                :recent-count="premium.recent.length"
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
