<script setup lang="ts">
import type { Episode, Media } from '~/utils/tmdb'
import { mdiCheck, mdiPlay, mdiStar } from '@mdi/js'

// `show` is only passed through to the watched button, for the card snapshot.
const props = defineProps<{
  showId: string
  season: number
  episode: Episode
  show?: Media | null
  /** The first unwatched episode, for someone part-way through: lit and labelled. */
  upNext?: boolean
}>()

const library = useLibraryStore()

const progress = computed(() => library.episodeProgress(props.showId, props.season, props.episode.number))
const played = computed(() => fraction(progress.value))
const watched = computed(() => !!progress.value?.watched)

/** Not out yet: no Play, and the date says when instead. */
const upcoming = computed(() => !!props.episode.air && new Date(`${props.episode.air}T00:00:00`) > new Date())

function play() {
  navigateTo(watchLink('tv', props.showId, props.season, props.episode.number))
}

function armEpisode(event: PointerEvent) {
  if (event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey)
    prefetchEpisode(props.showId, props.season, props.episode.number)
}
</script>

<template>
  <!-- One card per episode: the still carries the number and the play, the
       text gets room to be read. On a phone it stacks; from `sm` it sits in a
       row. The row opens the episode page — only the Play button plays. -->
  <nuxt-link
    :to="episodeLink(showId, season, episode.number)"
    no-prefetch
    class="group flex flex-col gap-4 rounded-2xl border p-3 outline-none transition-[background-color,border-color] sm:flex-row sm:items-center md:p-4"
    :class="upNext
      ? 'bg-surface-container-high/70'
      : 'border-white/5 bg-surface-container/30 hover:border-white/15 hover:bg-surface-container-high/60 focus-visible:border-white/15 focus-visible:bg-surface-container-high/60'"
    :style="upNext ? { borderColor: 'var(--title-accent, rgb(var(--v-theme-primary)))' } : undefined"
    @pointerdown="armEpisode"
  >
    <div class="relative aspect-video w-full shrink-0 overflow-hidden rounded-xl bg-black sm:w-64 lg:w-72">
      <media-poster
        :src="stillUrl(episode.still, 'w780') || backdropUrl(show?.backdrop, 'w780')"
        :alt="episode.name"
        :class="watched ? 'opacity-60' : episode.still ? '' : 'opacity-50'"
      />
      <div class="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent" />
      <span class="absolute bottom-2 start-3 text-headline-small font-black text-white drop-shadow-[0_2px_8px_rgba(0,0,0,0.8)]">
        {{ episode.number }}
      </span>
      <div class="absolute inset-0 grid place-items-center opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
        <span class="grid size-12 place-items-center rounded-full bg-white/90 text-black shadow-xl">
          <v-icon :icon="mdiPlay" size="28" />
        </span>
      </div>
      <span v-if="watched" class="absolute end-2 top-2 flex items-center gap-1 rounded-full bg-black/75 px-2 py-0.5 text-label-medium font-semibold text-white">
        <v-icon :icon="mdiCheck" size="14" />{{ $t('Watched') }}
      </span>
      <div v-if="played > 0 && !watched" class="absolute inset-x-0 bottom-0 h-1 bg-white/25">
        <div class="h-full bg-[var(--title-accent,rgb(var(--v-theme-primary)))]" :style="{ width: `${played * 100}%` }" />
      </div>
    </div>

    <div class="flex min-w-0 flex-1 flex-col gap-1.5">
      <p v-if="upNext" class="text-label-medium font-bold uppercase tracking-widest text-[var(--title-accent,rgb(var(--v-theme-primary)))]">
        {{ $t('Next up') }}
      </p>
      <h3 class="truncate text-title-large font-bold" :class="watched ? 'opacity-70' : ''">
        {{ episode.name || $t('Episode {n}', { n: episode.number }) }}
      </h3>
      <div class="flex flex-wrap items-center gap-x-2.5 gap-y-0.5 text-body-medium opacity-70">
        <span v-if="episode.rating" class="flex items-center gap-1 font-semibold text-amber-300 opacity-100">
          <v-icon :icon="mdiStar" size="14" />{{ episode.rating.toFixed(1) }}
        </span>
        <span v-if="episode.runtime">{{ runtimeText(episode.runtime) }}</span>
        <span v-if="episode.air" :class="upcoming ? 'font-semibold text-[var(--title-accent,rgb(var(--v-theme-primary)))] opacity-100' : ''">
          {{ upcoming ? $t('Airs {date}', { date: dateText(episode.air) }) : dateText(episode.air) }}
        </span>
      </div>
      <p class="line-clamp-3 max-w-4xl text-body-medium leading-relaxed opacity-80">
        {{ episode.overview || $t('No overview.') }}
      </p>
    </div>

    <div class="flex shrink-0 items-center gap-1 sm:flex-col sm:items-end sm:gap-2">
      <v-btn
        v-if="!upcoming"
        :prepend-icon="mdiPlay"
        :variant="upNext ? 'flat' : 'tonal'"
        :color="upNext ? 'white' : undefined"
        rounded="lg"
        :class="upNext ? 'font-bold text-black' : ''"
        @click.stop.prevent="play"
      >
        {{ played > 0 && !watched ? $t('Resume') : $t('Play') }}
      </v-btn>

      <!-- Says you've seen it without playing it, which is what moves the show
           page's Play button on to the next episode. -->
      <watched-button
        :show-id="showId"
        :season="season"
        :episode="episode.number"
        :show="show"
        size="small"
      />
    </div>
  </nuxt-link>
</template>
