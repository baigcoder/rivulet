<script setup lang="ts">
/**
 * Premium series detail — the library title page. Episodes play the
 * provider stream, not a torrent season.
 */
import type { PremiumEpisode, PremiumSeriesDetail } from '~/types/premium'
import { premiumApi, proxyLogo } from '~/utils/premiumTv'

definePageMeta({ layout: 'default' })

const route = useRoute()
const router = useRouter()
const premium = usePremiumTvStore()

const seriesId = computed(() => String(route.params.id ?? ''))
const detail = ref<PremiumSeriesDetail | null>(null)
const selectedSeason = ref<number | null>(null)
const tmdbId = ref(peekPremiumTmdb('tv', String(route.params.id ?? '')))

const rawTitle = computed(() =>
  detail.value?.name || String(route.query.title ?? '') || '',
)
const labelled = computed(() => vodDisplayName(rawTitle.value))
const displayName = computed(() => labelled.value.name || $t('TV show'))
const fallbackYear = computed(() => labelled.value.year)
const fallbackPoster = computed(() =>
  proxyLogo(detail.value?.posterUrl || String(route.query.poster ?? '') || '') || '',
)
const fallbackOverview = computed(() => detail.value?.plot || '')
const fallbackRating = computed(() => {
  const n = Number(detail.value?.rating)
  return Number.isFinite(n) && n > 0 ? n : 0
})

const seasons = computed(() => {
  const map = new Map<number, PremiumEpisode[]>()
  for (const ep of detail.value?.episodes ?? []) {
    const list = map.get(ep.season) ?? []
    list.push(ep)
    map.set(ep.season, list)
  }
  return [...map.entries()].sort((a, b) => a[0] - b[0])
})

const seasonNumbers = computed(() => seasons.value.map(([n]) => n))

const activeSeason = computed(() => {
  if (selectedSeason.value != null)
    return selectedSeason.value
  return seasonNumbers.value[0] ?? null
})

const visibleEpisodes = computed(() => {
  if (activeSeason.value == null)
    return []
  return seasons.value.find(([n]) => n === activeSeason.value)?.[1] ?? []
})

watch(seriesId, id => {
  const hit = peekPremiumTmdb('tv', id)
  if (hit)
    tmdbId.value = hit
}, { immediate: true })

watch(rawTitle, async name => {
  if (!name)
    return
  const hit = await tmdbMatchByTitle(name, 'tv')
  if (hit) {
    tmdbId.value = String(hit)
    snapPremiumTmdb('tv', seriesId.value, hit)
  }
}, { immediate: true })

onMounted(() => {
  void (async () => {
    await premium.ensureLoaded()
    try {
      const cached = premium.seriesDetailCache.get(seriesId.value)
      detail.value = cached ?? await premiumApi.vodSeriesDetail(seriesId.value)
      if (!cached && detail.value)
        premium.cacheSeriesDetail(seriesId.value, detail.value)
      selectedSeason.value = seasonNumbers.value[0] ?? null
    }
    catch {
      // Title + TMDB extras still render; episodes stay empty.
    }
  })()
})

function playEpisode(ep: PremiumEpisode): void {
  void router.push({
    path: localePath('/live-tv/premium/watch'),
    query: {
      kind: 'episode',
      id: ep.id,
      ext: ep.containerExtension || 'mkv',
      title: `${detail.value?.name ?? displayName.value} · S${ep.season}E${ep.episode}`,
      from: route.fullPath,
    },
  })
}

function play(): void {
  const ep = visibleEpisodes.value[0]
  if (ep)
    playEpisode(ep)
}

function pickSeason(n: number): void {
  selectedSeason.value = n
}
</script>

<template>
  <media-detail-view
    :id="tmdbId"
    type="tv"
    provider-play
    hide-seasons
    :fallback-title="displayName"
    :fallback-poster="fallbackPoster"
    :fallback-overview="fallbackOverview"
    :fallback-year="fallbackYear"
    :fallback-rating="fallbackRating"
    @play="play"
  >
    <template #below>
      <section v-if="seasonNumbers.length || visibleEpisodes.length" class="px-4 md:px-6">
        <div v-if="seasonNumbers.length > 1" class="mb-3 flex gap-2 overflow-x-auto pb-1">
          <button
            v-for="n in seasonNumbers"
            :key="n"
            type="button"
            class="shrink-0 rounded-full px-4 py-2 text-label-medium font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            :class="activeSeason === n
              ? 'bg-primary text-on-primary'
              : 'bg-surface-container-high text-on-surface/75 hover:bg-surface-container-highest hover:text-on-surface focus-visible:bg-surface-container-highest focus-visible:text-on-surface'"
            :aria-current="activeSeason === n ? 'true' : undefined"
            @click="pickSeason(n)"
          >
            {{ $t('Season {n}', { n }) }}
          </button>
        </div>

        <h2 class="mb-3 text-title-medium font-bold">
          {{ activeSeason != null ? $t('Season {n}', { n: activeSeason }) : $t('Seasons') }}
        </h2>

        <premium-episode-list :episodes="visibleEpisodes" @play="playEpisode" />
      </section>
    </template>
  </media-detail-view>
</template>
