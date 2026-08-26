<script setup lang="ts">
import type { MediaType } from '~/utils/tmdb'
import { mdiAlertCircleOutline, mdiBookmark, mdiBookmarkOutline, mdiClose, mdiEye, mdiEyeOutline, mdiHeart, mdiHeartOutline, mdiOpenInNew, mdiPlay, mdiStar, mdiVolumeHigh, mdiVolumeOff, mdiYoutube } from '@mdi/js'

// Keeps /foo/123 out of here; anything else 404s instead of asking TMDB.
definePageMeta({
  validate: ({ params }) => 'type' in params && (params.type === 'movie' || params.type === 'tv'),
})

const route = useRoute()
const ui = useUiStore()
const library = useLibraryStore()
const { mobile } = useDisplay()

const type = computed(() => route.params.type as MediaType)
const id = computed(() => String(route.params.id))

const { data: media, status, error } = useMediaDetail(type, id)

// --- Trailer hero -----------------------------------------------------------
// The cover plays the YouTube trailer muted on loop while the page is up,
// Netflix-style. Gated by the reduce-effects switch (TVs default it on, so
// they keep the calm static backdrop), and the poster/backdrop always paints
// first — the iframe only fades in once it has something to show.
const settings = useSettingsStore()
const videoHidden = ref(false)
const trailerReady = ref(false)
const heroVideoOn = ref(false)
const heroMuted = ref(true)
const heroFrame = ref<HTMLIFrameElement | null>(null)
let heroTimer: ReturnType<typeof setTimeout> | undefined

const heroEligible = computed(() =>
  !!media.value?.trailer && !settings.reduceEffects && !videoHidden.value)

const origin = import.meta.client ? window.location.origin : 'https://www.youtube-nocookie.com'

const heroSrc = computed(() => {
  const key = media.value?.trailer
  if (!key)
    return ''
  const params = new URLSearchParams({
    enablejsapi: '1',
    autoplay: '1',
    mute: '1',
    loop: '1',
    playlist: key,
    controls: '0',
    modestbranding: '1',
    rel: '0',
    iv_load_policy: '3',
    playsinline: '1',
    disablekb: '1',
    fs: '0',
    origin,
  })
  return `https://www.youtube-nocookie.com/embed/${key}?${params}`
})

watch(heroEligible, ok => {
  clearTimeout(heroTimer)
  heroVideoOn.value = false
  trailerReady.value = false
  if (ok)
    heroTimer = setTimeout(() => (heroVideoOn.value = true), 800)
}, { immediate: true })

onUnmounted(() => clearTimeout(heroTimer))

/** YouTube's postMessage API is the only way to unmute a muted autoplay embed. */
function toggleHeroSound() {
  heroFrame.value?.contentWindow?.postMessage(
    JSON.stringify({ event: 'command', func: heroMuted.value ? 'unMute' : 'mute', args: [] }),
    '*',
  )
  heroMuted.value = !heroMuted.value
}

// This page's art is the app backdrop, for exactly as long as the page is up.
let mine = 0
watch(media, value => value && (mine = ui.select(value)), { immediate: true })
onUnmounted(() => ui.release(mine))

const meta = computed(() => {
  const m = media.value
  if (!m)
    return []
  return [
    m.year,
    m.type === 'movie'
      ? runtimeText(m.runtime)
      : m.episodeCount
        ? $t('{seasons} seasons · {episodes} episodes', { seasons: m.seasons.length, episodes: m.episodeCount })
        : '',
    m.status,
    m.companies.join(', '),
  ].filter(Boolean)
})

const credits = computed(() => {
  const m = media.value
  if (!m)
    return []
  return [
    { label: m.directors.length > 1 ? $t('Directors') : $t('Director'), value: m.directors.join(', ') },
    { label: m.writers.length > 1 ? $t('Writers') : $t('Writer'), value: m.writers.slice(0, 3).join(', ') },
    { label: $t('Budget'), value: moneyText(m.budget) },
    { label: $t('Revenue'), value: moneyText(m.revenue) },
  ].filter(row => row.value)
})

const trailer = ref(false)

// Escape hatch: YouTube refuses to embed some titles, and the dialog just shows
// its "unavailable" card. Opens in the OS browser under Tauri, a tab under `bun dev`.
async function openTrailer() {
  const url = `https://www.youtube.com/watch?v=${media.value?.trailer}`
  try {
    await useTauriShellOpen(url)
  }
  catch {
    window.open(url, '_blank')
  }
}

// A show has no single thing to play, so the button names (and starts) whatever
// comes next: the episode left part-way through, the one after the last one
// finished, or the very first if the show is new to you (and if it's finished,
// the very first again — there is nowhere else to go).
const firstSeason = computed(() => media.value?.seasons[0]?.number ?? 0)

const target = computed(() => {
  if (type.value !== 'tv' || !media.value)
    return null
  return nextEpisode(media.value.seasons, library.lastEpisode(id.value))
    ?? (firstSeason.value ? { season: firstSeason.value, episode: 1 } : null)
})

/** "S2 E4" — also what tells the template a show has anything playable at all. */
const targetText = computed(() => target.value ? `S${target.value.season} E${target.value.episode}` : '')

/**
 * Who this title is, handed straight to the player so the sources can be asked
 * without waiting on a TMDB round trip first. The player still fetches the
 * detail itself in the background — this only removes it from the critical path.
 */
const handoff = computed(() => ({
  imdb: media.value?.imdbId ?? undefined,
  title: media.value?.title,
  year: media.value?.year,
}))

const playLink = computed(() =>
  target.value
    ? watchLink('tv', id.value, target.value.season, target.value.episode, handoff.value)
    : watchLink('movie', id.value, undefined, undefined, handoff.value),
)

// Part-way through is a resume; anything else is a play, including the next
// episode of a show you're in the middle of.
const started = computed(() => {
  const p = media.value && (target.value
    ? library.episodeProgress(id.value, target.value.season, target.value.episode)
    : library.cardProgress(media.value))
  return p && !p.watched && resumable(p.position, p.duration) ? p : null
})

const playLabel = computed(() => [
  started.value ? $t('Resume') : $t('Play'),
  targetText.value,
  remainingText(started.value) && `· ${remainingText(started.value)}`,
].filter(Boolean).join(' '))
</script>

<template>
  <div class="h-full overflow-y-auto pb-12">
    <div v-if="error" class="flex h-full flex-col items-center justify-center gap-2">
      <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
      <span class="text-body-medium opacity-70">{{ $t('Couldn\'t load this title.') }}</span>
      <v-btn variant="tonal" :to="localePath('/')">
        {{ $t('Go home') }}
      </v-btn>
    </div>

    <template v-else>
      <!-- Trailer-as-cover hero. Backdrop paints immediately; the muted
           trailer fades in over it ~0.8s later (and never at all when
           reduce-effects is on or the title has no trailer). -->
      <section
        v-if="media"
        class="relative mb-6 h-[46vh] min-h-[320px] overflow-hidden rounded-b-3xl md:h-[56vh] md:min-h-[420px]"
      >
        <img
          :src="backdropUrl(media.backdrop, 'w1280') ?? ''"
          :alt="media.title"
          class="absolute inset-0 h-full w-full object-cover"
        >
        <div
          v-if="heroVideoOn"
          class="absolute inset-0 transition-opacity duration-700"
          :class="trailerReady ? 'opacity-100' : 'opacity-0'"
        >
          <iframe
            ref="heroFrame"
            :src="heroSrc"
            class="pointer-events-none absolute left-1/2 top-1/2 h-full w-full -translate-x-1/2 -translate-y-1/2 scale-[1.45]"
            frameborder="0"
            allow="autoplay; encrypted-media"
            tabindex="-1"
            aria-hidden="true"
            @load="trailerReady = true"
          />
        </div>
        <div class="absolute inset-0 bg-gradient-to-t from-black/90 via-black/40 to-transparent" />

        <div class="absolute right-3 top-3 z-10 flex gap-2">
          <button
            v-tooltip:bottom="heroMuted ? $t('Sound on') : $t('Sound off')"
            class="grid size-9 place-items-center rounded-full border border-white/15 bg-black/50 text-white opacity-90 backdrop-blur-md transition-all hover:bg-black/75 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-primary"
            :aria-label="heroMuted ? $t('Sound on') : $t('Sound off')"
            @click="toggleHeroSound"
          >
            <v-icon :icon="heroMuted ? mdiVolumeOff : mdiVolumeHigh" size="18" />
          </button>
          <button
            v-tooltip:bottom="$t('Hide video')"
            class="grid size-9 place-items-center rounded-full border border-white/15 bg-black/50 text-white opacity-90 backdrop-blur-md transition-all hover:bg-black/75 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-primary"
            :aria-label="$t('Hide video')"
            @click="videoHidden = true"
          >
            <v-icon :icon="mdiClose" size="18" />
          </button>
        </div>

        <div class="absolute inset-x-0 bottom-0 p-4 md:p-8">
          <!-- max-w-full: a wordmark is a wide image, and `max-w-md` alone is
               wider than a phone — the title ran off the side of the screen. -->
          <img
            v-if="media.logo"
            :src="logoUrl(media.logo)!"
            :alt="media.title"
            class="max-h-14 max-w-full object-contain drop-shadow-[0_2px_24px_rgba(0,0,0,0.7)] md:max-h-20 md:max-w-lg"
          >
          <h1 v-else class="text-headline-large font-bold text-white drop-shadow-[0_2px_24px_rgba(0,0,0,0.7)]">
            {{ media.title }}
          </h1>
          <p v-if="media.tagline" class="mt-1 max-w-3xl text-body-medium italic text-white/70">
            {{ media.tagline }}
          </p>
        </div>
      </section>

      <section class="px-4 pb-8 pt-4 md:px-6">
        <div class="flex flex-col gap-6 sm:flex-row sm:items-end">
          <div class="aspect-2/3 w-32 shrink-0 overflow-hidden rounded-2xl shadow-2xl sm:w-44 lg:w-52">
            <media-poster :src="posterUrl(media?.poster, 'w500')" :alt="media?.title" />
          </div>

          <div v-if="media" class="flex min-w-0 flex-1 flex-col gap-3">
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-body-small opacity-75">
              <span class="flex items-center gap-1 opacity-100">
                <v-icon :icon="mdiStar" size="14" class="text-amber-400" />
                <span class="font-medium">{{ media.rating.toFixed(1) }}</span>
                <span class="opacity-60">({{ media.votes.toLocaleString(uiLocale()) }})</span>
              </span>
              <span v-if="media.certification" class="rounded border border-outline-variant px-1.5 py-0.5 text-label-small">
                {{ media.certification }}
              </span>
              <span v-for="part in meta" :key="part">{{ part }}</span>
            </div>

            <div class="flex flex-wrap gap-1.5">
              <v-chip v-for="genre in media.genres" :key="genre.id" size="small" :text="genre.name" />
            </div>

            <p class="max-w-3xl text-body-medium opacity-85">
              {{ media.overview || $t('No overview.') }}
            </p>

            <dl v-if="credits.length" class="grid grid-cols-1 gap-x-6 gap-y-1 text-body-small sm:grid-cols-2 lg:max-w-2xl">
              <div v-for="row in credits" :key="row.label" class="flex gap-2">
                <dt class="shrink-0 opacity-50">
                  {{ row.label }}
                </dt>
                <dd class="truncate opacity-85">
                  {{ row.value }}
                </dd>
              </div>
            </dl>

            <!-- Six controls at `large` came to three ragged rows on a phone.
                 Play takes its own full-width row there — it is what the page is
                 for — and the rest fall in behind it at the normal size. -->
            <div class="flex flex-wrap items-center gap-2 pt-2">
              <v-btn
                v-if="type === 'movie' || target"
                :prepend-icon="mdiPlay"
                :size="mobile ? 'default' : 'large'"
                :block="mobile"
                :to="playLink"
              >
                {{ playLabel }}
              </v-btn>
              <download-button
                v-if="type === 'movie' || target"
                :id="id"
                :type="type"
                :imdb-id="media.imdbId"
                :season="target?.season"
                :episode="target?.episode"
                :size="mobile ? 'default' : 'large'"
              />
              <torrent-picker
                v-if="type === 'movie' || target"
                :id="id"
                :type="type"
                :imdb-id="media.imdbId"
                :season="target?.season"
                :episode="target?.episode"
                :size="mobile ? 'default' : 'large'"
              />
              <v-btn
                v-if="media.trailer"
                :prepend-icon="mdiYoutube"
                :size="mobile ? 'default' : 'large'"
                variant="tonal"
                @click="trailer = true"
              >
                {{ $t('Trailer') }}
              </v-btn>
              <v-spacer v-if="mobile" />
              <!-- Whole-title mark. For a show that's a manual override — the
                   app can't know every episode has been seen without a season
                   fetch, and the per-episode ticks already say it. -->
              <v-btn icon variant="text" color="on-surface" :size="mobile ? 'default' : 'large'" @click="library.toggleWatched(media)">
                <v-icon :icon="library.isWatched(media) ? mdiEye : mdiEyeOutline" :color="library.isWatched(media) ? 'primary' : undefined" />
                <v-tooltip activator="parent" :text="library.isWatched(media) ? $t('Mark unwatched') : $t('Mark watched')" />
              </v-btn>
              <v-btn icon variant="text" color="on-surface" :size="mobile ? 'default' : 'large'" @click="library.toggleWatchlist(media)">
                <v-icon :icon="library.inWatchlist(media) ? mdiBookmark : mdiBookmarkOutline" :color="library.inWatchlist(media) ? 'primary' : undefined" />
                <v-tooltip activator="parent" :text="library.inWatchlist(media) ? $t('Remove from watchlist') : $t('Add to watchlist')" />
              </v-btn>
              <v-btn icon variant="text" color="on-surface" :size="mobile ? 'default' : 'large'" @click="library.toggleFavourite(media)">
                <v-icon :icon="library.isFavourite(media) ? mdiHeart : mdiHeartOutline" :color="library.isFavourite(media) ? 'primary' : undefined" />
                <v-tooltip activator="parent" :text="library.isFavourite(media) ? $t('Remove from favourites') : $t('Favourite')" />
              </v-btn>
            </div>
          </div>

          <div v-else class="flex min-w-0 flex-1 flex-col gap-3">
            <div class="animate-pulse h-10 w-2/3 max-w-sm rounded-lg bg-surface-container/60" />
            <div class="animate-pulse h-4 w-40 rounded bg-surface-container/60" />
            <div class="animate-pulse h-20 w-full max-w-2xl rounded-lg bg-surface-container/60" />
            <div class="animate-pulse h-10 w-48 rounded-lg bg-surface-container/60" />
          </div>
        </div>
      </section>

      <div class="flex flex-col gap-8">
        <!-- Above the cast: on a show this row is what the page is for — the
             way to the next episode — and the cast is something you read. -->
        <media-seasons
          v-if="type === 'tv' && media?.seasons.length"
          :key="id"
          :show-id="id"
          :seasons="media.seasons"
          :poster="media.poster"
          :show="media"
        />

        <cast-row v-if="media?.cast.length" :title="$t('Cast')" :people="media.cast" />

        <media-slider
          v-if="status !== 'pending'"
          :title="$t('More like this')"
          :request="{ path: `/${type}/${id}/recommendations`, type }"
        />
      </div>

      <!-- v-if on the iframe, not just the dialog: v-dialog keeps its content
           mounted after the first close, and YouTube would keep playing. -->
      <v-dialog v-model="trailer" max-width="1100">
        <v-card class="overflow-hidden">
          <iframe
            v-if="trailer"
            :src="`https://www.youtube-nocookie.com/embed/${media?.trailer}?autoplay=1`"
            class="aspect-video w-full border-0"
            style="zoom: var(--frame-zoom, 1)"
            allow="autoplay; encrypted-media; fullscreen"
            allowfullscreen
          />
          <v-card-actions>
            <v-btn :prepend-icon="mdiOpenInNew" size="small" variant="text" @click="openTrailer">
              {{ $t('Open on YouTube') }}
            </v-btn>
            <v-spacer />
            <v-btn size="small" variant="text" @click="trailer = false">
              {{ $t('Close') }}
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-dialog>
    </template>
  </div>
</template>
