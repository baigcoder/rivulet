<script setup lang="ts">
/**
 * Premium movie detail — the library title page, Play stays on the
 * provider stream.
 */
import type { PremiumVodItem } from '~/types/premium'
import { computed, onMounted, ref, watch } from 'vue'
import { proxyLogo } from '~/utils/premiumTv'

definePageMeta({ layout: 'default' })

const route = useRoute()
const router = useRouter()
const premium = usePremiumTvStore()

const movieId = computed(() => String(route.params.id ?? ''))
const ext = computed(() => String(route.query.ext ?? 'mkv'))

const movie = ref<PremiumVodItem | null>(null)
const tmdbId = ref('')

const rawTitle = computed(() =>
  movie.value?.name || String(route.query.title ?? '') || '',
)
const labelled = computed(() => vodDisplayName(rawTitle.value))
const displayName = computed(() => labelled.value.name || $t('Movie'))
const fallbackYear = computed(() => labelled.value.year)
const fallbackPoster = computed(() =>
  proxyLogo(movie.value?.posterUrl || String(route.query.poster ?? '') || '') || '',
)
const fallbackOverview = computed(() => movie.value?.plot || '')
const fallbackRating = computed(() => {
  const n = Number(movie.value?.rating)
  return Number.isFinite(n) && n > 0 ? n : 0
})

onMounted(async () => {
  await premium.ensureLoaded()
  movie.value = premium.vodMovies.find(m => m.id === movieId.value) ?? null
})

watch(rawTitle, async name => {
  if (!name)
    return
  const hit = await tmdbMatchByTitle(name, 'movie')
  if (hit)
    tmdbId.value = String(hit)
}, { immediate: true })

function play(): void {
  void router.push({
    path: localePath('/live-tv/premium/watch'),
    query: {
      kind: 'movie',
      id: movieId.value,
      ext: ext.value,
      title: displayName.value,
      from: route.fullPath,
    },
  })
}
</script>

<template>
  <media-detail-view
    :id="tmdbId"
    type="movie"
    provider-play
    :fallback-title="displayName"
    :fallback-poster="fallbackPoster"
    :fallback-overview="fallbackOverview"
    :fallback-year="fallbackYear"
    :fallback-rating="fallbackRating"
    @play="play"
  />
</template>
