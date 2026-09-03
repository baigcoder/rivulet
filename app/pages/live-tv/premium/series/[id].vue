<script setup lang="ts">
/**
 * Premium TV series detail — hero, season picker, virtualized episodes.
 */
import type { ComponentPublicInstance } from 'vue'
import type { PremiumEpisode, PremiumSeriesDetail } from '~/types/premium'
import { mdiArrowLeft, mdiPlay } from '@mdi/js'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { premiumApi, proxyLogo } from '~/utils/premiumTv'

definePageMeta({ layout: 'default' })

const route = useRoute()
const router = useRouter()
const premium = usePremiumTvStore()
const settings = useSettingsStore()

const seriesId = computed(() => String(route.params.id ?? ''))
const detail = ref<PremiumSeriesDetail | null>(null)
const loading = ref(true)
const error = ref('')
const selectedSeason = ref<number | null>(null)

const poster = computed(() => proxyLogo(detail.value?.posterUrl))
const heroBlur = computed(() => settings.reduceEffects ? '' : 'backdrop-blur-xl')

const seasons = computed(() => {
  const map = new Map<number, PremiumEpisode[]>()
  for (const ep of detail.value?.episodes ?? []) {
    const list = map.get(ep.season) ?? []
    list.push(ep)
    map.set(ep.season, list)
  }
  return [...map.entries()].sort((a, b) => a[0] - b[0])
})

const seasonNumbers = computed(() => seasons.value.map(([n]) => n))

const activeSeason = computed(() => {
  if (selectedSeason.value != null)
    return selectedSeason.value
  return seasonNumbers.value[0] ?? null
})

const visibleEpisodes = computed(() => {
  if (activeSeason.value == null)
    return []
  return seasons.value.find(([n]) => n === activeSeason.value)?.[1] ?? []
})

const scrollRef = ref<HTMLElement>()
const margin = ref(0)
const headerRef = ref<HTMLElement>()

function measureMargin() {
  const scroller = scrollRef.value
  const header = headerRef.value
  if (!scroller || !header)
    return
  margin.value = header.getBoundingClientRect().height
}

const virtualizer = useVirtualizer(computed(() => ({
  count: visibleEpisodes.value.length,
  getScrollElement: () => scrollRef.value ?? null,
  scrollMargin: margin.value,
  estimateSize: () => 96,
  overscan: 4,
})))

function measure(el: Element | ComponentPublicInstance | null): void {
  if (el instanceof HTMLElement)
    virtualizer.value?.measureElement(el)
}

watch(visibleEpisodes, () => {
  void nextTick(() => {
    measureMargin()
    virtualizer.value?.measure()
  })
})

onMounted(async () => {
  await premium.ensureLoaded()
  loading.value = true
  error.value = ''
  try {
    const cached = premium.seriesDetailCache.get(seriesId.value)
    detail.value = cached ?? await premiumApi.vodSeriesDetail(seriesId.value)
    if (!cached && detail.value)
      premium.cacheSeriesDetail(seriesId.value, detail.value)
    selectedSeason.value = seasonNumbers.value[0] ?? null
  }
  catch (e) {
    error.value = e instanceof Error ? e.message : $t('Could not load this series.')
  }
  finally {
    loading.value = false
    void nextTick(measureMargin)
  }
})

function goBack(): void {
  const from = String(route.query.from ?? '')
  if (from)
    void router.push(from)
  else
    void router.replace(localePath('/live-tv/premium'))
}

function pickSeason(n: number): void {
  selectedSeason.value = n
  scrollRef.value?.scrollTo({ top: 0 })
}

function playEpisode(ep: PremiumEpisode): void {
  void router.push({
    path: localePath('/live-tv/premium/watch'),
    query: {
      kind: 'episode',
      id: ep.id,
      ext: ep.containerExtension || 'mkv',
      title: `${detail.value?.name ?? ''} · S${ep.season}E${ep.episode}`,
      from: route.fullPath,
    },
  })
}
</script>

<template>
  <div ref="scrollRef" class="h-full overflow-y-auto pb-12">
    <div v-if="loading" class="px-4 pt-4 md:px-6">
      <div class="flex flex-col gap-6 sm:flex-row sm:items-end">
        <div class="mx-auto aspect-[2/3] w-32 shrink-0 animate-pulse rounded-2xl bg-surface-container-high sm:mx-0 sm:w-40" />
        <div class="flex min-w-0 flex-1 flex-col gap-3">
          <div class="h-8 w-3/4 max-w-md animate-pulse rounded-lg bg-surface-container-high" />
          <div class="flex gap-2">
            <div class="h-6 w-16 animate-pulse rounded-md bg-surface-container-high/80" />
            <div class="h-6 w-12 animate-pulse rounded-md bg-surface-container-high/80" />
          </div>
          <div class="space-y-2">
            <div class="h-4 w-full max-w-2xl animate-pulse rounded bg-surface-container-high/70" />
            <div class="h-4 w-5/6 max-w-xl animate-pulse rounded bg-surface-container-high/70" />
          </div>
        </div>
      </div>
      <div class="mt-8 space-y-2">
        <div
          v-for="n in 6"
          :key="n"
          class="flex items-center gap-3 rounded-xl px-3 py-3"
        >
          <div class="size-11 shrink-0 animate-pulse rounded-full bg-surface-container-high" />
          <div class="min-w-0 flex-1 space-y-2">
            <div class="h-4 w-2/3 animate-pulse rounded bg-surface-container-high/80" />
            <div class="h-3 w-full animate-pulse rounded bg-surface-container-high/60" />
          </div>
        </div>
      </div>
    </div>

    <div v-else-if="error" class="grid min-h-[50vh] place-items-center px-6 text-center">
      <p class="text-body-medium opacity-70">
        {{ error }}
      </p>
      <button
        type="button"
        class="mt-4 rounded-xl bg-surface-container-high px-4 py-2 text-body-small font-medium hover:bg-surface-container-highest focus-visible:bg-surface-container-highest focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        @click="goBack"
      >
        {{ $t('Back') }}
      </button>
    </div>

    <template v-else-if="detail">
      <section ref="headerRef" class="relative overflow-hidden">
        <div
          v-if="poster"
          class="absolute inset-0 scale-110 bg-cover bg-center opacity-30"
          :class="heroBlur"
          :style="{ backgroundImage: `url(${poster})` }"
        />
        <div class="absolute inset-0 bg-gradient-to-t from-surface via-surface/92 to-surface/50" />

        <div class="relative px-4 pb-6 pt-4 md:px-6">
          <button
            type="button"
            class="mb-4 grid size-11 w-fit place-items-center rounded-lg text-on-surface/80 transition-colors hover:bg-surface-container-high hover:text-on-surface focus-visible:bg-surface-container-high focus-visible:text-on-surface"
            :aria-label="$t('Back')"
            @click="goBack"
          >
            <v-icon :icon="mdiArrowLeft" size="24" />
          </button>

          <div class="flex flex-col gap-6 sm:flex-row sm:items-end">
            <div class="mx-auto aspect-[2/3] w-32 shrink-0 overflow-hidden rounded-2xl bg-surface-container-high shadow-2xl ring-1 ring-white/10 sm:mx-0 sm:w-40">
              <img
                v-if="poster"
                :src="poster"
                :alt="detail.name"
                class="size-full object-cover"
              >
            </div>
            <div class="flex min-w-0 flex-1 flex-col gap-3">
              <h1 class="text-headline-small font-bold sm:text-headline-medium">
                {{ detail.name }}
              </h1>
              <div class="flex flex-wrap items-center gap-2 text-body-small opacity-75">
                <span class="rounded-md bg-primary/15 px-2 py-0.5 font-medium text-primary">{{ $t('TV show') }}</span>
                <span v-if="detail.rating" class="text-amber-200">{{ detail.rating }}</span>
                <span v-if="visibleEpisodes.length">{{ $t('{count} episodes', { count: detail.episodes.length }) }}</span>
              </div>
              <p v-if="detail.plot" class="max-w-3xl text-body-medium leading-relaxed opacity-85">
                {{ detail.plot }}
              </p>
            </div>
          </div>
        </div>
      </section>

      <section v-if="seasonNumbers.length > 1" class="sticky top-0 z-10 border-b border-outline/15 bg-surface/95 px-4 py-2 md:px-6">
        <div class="flex gap-2 overflow-x-auto pb-1">
          <button
            v-for="n in seasonNumbers"
            :key="n"
            type="button"
            class="shrink-0 rounded-full px-4 py-2 text-label-medium font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            :class="activeSeason === n
              ? 'bg-primary text-on-primary'
              : 'bg-surface-container-high text-on-surface/75 hover:bg-surface-container-highest hover:text-on-surface focus-visible:bg-surface-container-highest focus-visible:text-on-surface'"
            :aria-current="activeSeason === n ? 'true' : undefined"
            @click="pickSeason(n)"
          >
            {{ $t('Season {n}', { n }) }}
          </button>
        </div>
      </section>

      <section class="px-4 pt-4 md:px-6">
        <h2 v-if="seasonNumbers.length === 1" class="mb-3 text-title-medium font-bold">
          {{ $t('Season {n}', { n: seasonNumbers[0] }) }}
        </h2>

        <div
          v-if="visibleEpisodes.length"
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
            <button
              v-if="visibleEpisodes[virtualRow.index]"
              type="button"
              class="flex w-full items-center gap-3 rounded-xl px-3 py-3 text-start transition-colors hover:bg-surface-container-high focus-visible:bg-surface-container-high focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
              @click="playEpisode(visibleEpisodes[virtualRow.index]!)"
            >
              <span class="grid size-11 shrink-0 place-items-center rounded-full bg-primary/15 text-primary">
                <v-icon :icon="mdiPlay" size="20" />
              </span>
              <span class="min-w-0 flex-1">
                <span class="block text-body-medium font-semibold">
                  {{ $t('Episode {n}', { n: visibleEpisodes[virtualRow.index]!.episode }) }}
                  <span class="opacity-60">· {{ $t('Season {n}', { n: visibleEpisodes[virtualRow.index]!.season }) }}</span>
                  <template v-if="visibleEpisodes[virtualRow.index]!.title">
                    — {{ visibleEpisodes[virtualRow.index]!.title }}
                  </template>
                </span>
                <span
                  v-if="visibleEpisodes[virtualRow.index]!.plot"
                  class="line-clamp-2 text-body-small opacity-55"
                >
                  {{ visibleEpisodes[virtualRow.index]!.plot }}
                </span>
              </span>
            </button>
          </div>
        </div>

        <div v-else class="py-12 text-center text-body-medium opacity-60">
          {{ $t('No episodes listed.') }}
        </div>
      </section>
    </template>
  </div>
</template>
