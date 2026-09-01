<script setup lang="ts">
import type { TmdbPage } from '~/utils/tmdb'
import { mdiArrowRight, mdiArrowUp, mdiBookmark, mdiBookmarkOutline, mdiChevronLeft, mdiChevronRight, mdiClose, mdiHeart, mdiHeartOutline, mdiInformationOutline, mdiPlay, mdiStar, mdiTelevision } from '@mdi/js'

const ui = useUiStore()
const library = useLibraryStore()

const { data: trending } = useAsyncData(
  'home-trending',
  () => tmdb<TmdbPage>('/trending/all/day'),
  { lazy: true, transform: page => page.results.flatMap(item => toMedia(item) ?? []) },
)

/**
 * The top spotlight items.
 */
const spotlight = computed(() => (trending.value ?? []).filter(m => m.backdrop).slice(0, 10))

const at = ref(0)
const featured = computed(() => spotlight.value[Math.min(at.value, spotlight.value.length - 1)])

// Update ambient background when featured title changes
watch(featured, media => media && ui.ambient(media), { immediate: true })

// Fetch detail for the featured item (for logo, trailer, runtime, certification)
const featuredDetail = ref<{ logo: string | null, trailer: string | null, runtime: number, certification: string, genres: { name: string }[] } | null>(null)
watch(featured, async m => {
  if (!m) { featuredDetail.value = null; return }
  try {
    const data = await tmdb<any>(`/${m.type}/${m.id}`)
    featuredDetail.value = {
      logo: data.logo?.file_path ?? null,
      trailer: data.videos?.results?.find((v: any) => v.type === 'Trailer' && v.site === 'YouTube')?.key ?? null,
      runtime: data.runtime ?? data.episode_run_time?.[0] ?? 0,
      certification: data.release_dates?.results?.find((d: any) => d.iso_3166_1 === 'US')?.release_dates?.[0]?.certification ?? '',
      genres: data.genres ?? [],
    }
  }
  catch {
    featuredDetail.value = null
  }
}, { immediate: true })

// Preload the next backdrop image for instant crossfade
watch(featured, (_m, _old, onCleanup) => {
  const nextIdx = (at.value + 1) % spotlight.value.length
  const next = spotlight.value[nextIdx]
  if (next?.backdrop) {
    const img = new Image()
    img.src = backdropUrl(next.backdrop, 'w1280') ?? ''
    onCleanup(() => { img.src = '' })
  }
})

// ── Hero actions ──────────────────────────────────────────────────────────────
const trailerDialog = ref(false)
const trailerKey = computed(() => featuredDetail.value?.trailer)

function runtimeText(min?: number) {
  if (!min)
    return ''
  const h = Math.floor(min / 60)
  const m = min % 60
  return h ? `${h}h ${m ? `${m}m` : ''}` : `${m}m`
}

// ── Auto-advance spotlight slider every 6 seconds, pausing on hover or focus ──
const isHovered = ref(false)
const progress = ref(0)
let timer: ReturnType<typeof setInterval> | null = null
let animFrame: number | null = null
let startTime = Date.now()
const DURATION = 6000

function nextSlide() {
  if (!spotlight.value.length)
    return
  at.value = (at.value + 1) % spotlight.value.length
}

function prevSlide() {
  if (!spotlight.value.length)
    return
  at.value = (at.value - 1 + spotlight.value.length) % spotlight.value.length
}

function startTimer() {
  stopTimer()
  startTime = Date.now()
  timer = setInterval(() => {
    if (!isHovered.value && spotlight.value.length > 1) {
      nextSlide()
      startTime = Date.now()
    }
  }, DURATION)

  function updateProgress() {
    if (!isHovered.value && spotlight.value.length > 1) {
      const elapsed = (Date.now() - startTime) % DURATION
      progress.value = Math.min(100, (elapsed / DURATION) * 100)
    }
    else if (isHovered.value) {
      startTime = Date.now() - (progress.value / 100) * DURATION
    }
    animFrame = requestAnimationFrame(updateProgress)
  }
  animFrame = requestAnimationFrame(updateProgress)
}

function stopTimer() {
  if (timer)
    clearInterval(timer)
  if (animFrame)
    cancelAnimationFrame(animFrame)
  timer = null
  animFrame = null
}

watch(at, () => {
  startTime = Date.now()
  progress.value = 0
})

onMounted(() => {
  startTimer()
})

onUnmounted(() => {
  stopTimer()
})

// ── TMDB content rows ─────────────────────────────────────────────────────────
const rows = computed(() => {
  const day = (offset = 0) => new Date(Date.now() + offset * 864e5).toISOString().slice(0, 10)
  return [
    { title: $t('Trending this week'), request: { path: '/trending/all/week' }, to: `/movies?cat=trending&label=${encodeURIComponent($t('Trending this week'))}` },
    { title: $t('Popular movies'), request: { path: '/movie/popular', type: 'movie' as const }, to: `/movies?label=${encodeURIComponent($t('Popular movies'))}` },
    { title: $t('Popular TV'), request: { path: '/tv/popular', type: 'tv' as const }, to: `/tv?label=${encodeURIComponent($t('Popular TV'))}` },
    {
      title: $t('Bollywood Trending'),
      request: { path: '/discover/movie', type: 'movie' as const, params: { with_original_language: 'hi', sort_by: 'popularity.desc' } },
      to: `/movies?label=${encodeURIComponent($t('Bollywood Trending'))}`,
    },
    { title: $t('Top rated'), request: { path: '/movie/top_rated', type: 'movie' as const }, to: `/movies?cat=top&label=${encodeURIComponent($t('Top rated'))}` },
    { title: $t('Now playing'), request: { path: '/movie/now_playing', type: 'movie' as const }, to: `/movies?cat=now&label=${encodeURIComponent($t('Now playing'))}` },
    {
      title: $t('Upcoming movies'),
      request: {
        path: '/discover/movie',
        type: 'movie' as const,
        params: { 'include_adult': false, 'sort_by': 'popularity.desc', 'primary_release_date.gte': day(), 'primary_release_date.lte': day(180) },
      },
      to: `/movies?cat=upcoming&label=${encodeURIComponent($t('Upcoming movies'))}`,
    },
    { title: $t('Airing today'), request: { path: '/tv/airing_today', type: 'tv' as const }, to: `/tv?cat=airing&label=${encodeURIComponent($t('Airing today'))}` },
    {
      title: $t('Action & Adventure'),
      request: { path: '/discover/movie', type: 'movie' as const, params: { with_genres: '28,12', sort_by: 'popularity.desc' } },
      to: `/movies?genre=28&label=${encodeURIComponent($t('Action & Adventure'))}`,
    },
    {
      title: $t('Comedy'),
      request: { path: '/discover/movie', type: 'movie' as const, params: { with_genres: '35', sort_by: 'popularity.desc' } },
      to: `/movies?genre=35&label=${encodeURIComponent($t('Comedy'))}`,
    },
    {
      title: $t('Sci-Fi & Fantasy'),
      request: { path: '/discover/movie', type: 'movie' as const, params: { with_genres: '878,14', sort_by: 'popularity.desc' } },
      to: `/movies?genre=878&label=${encodeURIComponent($t('Sci-Fi & Fantasy'))}`,
    },
  ]
})

// ── Recommendations row based on the first favourite or recently watched title ──
const recommendationSource = computed(() => library.favouriteList[0] ?? library.history[0])
const { data: recommendations } = useAsyncData(
  () => recommendationSource.value ? `recs-${recommendationSource.value.id}` : '',
  async () => {
    const m = recommendationSource.value
    if (!m)
      return []
    const data = await tmdb<TmdbPage>(`/${m.type}/${m.id}/recommendations`)
    return data.results.flatMap(item => toMedia(item) ?? []).slice(0, 20)
  },
  { lazy: true },
)

// ── Scroll position for back-to-top button ───────────────────────────────────
const scroller = ref<HTMLElement | null>(null)
const showBackToTop = ref(false)

onMounted(() => {
  const el = scroller.value
  if (!el)
    return
  el.addEventListener('scroll', () => {
    showBackToTop.value = el.scrollTop > 600
  }, { passive: true })
})

function scrollToTop() {
  scroller.value?.scrollTo({ top: 0, behavior: 'smooth' })
}

const rowHeight = computed(() => Math.round(ui.cardWidth * 1.5) + 92)
</script>

<template>
  <div ref="scroller" class="h-full overflow-y-auto pb-10">
    <!-- Cover Banner Hero Section -->
    <section
      class="group/hero relative mx-4 mt-2 h-[62vh] min-h-[480px] overflow-hidden rounded-2xl md:mx-6 md:h-[68vh]"
      @mouseenter="isHovered = true"
      @mouseleave="isHovered = false"
    >
      <!-- Backdrop Image with Smooth Fade & Scale Zoom. Only transform/opacity
           animate — the crossfade runs on a full-window element and
           `transition-all` there repaints the whole hero for 700ms. -->
      <transition
        enter-active-class="transition-[opacity,transform] duration-700 ease-out"
        leave-active-class="transition-[opacity,transform] duration-700 ease-in"
        enter-from-class="opacity-0 scale-105"
        leave-to-class="opacity-0 scale-95"
      >
        <div v-if="featured" :key="featured.id" class="absolute inset-0 transform transition-transform duration-1000 ease-out hover:scale-105">
          <media-poster :src="backdropUrl(featured.backdrop, 'w1280')" :alt="featured.title" class="[&_img]:object-top" />
        </div>
      </transition>

      <!-- Gradient Overlays for High Legibility -->
      <div class="absolute inset-0 bg-gradient-to-t from-black/95 via-black/50 to-black/20" />
      <div class="absolute inset-0 bg-gradient-to-r from-black/90 via-black/40 to-transparent" />

      <!-- Left / Right Carousel Navigation Arrows -->
      <button
        v-if="spotlight.length > 1"
        type="button"
        class="absolute left-3 top-1/2 z-10 grid size-11 -translate-y-1/2 place-items-center rounded-full bg-black/40 text-white backdrop-blur-md border border-white/10 opacity-0 transition-[opacity,transform,background-color] hover:bg-primary hover:text-on-primary hover:scale-110 group-hover/hero:opacity-100 focus-visible:opacity-100"
        :aria-label="$t('Previous')"
        @click="prevSlide"
      >
        <v-icon :icon="mdiChevronLeft" size="28" />
      </button>

      <button
        v-if="spotlight.length > 1"
        type="button"
        class="absolute right-3 top-1/2 z-10 grid size-11 -translate-y-1/2 place-items-center rounded-full bg-black/40 text-white backdrop-blur-md border border-white/10 opacity-0 transition-[opacity,transform,background-color] hover:bg-primary hover:text-on-primary hover:scale-110 group-hover/hero:opacity-100 focus-visible:opacity-100"
        :aria-label="$t('Next')"
        @click="nextSlide"
      >
        <v-icon :icon="mdiChevronRight" size="28" />
      </button>

      <!-- Banner Content Layout (Poster + Details) -->
      <div v-if="featured" class="relative h-full flex flex-col justify-end p-4 text-white md:p-8">
        <div class="flex items-end gap-5 md:gap-7">
          <!-- Featured Vertical Poster Card -->
          <nuxt-link
            :to="mediaLink(featured)"
            class="group/poster relative hidden sm:block w-32 shrink-0 overflow-hidden rounded-xl border border-white/20 bg-surface-container shadow-[0_16px_40px_rgba(0,0,0,0.8)] transition-[transform,border-color] duration-300 md:w-44 lg:w-48 aspect-2/3 hover:scale-105 hover:border-primary hover:shadow-[0_20px_50px_rgba(111,227,255,0.3)] focus-visible:scale-105 focus-visible:ring-2 focus-visible:ring-primary"
          >
            <media-poster :src="posterUrl(featured.poster, 'w342')" :alt="featured.title" />
            <div class="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent opacity-0 transition-opacity group-hover/poster:opacity-100 flex items-end justify-center pb-3">
              <v-icon :icon="mdiPlay" size="36" color="primary" />
            </div>
          </nuxt-link>

          <!-- Text Details -->
          <div class="min-w-0 flex-1 flex flex-col justify-end gap-2.5">
            <div class="motion-reveal flex flex-wrap items-center gap-2" style="--i: 0">
              <v-chip size="small" :prepend-icon="mdiStar" class="font-medium bg-primary/20 text-primary border border-primary/30">
                {{ featured.rating.toFixed(1) }}
              </v-chip>
              <span class="text-label-medium uppercase tracking-wider opacity-85">
                {{ featured.type === 'movie' ? $t('Movie') : $t('TV Show') }} · {{ featured.year || $t('unknown') }}
              </span>
              <template v-if="featuredDetail?.runtime">
                <span class="text-label-medium opacity-60">·</span>
                <span class="text-label-medium uppercase tracking-wider opacity-85">{{ runtimeText(featuredDetail.runtime) }}</span>
              </template>
              <template v-if="featuredDetail?.certification">
                <span class="text-label-medium opacity-60">·</span>
                <span class="rounded border border-white/30 px-1 text-label-small font-semibold opacity-85">{{ featuredDetail.certification }}</span>
              </template>
            </div>

            <div class="motion-clip-box -my-1 py-1 drop-shadow-[0_2px_24px_rgba(0,0,0,0.8)]">
              <!-- Logo treatment if available, otherwise text title -->
              <img
                v-if="featuredDetail?.logo"
                :src="logoUrl(featuredDetail.logo, 'w500') ?? ''"
                :alt="featured.title"
                class="h-10 md:h-14 lg:h-16 w-auto object-contain"
              >
              <h1 v-else class="motion-clip max-w-3xl text-headline-medium font-bold md:text-display-small" style="--i: 1">
                {{ featured.title }}
              </h1>
            </div>

            <div class="motion-trace h-0.5 w-20 bg-primary shadow-[0_0_12px_rgba(111,227,255,0.8)]" style="--i: 2" />

            <!-- Genre tags -->
            <div v-if="featuredDetail?.genres?.length" class="motion-reveal flex flex-wrap gap-1.5" style="--i: 3">
              <span
                v-for="g in featuredDetail.genres.slice(0, 3)"
                :key="g.name"
                class="rounded-full bg-white/10 px-2.5 py-0.5 text-label-small text-white/80"
              >
                {{ g.name }}
              </span>
            </div>

            <p class="motion-reveal line-clamp-2 max-w-2xl text-body-medium text-white/90" style="--i: 4">
              {{ featured.overview }}
            </p>

            <!-- Action Buttons + Spotlight Poster Thumbnails -->
            <div class="motion-reveal flex flex-wrap items-end gap-x-3 gap-y-3 pt-2" style="--i: 5">
              <v-btn :prepend-icon="mdiPlay" size="large" color="primary" class="font-semibold px-6 shadow-lg shadow-primary/25" :to="library.resumeLink(featured)">
                {{ $t('Play') }}
              </v-btn>
              <v-btn :prepend-icon="mdiInformationOutline" size="large" variant="tonal" class="bg-white/10 hover:bg-white/20" :to="mediaLink(featured)">
                {{ $t('Details') }}
              </v-btn>
              <v-btn v-if="trailerKey" size="large" variant="tonal" class="bg-white/10 hover:bg-white/20" @click="trailerDialog = true">
                <v-icon :icon="mdiPlay" size="18" class="mr-1" />
                {{ $t('Watch Trailer') }}
              </v-btn>
              <v-btn icon variant="text" color="white" size="large" @click="library.toggleWatchlist(featured)">
                <v-icon :icon="library.inWatchlist(featured) ? mdiBookmark : mdiBookmarkOutline" :color="library.inWatchlist(featured) ? 'primary' : undefined" />
                <v-tooltip activator="parent" :text="library.inWatchlist(featured) ? $t('Remove from watchlist') : $t('Add to watchlist')" />
              </v-btn>
              <v-btn icon variant="text" color="white" size="large" @click="library.toggleFavourite(featured)">
                <v-icon :icon="library.isFavourite(featured) ? mdiHeart : mdiHeartOutline" :color="library.isFavourite(featured) ? 'primary' : undefined" />
                <v-tooltip activator="parent" :text="library.isFavourite(featured) ? $t('Remove from favourites') : $t('Favourite')" />
              </v-btn>

              <v-spacer />

              <!-- Animated Spotlight Thumbnails Strip (10 Banner Slides) -->
              <div class="flex max-w-[360px] sm:max-w-[480px] md:max-w-[560px] items-center gap-2 overflow-x-auto py-1 scrollbar-none">
                <button
                  v-for="(media, index) in spotlight"
                  :key="media.id"
                  type="button"
                  class="group/thumb relative h-16 w-11 shrink-0 overflow-hidden rounded-xl outline-none transition-[transform,box-shadow] duration-300 md:h-20 md:w-14 hover:scale-110 hover:ring-2 hover:ring-white focus-visible:ring-2 focus-visible:ring-primary"
                  :class="index === at ? 'ring-2 ring-primary shadow-[0_0_20px_rgba(111,227,255,0.5)] opacity-100 scale-105' : 'opacity-60 hover:opacity-100'"
                  :aria-label="media.title"
                  :aria-current="index === at"
                  @click="at = index"
                  @focus="at = index"
                >
                  <media-poster :src="posterUrl(media.poster, 'w185')" :alt="media.title" />
                  <v-tooltip activator="parent" location="top" :text="media.title" />

                  <!-- Active item timer progress bar. scaleX, not width: the
                       bar ticks every 75ms for the whole autoplay window, and
                       a width tween is layout per tick. The glow stays — it
                       paints once, the scale doesn't touch it. -->
                  <div v-if="index === at" class="absolute inset-x-0 bottom-0 h-1.5 bg-black/70">
                    <div
                      class="h-full w-full origin-left bg-primary transition-transform duration-75 ease-linear shadow-[0_0_8px_rgba(111,227,255,0.8)]"
                      :style="{ transform: `scaleX(${progress / 100})` }"
                    />
                  </div>
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div v-else class="relative h-full flex flex-col justify-end gap-3 p-4 md:p-8">
        <div class="animate-pulse h-10 max-w-md w-2/3 rounded-lg bg-surface-container/60" />
        <div class="animate-pulse h-12 max-w-2xl w-full rounded-lg bg-surface-container/60" />
      </div>
    </section>

    <!-- Trailer dialog -->
    <v-dialog v-model="trailerDialog" max-width="900" :scrim-opacity="0.85">
      <v-card v-if="trailerKey" rounded="xl" class="overflow-hidden">
        <div class="relative aspect-video">
          <iframe
            :src="`https://www.youtube.com/embed/${trailerKey}?autoplay=1&rel=0`"
            class="absolute inset-0 h-full w-full"
            allow="autoplay; encrypted-media"
            allowfullscreen
          />
        </div>
        <v-btn icon variant="text" class="absolute top-2 right-2" @click="trailerDialog = false">
          <v-icon :icon="mdiClose" />
        </v-btn>
      </v-card>
    </v-dialog>

    <!-- Streaming providers: straight under the hero, since "what service is
         this on" is the first question after what's new. -->
    <provider-strip />

    <!-- Media Rows -->
    <div class="flex flex-col gap-7 pt-7">
      <!-- Continue Watching (local data) -->
      <scroll-row v-if="library.resumeRow.length" class="motion-reveal" :title="$t('Continue watching')">
        <media-card
          v-for="entry in library.resumeRow"
          :key="entry.key"
          :media="entry.media"
          :detail="ui.isDetailed"
          :resume-to="watchLink(entry.media.type, entry.media.id, entry.season, entry.episode)"
          :on-remove="() => library.removeFromContinueWatching(entry.key)"
          class="shrink-0"
          :style="{ width: `${ui.cardWidth}px` }"
        />
      </scroll-row>

      <!-- Your Favourites (local data) -->
      <scroll-row v-if="library.favouriteList.length" class="motion-reveal" :title="$t('Your favourites')">
        <media-card
          v-for="m in library.favouriteList.slice(0, 20)"
          :key="`fav-${m.type}-${m.id}`"
          :media="m"
          :detail="ui.isDetailed"
          class="shrink-0"
          :style="{ width: `${ui.cardWidth}px` }"
        />
      </scroll-row>

      <!-- Your Watchlist (local data) -->
      <scroll-row v-if="library.watchlistItems.length" class="motion-reveal" :title="$t('Your watchlist')">
        <media-card
          v-for="m in library.watchlistItems.slice(0, 20)"
          :key="`wl-${m.type}-${m.id}`"
          :media="m"
          :detail="ui.isDetailed"
          class="shrink-0"
          :style="{ width: `${ui.cardWidth}px` }"
        />
      </scroll-row>

      <!-- Because you watched… (recommendations) -->
      <v-lazy v-if="recommendations?.length" :min-height="rowHeight" transition="fade-transition">
        <scroll-row class="motion-reveal" :title="$t('Because you watched {title}', { title: recommendationSource?.title ?? '' })">
          <media-card
            v-for="m in recommendations.slice(0, 20)"
            :key="`rec-${m.type}-${m.id}`"
            :media="m"
            :detail="ui.isDetailed"
            class="shrink-0"
            :style="{ width: `${ui.cardWidth}px` }"
          />
        </scroll-row>
      </v-lazy>

      <!-- TMDB data rows (trending, popular, top rated, genres, etc.) -->
      <v-lazy
        v-for="row in rows"
        :key="row.title"
        :min-height="rowHeight"
        transition="fade-transition"
      >
        <media-slider class="motion-reveal" :title="row.title" :request="row.request" :to="row.to" />
      </v-lazy>

      <!-- Live TV teaser -->
      <div class="motion-reveal mx-4 md:mx-6">
        <nuxt-link
          :to="localePath('/live-tv')"
          class="group flex items-center gap-4 rounded-2xl border border-white/10 bg-gradient-to-r from-primary/15 via-surface-container-high to-surface-container-high p-4 transition-[border-color,box-shadow] duration-300 hover:border-primary/50 hover:shadow-lg hover:shadow-primary/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          <div class="grid size-12 shrink-0 place-items-center rounded-xl bg-primary/20 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary">
            <v-icon :icon="mdiTelevision" size="24" />
          </div>
          <div class="min-w-0 flex-1">
            <h3 class="text-title-medium font-bold">
              {{ $t('Live TV') }}
            </h3>
            <p class="text-body-small opacity-60">
              {{ $t('Browse live channels and guides') }}
            </p>
          </div>
          <v-icon :icon="mdiArrowRight" size="20" class="shrink-0 opacity-40 transition-[transform,opacity] group-hover:translate-x-1 group-hover:opacity-100" />
        </nuxt-link>
      </div>
    </div>

    <!-- Back to top button -->
    <transition
      enter-active-class="transition-[opacity,transform] duration-200"
      leave-active-class="transition-[opacity,transform] duration-150"
      enter-from-class="opacity-0 translate-y-4"
      leave-to-class="opacity-0 translate-y-4"
    >
      <button
        v-if="showBackToTop"
        type="button"
        class="fixed bottom-6 right-6 z-50 grid size-12 place-items-center rounded-full bg-surface-container-high text-primary shadow-lg shadow-black/30 backdrop-blur-md border border-white/10 transition-[transform,background-color,color] hover:bg-primary hover:text-on-primary hover:scale-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :aria-label="$t('Back to top')"
        @click="scrollToTop"
      >
        <v-icon :icon="mdiArrowUp" size="22" />
      </button>
    </transition>
  </div>
</template>
