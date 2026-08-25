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
import { mdiTelevisionClassic } from '@mdi/js'

interface Provider {
  provider_id: number
  provider_name: string
  logo_path: string | null
}

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

const { data, error } = useAsyncData(
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

/** The household names lead; everything else follows alphabetically. */
const PINNED = ['netflix', 'prime video', 'amazon prime video', 'disney', 'max', 'hbo max', 'apple tv', 'hulu', 'paramount', 'peacock', 'crunchyroll']

function rank(p: Provider) {
  const name = p.provider_name.toLowerCase()
  const at = PINNED.findIndex(pin => name.includes(pin))
  return at === -1 ? PINNED.length : at
}

const providers = computed(() =>
  [...(data.value ?? [])].sort((a, b) => rank(a) - rank(b) || a.provider_name.localeCompare(b.provider_name)))

function to(p: Provider) {
  // No locale prefix exists in the URL (no_prefix strategy), so a plain route
  // object is the whole story.
  return { path: `/streaming/${p.provider_id}`, query: { name: p.provider_name, logo: p.logo_path ?? '' } }
}
</script>

<template>
  <section class="mt-5" aria-labelledby="streaming-heading">
    <div class="mx-4 mb-2 flex items-center gap-2 md:mx-6">
      <v-icon :icon="mdiTelevisionClassic" size="18" class="opacity-70" />
      <h2 id="streaming-heading" class="text-title-medium">
        {{ $t('Streaming') }}
      </h2>
      <span class="text-label-small opacity-50">{{ region }}</span>
    </div>

    <p v-if="error" class="mx-4 text-body-small opacity-60 md:mx-6">
      {{ $t('Couldn\'t load the provider list from TMDB.') }}
    </p>

    <!-- A scroll row, not a grid: dozens of services in a region, and the ones
         past the fold are exactly as reachable by d-pad as the rest — the
         plugin scrolls the row into view when focus asks it to. -->
    <nav v-else class="overflow-x-auto pb-1" data-dpad-start>
      <ul class="flex w-max gap-3 px-4 md:px-6">
        <li v-for="p in providers" :key="p.provider_id">
          <nuxt-link
            :to="to(p)"
            class="group flex h-16 w-32 items-center justify-center rounded-xl border border-white/9 bg-surface-container p-3 transition-all hover:border-primary hover:bg-surface-container-high focus-visible:border-primary focus-visible:bg-surface-container-high focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            :aria-label="$t('Browse {provider}', { provider: p.provider_name })"
          >
            <img
              v-if="p.logo_path"
              :src="logoUrl(p.logo_path, 'w300') ?? ''"
              :alt="p.provider_name"
              loading="lazy"
              class="max-h-full max-w-full object-contain transition-transform group-hover:scale-105"
            >
            <span v-else class="truncate text-center text-body-small">{{ p.provider_name }}</span>
          </nuxt-link>
        </li>
      </ul>
    </nav>
  </section>
</template>
