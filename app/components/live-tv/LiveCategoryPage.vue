<script setup lang="ts">
import type { LiveChannel } from '~/utils/iptv'
import { mdiChevronLeft, mdiFormatListBulleted, mdiMenu, mdiTelevision, mdiViewGrid } from '@mdi/js'

const props = defineProps<{ source: 'free' | 'premium' }>()

const liveTv = useLiveTvStore()
const route = useRoute()
const categoryName = computed(() => decodeURIComponent(String((route.params as Record<string, string>).category ?? '')))
const isFree = computed(() => props.source === 'free')
const sourceLabel = computed(() => isFree.value ? $t('Free TV') : $t('Premium TV'))
const sourcePath = computed(() => isFree.value ? '/live-tv/free' : '/live-tv/premium')

useHead({ title: () => `${categoryName.value} · ${sourceLabel.value}` })

const channels = shallowRef<LiveChannel[]>([])
const total = ref(0)
const nextCursor = ref<string | null>(null)
const loading = ref(false)
const search = ref('')
const searchDebounced = ref('')
const viewMode = ref<'grid' | 'list'>('grid')
let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null

watch(search, v => {
  if (searchDebounceTimer)
    clearTimeout(searchDebounceTimer)
  searchDebounceTimer = setTimeout(() => {
    searchDebounced.value = v
  }, 250)
})
let requestId = 0

const visibleChannels = computed(() => {
  const query = searchDebounced.value.trim().toLocaleLowerCase()
  if (!query)
    return channels.value
  return channels.value.filter(channel =>
    channel.name.toLocaleLowerCase().includes(query)
    || channel.country?.toLocaleLowerCase().includes(query),
  )
})

const cleanCategoryTitle = computed(() => {
  let name = categoryName.value || ''
  name = name.replace(/^\d+[:\-\s]+/, '')
  name = name.replace(/^\([\w\-\s]+\):?\s*/, '')
  name = name.replace(/^[\w\-]{2,6}:\s*/, '')
  name = name.replace(/\[[^\]]+\]/g, '')
  name = name.replace(/\([^)]+\)/g, '')
  return name.trim() || categoryName.value
})

async function loadPage(cursor?: string) {
  const id = ++requestId
  loading.value = true
  try {
    let page = await liveTv.loadChannelByCategory(categoryName.value, cursor)
    if (page.total === 0 && cleanCategoryTitle.value && cleanCategoryTitle.value !== categoryName.value) {
      page = await liveTv.loadChannelByCategory(cleanCategoryTitle.value, cursor)
    }
    if (id !== requestId)
      return
    channels.value = cursor ? [...channels.value, ...page.items] : page.items
    total.value = page.total
    nextCursor.value = page.nextCursor
  }
  finally {
    if (id === requestId)
      loading.value = false
  }
}

async function loadMore() {
  if (!loading.value && nextCursor.value)
    await loadPage(nextCursor.value)
}

function playChannel(channel: LiveChannel) {
  if (!channel.streamUrl || channel.streamUrl === 'undefined' || channel.streamUrl === 'null')
    return
  if (!liveTv.activeSourceId)
    return
  liveTv.rememberChannel(channel.id)
  const zapList = channels.value.filter(c => c.streamUrl).map(c => ({
    id: c.id,
    name: c.name,
    logoUrl: c.logoUrl,
    streamUrl: c.streamUrl,
    userAgent: c.userAgent,
    referer: c.referer,
  }))
  navigateTo({
    path: '/live-tv/watch',
    query: {
      url: channel.streamUrl,
      title: channel.name,
      logo: channel.logoUrl ?? '',
      id: channel.id,
      sourceId: liveTv.activeSourceId,
      list: encodeURIComponent(JSON.stringify(zapList)),
      from: route.fullPath,
    },
  })
}

onMounted(async () => {
  if (isFree.value) {
    await liveTv.useFreeSource()
  }
  await Promise.all([loadPage(), liveTv.loadFavorites()])
})
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- Breadcrumb bar -->
    <div class="flex items-center gap-3 px-4 pt-4 md:px-6">
      <v-btn icon variant="text" color="on-surface" density="comfortable" @click="navigateTo(sourcePath)">
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
          :to="localePath(sourcePath)"
          class="opacity-60 transition-opacity hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none"
        >
          {{ sourceLabel }}
        </nuxt-link>
        <span class="opacity-30">/</span>
        <span class="truncate font-semibold">{{ cleanCategoryTitle }}</span>
      </nav>
    </div>

    <!-- Category Hero Header -->
    <header class="relative mx-4 mt-3 overflow-hidden rounded-3xl border border-white/10 bg-gradient-to-b from-surface-container-high/90 via-surface-container/75 to-surface-container-lowest/85 p-5 shadow-2xl backdrop-blur-xl md:mx-6 md:p-6">
      <!-- Ambient background glows -->
      <div class="absolute -right-20 -top-20 size-72 rounded-full bg-primary/20 blur-3xl pointer-events-none" />
      <div class="absolute -left-20 -bottom-20 size-64 rounded-full bg-purple-600/15 blur-3xl pointer-events-none" />

      <div class="relative z-10">
        <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div class="flex items-center gap-3.5">
            <div class="grid size-12 shrink-0 place-items-center rounded-2xl bg-gradient-to-br from-primary/30 to-primary/10 ring-1 ring-primary/30 shadow-lg shadow-primary/20">
              <v-icon :icon="mdiMenu" size="26" color="primary" />
            </div>
            <div>
              <div class="flex items-center gap-2">
                <span class="inline-flex items-center gap-1.5 rounded-full bg-primary/15 px-2.5 py-0.5 text-[10px] font-bold uppercase tracking-widest text-primary ring-1 ring-primary/30">
                  {{ sourceLabel }}
                </span>
                <span class="text-label-small text-white/50">· {{ $t('Category') }}</span>
              </div>
              <h1 class="mt-1 text-headline-small font-black tracking-tight leading-none text-white md:text-headline-medium">
                {{ cleanCategoryTitle }}
              </h1>
            </div>
          </div>

          <!-- Channel Stats & View Controls -->
          <div class="flex items-center gap-2.5 self-start sm:self-center">
            <div class="flex items-center gap-2 rounded-2xl border border-white/10 bg-black/30 px-3.5 py-2 backdrop-blur-md">
              <span class="text-title-medium font-bold tabular-nums text-white">{{ total.toLocaleString() }}</span>
              <span class="text-label-small text-white/50">{{ $t('Channels') }}</span>
            </div>

            <!-- Grid vs List view toggle -->
            <div class="flex p-1 rounded-2xl border border-white/10 bg-black/40 backdrop-blur-md">
              <button
                type="button"
                class="grid size-8 place-items-center rounded-xl transition-all"
                :class="viewMode === 'grid' ? 'bg-primary text-on-primary font-bold shadow-md shadow-primary/30' : 'text-white/50 hover:bg-white/10 hover:text-white'"
                @click="viewMode = 'grid'"
              >
                <v-icon :icon="mdiViewGrid" size="18" />
              </button>
              <button
                type="button"
                class="grid size-8 place-items-center rounded-xl transition-all"
                :class="viewMode === 'list' ? 'bg-primary text-on-primary font-bold shadow-md shadow-primary/30' : 'text-white/50 hover:bg-white/10 hover:text-white'"
                @click="viewMode = 'list'"
              >
                <v-icon :icon="mdiFormatListBulleted" size="18" />
              </button>
            </div>
          </div>
        </div>

        <!-- Search bar inside header -->
        <div class="mt-4">
          <live-tv-search v-model="search" />
        </div>
      </div>
    </header>

    <main class="min-h-0 flex-1 overflow-y-auto px-4 pt-4 pb-6 md:px-6">
      <div v-if="loading && channels.length === 0" class="grid place-items-center py-20 text-white/55">
        <v-progress-circular indeterminate color="primary" size="32" />
      </div>
      <div v-else-if="visibleChannels.length === 0" class="grid place-items-center py-20 text-center text-white/60">
        <v-icon :icon="mdiTelevision" size="52" class="mb-3 opacity-20" />
        <p class="text-body-large font-medium">
          {{ $t('No channels found') }}
        </p>
      </div>

      <live-tv-live-channel-grid
        v-else-if="viewMode === 'grid'"
        :channels="visibleChannels"
        :get-epg="liveTv.getEpg"
        :is-favorite="liveTv.isFavorite"
        :density="liveTv.density"
        :load-epg="liveTv.loadEpgBatch"
        :has-more="!!nextCursor && !search"
        :loading="loading"
        @load-more="loadMore"
        @play="playChannel"
        @toggle-favorite="liveTv.toggleFavorite"
      />

      <live-tv-live-channel-list
        v-else
        :channels="visibleChannels"
        :get-epg="liveTv.getEpg"
        :is-favorite="liveTv.isFavorite"
        :load-epg="liveTv.loadEpgBatch"
        :has-more="!!nextCursor && !search"
        :loading="loading"
        @load-more="loadMore"
        @play="playChannel"
        @toggle-favorite="liveTv.toggleFavorite"
      />
    </main>
  </div>
</template>
