<script setup lang="ts">
import type { EpisodeDetail } from '~/utils/tmdb'
import { mdiPlay } from '@mdi/js'

/**
 * The one episode a returning viewer wants, as a picture: where they left off,
 * or the first one. The season list below has the rest; this is the shortcut
 * past it, one press from the top of the page.
 */
const props = defineProps<{
  showId: string
  season: number
  episode: number
  to: string
  /** They have watched some of this show — "Next up" rather than "Start watching". */
  returning: boolean
  /** Part-watched: how far, 0–1. */
  progress?: number
}>()

const ep = ref<EpisodeDetail | null>(null)
watch(() => [props.showId, props.season, props.episode] as const, async ([id, season, episode], _old, onCleanup) => {
  let live = true
  onCleanup(() => {
    live = false
  })
  ep.value = null
  const got = await loadEpisode(id, season, episode).catch(() => null)
  if (live)
    ep.value = got
}, { immediate: true })
</script>

<template>
  <section class="mx-4 md:mx-6" :aria-label="returning ? $t('Next up') : $t('Start watching')">
    <nuxt-link
      :to="to"
      class="group flex max-w-3xl flex-col overflow-hidden rounded-2xl border border-white/10 bg-surface-container-high/60 outline-none transition-[border-color,transform] duration-200 hover:border-white/30 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary sm:flex-row"
    >
      <div class="relative aspect-video w-full shrink-0 overflow-hidden bg-black sm:w-72">
        <media-poster v-if="ep?.still" :src="stillUrl(ep.still, 'w780')" :alt="ep.name" />
        <div class="absolute inset-0 grid place-items-center bg-black/25 transition-colors group-hover:bg-black/10 group-focus-visible:bg-black/10">
          <span class="grid size-14 place-items-center rounded-full bg-white/90 text-black shadow-xl transition-transform group-hover:scale-110 group-focus-visible:scale-110">
            <v-icon :icon="mdiPlay" size="30" />
          </span>
        </div>
        <div v-if="progress" class="absolute inset-x-0 bottom-0 h-1 bg-white/25">
          <div class="h-full bg-[var(--title-accent,rgb(var(--v-theme-primary)))]" :style="{ width: `${Math.round(progress * 100)}%` }" />
        </div>
      </div>
      <div class="flex min-w-0 flex-col justify-center gap-1.5 p-4 md:p-5">
        <p class="text-label-large font-semibold uppercase tracking-widest text-[var(--title-accent,rgb(var(--v-theme-primary)))]">
          {{ returning ? $t('Next up') : $t('Start watching') }} · S{{ season }} E{{ episode }}
        </p>
        <h3 class="truncate text-title-large font-bold">
          {{ ep?.name || $t('Episode {n}', { n: episode }) }}
        </h3>
        <p v-if="ep?.runtime || ep?.air" class="text-body-medium opacity-70">
          {{ [runtimeText(ep?.runtime), ep?.air?.slice(0, 4)].filter(Boolean).join(' · ') }}
        </p>
        <p v-if="ep?.overview" class="line-clamp-2 text-body-medium opacity-85">
          {{ ep.overview }}
        </p>
      </div>
    </nuxt-link>
  </section>
</template>
