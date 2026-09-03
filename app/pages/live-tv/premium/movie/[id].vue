<script setup lang="ts">
/**
 * Premium TV movie detail — provider metadata and a Play action.
 *
 * Metadata comes from the browse list already in memory; there is no
 * separate movie-info API on the panel. A cold deep link still plays
 * if `ext` is in the query.
 */
import type { PremiumVodItem } from '~/types/premium'
import { mdiArrowLeft, mdiPlay } from '@mdi/js'
import { computed, onMounted, ref } from 'vue'
import { proxyLogo } from '~/utils/premiumTv'

definePageMeta({ layout: 'default' })

const route = useRoute()
const router = useRouter()
const premium = usePremiumTvStore()
const settings = useSettingsStore()

const movieId = computed(() => String(route.params.id ?? ''))
const ext = computed(() => String(route.query.ext ?? 'mkv'))

const movie = ref<PremiumVodItem | null>(null)
const loading = ref(true)

const poster = computed(() => proxyLogo(movie.value?.posterUrl))
const heroBlur = computed(() => settings.reduceEffects ? '' : 'backdrop-blur-xl')

onMounted(async () => {
  await premium.ensureLoaded()
  loading.value = true
  movie.value = premium.vodMovies.find(m => m.id === movieId.value) ?? null
  loading.value = false
})

function goBack(): void {
  const from = String(route.query.from ?? '')
  if (from)
    void router.push(from)
  else
    void router.replace(localePath('/live-tv/premium'))
}

function play(): void {
  void router.push({
    path: localePath('/live-tv/premium/watch'),
    query: {
      kind: 'movie',
      id: movieId.value,
      ext: ext.value,
      title: movie.value?.name ?? $t('Movie'),
      from: route.fullPath,
    },
  })
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <div v-if="loading" class="grid flex-1 place-items-center">
      <v-progress-circular indeterminate color="primary" size="36" />
    </div>

    <template v-else>
      <div class="relative shrink-0 overflow-hidden">
        <div
          v-if="poster"
          class="absolute inset-0 scale-110 bg-cover bg-center opacity-35"
          :class="heroBlur"
          :style="{ backgroundImage: `url(${poster})` }"
        />
        <div class="absolute inset-0 bg-gradient-to-t from-surface via-surface/90 to-surface/40" />

        <div class="relative flex flex-col gap-4 px-4 pb-6 pt-4 md:px-6">
          <button
            type="button"
            class="grid size-11 w-fit shrink-0 place-items-center rounded-lg text-on-surface/80 transition-colors hover:bg-surface-container-high hover:text-on-surface focus-visible:bg-surface-container-high focus-visible:text-on-surface"
            :aria-label="$t('Back')"
            @click="goBack"
          >
            <v-icon :icon="mdiArrowLeft" size="24" />
          </button>

          <div class="flex flex-col gap-6 sm:flex-row sm:items-end">
            <div class="mx-auto aspect-[2/3] w-36 shrink-0 overflow-hidden rounded-2xl bg-surface-container-high shadow-2xl ring-1 ring-white/10 sm:mx-0 sm:w-44">
              <img
                v-if="poster"
                :src="poster"
                :alt="movie?.name ?? ''"
                class="size-full object-cover"
              >
            </div>

            <div class="flex min-w-0 flex-1 flex-col gap-3 pb-1">
              <h1 class="text-headline-small font-bold sm:text-headline-medium">
                {{ movie?.name ?? $t('Movie') }}
              </h1>
              <div class="flex flex-wrap items-center gap-2 text-body-small opacity-75">
                <span class="rounded-md bg-primary/15 px-2 py-0.5 font-medium text-primary">{{ $t('Movie') }}</span>
                <span v-if="movie?.rating" class="text-amber-200">{{ movie.rating }}</span>
                <span v-if="movie?.categoryName">{{ movie.categoryName }}</span>
              </div>
              <p v-if="movie?.plot" class="max-w-3xl text-body-medium leading-relaxed opacity-85">
                {{ movie.plot }}
              </p>
              <button
                type="button"
                class="mt-1 inline-flex min-h-12 w-fit items-center gap-2 rounded-xl bg-primary px-6 text-body-medium font-semibold text-on-primary transition-colors hover:brightness-110 focus-visible:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                @click="play"
              >
                <v-icon :icon="mdiPlay" size="22" />
                {{ $t('Play') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
