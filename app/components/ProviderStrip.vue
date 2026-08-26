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
 * TMDB's provider wordmarks come in every darkness, so they're repainted as
 * single-tone marks that match the active theme: white on dark themes, black
 * on light ones. Reactive, so flipping themes repaints the strip instantly.
 */
const vtheme = useTheme()
const logoFilter = computed(() => vtheme.current.value.dark ? 'brightness(0) invert(1)' : 'brightness(0)')

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

/** The household names lead; everything else follows alphabetically. */
const ORDER: string[][] = [
  ['netflix'],
  ['amazon prime video', 'prime video'],
  ['hbo max', 'max'],
  ['apple tv', 'apple tv plus'],
  ['hulu'],
  ['paramount plus'],
  ['showtime'],
  ['amc plus', 'amc'],
]

/**
 * "Paramount+" and "Disney+" normalize onto their spelled-out aliases, so one
 * comparison covers every spelling TMDB uses.
 */
function normalize(name: string) {
  return name.toLowerCase().replace(/\+/g, ' plus').replace(/[^a-z0-9]+/g, ' ').replace(/\s+/g, ' ').trim()
}

function rank(p: Provider) {
  const name = normalize(p.provider_name)
  const at = ORDER.findIndex(aliases => aliases.includes(name))
  return at
}

const providers = computed(() => {
  // Only the strip's own eight, each once: TMDB lists the same service under
  // different ids across its movie and TV catalogues, so the name is what two
  // copies of a service are recognised by.
  const seen = new Map<string, Provider>()
  for (const p of data.value ?? []) {
    const at = rank(p)
    if (at === -1)
      continue
    const key = normalize(p.provider_name)
    if (!seen.has(key) || rank(seen.get(key)!) > at)
      seen.set(key, p)
  }
  return [...seen.values()].sort((a, b) => rank(a) - rank(b))
})

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

    <!-- A wrapped grid, not a sideways scroller: all eight are on the page at
     once — nothing hangs off the edge, nothing gets cut mid-card, and the
     d-pad walks a plain grid. -->
    <div v-if="pending && !providers.length" class="grid grid-cols-3 gap-4 px-4 sm:grid-cols-4 md:px-6 lg:grid-cols-8" aria-hidden="true">
      <div v-for="i in 8" :key="i" class="h-20 animate-pulse rounded-2xl border border-white/9 bg-surface-container lg:h-24" />
    </div>

    <div v-else-if="!error" data-dpad-start class="grid grid-cols-3 gap-4 px-4 sm:grid-cols-4 md:px-6 lg:grid-cols-8">
      <nuxt-link
        v-for="p in providers"
        :key="p.provider_id"
        :to="to(p)"
        class="group relative grid h-20 place-items-center overflow-hidden rounded-2xl border border-white/9 bg-surface-container transition-all duration-200 hover:border-primary focus-visible:border-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary lg:h-24"
        :aria-label="$t('Browse {provider}', { provider: p.provider_name })"
      >
        <!-- Logos fill the tile as clean white marks: every service reads
         identically on the dark theme, and none of TMDB's dark wordmarks
         disappear into the background. -->
        <img
          v-if="p.logo_path"
          :src="logoUrl(p.logo_path, 'w500') ?? ''"
          :alt="p.provider_name"
          loading="lazy"
          class="max-h-[62%] max-w-[78%] object-contain opacity-90 transition-transform duration-200 group-hover:scale-105 group-focus-visible:scale-105"
          :style="{ filter: logoFilter }"
        >
        <span v-else class="text-2xl font-bold tracking-wide">{{ p.provider_name[0] }}</span>
      </nuxt-link>
    </div>
  </section>
</template>
