<script lang="ts" setup>
/**
 * The Streaming strip on Home, under the hero: one card per watch provider in
 * the user's region, straight into that provider's catalogue.
 *
 * Everything here is TMDB metadata — which services carry what where, and
 * their logos. The names pinned to the front are an ordering choice, not a
 * recommendation of anywhere to watch anything; what actually plays is decided
 * entirely by the sources the user added.
 */
import type { WatchProvider } from '~/utils/streamingProviders'
import { mdiTelevisionClassic } from '@mdi/js'
import { useElementSize } from '@vueuse/core'
import { computed, ref } from 'vue'
import { fillRows, providerColumns, streamingProviders } from '~/utils/streamingProviders'

type Provider = WatchProvider

const settings = useSettingsStore()

/**
 * TMDB's provider catalogues are per-country. The app language usually knows
 * its region ("pt-BR"), and that is the right default; '' falls back to the US
 * catalogue, the largest one TMDB has. The choice itself lives on the provider
 * page — this strip just follows it. `uiLocale` reads `<html lang>`, which is
 * also what keeps `useI18n`'s types out of this file.
 */
const region = computed(() => {
  if (settings.watchRegion)
    return settings.watchRegion
  try {
    const tag = new Intl.Locale(uiLocale()).maximize()
    return tag.region ?? 'US'
  }
  catch {
    return 'US'
  }
})

const { data, error, pending } = useAsyncData(
  () => `providers-${region.value}`,
  async () => {
    // Movies and shows have different availability, so both lists are asked
    // and merged on the provider id — one card per service either way.
    const [tv, movie] = await Promise.all([
      tmdb<{ results: Provider[] }>('/watch/providers/tv', { watch_region: region.value }),
      tmdb<{ results: Provider[] }>('/watch/providers/movie', { watch_region: region.value }),
    ])
    const byId = new Map<number, Provider>()
    for (const p of [...(tv?.results ?? []), ...(movie?.results ?? [])]) {
      if (!byId.has(p.provider_id))
        byId.set(p.provider_id, p)
    }
    return [...byId.values()]
  },
  { watch: [region] },
)

/** Every service in the region, once each, household names first — see `streamingProviders`. */
const all = computed(() => streamingProviders(data.value ?? [], region.value))

/**
 * Columns follow the strip's real width rather than breakpoints, so the count
 * of cards can be chosen to fill whole rows — eleven services on a ten-column
 * screen used to leave one card alone on a second line.
 */
const grid = ref<HTMLElement>()
const { width } = useElementSize(grid)
const cols = computed(() => providerColumns(width.value))
const gridStyle = computed(() => ({ gridTemplateColumns: `repeat(${cols.value}, minmax(0, 1fr))` }))

/** Two full rows on Home; the rest one press away rather than 150 cards up front. */
const ROWS = 2
const expanded = ref(false)
const collapsedCount = computed(() => fillRows(all.value.length, cols.value, ROWS))
const providers = computed(() => expanded.value ? all.value : all.value.slice(0, collapsedCount.value))

function to(p: Provider) {
  // No locale prefix exists in the URL (no_prefix strategy), so a plain route
  // object is the whole story.
  return { path: `/streaming/${p.provider_id}`, query: { name: p.provider_name, logo: p.logo_path ?? '' } }
}
</script>

<template>
  <section class="mt-5" aria-labelledby="streaming-heading">
    <div class="mx-4 mb-3 flex items-center gap-2 md:mx-6">
      <v-icon :icon="mdiTelevisionClassic" size="22" class="opacity-70" />
      <h2 id="streaming-heading" class="text-title-large">
        {{ $t('Streaming') }}
      </h2>
      <span class="text-body-small opacity-50">{{ region }}</span>
    </div>

    <p v-if="error" class="mx-4 text-body-small opacity-60 md:mx-6">
      {{ $t('Couldn\'t load the provider list from TMDB.') }}
    </p>

    <!-- Loading skeleton -->
    <div v-if="pending && !providers.length" class="grid grid-cols-5 gap-3 px-4 sm:grid-cols-7 md:gap-3.5 md:px-6 lg:grid-cols-10 xl:grid-cols-12" aria-hidden="true">
      <div v-for="i in 10" :key="i" class="aspect-square animate-pulse rounded-2xl bg-surface-container-high" />
    </div>

    <div v-else-if="!error" ref="grid" data-dpad-start class="grid gap-3.5 px-4 md:px-6" :style="gridStyle">
      <nuxt-link
        v-for="p in providers"
        :key="p.provider_id"
        :to="to(p)"
        class="provider-card group relative aspect-square w-full overflow-hidden rounded-2xl border border-white/10 bg-surface-container-high shadow-md transition-[transform,border-color] duration-300 hover:scale-[1.06] hover:border-primary/60 hover:shadow-xl hover:shadow-primary/20 focus-visible:border-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :aria-label="$t('Browse {provider}', { provider: p.provider_name })"
      >
        <img
          v-if="p.logo_path"
          :src="logoUrl(p.logo_path, 'w500') ?? ''"
          :alt="p.provider_name"
          class="h-full w-full rounded-2xl object-cover transition-transform duration-300 group-hover:scale-110"
        >
        <span v-else class="absolute inset-0 grid place-items-center text-xl font-bold tracking-wide opacity-70 transition-opacity group-hover:opacity-100">{{ p.provider_name[0] }}</span>

        <!-- Subtle gradient gloss on hover -->
        <div class="pointer-events-none absolute inset-0 rounded-2xl bg-gradient-to-t from-white/10 via-transparent to-transparent opacity-0 transition-opacity duration-200 group-hover:opacity-100 group-focus-visible:opacity-100" />
      </nuxt-link>
    </div>

    <div v-if="!error && all.length > collapsedCount" class="mx-4 mt-3 md:mx-6">
      <button
        type="button"
        class="rounded-full px-4 py-2 text-label-large font-medium text-primary transition-colors hover:bg-primary/10 focus-visible:bg-primary/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :aria-expanded="expanded ? 'true' : 'false'"
        @click="expanded = !expanded"
      >
        {{ expanded ? $t('Show fewer services') : $t('All {count} services', { count: all.length }) }}
      </button>
    </div>
  </section>
</template>
