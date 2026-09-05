<script setup lang="ts">
/**
 * The title page — library and Premium VOD both render this.
 * Play is a torrent watch link unless `providerPlay` is set, in which
 * case it stays on the caller's stream.
 */
import type { Media, MediaType } from '~/utils/tmdb'
import { mdiAlertCircleOutline, mdiBookmark, mdiBookmarkOutline, mdiClose, mdiEye, mdiEyeOutline, mdiHeart, mdiHeartOutline, mdiOpenInNew, mdiPlay, mdiShieldLockOutline, mdiStar, mdiVolumeHigh, mdiVolumeOff, mdiYoutube } from '@mdi/js'
import { useTitleImages } from '~/utils/titleImages'

const props = defineProps<{
  type: MediaType
  id: string
  /** Hide Download / torrent picker (Premium already has a stream). */
  providerPlay?: boolean
  hideSeasons?: boolean
  fallbackTitle?: string
  fallbackPoster?: string
  fallbackOverview?: string
  fallbackYear?: string
  fallbackRating?: number
}>()

const emit = defineEmits<{
  play: []
}>()

const ui = useUiStore()
const library = useLibraryStore()
const premium = usePremiumTvStore()
const { mobile } = useDisplay()
const settings = useSettingsStore()

/**
 * One size for the whole action row. Spelled out at each of the eight buttons it
 * only looked consistent — Releases was quietly rendering 8px shorter than the
 * rest for want of a prop, and nothing said the row was meant to match. `large`
 * is 44px, which is also what a d-pad needs; a TV asks for a 1280 viewport, so
 * it is never the `mobile` branch.
 */
const btnSize = computed(() => mobile.value ? 'default' : 'large')

/**
 * Vuetify pads an `icon` button a further 12px past its `size` (`$button-icon-
 * density`), so at `large` the three toggles came out 56px against the 44px pills
 * beside them and the row centred them in 6px of dead space. `comfortable` is the
 * density whose multiplier is 0, which lands them on 44px exactly.
 *
 * A phone keeps the padding. There Play is `:block` and a spacer splits the row,
 * so there is no common height to match — and 48px is the touch target Material
 * asks for, which 36px is not.
 */
const iconDensity = computed(() => mobile.value ? 'default' : 'comfortable')

/** Who is serving this title. Library pages have no provider. */
const sourceLabel = computed(() => {
  if (!props.providerPlay)
    return ''
  return premium.account?.accountName?.trim() || premium.account?.username || $t('Premium TV')
})

const { data: fetched, status, error } = useMediaDetail(() => props.type, () => props.id)
/** Prefetch often finished on press — don't wait a tick for useAsyncData. */
const media = computed(() => fetched.value ?? peekMediaDetail(props.type, props.id) ?? null)

const fallback = computed<Media | null>(() => {
  if (!props.fallbackTitle)
    return null
  return {
    id: Number(props.id) || 0,
    type: props.type,
    title: props.fallbackTitle,
    year: props.fallbackYear ?? '',
    poster: null,
    backdrop: null,
    overview: props.fallbackOverview ?? '',
    rating: props.fallbackRating ?? 0,
    genreIds: [],
    lang: '',
  }
})

const cover = computed(() => {
  if (media.value)
    return media.value
  const from = ui.opening ?? ui.selected
  if (from && from.type === props.type && String(from.id) === props.id)
    return from
  const snap = peekSnapMedia(props.type, props.id)
  if (snap)
    return snap
  return fallback.value
})

/** No snapshot and no TMDB yet — skeleton, never an empty page. */
const heroPending = computed(() => !cover.value && !error.value)

const { data: titleArt, execute: loadTitleArt } = useTitleImages(() => props.type, () => props.id)
const sectionStage = ref(0)
let artFrame = 0
let sectionFrame = 0
let artScheduled = false
let sectionsScheduled = false

watch(() => `${props.type}-${props.id}`, () => {
  sectionStage.value = 0
  artScheduled = false
  sectionsScheduled = false
  if (import.meta.server)
    return
  cancelAnimationFrame(artFrame)
  cancelAnimationFrame(sectionFrame)
}, { immediate: true, flush: 'post' })

watch(media, value => {
  if (!value || import.meta.server)
    return
  if (!artScheduled) {
    artScheduled = true
    // Core data wins the network. Logos and stills start after its first paint.
    artFrame = requestAnimationFrame(() => {
      artFrame = requestAnimationFrame(() => void loadTitleArt())
    })
  }
  if (sectionsScheduled)
    return
  sectionsScheduled = true
  const next = () => {
    sectionStage.value++
    if (sectionStage.value < 4)
      sectionFrame = requestAnimationFrame(next)
  }
  // Split the heavy rows across frames instead of one navigation-blocking patch.
  sectionFrame = requestAnimationFrame(() => {
    sectionFrame = requestAnimationFrame(next)
  })
}, { immediate: true, flush: 'post' })

const stills = computed(() => titleArt.value?.stills ?? [])
const coverLogo = computed(() => media.value?.logo || titleArt.value?.logo || null)

/** Pinned on first paint so TMDB replacing the snapshot does not swap hero URLs. */
const pinnedPoster = ref('')
const pinnedBackdrop = ref('')
const heroArtReady = ref(false)
watch(() => `${props.type}-${props.id}`, () => {
  pinnedPoster.value = ''
  pinnedBackdrop.value = ''
  heroArtReady.value = false
}, { flush: 'sync' })
watch([cover, () => props.fallbackPoster], ([c, fb]) => {
  if (!c && !fb)
    return
  if (!pinnedPoster.value)
    pinnedPoster.value = posterUrl(c?.poster, ui.posterSize) || fb || ''
  if (!pinnedBackdrop.value && c?.backdrop)
    pinnedBackdrop.value = backdropUrl(c.backdrop, 'w780') ?? ''
}, { immediate: true })
watch(pinnedBackdrop, (url, prev) => {
  if (url === prev)
    return
  heroArtReady.value = !url
})

const videoHidden = ref(false)
const heroMuted = ref(true)
const heroIdle = ref(false)
const trailerPick = ref(0)
const trailerKeys = computed(() => media.value?.trailers?.length
  ? media.value.trailers
  : media.value?.trailer ? [media.value.trailer] : [])
const trailerKey = computed(() => trailerKeys.value[trailerPick.value] ?? '')
let idleHandle = 0

function cancelHeroIdle() {
  if (!idleHandle)
    return
  clearTimeout(idleHandle)
  idleHandle = 0
}

function nextTrailer() {
  if (trailerPick.value + 1 < trailerKeys.value.length)
    trailerPick.value += 1
  else
    videoHidden.value = true
}

watch(() => trailerKeys.value.join(',') || media.value?.trailer || '', keys => {
  cancelHeroIdle()
  heroIdle.value = false
  videoHidden.value = false
  trailerPick.value = 0
  if (!keys || import.meta.server)
    return
  const go = () => {
    heroIdle.value = true
  }
  idleHandle = window.setTimeout(go, 4000)
}, { immediate: true })

const heroSrc = computed(() => {
  const key = trailerKey.value
  if (!key || videoHidden.value || !heroIdle.value)
    return ''
  return youtubeEmbedSrc(key, { mute: true, loop: true })
})

const heroFrame = ref<HTMLIFrameElement | null>(null)
const heroPlaying = ref(false)
let showHero = 0
watch(heroSrc, src => {
  heroPlaying.value = false
  clearTimeout(showHero)
  if (src) {
    showHero = window.setTimeout(() => {
      if (!heroPlaying.value)
        nextTrailer()
    }, 5000)
  }
})

function heroCommand(func: string, args: unknown[] = []) {
  heroFrame.value?.contentWindow?.postMessage(youtubeCommand(func, args), '*')
}

function lockHeroQuality() {
  heroCommand('setPlaybackQuality', ['hd720'])
  heroCommand('setPlaybackQualityRange', ['hd720', 'hd720'])
}

function toggleHeroSound() {
  heroIdle.value = true
  heroMuted.value = !heroMuted.value
  heroCommand(heroMuted.value ? 'mute' : 'unMute')
}

function onHeroReady() {
  heroFrame.value?.contentWindow?.postMessage(JSON.stringify({ event: 'listening' }), '*')
  heroCommand(heroMuted.value ? 'mute' : 'unMute')
  lockHeroQuality()
}

function onHeroMessage(e: MessageEvent) {
  if (e.source !== heroFrame.value?.contentWindow)
    return
  if (youtubeError(e.data)) {
    nextTrailer()
    return
  }
  if (!youtubePlaying(e.data))
    return
  heroPlaying.value = true
}

const heroBox = ref<HTMLElement | null>(null)
useIntersectionObserver(heroBox, ([entry]) => {
  if (!heroSrc.value)
    return
  heroCommand(entry?.isIntersecting ? 'playVideo' : 'pauseVideo')
}, { threshold: 0.35 })

onMounted(() => window.addEventListener('message', onHeroMessage))
onUnmounted(() => {
  cancelHeroIdle()
  clearTimeout(showHero)
  window.removeEventListener('message', onHeroMessage)
})

const trailerSrc = computed(() => {
  const key = trailerKey.value
  if (!key)
    return ''
  return youtubeEmbedSrc(key)
})

const RATING_ORDER = ['G', 'PG', 'PG-13', 'R', 'NC-17', '']
const parentalBlocked = computed(() => {
  if (!settings.parentalEnabled || !media.value?.certification)
    return false
  const maxIdx = RATING_ORDER.indexOf(settings.parentalMaxRating)
  const itemIdx = RATING_ORDER.indexOf(media.value.certification)
  return itemIdx > maxIdx
})

const pinDialog = ref(false)
const pinInput = ref('')
const pinError = ref('')
const pinUnlocked = ref(false)

function checkPin() {
  if (pinInput.value === settings.parentalPin) {
    pinUnlocked.value = true
    pinDialog.value = false
    pinError.value = ''
  }
  else {
    pinError.value = $t('Incorrect PIN')
  }
  pinInput.value = ''
}

let mine = 0
let selectFrame = 0
watch(cover, value => {
  if (import.meta.server)
    return
  cancelAnimationFrame(selectFrame)
  if (value)
    selectFrame = requestAnimationFrame(() => (mine = ui.select(value)))
}, { immediate: true, flush: 'post' })
onUnmounted(() => {
  if (!import.meta.server) {
    cancelAnimationFrame(artFrame)
    cancelAnimationFrame(sectionFrame)
    cancelAnimationFrame(selectFrame)
  }
  ui.release(mine)
})

const meta = computed(() => {
  const m = media.value
  if (!m)
    return []
  return [
    m.year,
    m.type === 'movie'
      ? runtimeText(m.runtime)
      : m.episodeCount
        ? $t('{seasons} seasons · {episodes} episodes', { seasons: m.seasons.length, episodes: m.episodeCount })
        : '',
    m.status,
    m.companies.join(', '),
  ].filter(Boolean)
})

const credits = computed(() => {
  const m = media.value
  if (!m)
    return []
  return [
    { label: m.directors.length > 1 ? $t('Directors') : $t('Director'), value: m.directors.join(', ') },
    { label: m.writers.length > 1 ? $t('Writers') : $t('Writer'), value: m.writers.slice(0, 3).join(', ') },
    { label: $t('Budget'), value: moneyText(m.budget) },
    { label: $t('Revenue'), value: moneyText(m.revenue) },
  ].filter(row => row.value)
})

const trailer = ref(false)
const torrentPickerRef = ref<{ open: () => void } | null>(null)

async function openTrailer() {
  const url = `https://www.youtube.com/watch?v=${trailerKey.value || media.value?.trailer}`
  try {
    await useTauriShellOpen(url)
  }
  catch {
    window.open(url, '_blank')
  }
}

const firstSeason = computed(() => media.value?.seasons[0]?.number ?? 1)

const target = computed(() => {
  if (props.type !== 'tv')
    return null
  return nextEpisode(media.value?.seasons ?? [], library.lastEpisode(props.id))
    ?? { season: firstSeason.value, episode: 1 }
})

const targetText = computed(() => target.value ? `S${target.value.season} E${target.value.episode}` : '')

const handoff = computed(() => ({
  imdb: media.value?.imdbId ?? undefined,
  title: cover.value?.title,
  year: cover.value?.year,
}))

const playLink = computed(() =>
  target.value
    ? watchLink('tv', props.id, target.value.season, target.value.episode, handoff.value)
    : watchLink('movie', props.id, undefined, undefined, handoff.value),
)

const started = computed(() => {
  const m = media.value ?? cover.value
  const p = m && (target.value
    ? library.episodeProgress(props.id, target.value.season, target.value.episode)
    : library.cardProgress(m))
  return p && !p.watched && resumable(p.position, p.duration) ? p : null
})

const playLabel = computed(() => [
  started.value ? $t('Resume') : $t('Play'),
  props.providerPlay ? '' : targetText.value,
  remainingText(started.value) && `· ${remainingText(started.value)}`,
].filter(Boolean).join(' '))

const showPlay = computed(() => props.providerPlay || props.type === 'movie' || !!target.value)

const scroller = ref<HTMLElement>()
provide('detailScroller', scroller)

watch(() => props.id, () => {
  nextTick(() => {
    scroller.value?.scrollTo(0, 0)
    const shell = document.querySelector('[data-dpad-start]')
    if (shell instanceof HTMLElement)
      shell.scrollTop = 0
  })
}, { immediate: true })
</script>

<template>
  <div
    ref="scroller"
    class="h-full min-h-0 overflow-y-auto pb-12 [scrollbar-gutter:stable]"
    style="background-color: rgb(var(--v-theme-background))"
  >
    <div v-if="error && !fallbackTitle" class="flex h-full flex-col items-center justify-center gap-2">
      <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
      <span class="text-body-medium opacity-70">{{ $t('Couldn\'t load this title.') }}</span>
    </div>

    <div v-else-if="parentalBlocked && !pinUnlocked" class="flex h-full flex-col items-center justify-center gap-4 px-4 text-center">
      <v-icon :icon="mdiShieldLockOutline" size="48" class="opacity-60" />
      <h2 class="text-title-large font-semibold">
        {{ $t('Content restricted') }}
      </h2>
      <p class="max-w-md text-body-medium opacity-70">
        {{ $t('This content is rated {rating} and exceeds your parental control settings.', { rating: media?.certification }) }}
      </p>
      <!-- The only thing on this screen a remote can reach, so it gets the same
           44px the action row's buttons do rather than the 36px default. -->
      <v-btn v-if="settings.parentalPin" size="large" variant="tonal" @click="pinDialog = true">
        {{ $t('Enter PIN to unlock') }}
      </v-btn>

      <v-dialog v-model="pinDialog" max-width="360">
        <v-card rounded="xl" class="p-2">
          <v-card-title class="text-title-medium">
            {{ $t('Enter PIN') }}
          </v-card-title>
          <v-card-text>
            <v-text-field
              v-model="pinInput"
              type="password"
              :label="$t('PIN')"
              maxlength="8"
              density="comfortable"
              hide-details
              autofocus
              @keydown.enter="checkPin"
            />
            <div v-if="pinError" class="pt-2 text-body-small text-error">
              {{ pinError }}
            </div>
          </v-card-text>
          <v-card-actions>
            <v-spacer />
            <v-btn variant="text" size="small" @click="pinDialog = false">
              {{ $t('Cancel') }}
            </v-btn>
            <v-btn variant="tonal" size="small" @click="checkPin">
              {{ $t('Unlock') }}
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-dialog>
    </div>

    <template v-else>
      <section
        v-if="cover || heroPending"
        ref="heroBox"
        class="relative mb-6 h-[50vh] min-h-[380px] overflow-hidden rounded-b-3xl md:h-[60vh] md:min-h-[460px]"
      >
        <template v-if="heroPending">
          <div class="absolute inset-0 animate-pulse bg-surface-container/35" />
          <div class="absolute inset-0 bg-gradient-to-t from-black/95 via-black/40 to-black/20" />
          <div class="absolute inset-x-0 bottom-0 p-4 md:p-8">
            <div class="h-10 w-2/3 max-w-md animate-pulse rounded-lg bg-white/10 md:h-12" />
          </div>
        </template>
        <template v-else>
          <img
            v-if="pinnedPoster"
            :src="pinnedPoster"
            :alt="cover!.title"
            fetchpriority="high"
            decoding="async"
            class="absolute inset-0 h-full w-full object-cover"
          >
          <img
            v-if="pinnedBackdrop"
            :src="pinnedBackdrop"
            :alt="cover!.title"
            decoding="async"
            class="absolute inset-0 h-full w-full object-cover transition-opacity duration-300"
            :class="heroArtReady ? 'opacity-100' : 'opacity-0'"
            @load="heroArtReady = true"
          >
          <div
            v-if="heroSrc"
            class="absolute inset-0 overflow-hidden"
          >
            <iframe
              ref="heroFrame"
              :src="heroSrc"
              class="absolute left-1/2 top-1/2 h-full w-full -translate-x-1/2 -translate-y-1/2 scale-[1.45] transition-opacity duration-300"
              :class="heroPlaying ? 'opacity-100' : 'opacity-0'"
              frameborder="0"
              allow="autoplay; encrypted-media; gyroscope; picture-in-picture; web-share"
              referrerpolicy="strict-origin-when-cross-origin"
              allowfullscreen
              tabindex="-1"
              aria-hidden="true"
              @load="onHeroReady"
            />
          </div>
          <div class="absolute inset-0 bg-gradient-to-t from-black/95 via-black/40 to-black/20" />
          <div class="absolute inset-0 bg-gradient-to-r from-black/80 via-transparent to-transparent" />

          <p
            v-if="sourceLabel"
            class="absolute start-4 top-4 z-10 max-w-[min(70%,18rem)] truncate rounded-full border border-white/20 bg-black/70 px-3 py-1 text-label-small font-semibold tracking-wide text-white"
          >
            {{ sourceLabel }}
          </p>

          <div v-if="trailerKey" class="absolute right-4 top-4 z-10 flex items-center gap-2">
            <button
              v-tooltip:bottom="heroMuted ? $t('Sound on') : $t('Sound off')"
              class="grid size-10 place-items-center rounded-full border border-white/20 bg-black/60 text-white opacity-95 transition-[transform,background-color] hover:scale-110 hover:bg-black/80 focus-visible:scale-110 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-primary"
              :aria-label="heroMuted ? $t('Sound on') : $t('Sound off')"
              @click="toggleHeroSound"
            >
              <v-icon :icon="heroMuted ? mdiVolumeOff : mdiVolumeHigh" size="18" />
            </button>
            <button
              v-tooltip:bottom="$t('Hide video')"
              class="grid size-10 place-items-center rounded-full border border-white/20 bg-black/60 text-white opacity-95 transition-[transform,background-color] hover:scale-110 hover:bg-black/80 focus-visible:scale-110 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-primary"
              :aria-label="$t('Hide video')"
              @click="videoHidden = true"
            >
              <v-icon :icon="mdiClose" size="18" />
            </button>
          </div>

          <div class="absolute inset-x-0 bottom-0 p-4 md:p-8">
            <img
              v-if="coverLogo"
              :src="logoUrl(coverLogo)!"
              :alt="cover!.title"
              class="h-16 w-auto max-w-md object-contain drop-shadow-[0_2px_24px_rgba(0,0,0,0.8)] md:h-24 md:max-w-lg"
            >
            <h1 v-else class="text-headline-large font-bold text-white drop-shadow-[0_2px_24px_rgba(0,0,0,0.7)]">
              {{ cover!.title }}
            </h1>
            <p v-if="media?.tagline" class="mt-1 max-w-3xl text-body-medium italic text-white/70">
              {{ media.tagline }}
            </p>
          </div>
        </template>
      </section>

      <section class="px-4 pb-8 pt-4 md:px-6">
        <div class="flex flex-col gap-6 sm:flex-row sm:items-end">
          <div class="aspect-2/3 w-32 shrink-0 overflow-hidden rounded-2xl shadow-2xl sm:w-44 lg:w-52">
            <media-poster eager :src="posterUrl(cover?.poster, ui.posterSize) || fallbackPoster" :alt="cover?.title" />
          </div>

          <div v-if="cover && !heroPending" class="flex min-w-0 flex-1 flex-col gap-3">
            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-body-small opacity-75">
              <media-reviews
                v-if="id"
                :id="id"
                :type="type"
                :rating="cover.rating"
                :votes="media?.votes"
              />
              <span v-else-if="cover.rating" class="flex items-center gap-1">
                <v-icon :icon="mdiStar" size="14" class="text-amber-400" />
                <span class="font-medium">{{ cover.rating.toFixed(1) }}</span>
              </span>
              <span v-if="media?.certification" class="rounded border border-outline-variant px-1.5 py-0.5 text-label-small">
                {{ media.certification }}
              </span>
              <span v-for="part in meta" :key="part">{{ part }}</span>
              <span v-if="!media && cover.year">{{ cover.year }}</span>
            </div>

            <div v-if="media" class="flex flex-wrap gap-1.5">
              <v-chip v-for="genre in media.genres" :key="genre.id" size="small" :text="genre.name" />
              <v-chip
                v-if="media.collection"
                size="small"
                variant="tonal"
                :text="media.collection.name"
                :to="collectionLink(media.collection.id)"
              />
            </div>
            <div v-else-if="status === 'pending'" class="flex flex-wrap gap-1.5">
              <div v-for="n in 3" :key="n" class="h-7 w-16 animate-pulse rounded-full bg-surface-container/60" />
            </div>

            <p class="max-w-3xl text-body-medium opacity-85">
              {{ cover.overview || $t('No overview.') }}
            </p>

            <dl v-if="credits.length" class="grid grid-cols-1 gap-x-6 gap-y-1 text-body-small sm:grid-cols-2 lg:max-w-2xl">
              <div v-for="row in credits" :key="row.label" class="flex gap-2">
                <dt class="shrink-0 opacity-50">
                  {{ row.label }}
                </dt>
                <dd class="truncate opacity-85">
                  {{ row.value }}
                </dd>
              </div>
            </dl>

            <div class="flex flex-wrap items-center gap-2 pt-2">
              <v-btn
                v-if="showPlay && providerPlay"
                :prepend-icon="mdiPlay"
                :size="btnSize"
                :block="mobile"
                @click="emit('play')"
              >
                {{ playLabel }}
              </v-btn>
              <v-btn
                v-else-if="showPlay"
                :prepend-icon="mdiPlay"
                :size="btnSize"
                :block="mobile"
                :to="playLink"
              >
                {{ playLabel }}
              </v-btn>
              <download-button
                v-if="!providerPlay && media && (type === 'movie' || target)"
                :id="id"
                :type="type"
                :imdb-id="media.imdbId"
                :season="target?.season"
                :episode="target?.episode"
                :size="btnSize"
                @pick="torrentPickerRef?.open()"
              />
              <template v-else-if="!providerPlay && status === 'pending' && (type === 'movie' || target)">
                <!-- The buttons these stand in for are `btnSize` tall (44px, or
                     36px on a phone). A placeholder of any other height moves the
                     whole row the moment the request lands. -->
                <div class="animate-pulse rounded-lg bg-surface-container/60" :class="mobile ? 'h-9 w-full' : 'h-11 w-28'" />
                <div class="animate-pulse rounded-lg bg-surface-container/60" :class="mobile ? 'h-9 w-full' : 'h-11 w-28'" />
              </template>
              <torrent-picker
                v-if="!providerPlay && media && (type === 'movie' || target)"
                :id="id"
                ref="torrentPickerRef"
                :type="type"
                :imdb-id="media.imdbId"
                :season="target?.season"
                :episode="target?.episode"
                :size="btnSize"
              />
              <v-btn
                v-if="trailerKey"
                :prepend-icon="mdiYoutube"
                :size="btnSize"
                variant="tonal"
                @click="trailer = true"
              >
                {{ $t('Trailer') }}
              </v-btn>
              <v-spacer v-if="mobile" />
              <!-- `iconDensity` is what keeps these level with the pills above,
                   rather than 12px taller — see the computed. -->
              <v-btn
                v-if="cover"
                icon
                variant="text"
                color="on-surface"
                :density="iconDensity"
                :size="btnSize"
                @click="library.toggleWatched(cover)"
              >
                <v-icon :icon="library.isWatched(cover) ? mdiEye : mdiEyeOutline" :color="library.isWatched(cover) ? 'primary' : undefined" />
                <v-tooltip activator="parent" :text="library.isWatched(cover) ? $t('Mark unwatched') : $t('Mark watched')" />
              </v-btn>
              <v-btn
                v-if="cover"
                icon
                variant="text"
                color="on-surface"
                :density="iconDensity"
                :size="btnSize"
                @click="library.toggleWatchlist(cover)"
              >
                <v-icon :icon="library.inWatchlist(cover) ? mdiBookmark : mdiBookmarkOutline" :color="library.inWatchlist(cover) ? 'primary' : undefined" />
                <v-tooltip activator="parent" :text="library.inWatchlist(cover) ? $t('Remove from watchlist') : $t('Add to watchlist')" />
              </v-btn>
              <v-btn
                v-if="cover"
                icon
                variant="text"
                color="on-surface"
                :density="iconDensity"
                :size="btnSize"
                @click="library.toggleFavourite(cover)"
              >
                <v-icon :icon="library.isFavourite(cover) ? mdiHeart : mdiHeartOutline" :color="library.isFavourite(cover) ? 'primary' : undefined" />
                <v-tooltip activator="parent" :text="library.isFavourite(cover) ? $t('Remove from favourites') : $t('Favourite')" />
              </v-btn>
            </div>
          </div>

          <div v-else class="flex min-w-0 flex-1 flex-col gap-3">
            <div class="h-10 w-2/3 max-w-sm animate-pulse rounded-lg bg-surface-container/60" />
            <div class="h-4 w-40 animate-pulse rounded bg-surface-container/60" />
            <div class="h-20 w-full max-w-2xl animate-pulse rounded-lg bg-surface-container/60" />
            <div class="h-10 w-48 animate-pulse rounded-lg bg-surface-container/60" />
          </div>
        </div>
      </section>

      <div class="flex flex-col gap-8">
        <slot name="below" />

        <media-seasons
          v-if="sectionStage >= 1 && !hideSeasons && type === 'tv' && media?.seasons?.length"
          :key="id"
          :show-id="id"
          :seasons="media.seasons"
          :poster="media.poster"
          :show="media"
        />

        <cast-row
          v-if="sectionStage >= 2 && media?.cast?.length"
          :title="$t('Cast')"
          :people="media.cast"
        />

        <media-images
          v-if="sectionStage >= 3 && id"
          :id="id"
          :key="`${type}-${id}`"
          :type="type"
          :stills="stills"
        />

        <media-slider
          v-if="sectionStage >= 4 && id"
          :title="$t('More like this')"
          :request="{ path: `/${type}/${id}/recommendations`, type }"
        />
      </div>

      <v-dialog v-model="trailer" max-width="1100">
        <v-card class="overflow-hidden">
          <iframe
            v-if="trailer"
            :src="trailerSrc"
            class="aspect-video w-full border-0"
            style="zoom: var(--frame-zoom, 1)"
            allow="autoplay; encrypted-media; gyroscope; picture-in-picture; web-share"
            referrerpolicy="strict-origin-when-cross-origin"
            allowfullscreen
          />
          <v-card-actions>
            <v-btn :prepend-icon="mdiOpenInNew" size="small" variant="text" @click="openTrailer">
              {{ $t('Open on YouTube') }}
            </v-btn>
            <v-spacer />
            <v-btn size="small" variant="text" @click="trailer = false">
              {{ $t('Close') }}
            </v-btn>
          </v-card-actions>
        </v-card>
      </v-dialog>
    </template>
  </div>
</template>
