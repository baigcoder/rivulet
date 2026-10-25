<script setup lang="ts">
import { mdiAlertCircleOutline, mdiArrowLeft, mdiCalendarClock, mdiChevronLeft, mdiChevronRight, mdiPlay, mdiStar } from '@mdi/js'

definePageMeta({
  validate: ({ params }) => 'episode' in params && /^\d+$/.test(params.season) && /^\d+$/.test(params.episode),
})

const route = useRoute()
const ui = useUiStore()
const library = useLibraryStore()

const id = computed(() => String(route.params.id))
const seasonNumber = computed(() => Number(route.params.season))
const episodeNumber = computed(() => Number(route.params.episode))

// Cached by the show page under the same key when you arrive from it.
const { data: show } = useMediaDetail('tv', id)
const { data: episode, pending, error } = useEpisode(id, seasonNumber, episodeNumber)
// Usually cached too: the season page is how most people get here. It is what
// names the episodes either side and fills the row at the bottom.
const { data: season } = useSeason(id, seasonNumber)

let mine = 0
watch(show, value => value && (mine = ui.select(value)), { immediate: true })
onUnmounted(() => ui.release(mine))

const credits = computed(() => {
  const e = episode.value
  if (!e)
    return []
  return [
    { label: e.directors.length > 1 ? $t('Directors') : $t('Director'), value: e.directors.join(', ') },
    { label: e.writers.length > 1 ? $t('Writers') : $t('Writer'), value: e.writers.join(', ') },
  ].filter(row => row.value)
})

const torrentPickerRef = ref<{ open: () => void } | null>(null)

// Same as the title page: ask the sources while this one is read, so Play
// starts without a search — unless the episode is already on the disk.
const downloadsStore = useDownloadsStore()
watch(() => [show.value?.imdbId, seasonNumber.value, episodeNumber.value] as const, ([imdb, season, episode]) => {
  if (imdb && !downloadsStore.cachedFor(progressKey('tv', id.value, season, episode)))
    searchAhead(imdb, season, episode)
}, { immediate: true })

const playHandoff = computed(() => ({
  imdb: show.value?.imdbId ?? undefined,
  title: show.value?.title,
  year: show.value?.year,
}))

const progress = computed(() => library.episodeProgress(id.value, seasonNumber.value, episodeNumber.value))
const started = computed(() => {
  const p = progress.value
  return p && !p.watched && resumable(p.position, p.duration) ? p : null
})
const startedFraction = computed(() => started.value?.duration ? Math.min(1, started.value.position / started.value.duration) : 0)

/** Within the season only: crossing into the next one is the season page's job. */
const episodes = computed(() => season.value?.episodes ?? [])
const at = computed(() => episodes.value.findIndex(e => e.number === episodeNumber.value))
const previous = computed(() => at.value > 0 ? episodes.value[at.value - 1] : null)
const next = computed(() => at.value >= 0 ? episodes.value[at.value + 1] ?? null : null)

const accent = useArtAccent(() => show.value?.backdrop)

/** Announced but not out: TMDB lists the date before there is anything to play. */
const upcoming = computed(() => !!episode.value?.air && new Date(`${episode.value.air}T00:00:00`) > new Date())

/** A still, or the show's backdrop for an episode TMDB has no frame from yet. */
function frame(path?: string | null) {
  return stillUrl(path) || backdropUrl(show.value?.backdrop, 'w780')
}

/** The still is small art; on desktop the full-size copy is worth the bytes. */
const still = computed(() => episode.value?.still
  ? (isDesktop() ? `https://image.tmdb.org/t/p/original${episode.value.still}` : stillUrl(episode.value.still, 'w780'))
  : backdropUrl(show.value?.backdrop, 'w1280'))
</script>

<template>
  <div class="h-full overflow-y-auto pb-12" :style="{ '--title-accent': accent || undefined }">
    <div v-if="error" class="flex h-full flex-col items-center justify-center gap-2">
      <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
      <span class="text-body-medium opacity-70">{{ $t('Couldn\'t load this episode.') }}</span>
      <v-btn variant="tonal" :to="seasonLink(id, seasonNumber)">
        {{ $t('Back to season') }}
      </v-btn>
    </div>

    <template v-else>
      <section class="relative">
        <!-- The episode's own frame, full width and faded into the page. -->
        <div class="absolute inset-0 overflow-hidden" aria-hidden="true">
          <img v-if="still" :src="still" alt="" decoding="async" class="h-full w-full object-cover">
          <div class="absolute inset-0 bg-gradient-to-r from-black/85 via-black/40 to-transparent" />
          <div
            class="absolute inset-0"
            style="background: linear-gradient(to top, rgb(var(--v-theme-background)) 3%, rgba(var(--v-theme-background), 0.75) 35%, transparent)"
          />
        </div>

        <div class="relative flex min-h-[62vh] flex-col px-4 pb-8 pt-4 md:px-6">
          <div class="flex flex-wrap items-center gap-1 -ml-2 text-white">
            <v-btn :prepend-icon="mdiArrowLeft" variant="text" size="small" :to="localePath(`/tv/${id}`)">
              {{ show?.title ?? $t('Show') }}
            </v-btn>
            <span class="opacity-40">/</span>
            <v-btn variant="text" size="small" :to="seasonLink(id, seasonNumber)">
              {{ $t('Season {number}', { number: seasonNumber }) }}
            </v-btn>
          </div>

          <div v-if="episode" class="mt-auto flex max-w-4xl flex-col gap-4 pt-[16vh] text-white">
            <p class="text-label-large font-semibold uppercase tracking-[0.2em] text-[var(--title-accent,rgb(var(--v-theme-primary)))]">
              {{ show?.title }} · {{ $t('Season {number}', { number: seasonNumber }) }} · {{ $t('Episode {n}', { n: episodeNumber }) }}
            </p>
            <h1 class="text-display-small font-black leading-tight tracking-tight md:text-display-medium">
              {{ episode.name || $t('Episode {number}', { number: episodeNumber }) }}
            </h1>

            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-body-large text-white/90">
              <span v-if="episode.rating" class="flex items-center gap-1 font-semibold text-amber-300">
                <v-icon :icon="mdiStar" size="18" />{{ episode.rating.toFixed(1) }}
                <span class="font-normal text-white/60">({{ episode.votes.toLocaleString(uiLocale()) }})</span>
              </span>
              <span v-if="episode.air">{{ dateText(episode.air) }}</span>
              <span v-if="episode.runtime">{{ runtimeText(episode.runtime) }}</span>
              <span v-if="show?.certification" class="rounded border border-white/50 px-1.5 text-label-medium font-semibold">
                {{ show.certification }}
              </span>
            </div>

            <p class="max-w-3xl text-body-large leading-relaxed text-white/90">
              {{ episode.overview || $t('No overview.') }}
            </p>

            <dl v-if="credits.length" class="grid max-w-2xl grid-cols-1 gap-x-8 gap-y-1.5 text-body-medium sm:grid-cols-2">
              <div v-for="row in credits" :key="row.label" class="flex gap-2">
                <dt class="shrink-0 text-white/55">
                  {{ row.label }}
                </dt>
                <dd class="ms-0 truncate">
                  {{ row.value }}
                </dd>
              </div>
            </dl>

            <p
              v-if="upcoming"
              class="inline-flex items-center gap-2 self-start rounded-full border border-white/25 bg-white/10 px-4 py-1.5 text-label-large font-semibold"
            >
              <v-icon :icon="mdiCalendarClock" size="18" />
              {{ $t('Airs {date}', { date: dateText(episode.air) }) }}
            </p>
            <title-status
              v-else
              :progress-key="progressKey('tv', id, seasonNumber, episodeNumber)"
              :imdb-id="show?.imdbId"
              :season="seasonNumber"
              :episode="episodeNumber"
              class="mt-1"
            />

            <div class="flex flex-wrap items-center gap-3 pt-1">
              <template v-if="!upcoming">
                <v-btn
                  :prepend-icon="mdiPlay"
                  size="x-large"
                  color="white"
                  variant="flat"
                  rounded="lg"
                  class="relative overflow-hidden px-8 font-bold text-black"
                  :to="watchLink('tv', id, seasonNumber, episodeNumber, playHandoff)"
                >
                  {{ started ? $t('Resume') : $t('Play') }}
                  <span v-if="startedFraction" class="absolute inset-x-0 bottom-0 h-1 bg-black/15" aria-hidden="true">
                    <span class="block h-full bg-[var(--title-accent,rgb(var(--v-theme-primary)))]" :style="{ width: `${Math.round(startedFraction * 100)}%` }" />
                  </span>
                </v-btn>
                <download-button
                  :id="id"
                  type="tv"
                  :imdb-id="show?.imdbId"
                  :season="seasonNumber"
                  :episode="episodeNumber"
                  size="large"
                  @pick="torrentPickerRef?.open()"
                />
                <torrent-picker
                  :id="id"
                  ref="torrentPickerRef"
                  type="tv"
                  :imdb-id="show?.imdbId"
                  :season="seasonNumber"
                  :episode="episodeNumber"
                  size="large"
                />
              </template>
              <watched-button
                :show-id="id"
                :season="seasonNumber"
                :episode="episodeNumber"
                :show="show"
                size="large"
              />
            </div>
          </div>

          <div v-else class="mt-auto flex min-w-0 max-w-3xl flex-col gap-3 pt-[16vh]">
            <div class="h-4 w-48 animate-pulse rounded bg-white/10" />
            <div class="h-12 w-2/3 max-w-md animate-pulse rounded-lg bg-white/10" />
            <div class="h-20 w-full animate-pulse rounded-lg bg-white/10" />
          </div>
        </div>
      </section>

      <!-- Either side of this one, named: the next press after "what was that
           one about" is usually "and the one after". -->
      <nav v-if="previous || next" class="relative grid gap-3 px-4 pb-8 sm:grid-cols-2 md:px-6" :aria-label="$t('Episodes')">
        <nuxt-link
          v-if="previous"
          :to="episodeLink(id, seasonNumber, previous.number)"
          class="group flex items-center gap-3 rounded-2xl border border-white/10 bg-surface-container/40 p-3 outline-none transition-colors hover:border-white/30 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary"
        >
          <v-icon :icon="mdiChevronLeft" size="28" class="shrink-0 opacity-60" />
          <div class="aspect-video w-28 shrink-0 overflow-hidden rounded-lg bg-black">
            <media-poster :src="frame(previous.still)" :alt="previous.name" />
          </div>
          <div class="min-w-0">
            <p class="text-label-medium uppercase tracking-wider opacity-55">
              {{ $t('Previous') }} · E{{ previous.number }}
            </p>
            <p class="truncate text-title-small font-semibold">
              {{ previous.name }}
            </p>
          </div>
        </nuxt-link>
        <span v-else class="hidden sm:block" />
        <nuxt-link
          v-if="next"
          :to="episodeLink(id, seasonNumber, next.number)"
          class="group flex items-center justify-end gap-3 rounded-2xl border border-white/10 bg-surface-container/40 p-3 text-end outline-none transition-colors hover:border-white/30 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary"
        >
          <div class="min-w-0">
            <p class="text-label-medium uppercase tracking-wider opacity-55">
              {{ $t('Next') }} · E{{ next.number }}
            </p>
            <p class="truncate text-title-small font-semibold">
              {{ next.name }}
            </p>
          </div>
          <div class="aspect-video w-28 shrink-0 overflow-hidden rounded-lg bg-black">
            <media-poster :src="frame(next.still)" :alt="next.name" />
          </div>
          <v-icon :icon="mdiChevronRight" size="28" class="shrink-0 opacity-60" />
        </nuxt-link>
      </nav>

      <div class="flex flex-col gap-10">
        <cast-row v-if="!pending && episode?.guests.length" :title="$t('Guest stars')" :people="episode.guests" />

        <scroll-row v-if="episodes.length > 1" :title="$t('More from season {number}', { number: seasonNumber })" :to="seasonLink(id, seasonNumber)">
          <nuxt-link
            v-for="e in episodes"
            :key="e.number"
            :to="episodeLink(id, seasonNumber, e.number)"
            class="group flex w-64 shrink-0 flex-col gap-2 rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-primary"
            :aria-current="e.number === episodeNumber ? 'page' : undefined"
          >
            <div
              class="relative aspect-video overflow-hidden rounded-xl bg-black"
              :class="e.number === episodeNumber ? 'ring-2 ring-inset ring-white' : ''"
            >
              <media-poster :src="frame(e.still)" :alt="e.name" />
              <div class="absolute inset-0 bg-gradient-to-t from-black/70 to-transparent" />
              <span class="absolute bottom-1.5 start-2.5 text-title-large font-black text-white">{{ e.number }}</span>
              <span
                v-if="library.episodeProgress(id, seasonNumber, e.number)?.watched"
                class="absolute end-2 top-2 rounded-full bg-black/70 px-2 py-0.5 text-label-small font-semibold text-white"
              >{{ $t('Watched') }}</span>
            </div>
            <p class="truncate text-body-medium font-medium" :class="e.number === episodeNumber ? 'font-bold' : ''">
              {{ e.name }}
            </p>
          </nuxt-link>
        </scroll-row>
      </div>
    </template>
  </div>
</template>
