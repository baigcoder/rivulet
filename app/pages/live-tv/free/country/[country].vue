<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { mdiChevronLeft, mdiEarth, mdiFormatListBulleted, mdiTelevision, mdiViewGrid } from '@mdi/js'

definePageMeta({ layout: 'default' })

const liveTv = useLiveTvStore()
const route = useRoute()
const { mobile } = useDisplay()

const countryName = computed(() => decodeURIComponent(String(route.params.country)))

useHead({
  title: () => `${countryName.value} · ${$t('Free TV')}`,
})

const visibleChannels = shallowRef<LiveChannel[]>([])
const totalChannels = ref(0)
const nextCursor = ref<string | null>(null)
const loading = ref(false)
const searchQuery = ref('')
const searchDebounced = ref('')
let searchTimer: ReturnType<typeof setTimeout> | null = null
const viewMode = ref<'grid' | 'list'>('grid')

watch(searchQuery, val => {
  if (searchTimer)
    clearTimeout(searchTimer)
  searchTimer = setTimeout(() => {
    searchDebounced.value = val
  }, 200)
})

let currentRequestId = 0

async function loadPage(cursor?: string) {
  const id = liveTv.activeSourceId
  if (!id)
    return
  const reqId = ++currentRequestId
  loading.value = true
  try {
    const page = await liveTv.loadChannelByCountry(countryName.value, cursor)
    if (reqId !== currentRequestId)
      return
    if (cursor) {
      visibleChannels.value = [...visibleChannels.value, ...page.items]
    }
    else {
      visibleChannels.value = page.items
    }
    totalChannels.value = page.total
    nextCursor.value = page.nextCursor
  }
  finally {
    if (reqId === currentRequestId)
      loading.value = false
  }
}

async function loadMore() {
  if (!nextCursor.value || loading.value)
    return
  await loadPage(nextCursor.value)
}

const countryCategories = computed(() => {
  const map = new Map<string, number>()
  for (const ch of visibleChannels.value) {
    if (ch.categoryName) {
      map.set(ch.categoryName, (map.get(ch.categoryName) ?? 0) + 1)
    }
  }
  return [...map.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count)
})

const selectedCategory = ref('')

onMounted(async () => {
  await liveTv.useFreeSource()
  await loadPage()
  await liveTv.loadFavorites()
})

function playChannel(ch: LiveChannel) {
  if (!ch.streamUrl || ch.streamUrl === 'undefined' || ch.streamUrl === 'null')
    return
  liveTv.rememberChannel(ch.id)
  const zapList = visibleChannels.value
    .filter(c => c.streamUrl)
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

function selectCategory(name: string) {
  selectedCategory.value = selectedCategory.value === name ? '' : name
}

function goBack() {
  navigateTo(localePath('/live-tv/free'))
}

const filteredChannels = computed(() => {
  let list = visibleChannels.value
  if (selectedCategory.value) {
    list = list.filter(ch => ch.categoryName === selectedCategory.value)
  }
  const q = searchDebounced.value.trim().toLowerCase()
  if (q) {
    list = list.filter(ch =>
      ch.name.toLowerCase().includes(q)
      || (ch.categoryName && ch.categoryName.toLowerCase().includes(q)),
    )
  }
  return list
})
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- Breadcrumb header -->
    <div class="flex items-center gap-3 px-4 pt-4 md:px-6">
      <v-btn icon variant="text" color="on-surface" density="comfortable" @click="goBack">
        <v-icon :icon="mdiChevronLeft" />
      </v-btn>
      <nav class="flex min-w-0 items-center gap-1 text-body-small">
        <nuxt-link
          :to="localePath('/live-tv')"
          class="opacity-60 transition-opacity hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none"
        >
          {{ $t('Live TV') }}
        </nuxt-link>
        <span class="opacity-30">/</span>
        <nuxt-link
          :to="localePath('/live-tv/free')"
          class="opacity-60 transition-opacity hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none"
        >
          {{ $t('Free TV') }}
        </nuxt-link>
        <span class="opacity-30">/</span>
        <span class="truncate font-semibold">{{ countryName }}</span>
      </nav>
    </div>

    <!-- Country Hero Banner -->
    <header class="relative mx-4 mt-3 overflow-hidden rounded-2xl md:mx-6">
      <div class="absolute inset-0 bg-surface-container-high" />
      <div class="absolute inset-0 bg-[radial-gradient(ellipse_70%_90%_at_100%_0%,rgba(var(--v-theme-primary),0.25),transparent)]" />
      <div class="absolute inset-0 rounded-2xl ring-1 ring-inset ring-primary/20 pointer-events-none" />

      <div class="relative px-5 py-4 md:px-6">
        <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div class="flex items-center gap-3.5">
            <div class="grid size-12 shrink-0 place-items-center rounded-2xl bg-primary/15 ring-1 ring-primary/30">
              <v-icon :icon="mdiEarth" size="26" color="primary" />
            </div>
            <div>
              <div class="flex items-center gap-2">
                <span class="text-label-small font-semibold uppercase tracking-[0.14em] text-primary">
                  {{ $t('Free TV') }}
                </span>
                <span class="size-1 rounded-full bg-white/20" />
                <span class="text-label-small text-white/50">{{ $t('Country view') }}</span>
              </div>
              <h1 class="text-headline-medium font-black tracking-tight leading-tight">
                {{ countryName }}
              </h1>
            </div>
          </div>

          <!-- Channel Stats & View Controls -->
          <div class="flex items-center gap-2 self-start sm:self-center">
            <span class="rounded-xl border border-white/10 bg-black/25 px-3.5 py-1.5 text-label-medium font-bold text-white/80 backdrop-blur-sm">
              {{ totalChannels.toLocaleString() }} {{ $t('channels') }}
            </span>

            <!-- Grid vs List view toggle -->
            <div class="flex overflow-hidden rounded-xl border border-white/10 bg-black/25 backdrop-blur-sm">
              <button
                type="button"
                class="grid size-8.5 place-items-center transition-colors"
                :class="viewMode === 'grid' ? 'bg-primary text-on-primary font-bold' : 'text-white/50 hover:bg-white/10'"
                @click="viewMode = 'grid'"
              >
                <v-icon :icon="mdiViewGrid" size="18" />
              </button>
              <button
                type="button"
                class="grid size-8.5 place-items-center transition-colors"
                :class="viewMode === 'list' ? 'bg-primary text-on-primary font-bold' : 'text-white/50 hover:bg-white/10'"
                @click="viewMode = 'list'"
              >
                <v-icon :icon="mdiFormatListBulleted" size="18" />
              </button>
            </div>
          </div>
        </div>

        <!-- Search bar inside header -->
        <div class="mt-4">
          <live-tv-search v-model="searchQuery" />
        </div>
      </div>
    </header>

    <!-- Category filter chips -->
    <div v-if="countryCategories.length" class="mt-3 flex gap-2 overflow-x-auto px-4 pb-1 scrollbar-none md:px-6">
      <button
        type="button"
        class="flex shrink-0 items-center gap-1.5 rounded-full border px-3.5 py-1.5 text-body-small font-medium transition-colors"
        :class="!selectedCategory
          ? 'border-primary bg-primary/20 text-primary font-semibold ring-1 ring-primary/40'
          : 'border-white/10 bg-surface-container-high text-white/70 hover:bg-surface-container hover:text-white'"
        @click="selectCategory('')"
      >
        <span>{{ $t('All') }}</span>
        <span class="rounded-full bg-white/10 px-1.5 py-0.5 text-[10px] tabular-nums">{{ visibleChannels.length }}</span>
      </button>

      <button
        v-for="cat in countryCategories"
        :key="cat.name"
        type="button"
        class="flex shrink-0 items-center gap-1.5 rounded-full border px-3.5 py-1.5 text-body-small font-medium transition-colors"
        :class="selectedCategory === cat.name
          ? 'border-primary bg-primary/20 text-primary font-semibold ring-1 ring-primary/40'
          : 'border-white/10 bg-surface-container-high text-white/70 hover:bg-surface-container hover:text-white'"
        @click="selectCategory(cat.name)"
      >
        <span>{{ cat.name }}</span>
        <span class="rounded-full bg-white/10 px-1.5 py-0.5 text-[10px] tabular-nums">{{ cat.count }}</span>
      </button>
    </div>

    <!-- Main Content Area -->
    <main class="min-h-0 flex-1 overflow-y-auto px-4 pt-3 pb-6 md:px-6">
      <div v-if="filteredChannels.length === 0 && !loading" class="grid place-items-center py-20 text-center text-white/55">
        <v-icon :icon="mdiTelevision" size="52" class="mb-3 opacity-20" />
        <p class="text-body-large font-medium">
          {{ $t('No channels found') }}
        </p>
        <p v-if="searchQuery" class="mt-1 text-body-small opacity-50">
          {{ $t('Try searching with a different term or clearing your category filter.') }}
        </p>
      </div>

      <live-tv-live-channel-grid
        v-else-if="viewMode === 'grid'"
        :channels="filteredChannels"
        :get-epg="liveTv.getEpg"
        :is-favorite="liveTv.isFavorite"
        :density="liveTv.density"
        :has-more="!!nextCursor && !searchQuery && !selectedCategory"
        :loading="loading"
        @load-more="loadMore"
        @play="playChannel"
        @toggle-favorite="liveTv.toggleFavorite"
      />

      <live-tv-live-channel-list
        v-else
        :channels="filteredChannels"
        :get-epg="liveTv.getEpg"
        :is-favorite="liveTv.isFavorite"
        :has-more="!!nextCursor && !searchQuery && !selectedCategory"
        :loading="loading"
        @load-more="loadMore"
        @play="playChannel"
        @toggle-favorite="liveTv.toggleFavorite"
      />
    </main>
  </div>
</template>
