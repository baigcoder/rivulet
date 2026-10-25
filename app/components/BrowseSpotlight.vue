<script setup lang="ts">
import type { Media } from '~/utils/tmdb'
import { mdiInformationOutline, mdiPlay, mdiStar } from '@mdi/js'
import { loadTitleLogo } from '~/utils/titleImages'

/**
 * The top of a browse page: the list's first few titles as a banner, so
 * Movies and TV Shows open on something to watch rather than straight into a
 * wall of posters. Fed from the list the page already fetched — no request
 * of its own beyond each title's logo.
 *
 * Still, apart from the picks: no timer. Home's spotlight turns on its own;
 * this one is a header, and on a TV the GPU pays for every frame it animates.
 */
const props = defineProps<{
  items: Media[]
  /** What the list is, e.g. "Top rated", for the rank line. */
  label: string
  genres?: { id: number, name: string }[]
}>()

const library = useLibraryStore()

const picks = computed(() => props.items.filter(m => m.backdrop).slice(0, 5))
const at = ref(0)
watch(() => picks.value[0]?.id, () => (at.value = 0))
const current = computed(() => picks.value[Math.min(at.value, picks.value.length - 1)])

const logo = ref<string | null>(null)
watch(current, async (m, _old, onCleanup) => {
  logo.value = null
  if (!m)
    return
  let live = true
  onCleanup(() => {
    live = false
  })
  const found = await loadTitleLogo(m.type, m.id, m.lang)
  if (live)
    logo.value = found
}, { immediate: true })

const genreNames = computed(() => {
  const ids = current.value?.genreIds ?? []
  return ids.map(id => props.genres?.find(g => g.id === id)?.name).filter(Boolean).slice(0, 3).join(' · ')
})
</script>

<template>
  <section
    v-if="current"
    class="relative -mx-4 mb-6 flex min-h-[max(360px,48vh)] flex-col overflow-hidden md:-mx-6"
    :aria-label="label"
  >
    <div class="hero-mask absolute inset-0">
      <transition
        enter-active-class="transition-opacity duration-500 ease-out"
        leave-active-class="transition-opacity duration-500 ease-in"
        enter-from-class="opacity-0"
        leave-to-class="opacity-0"
      >
        <img
          :key="current.id"
          :src="backdropUrl(current.backdrop, 'w1280') ?? ''"
          alt=""
          decoding="async"
          class="absolute inset-0 size-full object-cover object-[70%_25%]"
        >
      </transition>
      <div class="absolute inset-0 bg-gradient-to-r from-black/85 via-black/40 to-transparent" />
      <div class="absolute inset-x-0 bottom-0 h-2/3 bg-gradient-to-t from-black/60 to-transparent" />
    </div>

    <div class="relative flex flex-1 flex-col justify-end gap-3 px-4 pb-6 pt-16 text-white md:px-10 md:pb-8">
      <p class="flex items-center gap-2 text-label-large font-semibold uppercase tracking-[0.18em] text-white/90">
        <span class="grid h-6 min-w-6 place-items-center rounded-md bg-primary px-1.5 font-black text-on-primary">{{ at + 1 }}</span>
        {{ $t('in {list}', { list: label }) }}
      </p>
      <img
        v-if="logo"
        :src="logoUrl(logo, 'w500') ?? ''"
        :alt="current.title"
        class="max-h-24 w-auto max-w-[min(100%,420px)] self-start object-contain object-left drop-shadow-[0_4px_24px_rgba(0,0,0,0.7)] md:max-h-28"
      >
      <h2 v-else class="max-w-3xl text-display-small font-black leading-none tracking-tight">
        {{ current.title }}
      </h2>
      <p class="flex flex-wrap items-center gap-x-3 text-body-large text-white/90">
        <span v-if="current.rating" class="flex items-center gap-1 font-semibold text-amber-300">
          <v-icon :icon="mdiStar" size="18" />{{ current.rating.toFixed(1) }}
        </span>
        <span v-if="current.year">{{ current.year }}</span>
        <span v-if="genreNames" class="text-white/75">{{ genreNames }}</span>
      </p>
      <p class="line-clamp-2 max-w-2xl text-body-large text-white/85">
        {{ current.overview }}
      </p>
      <div class="flex flex-wrap items-center gap-3 pt-1">
        <v-btn :prepend-icon="mdiPlay" size="large" color="white" variant="flat" rounded="lg" class="px-7 font-bold text-black" :to="library.resumeLink(current)">
          {{ $t('Play') }}
        </v-btn>
        <v-btn :prepend-icon="mdiInformationOutline" size="large" variant="flat" rounded="lg" class="bg-white/20! px-5 font-semibold text-white" :to="mediaLink(current)">
          {{ $t('More info') }}
        </v-btn>
        <!-- The picks, as real buttons: a remote steps along them like any row. -->
        <div v-if="picks.length > 1" class="ms-auto flex items-center gap-1.5">
          <button
            v-for="(m, i) in picks"
            :key="m.id"
            type="button"
            class="group/dot py-2 outline-none"
            :aria-label="m.title"
            :aria-current="i === at"
            @click="at = i"
          >
            <span
              class="block h-1.5 rounded-full transition-[width,background-color] duration-300 group-focus-visible/dot:ring-2 group-focus-visible/dot:ring-primary"
              :class="i === at ? 'w-8 bg-white' : 'w-4 bg-white/35 group-hover/dot:bg-white/70'"
            />
          </button>
        </div>
      </div>
    </div>
  </section>
</template>
