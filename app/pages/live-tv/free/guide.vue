<script setup lang="ts">
/**
 * Free TV guide page. Shows the same country/category filters as the
 * browse page, and a TV guide grid of the matching channels.
 */
import type { EpgProgram, LiveChannel } from '~/utils/iptv'
import { mdiArrowLeft, mdiViewGrid } from '@mdi/js'

definePageMeta({ layout: 'default' })

const liveTv = useLiveTvStore()
const route = useRoute()
const router = useRouter()
const { mobile } = useDisplay()

useHead({
  title: () => `${$t('TV Guide')} · ${$t('Free TV')}`,
})

/** Free channels for the grid, filtered by the store's current country/category. */
const guideChannels = computed<LiveChannel[]>(() => {
  // The store holds a 60-channel page; the guide is a one-screen view,
  // so we use it directly. Country/category filtering is server-side
  // already — the store's `visibleChannels` is the result of the last
  // query, which the user drove with the country/category filter.
  return liveTv.visibleChannels
})

/** Per-channel EPG map. No longer pre-built; we pass `liveTv.getEpg`
 *  directly to the grid so individual channel rows read lazily. This
 *  avoids building a fresh 60-entry Map every time a single channel's
 *  EPG cache entry lands (which was forcing the entire grid to observe
 *  a new prop reference and re-render unnecessarily). */
const _epgEmptyMap = new Map<string, EpgProgram[]>()

/** Eagerly load EPG for every visible channel. */
onMounted(async () => {
  // Make sure channels are loaded; we may have arrived directly.
  if (liveTv.visibleChannels.length === 0) {
    await liveTv.loadVisible({ reset: true })
  }
  // Load EPG for each channel (deduplicated, in-flight tracked inside the store).
  for (const ch of guideChannels.value) {
    if (ch.epgId)
      liveTv.loadFreeEpg(ch.epgId)
  }
})

/** Re-load EPG when the filtered set changes. */
watch(guideChannels, channels => {
  for (const ch of channels) {
    if (ch.epgId)
      liveTv.loadFreeEpg(ch.epgId)
  }
})

function goBack() {
  router.replace(localePath('/live-tv/free'))
}

function onProgram(ch: LiveChannel) {
  // Open the player with the channel list for zapping.
  const zapList = guideChannels.value
    .filter(c => c.streamUrl)
    .map(c => ({ id: c.id, name: c.name, logoUrl: c.logoUrl, streamUrl: c.streamUrl, userAgent: c.userAgent, referer: c.referer }))
  liveTv.setZapList(zapList)
  if (ch.streamUrl) {
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
  }
  navigateTo({
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
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- Header -->
    <div class="flex items-center gap-3 px-4 pt-4 md:px-6">
      <v-btn icon variant="text" color="on-surface" @click="goBack">
        <v-icon :icon="mdiArrowLeft" />
      </v-btn>
      <v-icon :icon="mdiViewGrid" size="24" color="primary" />
      <div class="min-w-0 flex-1">
        <h1 class="truncate text-headline-medium font-bold">
          {{ $t('TV Guide') }}
        </h1>
        <p class="text-body-small opacity-60">
          {{ $t('Free TV') }} · {{ $t('{count} channels', { count: guideChannels.length }) }}
        </p>
      </div>
    </div>

    <!-- Search -->
    <div class="mt-2 px-4 md:px-6">
      <live-tv-search v-model="liveTv.searchQuery" />
    </div>

    <!-- Guide grid -->
    <div class="min-h-0 flex-1">
      <live-tv-live-guide-grid
        :channels="guideChannels"
        :epg-by-channel="_epgEmptyMap"
        :get-epg="liveTv.getEpg"
        @channel="onProgram"
      />
    </div>
  </div>
</template>
