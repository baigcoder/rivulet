<script lang="ts" setup>
import type { MediaType } from '~/utils/tmdb'
import { mdiArrowLeft } from '@mdi/js'

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
 * is one someone actually looks for — and '' (follow the app language) sits on
 * top of it.
 */
const REGIONS = ['US', 'GB', 'CA', 'AU', 'DE', 'FR', 'ES', 'IT', 'BR', 'MX', 'IN', 'JP', 'KR', 'NL', 'SE']

const region = computed({
  get: () => settings.watchRegion,
  set: v => (settings.watchRegion = v),
})

const categories = [
  { value: 'popular', title: $t('Popular') },
  { value: 'new', title: $t('New') },
  { value: 'top_rated', title: $t('Top rated') },
]
</script>

<template>
  <div class="h-full overflow-y-auto pb-10">
    <div class="mx-4 flex flex-wrap items-center gap-3 md:mx-6">
      <v-btn icon variant="text" :aria-label="$t('Back')" @click="$router.back()">
        <v-icon :icon="mdiArrowLeft" />
      </v-btn>

      <img
        v-if="logo"
        :src="logoUrl(logo, 'w300') ?? ''"
        :alt="name"
        class="h-9 w-auto rounded-md bg-surface-container p-1"
      >
      <h1 class="text-title-large">
        {{ name }}
      </h1>

      <div class="flex-1" />

      <!-- Movies and shows are different catalogues with different provider
           ids on TMDB, so they are two requests rather than one filter. -->
      <v-chip-group
        :model-value="type"
        mandatory
        selected-class="bg-primary text-on-primary font-medium"
        @update:model-value="v => type !== v && $router.replace({ query: { ...route.query, type: v } })"
      >
        <v-chip value="tv" :text="$t('Shows')" size="small" />
        <v-chip value="movie" :text="$t('Movies')" size="small" />
      </v-chip-group>

      <v-select
        v-model="region"
        :items="REGIONS"
        :label="$t('Region')"
        clearable
        hide-details
        density="compact"
        variant="outlined"
        class="w-36 shrink-0"
      />
    </div>

    <media-browser
      :key="`${type}-${region}`"
      class="mt-2"
      :type="type"
      :categories-override="categories"
      :extra-params="{ with_watch_providers: id, watch_region: region || 'US' }"
    />
  </div>
</template>
