<script setup lang="ts">
import type { ComponentPublicInstance } from 'vue'
import { mdiAlertCircleOutline, mdiArrowLeft, mdiEye, mdiEyeOutline, mdiMovieOpenOutline, mdiPlay, mdiStar } from '@mdi/js'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { useResizeObserver } from '@vueuse/core'

definePageMeta({
  validate: ({ params }) => 'season' in params && /^\d+$/.test(params.season),
})

const route = useRoute()
const ui = useUiStore()

const id = computed(() => String(route.params.id))
const number = computed(() => Number(route.params.season))

// Same cache key as the show page, so arriving from it costs no request.
const { data: show } = useMediaDetail('tv', id)
const { data: season, pending, error } = useSeason(id, number)

let mine = 0
watch(show, value => value && (mine = ui.select(value)), { immediate: true })
onUnmounted(() => ui.release(mine))

// --- The list ---------------------------------------------------------------

/**
 * Some anime list a thousand episodes in one season, and mounting a row for
 * each — two Vuetify buttons and a watched dialog apiece — is what froze the
 * page until every one of them existed. Only the rows in the window are in the
 * DOM, same as the live-tv grids; the spacer keeps the scrollbar honest about
 * the full count.
 */
const scrollRef = ref<HTMLElement>()
const episodes = computed(() => season.value?.episodes ?? [])

/** The header scrolls away with the page, so the rows' offsets inside the
 * scroller all start below it. The core takes this into account itself: rows'
 * `start` includes it, `getTotalSize()` gives it back. */
const margin = ref(0)
const headerRef = ref<HTMLElement>()
const spacerRef = ref<HTMLElement>()

function measureMargin() {
  const scroller = scrollRef.value
  const spacer = spacerRef.value
  margin.value = scroller && spacer
    ? spacer.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop
    : 0
}

// The header grows when the season lands (overview, chips) and rewraps when
// the window does; either moves the list, so its own box is what to watch.
useResizeObserver(headerRef, measureMargin)
// The spacer exists only once episodes do, so the first measurement after it
// mounts is what's true.
watch(episodes, () => nextTick(measureMargin))

const virtualizer = useVirtualizer(computed(() => ({
  count: episodes.value.length,
  getScrollElement: () => scrollRef.value ?? null,
  scrollMargin: margin.value,
  // Only the scrollbar length depends on this until a row is measured; 196 is
  // a card at `sm` and up — the 16:9 still is the tall part there, on a phone
  // the stacked text is.
  estimateSize: () => 196,
  overscan: 6,
})))

/** Rows are measured, not estimated: a still is 63px on a phone and 99 at `sm`,
 * and being wrong makes absolutely-positioned rows overlap. `measureElement`
 * reads what the row actually is — which is why the gap travels inside it as
 * padding: an absolutely-positioned row leaves no sibling for a gap to sit
 * between, and the measured size is the whole row, gap included. */
function measure(el: Element | ComponentPublicInstance | null): void {
  if (el instanceof HTMLElement)
    virtualizer.value?.measureElement(el)
}

// --- The season at a glance -------------------------------------------------

const library = useLibraryStore()

const watchedCount = computed(() => episodes.value
  .filter(e => library.episodeProgress(id.value, number.value, e.number)?.watched)
  .length)
const allWatched = computed(() => !!episodes.value.length && watchedCount.value >= episodes.value.length)

/** The first episode not yet seen — what this page's Play starts. */
const upNext = computed(() => episodes.value
  .find(e => !library.episodeProgress(id.value, number.value, e.number)?.watched) ?? episodes.value[0] ?? null)

const totalRuntime = computed(() => episodes.value.reduce((sum, e) => sum + (e.runtime || 0), 0))

/** Episodes nobody has rated yet would drag the average to zero. */
const averageRating = computed(() => {
  const rated = episodes.value.filter(e => e.rating > 0)
  return rated.length ? rated.reduce((sum, e) => sum + e.rating, 0) / rated.length : 0
})

function toggleSeason() {
  const target = show.value ?? { id: Number(id.value), type: 'tv' as const }
  library.markSeason(target, number.value, episodes.value.length, !allWatched.value)
}

/** The show's own colour, from its artwork — the same accent its page uses. */
const accent = useArtAccent(() => show.value?.backdrop)

// The chips navigate within the same component, and the browser only clamps
// the stale scrollTop because the old list happened to be shorter — here the
// spacer is sized for a season that may no longer be the one on screen.
watch(number, () => {
  scrollRef.value?.scrollTo({ top: 0 })
})
</script>

<template>
  <div
    ref="scrollRef"
    class="h-full overflow-y-auto pb-12"
    :style="{ '--title-accent': accent || undefined }"
  >
    <div v-if="error" class="flex h-full flex-col items-center justify-center gap-2">
      <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
      <span class="text-body-medium opacity-70">{{ $t('Couldn\'t load this season.') }}</span>
      <v-btn variant="tonal" :to="localePath(`/tv/${id}`)">
        {{ $t('Back to show') }}
      </v-btn>
    </div>

    <template v-else>
      <section ref="headerRef" class="relative">
        <!-- The show's backdrop behind the season, faded into the page — the
             same picture the show page opened on, so the two read as one place. -->
        <div class="absolute inset-0 overflow-hidden" aria-hidden="true">
          <img
            v-if="show?.backdrop"
            :src="backdropUrl(show.backdrop, 'w1280') ?? ''"
            alt=""
            decoding="async"
            class="h-full w-full object-cover object-top opacity-70"
          >
          <div class="absolute inset-0 bg-gradient-to-r from-black/80 via-black/40 to-transparent" />
          <div
            class="absolute inset-0"
            style="background: linear-gradient(to top, rgb(var(--v-theme-background)) 4%, rgba(var(--v-theme-background), 0.7) 40%, transparent)"
          />
        </div>

        <div class="relative px-4 pb-8 pt-4 md:px-6">
          <v-btn :prepend-icon="mdiArrowLeft" variant="text" size="small" class="mb-6 -ml-2 text-white" :to="localePath(`/tv/${id}`)">
            {{ show?.title ?? $t('Back to show') }}
          </v-btn>

          <div class="flex flex-col gap-6 pt-[10vh] sm:flex-row sm:items-end md:gap-8">
            <div class="aspect-2/3 w-36 shrink-0 overflow-hidden rounded-2xl shadow-[0_24px_60px_rgba(0,0,0,0.6)] ring-1 ring-white/10 sm:w-48 lg:w-56">
              <media-poster :src="posterUrl(season?.poster ?? show?.poster, 'w342')" :alt="season?.name" />
            </div>

            <div class="flex min-w-0 max-w-4xl flex-1 flex-col gap-4 text-white">
              <p class="text-label-large font-semibold uppercase tracking-[0.2em] text-[var(--title-accent,rgb(var(--v-theme-primary)))]">
                {{ show?.title }}
              </p>
              <h1 class="text-display-small font-black leading-none tracking-tight md:text-display-medium">
                {{ season?.name ?? $t('Season {number}', { number }) }}
              </h1>

              <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-body-large text-white/90">
                <span v-if="averageRating" class="flex items-center gap-1 font-semibold text-amber-300">
                  <v-icon :icon="mdiStar" size="18" />{{ averageRating.toFixed(1) }}
                </span>
                <span v-if="season?.air">{{ season.air.slice(0, 4) }}</span>
                <span v-if="episodes.length">{{ $t('{count} episodes', { count: episodes.length }) }}</span>
                <span v-if="totalRuntime">{{ runtimeText(totalRuntime) }}</span>
                <span v-if="show?.certification" class="rounded border border-white/50 px-1.5 text-label-medium font-semibold">
                  {{ show.certification }}
                </span>
              </div>

              <p v-if="season?.overview" class="line-clamp-4 max-w-3xl text-body-large leading-relaxed text-white/85">
                {{ season.overview }}
              </p>

              <!-- How far through, as a bar: the one number a returning viewer wants. -->
              <div v-if="episodes.length" class="flex max-w-md flex-col gap-1.5">
                <div class="flex justify-between text-label-large text-white/75">
                  <span>{{ $t('{seen}/{total} watched', { seen: watchedCount, total: episodes.length }) }}</span>
                  <span>{{ Math.round((watchedCount / episodes.length) * 100) }}%</span>
                </div>
                <div class="h-1.5 overflow-hidden rounded-full bg-white/15">
                  <div
                    class="h-full rounded-full bg-[var(--title-accent,rgb(var(--v-theme-primary)))] transition-[width] duration-500"
                    :style="{ width: `${(watchedCount / episodes.length) * 100}%` }"
                  />
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-3 pt-1">
                <v-btn
                  v-if="upNext"
                  :prepend-icon="mdiPlay"
                  size="x-large"
                  color="white"
                  variant="flat"
                  rounded="lg"
                  class="px-8 font-bold text-black"
                  :to="watchLink('tv', id, number, upNext.number)"
                >
                  {{ watchedCount ? $t('Play E{n}', { n: upNext.number }) : $t('Play from the start') }}
                </v-btn>
                <v-btn
                  v-if="episodes.length"
                  :prepend-icon="allWatched ? mdiEye : mdiEyeOutline"
                  size="x-large"
                  variant="flat"
                  rounded="lg"
                  class="bg-white/15! px-6 font-semibold text-white"
                  @click="toggleSeason"
                >
                  {{ allWatched ? $t('Mark season unwatched') : $t('Mark season watched') }}
                </v-btn>
              </div>
            </div>
          </div>

          <!-- Every season as a tab, this one lit: moving between them is a
               step sideways, not a trip back to the show. -->
          <nav v-if="show && show.seasons.length > 1" class="mt-8 flex gap-2 overflow-x-auto pb-1" :aria-label="$t('Seasons')">
            <nuxt-link
              v-for="s in show.seasons"
              :key="s.number"
              :to="seasonLink(id, s.number)"
              class="shrink-0 rounded-full border px-4 py-2 text-label-large font-semibold outline-none transition-colors focus-visible:ring-2 focus-visible:ring-primary"
              :class="s.number === number
                ? 'border-transparent bg-white text-black'
                : 'border-white/20 text-white/85 hover:border-white/50 hover:bg-white/10 focus-visible:bg-white/10'"
              :aria-current="s.number === number ? 'page' : undefined"
            >
              {{ s.name }}
            </nuxt-link>
          </nav>
        </div>
      </section>

      <section class="flex flex-col gap-2 px-4 md:px-6">
        <h2 v-if="episodes.length" class="pb-2 text-title-large font-bold">
          {{ $t('Episodes') }}
        </h2>
        <div
          v-if="episodes.length"
          ref="spacerRef"
          :style="{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }"
        >
          <div
            v-for="virtualRow in virtualizer.getVirtualItems()"
            :key="virtualRow.index"
            :ref="measure"
            :data-index="virtualRow.index"
            class="pb-3"
            :style="{
              position: 'absolute',
              top: 0,
              left: 0,
              right: 0,
              transform: `translateY(${virtualRow.start - margin}px)`,
            }"
          >
            <episode-row
              v-if="episodes[virtualRow.index]"
              :show-id="id"
              :season="number"
              :episode="episodes[virtualRow.index]!"
              :show="show"
              :up-next="upNext?.number === episodes[virtualRow.index]!.number && watchedCount > 0"
            />
          </div>
        </div>

        <div
          v-for="n in pending && !season ? 6 : 0"
          :key="`skeleton-${n}`"
          class="h-40 animate-pulse rounded-2xl bg-surface-container/60"
        />

        <div v-if="!pending && !episodes.length" class="flex flex-col items-center gap-2 py-8">
          <v-icon :icon="mdiMovieOpenOutline" size="40" class="opacity-30" />
          <span class="text-body-medium opacity-70">{{ $t('No episodes listed.') }}</span>
        </div>
      </section>
    </template>
  </div>
</template>
