<script setup lang="ts">
import type { TmdbPage } from '~/utils/tmdb'
import { mdiBookmark, mdiBookmarkOutline, mdiChevronLeft, mdiChevronRight, mdiHeart, mdiHeartOutline, mdiInformationOutline, mdiPlay, mdiStar } from '@mdi/js'

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
const spotlight = computed(() => (trending.value ?? []).filter(m => m.backdrop).slice(0, 6))

const at = ref(0)
const featured = computed(() => spotlight.value[Math.min(at.value, spotlight.value.length - 1)])

// Update ambient background when featured title changes
watch(featured, media => media && ui.ambient(media), { immediate: true })

// Auto-advance spotlight slider every 6 seconds, pausing on hover or focus
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
      // Pause progress on hover
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

const rows = computed(() => {
  const day = (offset = 0) => new Date(Date.now() + offset * 864e5).toISOString().slice(0, 10)
  return [
    { title: $t('Trending this week'), request: { path: '/trending/all/week' }, to: '/movies?cat=trending' },
    { title: $t('Popular movies'), request: { path: '/movie/popular', type: 'movie' as const }, to: '/movies' },
    { title: $t('Popular TV'), request: { path: '/tv/popular', type: 'tv' as const }, to: '/tv' },
    {
      // Most-anticipated rather than soonest-alphabetical: popularity sorts the
      // dated window so the big releases lead the row.
      title: $t('Upcoming movies'),
      request: {
        path: '/discover/movie',
        type: 'movie' as const,
        params: { 'include_adult': false, 'sort_by': 'popularity.desc', 'primary_release_date.gte': day(), 'primary_release_date.lte': day(180) },
      },
      to: '/movies?cat=upcoming',
    },
    {
      title: $t('Popular Bollywood'),
      request: {
        path: '/discover/movie',
        type: 'movie' as const,
        params: { include_adult: false, with_original_language: 'hi', sort_by: 'popularity.desc' },
      },
      to: '/movies?lang=hi',
    },
    {
      title: $t('Popular Bollywood series'),
      request: {
        path: '/discover/tv',
        type: 'tv' as const,
        params: { include_adult: false, with_original_language: 'hi', sort_by: 'popularity.desc' },
      },
      to: '/tv?lang=hi',
    },
  ]
})

const rowHeight = computed(() => Math.round(ui.cardWidth * 1.5) + 92)
</script>

<template>
  <div class="h-full overflow-y-auto pb-10">
    <!-- Cover Banner Hero Section -->
    <section
      class="group/hero relative mx-4 mt-2 h-[48vh] min-h-[380px] overflow-hidden rounded-2xl md:mx-6 md:h-[54vh]"
      @mouseenter="isHovered = true"
      @mouseleave="isHovered = false"
    >
      <!-- Backdrop Image with Smooth Fade & Scale Zoom -->
      <transition
        enter-active-class="transition-all duration-700 ease-out"
        leave-active-class="transition-all duration-700 ease-in"
        enter-from-class="opacity-0 scale-105"
        leave-to-class="opacity-0 scale-95"
      >
        <div v-if="featured" :key="featured.id" class="absolute inset-0 transform transition-transform duration-1000 ease-out hover:scale-105">
          <media-poster :src="backdropUrl(featured.backdrop, 'w1280')" :alt="featured.title" />
        </div>
      </transition>

      <!-- Gradient Overlays for High Legibility -->
      <div class="absolute inset-0 bg-gradient-to-t from-black/95 via-black/50 to-black/20" />
      <div class="absolute inset-0 bg-gradient-to-r from-black/90 via-black/40 to-transparent" />

      <!-- Left / Right Carousel Navigation Arrows -->
      <button
        v-if="spotlight.length > 1"
        type="button"
        class="absolute left-3 top-1/2 z-10 grid size-11 -translate-y-1/2 place-items-center rounded-full bg-black/40 text-white backdrop-blur-md border border-white/10 opacity-0 transition-all hover:bg-primary hover:text-on-primary hover:scale-110 group-hover/hero:opacity-100 focus-visible:opacity-100"
        :aria-label="$t('Previous')"
        @click="prevSlide"
      >
        <v-icon :icon="mdiChevronLeft" size="28" />
      </button>

      <button
        v-if="spotlight.length > 1"
        type="button"
        class="absolute right-3 top-1/2 z-10 grid size-11 -translate-y-1/2 place-items-center rounded-full bg-black/40 text-white backdrop-blur-md border border-white/10 opacity-0 transition-all hover:bg-primary hover:text-on-primary hover:scale-110 group-hover/hero:opacity-100 focus-visible:opacity-100"
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
            class="group/poster relative hidden sm:block w-32 shrink-0 overflow-hidden rounded-xl border border-white/20 bg-surface-container shadow-[0_16px_40px_rgba(0,0,0,0.8)] transition-all duration-300 md:w-44 lg:w-48 aspect-2/3 hover:scale-105 hover:border-primary hover:shadow-[0_20px_50px_rgba(111,227,255,0.3)] focus-visible:scale-105 focus-visible:ring-2 focus-visible:ring-primary"
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
            </div>

            <div class="motion-clip-box -my-1 py-1 drop-shadow-[0_2px_24px_rgba(0,0,0,0.8)]">
              <h1 class="motion-clip max-w-3xl text-headline-medium font-bold md:text-display-small" style="--i: 1">
                {{ featured.title }}
              </h1>
            </div>

            <div class="motion-trace h-0.5 w-20 bg-primary shadow-[0_0_12px_rgba(111,227,255,0.8)]" style="--i: 2" />

            <p class="motion-reveal line-clamp-2 max-w-2xl text-body-medium text-white/90" style="--i: 3">
              {{ featured.overview }}
            </p>

            <!-- Action Buttons + Spotlight Poster Thumbnails -->
            <div class="motion-reveal flex flex-wrap items-end gap-x-3 gap-y-3 pt-2" style="--i: 4">
              <v-btn :prepend-icon="mdiPlay" size="large" color="primary" class="font-semibold px-6 shadow-lg shadow-primary/25" :to="library.resumeLink(featured)">
                {{ $t('Play') }}
              </v-btn>
              <v-btn :prepend-icon="mdiInformationOutline" size="large" variant="tonal" class="bg-white/10 hover:bg-white/20" :to="mediaLink(featured)">
                {{ $t('Details') }}
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

              <!-- Animated Spotlight Thumbnails with Progress Bar -->
              <div class="flex gap-2">
                <button
                  v-for="(media, index) in spotlight"
                  :key="media.id"
                  type="button"
                  class="group/thumb relative h-16 w-11 shrink-0 overflow-hidden rounded-lg outline-none transition-all duration-300 md:h-20 md:w-14 hover:scale-110 hover:ring-2 hover:ring-white focus-visible:ring-2 focus-visible:ring-primary"
                  :class="index === at ? 'ring-2 ring-primary shadow-[0_0_20px_rgba(111,227,255,0.4)] opacity-100 scale-105' : 'opacity-60 hover:opacity-100'"
                  :aria-label="media.title"
                  :aria-current="index === at"
                  @click="at = index"
                  @focus="at = index"
                >
                  <media-poster :src="posterUrl(media.poster, 'w185')" :alt="media.title" />

                  <!-- Active item timer progress bar -->
                  <div v-if="index === at" class="absolute inset-x-0 bottom-0 h-1 bg-black/60">
                    <div class="h-full bg-primary transition-all duration-75" :style="{ width: `${progress}%` }" />
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

    <!-- Streaming providers: straight under the hero, since "what service is
         this on" is the first question after what's new. -->
    <provider-strip />

    <!-- Media Rows -->
    <div class="flex flex-col gap-7 pt-7">
      <scroll-row v-if="library.resumeRow.length" class="motion-reveal" :title="$t('Continue watching')">
        <media-card
          v-for="entry in library.resumeRow"
          :key="entry.key"
          :media="entry.media"
          :to="watchLink(entry.media.type, entry.media.id, entry.season, entry.episode)"
          :detail="ui.isDetailed"
          class="shrink-0"
          :style="{ width: `${ui.cardWidth}px` }"
        />
      </scroll-row>

      <v-lazy
        v-for="row in rows"
        :key="row.title"
        :min-height="rowHeight"
        transition="fade-transition"
      >
        <media-slider class="motion-reveal" :title="row.title" :request="row.request" />
      </v-lazy>
    </div>
  </div>
</template>
