<script setup lang="ts">
import type { ComponentPublicInstance } from 'vue'
import { mdiAlertCircleOutline, mdiArrowLeft, mdiMovieOpenOutline } from '@mdi/js'
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

const others = computed(() => show.value?.seasons.filter(s => s.number !== number.value) ?? [])

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
  // Only the scrollbar length depends on this until a row is measured; 116 is
  // a row at `sm` and up — the still is the tall part there, on a phone the
  // text is.
  estimateSize: () => 116,
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

// The chips navigate within the same component, and the browser only clamps
// the stale scrollTop because the old list happened to be shorter — here the
// spacer is sized for a season that may no longer be the one on screen.
watch(number, () => {
  scrollRef.value?.scrollTo({ top: 0 })
})
</script>

<template>
  <div ref="scrollRef" class="h-full overflow-y-auto pb-12">
    <div v-if="error" class="flex h-full flex-col items-center justify-center gap-2">
      <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
      <span class="text-body-medium opacity-70">{{ $t('Couldn\'t load this season.') }}</span>
      <v-btn variant="tonal" :to="localePath(`/tv/${id}`)">
        {{ $t('Back to show') }}
      </v-btn>
    </div>

    <template v-else>
      <section ref="headerRef" class="px-4 pb-8 pt-4 md:px-6">
        <v-btn :prepend-icon="mdiArrowLeft" variant="text" size="small" class="mb-3 -ml-2" :to="localePath(`/tv/${id}`)">
          {{ show?.title ?? $t('Back to show') }}
        </v-btn>

        <div class="flex flex-col gap-6 sm:flex-row sm:items-end">
          <div class="aspect-2/3 w-32 shrink-0 overflow-hidden rounded-2xl shadow-2xl sm:w-40">
            <media-poster :src="posterUrl(season?.poster ?? show?.poster, 'w342')" :alt="season?.name" />
          </div>

          <div class="flex min-w-0 flex-1 flex-col gap-3">
            <h1 class="text-headline-large font-bold drop-shadow-[0_2px_24px_rgba(0,0,0,0.6)]">
              {{ season?.name ?? $t('Season {number}', { number }) }}
            </h1>

            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-body-small opacity-75">
              <span v-if="season?.air">{{ dateText(season.air) }}</span>
              <span v-if="season?.episodes.length">{{ $t('{count} episodes', { count: season.episodes.length }) }}</span>
              <span v-if="show?.certification" class="rounded border border-outline-variant px-1.5 py-0.5 text-label-small">
                {{ show.certification }}
              </span>
            </div>

            <p v-if="season?.overview" class="max-w-3xl text-body-medium opacity-85">
              {{ season.overview }}
            </p>

            <div v-if="others.length" class="flex flex-wrap gap-1.5">
              <v-chip
                v-for="other in others"
                :key="other.number"
                size="small"
                :text="other.name"
                :to="seasonLink(id, other.number)"
              />
            </div>
          </div>
        </div>
      </section>

      <section class="flex flex-col gap-2 px-4 md:px-6">
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
            class="pb-2"
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
            />
          </div>
        </div>

        <div
          v-for="n in pending && !season ? 6 : 0"
          :key="`skeleton-${n}`"
          class="animate-pulse h-26 rounded-xl bg-surface-container/60"
        />

        <div v-if="!pending && !episodes.length" class="flex flex-col items-center gap-2 py-8">
          <v-icon :icon="mdiMovieOpenOutline" size="40" class="opacity-30" />
          <span class="text-body-medium opacity-70">{{ $t('No episodes listed.') }}</span>
        </div>
      </section>
    </template>
  </div>
</template>
