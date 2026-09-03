<script lang="ts" setup>
import type { MediaType } from '~/utils/tmdb'

/**
 * One watch provider's catalogue — what TMDB says that service carries in the
 * chosen region, under Popular / New / Top. Metadata only: nothing here says
 * or implies where a title will actually play from, which is the sources' job.
 */
definePageMeta({ keepalive: true })

const route = useRoute()
const settings = useSettingsStore()

const id = computed(() => String(route.params.id ?? ''))
const name = computed(() => String(route.query.name ?? ''))
const logo = computed(() => String(route.query.logo ?? ''))

/** Catalogues are per-type; shows first, since that's where providers live. */
const type = computed<MediaType>(() => route.query.type === 'movie' ? 'movie' : 'tv')

/**
 * Regions worth offering by name. The list is deliberately short — every entry
 * is one someone actually looks for — and an empty stored value follows the
 * app language, same as the home strip.
 */
const REGIONS = ['US', 'GB', 'CA', 'AU', 'DE', 'FR', 'ES', 'IT', 'BR', 'MX', 'IN', 'JP', 'KR', 'NL', 'SE']

function inferredRegion() {
  try {
    return new Intl.Locale(uiLocale()).maximize().region ?? 'US'
  }
  catch {
    return 'US'
  }
}

const region = computed({
  get: () => settings.watchRegion || inferredRegion(),
  set: v => (settings.watchRegion = v || ''),
})

const categories = [
  { value: 'popular', title: $t('Popular') },
  { value: 'new', title: $t('New') },
  { value: 'top_rated', title: $t('Top rated') },
]

function setType(v: unknown) {
  const next = v === 'movie' ? 'movie' : 'tv'
  if (type.value === next)
    return
  void navigateTo({ path: route.path, query: { ...route.query, type: next } }, { replace: true })
}
</script>

<template>
  <!-- Flex column, not a second scroller: the shell already scrolls, and a
       nested `h-full overflow-y-auto` stacked this header on top of a
       viewport-tall grid — two back buttons and one poster on a phone. -->
  <div class="flex h-full flex-col">
    <div class="flex shrink-0 flex-col gap-2 px-4 pt-1 md:flex-row md:items-center md:gap-3 md:px-6">
      <div class="flex min-w-0 items-center gap-2">
        <img
          v-if="logo"
          :src="logoUrl(logo, 'w300') ?? ''"
          :alt="name"
          class="h-7 w-auto shrink-0 rounded-md bg-surface-container p-0.5 md:h-9 md:p-1"
        >
        <h1 class="min-w-0 truncate text-title-medium md:text-title-large">
          {{ name }}
        </h1>
      </div>

      <div class="flex min-w-0 items-center gap-2 md:ms-auto">
        <!-- Movies and shows are different catalogues with different provider
             ids on TMDB, so they are two requests rather than one filter. -->
        <v-btn-toggle
          :model-value="type"
          mandatory
          density="compact"
          variant="text"
          color="primary"
          class="shrink-0 rounded-lg bg-surface-container/50"
          @update:model-value="setType"
        >
          <v-btn value="tv" size="small">
            {{ $t('Shows') }}
          </v-btn>
          <v-btn value="movie" size="small">
            {{ $t('Movies') }}
          </v-btn>
        </v-btn-toggle>

        <v-select
          v-model="region"
          :items="REGIONS"
          :aria-label="$t('Region')"
          class="w-28 shrink-0"
        />
      </div>
    </div>

    <media-browser
      :key="`${type}-${region}`"
      class="min-h-0 flex-1"
      :type="type"
      :categories-override="categories"
      :extra-params="{ with_watch_providers: id, watch_region: region }"
    />
  </div>
</template>
