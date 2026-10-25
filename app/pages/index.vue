<script setup lang="ts">
import type { TmdbPage } from '~/utils/tmdb'
import { mdiArrowRight, mdiArrowUp, mdiBookmark, mdiBookmarkOutline, mdiChevronLeft, mdiChevronRight, mdiClose, mdiHeart, mdiHeartOutline, mdiInformationOutline, mdiMovieOpenPlayOutline, mdiPlay, mdiStar, mdiTelevision } from '@mdi/js'

definePageMeta({ keepalive: true })

const ui = useUiStore()
const library = useLibraryStore()

const { data: trending } = useAsyncData(
  'home-trending',
  () => tmdb<TmdbPage>('/trending/all/day'),
  { lazy: true, transform: page => (page?.results ?? []).flatMap(item => toMedia(item) ?? []) },
)

/**
 * The top spotlight items.
 */
const spotlight = computed(() => (trending.value ?? []).filter(m => m.backdrop).slice(0, 10))

const at = ref(0)
const featured = computed(() =>
  spotlight.value.length
    ? spotlight.value[Math.min(at.value, spotlight.value.length - 1)]
    : undefined)

/** The next slide, shown as a picture beside the progress. */
const upNext = computed(() => spotlight.value.length > 1 ? spotlight.value[(at.value + 1) % spotlight.value.length] : undefined)

// Update ambient background when featured title changes
watch(featured, media => media && ui.ambient(media), { immediate: true })

// Fetch detail for the featured item (for logo, trailer, runtime, certification)
const featuredDetail = ref<{ logo: string | null, trailer: string | null, runtime: number, certification: string, genres: { name: string }[], overview?: string } | null>(null)
watch(featured, async m => {
  if (!m) {
    featuredDetail.value = null
    return
  }
  // The shared detail request is lean (no images, no videos), so its logo and
  // trailer are always empty: those two come from their own cached lookups,
  // all three at once. A slide that moved on meanwhile drops the answer.
  try {
    const [data, logo, trailers] = await Promise.all([
      loadMediaDetail(m.type, m.id),
      loadTitleLogo(m.type, m.id, m.lang),
      loadTrailers(m.type, m.id),
    ])
    if (featured.value?.id !== m.id)
      return
    featuredDetail.value = {
      logo: logo ?? data.logo,
      trailer: trailers[0] ?? data.trailer,
      runtime: data.runtime,
      certification: data.certification,
      genres: data.genres,
      overview: data.overview,
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
    onCleanup(() => {
      img.src = ''
    })
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
let timer: ReturnType<typeof setInterval> | null = null
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
  timer = setInterval(() => {
    if (!isHovered.value && spotlight.value.length > 1)
      nextSlide()
  }, DURATION)
}

function stopTimer() {
  if (timer)
    clearInterval(timer)
  timer = null
}

watch(at, () => startTimer())

onMounted(() => {
  startTimer()
})

onActivated(() => {
  startTimer()
})

onDeactivated(() => {
  stopTimer()
})

onUnmounted(() => {
  stopTimer()
})

// ── TMDB content rows ─────────────────────────────────────────────────────────
const rows = computed(() => {
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
      title: $t('Horror'),
      request: { path: '/discover/movie', type: 'movie' as const, params: { with_genres: '27', sort_by: 'popularity.desc' } },
      to: `/movies?genre=27&label=${encodeURIComponent($t('Horror'))}`,
    },
    {
      title: $t('Sci-Fi & Fantasy'),
      request: { path: '/discover/movie', type: 'movie' as const, params: { with_genres: '878,14', sort_by: 'popularity.desc' } },
      to: `/movies?genre=878&label=${encodeURIComponent($t('Sci-Fi & Fantasy'))}`,
    },
  ]
})

const upcoming = computed(() => {
  const day = (offset = 0) => new Date(Date.now() + offset * 864e5).toISOString().slice(0, 10)
  return {
    title: $t('Upcoming movies'),
    request: {
      path: '/discover/movie',
      type: 'movie' as const,
      params: { 'include_adult': false, 'sort_by': 'popularity.desc', 'primary_release_date.gte': day(), 'primary_release_date.lte': day(180) },
    },
    to: `/movies?cat=upcoming&label=${encodeURIComponent($t('Upcoming movies'))}`,
  }
})

// ── Recommendations row based on the first favourite or recently watched title ──
const recommendationSource = computed(() => library.favouriteList[0] ?? library.history[0])
const { data: recommendations } = useAsyncData(
  () => recommendationSource.value ? `recs-${recommendationSource.value.id}` : 'home-recs',
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
  <div
    ref="scroller"
    class="h-full overflow-y-auto pb-10"
  >
    <!-- Spotlight. Full-bleed rather than a card: the picture is the point, so
         it runs to the edges and fades into the app's own background through a
         mask instead of being boxed in black. A mask is painted once — on a TV
         the cost is the crossfade, and that is opacity only. -->
    <section
      v-if="featured"
      class="group/hero relative flex min-h-[max(520px,74vh)] flex-col overflow-hidden md:min-h-[max(560px,80vh)]"
      @mouseenter="isHovered = true"
      @mouseleave="isHovered = false"
      @focusin="isHovered = true"
      @focusout="isHovered = false"
    >
      <div class="hero-mask absolute inset-0">
        <transition
          enter-active-class="transition-opacity duration-700 ease-out"
          leave-active-class="transition-opacity duration-700 ease-in"
          enter-from-class="opacity-0"
          leave-to-class="opacity-0"
        >
          <div :key="featured.id" class="absolute inset-0">
            <media-poster
              :src="backdropUrl(featured.backdrop, 'original')"
              :alt="featured.title"
              class="[&_img]:object-[70%_25%]"
            />
          </div>
        </transition>
        <!-- Legibility where the words sit, and nowhere else: the right of
             the picture stays as bright as it was shot. -->
        <div class="absolute inset-0 bg-gradient-to-r from-black/85 via-black/45 to-transparent md:via-black/25" />
        <div class="absolute inset-x-0 bottom-0 h-2/3 bg-gradient-to-t from-black/70 to-transparent" />
      </div>

      <button
        v-if="spotlight.length > 1"
        type="button"
        class="absolute left-3 top-1/2 z-10 grid size-12 -translate-y-1/2 place-items-center rounded-full bg-black/40 text-white opacity-0 transition-[opacity,background-color] hover:bg-white/25 group-hover/hero:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-primary"
        :aria-label="$t('Previous')"
        @click="prevSlide"
      >
        <v-icon :icon="mdiChevronLeft" size="30" />
      </button>
      <button
        v-if="spotlight.length > 1"
        type="button"
        class="absolute right-3 top-1/2 z-10 grid size-12 -translate-y-1/2 place-items-center rounded-full bg-black/40 text-white opacity-0 transition-[opacity,background-color] hover:bg-white/25 group-hover/hero:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-primary"
        :aria-label="$t('Next')"
        @click="nextSlide"
      >
        <v-icon :icon="mdiChevronRight" size="30" />
      </button>

      <!-- Grows with what it holds rather than clipping it: a fixed height
           cut the logo off the top on an 819px-tall window. -->
      <div class="relative flex flex-1 flex-col justify-end gap-6 px-6 pb-8 pt-20 text-white md:px-14 md:pb-10">
        <!-- Keyed on the title, so every line plays its reveal again. -->
        <div :key="featured.id" class="flex max-w-2xl flex-col gap-4">
          <div class="motion-reveal flex items-center gap-2" style="--i: 0">
            <span class="grid h-7 min-w-7 place-items-center rounded-md bg-primary px-1.5 text-label-large font-black text-on-primary">
              {{ at + 1 }}
            </span>
            <span class="text-label-large font-semibold uppercase tracking-[0.18em] text-white/90">
              {{ $t('Trending today') }} · {{ featured.type === 'movie' ? $t('Movie') : $t('TV Show') }}
            </span>
          </div>

          <div class="motion-reveal drop-shadow-[0_4px_30px_rgba(0,0,0,0.7)]" style="--i: 1">
            <img
              v-if="featuredDetail?.logo"
              :src="logoUrl(featuredDetail.logo, 'w500') ?? ''"
              :alt="featured.title"
              class="max-h-28 w-auto max-w-[min(100%,460px)] object-contain object-left md:max-h-36"
            >
            <h1 v-else class="text-display-small font-black leading-none tracking-tight md:text-display-large">
              {{ featured.title }}
            </h1>
          </div>

          <div class="motion-reveal flex flex-wrap items-center gap-x-3 gap-y-1 text-body-large text-white/90" style="--i: 2">
            <span class="flex items-center gap-1 font-semibold text-amber-300">
              <v-icon :icon="mdiStar" size="18" />{{ featured.rating.toFixed(1) }}
            </span>
            <span v-if="featured.year">{{ featured.year }}</span>
            <span v-if="featuredDetail?.runtime">{{ runtimeText(featuredDetail.runtime) }}</span>
            <span v-if="featuredDetail?.certification" class="rounded border border-white/50 px-1.5 text-label-medium font-semibold">
              {{ featuredDetail.certification }}
            </span>
            <span v-if="featuredDetail?.genres?.length" class="text-white/75">
              {{ featuredDetail.genres.slice(0, 3).map(g => g.name).join(' · ') }}
            </span>
          </div>

          <p class="motion-reveal line-clamp-3 text-body-large text-white/85" style="--i: 3">
            {{ featuredDetail?.overview || featured.overview }}
          </p>

          <div class="motion-reveal flex flex-wrap items-center gap-3 pt-1" style="--i: 4">
            <v-btn :prepend-icon="mdiPlay" size="x-large" color="white" variant="flat" rounded="lg" class="px-8 font-bold text-black" :to="library.resumeLink(featured)">
              {{ $t('Play') }}
            </v-btn>
            <v-btn :prepend-icon="mdiInformationOutline" size="x-large" variant="flat" rounded="lg" class="bg-white/20! px-6 font-semibold text-white" :to="mediaLink(featured)" @pointerdown="armDetailPress($event, featured)" @click="openDetail($event, featured)">
              {{ $t('More info') }}
            </v-btn>
            <v-btn v-if="trailerKey" :prepend-icon="mdiMovieOpenPlayOutline" size="x-large" variant="text" rounded="lg" class="text-white" @click="trailerDialog = true">
              {{ $t('Trailer') }}
            </v-btn>
            <v-btn icon variant="outlined" size="large" class="border-white/40! text-white" :aria-label="library.inWatchlist(featured) ? $t('Remove from watchlist') : $t('Add to watchlist')" @click="library.toggleWatchlist(featured)">
              <v-icon :icon="library.inWatchlist(featured) ? mdiBookmark : mdiBookmarkOutline" :color="library.inWatchlist(featured) ? 'primary' : undefined" />
              <v-tooltip activator="parent" :text="library.inWatchlist(featured) ? $t('Remove from watchlist') : $t('Add to watchlist')" />
            </v-btn>
            <v-btn icon variant="outlined" size="large" class="border-white/40! text-white" :aria-label="library.isFavourite(featured) ? $t('Remove from favourites') : $t('Favourite')" @click="library.toggleFavourite(featured)">
              <v-icon :icon="library.isFavourite(featured) ? mdiHeart : mdiHeartOutline" :color="library.isFavourite(featured) ? 'primary' : undefined" />
              <v-tooltip activator="parent" :text="library.isFavourite(featured) ? $t('Remove from favourites') : $t('Favourite')" />
            </v-btn>
          </div>
        </div>

        <div class="flex items-end justify-between gap-6">
          <!-- One segment per title, the current one filling over its turn.
               Each is a real button, so the remote can step through them. -->
          <div v-if="spotlight.length > 1" class="flex max-w-md flex-1 items-center gap-1.5">
            <button
              v-for="(media, index) in spotlight"
              :key="media.id"
              type="button"
              class="group/seg flex-1 py-2 outline-none"
              :aria-label="media.title"
              :aria-current="index === at"
              @click="at = index"
            >
              <span
                class="block h-1 overflow-hidden rounded-full transition-[background-color] group-hover/seg:bg-white/70 group-focus-visible/seg:bg-white group-focus-visible/seg:ring-2 group-focus-visible/seg:ring-primary"
                :class="index < at ? 'bg-white/85' : 'bg-white/30'"
              >
                <span
                  v-if="index === at"
                  :key="at"
                  class="block h-full w-full origin-left bg-white"
                  :style="{ animation: `trace-x ${DURATION}ms linear both`, animationPlayState: isHovered ? 'paused' : 'running' }"
                />
              </span>
            </button>
          </div>

          <!-- What comes next, as a picture rather than a dot. -->
          <button
            v-if="upNext"
            type="button"
            class="group/next hidden w-64 shrink-0 overflow-hidden rounded-xl bg-black/50 text-start outline-none ring-1 ring-white/15 transition-[transform,box-shadow] hover:scale-[1.03] hover:ring-white/50 focus-visible:scale-[1.03] focus-visible:ring-2 focus-visible:ring-primary lg:block"
            @click="nextSlide"
          >
            <div class="relative aspect-video">
              <media-poster :src="backdropUrl(upNext.backdrop, 'w780')" :alt="upNext.title" />
              <div class="absolute inset-0 bg-gradient-to-t from-black/90 via-black/20 to-transparent" />
              <div class="absolute inset-x-3 bottom-2">
                <p class="text-label-small font-semibold uppercase tracking-widest text-white/70">
                  {{ $t('Up next') }}
                </p>
                <p class="truncate text-title-small font-bold text-white">
                  {{ upNext.title }}
                </p>
              </div>
            </div>
          </button>
        </div>
      </div>
    </section>

    <!-- Trailer dialog -->
    <v-dialog v-model="trailerDialog" max-width="900" :scrim-opacity="0.85">
      <v-card v-if="trailerKey" rounded="xl" class="overflow-hidden">
        <div class="relative aspect-video">
          <iframe
            :src="youtubeEmbedSrc(trailerKey)"
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

      <!-- Today's chart, from the trending list the hero already has. -->
      <top10-row class="motion-reveal" :items="trending ?? []" />

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

      <!-- A choice of mood rather than a list of titles. -->
      <genre-tiles class="motion-reveal" />

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

      <v-lazy :min-height="rowHeight" transition="fade-transition">
        <media-slider class="motion-reveal" :title="upcoming.title" :request="upcoming.request" :to="upcoming.to" />
      </v-lazy>
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
        class="fixed bottom-6 right-6 z-50 grid size-12 place-items-center rounded-full border border-white/10 bg-surface-container-high text-primary shadow-lg shadow-black/30 transition-[transform,background-color,color] hover:scale-110 hover:bg-primary hover:text-on-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :aria-label="$t('Back to top')"
        @click="scrollToTop"
      >
        <v-icon :icon="mdiArrowUp" size="22" />
      </button>
    </transition>
  </div>
</template>
